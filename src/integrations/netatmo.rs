//! Netatmo and Legrand devices through Netatmo's cloud: weather stations, thermostats,
//! lights and sockets, and camera pictures. Each is a choice in the wizard, and the token
//! may hold only the permissions those choices need. Controlling anything is a separate
//! choice. Shutters, locks, sirens and contactors are deliberately not offered at all.
//!
//! Each token refresh invalidates the previous refresh token at once, so the
//! latest one is always kept, in memory and with `Event::Remember`.

use super::{get_str, Capability, Entity, Event, Field, FieldKind, Instance, Kind, Sink};
use crate::client::{parse_url, request, request_public};
use crate::util::percent_encode;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const API: &str = "https://api.netatmo.com";
const POLL: Duration = Duration::from_secs(5 * 60);
const MAX_HOMES: usize = 8;
/// A camera's address expires; asking for a new one is limited to this often.
const CAMERA_RELOAD: Duration = Duration::from_secs(60);
const LIGHTS: [&str; 10] = ["NLL", "NLF", "NLFE", "NLFN", "NLM", "NLIS", "Z3L", "BNLD", "BNIL", "BN3L"];
const SOCKETS: [&str; 4] = ["NLP", "NLPM", "NLC", "BNCS"];
const CAMERAS: [&str; 2] = ["NACamera", "NOC"];

/// Pasted refresh token → its latest rotation. `Registry` retries `start` with the
/// config it began with, and an old instance can overlap a new one; both must see
/// the current token, and only one may refresh at a time.
static TOKENS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

pub fn kind() -> Kind {
    Kind {
        id: "netatmo",
        name: "Netatmo",
        description: "Indoor, outdoor, wind and rain readings from your Netatmo weather station.",
        fields: vec![
            Field { key: "client_id", label: "Client ID", kind: FieldKind::Text, required: true, placeholder: "", help: "Create an app at dev.netatmo.com → My apps." },
            Field { key: "client_secret", label: "Client secret", kind: FieldKind::Secret, required: true, placeholder: "", help: "From the same app page." },
            Field { key: "refresh_token", label: "Refresh token", kind: FieldKind::Secret, required: true, placeholder: "", help: "On the app page, use the token generator with exactly the scopes for what you tick below; tokens with more access are refused. Paste a new one here if Netatmo ever stops accepting it." },
            Field { key: "skip_weather", label: "Leave out weather stations", kind: FieldKind::Bool, required: false, placeholder: "", help: "Otherwise needs read_station." },
            Field { key: "thermostats", label: "Thermostats and valves", kind: FieldKind::Bool, required: false, placeholder: "", help: "Needs read_thermostat (Smarther: read_smarther)." },
            Field { key: "lights", label: "Legrand and BTicino lights and sockets", kind: FieldKind::Bool, required: false, placeholder: "", help: "Needs read_magellan (MyHome: read_mhs1). Shutters, locks and contactors are never included." },
            Field { key: "cameras", label: "Camera pictures", kind: FieldKind::Bool, required: false, placeholder: "", help: "Needs read_camera and access_camera (outdoor: read_presence and access_presence). Only screens you assign a camera can see it." },
            Field { key: "control", label: "Allow screens to control thermostats, lights and sockets", kind: FieldKind::Bool, required: false, placeholder: "", help: "Off means read-only. On also needs the matching write_ scopes, such as write_thermostat and write_magellan." },
        ],
        test,
        start,
    }
}

/// The token to use: the remembered rotation, unless the admin has pasted a new one since.
fn stored_token(config: &Map<String, Value>) -> String {
    let pasted = get_str(config, "refresh_token");
    match config.get("_token") {
        Some(t) if t["from"] == pasted => t["token"].as_str().unwrap_or(pasted).to_string(),
        _ => pasted.to_string(),
    }
}

/// Forgets rotated tokens no integration uses any more. The caller must not hold the
/// store lock: `refresh` takes the store while it holds `TOKENS`.
pub fn keep_tokens(live: &[String]) {
    TOKENS.lock().unwrap().retain(|pasted, _| live.contains(pasted));
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct Features {
    weather: bool,
    thermostats: bool,
    lights: bool,
    cameras: bool,
    control: bool,
}

impl Features {
    fn from(c: &Map<String, Value>) -> Features {
        let on = |k: &str| c.get(k).and_then(Value::as_bool).unwrap_or(false);
        Features { weather: !on("skip_weather"), thermostats: on("thermostats"), lights: on("lights"), cameras: on("cameras"), control: on("control") }
    }

    fn homes(self) -> bool {
        self.thermostats || self.lights || self.cameras
    }

    /// Every permission these choices may use.
    fn allowed(self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.weather {
            out.push("read_station");
        }
        if self.thermostats {
            out.extend(["read_thermostat", "read_smarther"]);
            if self.control {
                out.extend(["write_thermostat", "write_smarther"]);
            }
        }
        if self.lights {
            out.extend(["read_magellan", "read_mhs1"]);
            if self.control {
                out.extend(["write_magellan", "write_mhs1"]);
            }
        }
        if self.cameras {
            out.extend(["read_camera", "access_camera", "read_presence", "access_presence"]);
        }
        out
    }

    /// For each choice, permissions of which the token must have at least one.
    fn needed(self) -> Vec<&'static [&'static str]> {
        let mut out: Vec<&'static [&'static str]> = Vec::new();
        if self.weather {
            out.push(&["read_station"]);
        }
        if self.thermostats {
            out.push(&["read_thermostat", "read_smarther"]);
        }
        if self.lights {
            out.push(&["read_magellan", "read_mhs1"]);
        }
        if self.cameras {
            out.push(&["access_camera", "access_presence"]);
        }
        if self.control && self.thermostats {
            out.push(&["write_thermostat", "write_smarther"]);
        }
        if self.control && self.lights {
            out.push(&["write_magellan", "write_mhs1"]);
        }
        out
    }
}

/// Refuses a token with more than the chosen features need, or too little for them.
/// A reply that lists no scopes cannot be judged and is accepted.
fn check_scopes(reply: &Value, features: Features) -> Result<(), String> {
    let scopes: Vec<&str> = match &reply["scope"] {
        Value::Array(list) => list.iter().map(|s| s.as_str().unwrap_or("unknown")).collect(),
        Value::String(s) => s.split([' ', ',']).filter(|x| !x.is_empty()).collect(),
        _ => return Ok(()),
    };
    if scopes.is_empty() {
        return Ok(());
    }
    let allowed = features.allowed();
    let extra: Vec<&str> = scopes.iter().copied().filter(|s| !allowed.contains(s)).collect();
    if !extra.is_empty() {
        return Err(format!("This token has more access than your choices need ({}). Generate a new one without those scopes and paste it here.", extra.join(", ")));
    }
    let missing: Vec<&str> = features.needed().iter().filter(|any| !any.iter().any(|n| scopes.contains(n))).map(|any| any[0]).collect();
    if !missing.is_empty() {
        return Err(format!("This token lacks what your choices need ({}). Generate a new one with those scopes and paste it here.", missing.join(", ")));
    }
    Ok(())
}

/// Where a command for an entity goes. Camera addresses stay here, never in an entity.
#[derive(Clone, Debug, PartialEq)]
enum Target {
    Room { home: String, room: String },
    Module { home: String, id: String, bridge: Option<String> },
    Camera { url: Option<String> },
}

/// Only Netatmo's own video relay, over HTTPS, may be fetched for a camera.
fn camera_url(url: &str) -> Option<String> {
    let t = parse_url(url).ok()?;
    (t.https && t.host.ends_with(".netatmo.net") && !url.contains(['?', '#', '\r', '\n', ' '])).then(|| url.trim_end_matches('/').to_string())
}

struct Api {
    base: String,
    client_id: String,
    client_secret: String,
    pasted: String,
    stored: String,
    features: Features,
    access: Mutex<String>,
    targets: Mutex<HashMap<String, Target>>,
    camera_checked: Mutex<Option<Instant>>,
    sink: Sink,
}

impl Api {
    fn refresh(&self) -> Result<(), String> {
        let mut tokens = TOKENS.lock().unwrap();
        let current = tokens.get(&self.pasted).unwrap_or(&self.stored);
        let form = format!("grant_type=refresh_token&refresh_token={}&client_id={}&client_secret={}", percent_encode(current), percent_encode(&self.client_id), percent_encode(&self.client_secret));
        let res = request(&self.base, "POST", "/oauth2/token", &[("Content-Type", "application/x-www-form-urlencoded;charset=UTF-8")], Some(form.as_bytes()))?;
        if res.status != 200 {
            let reason = res.json().ok().and_then(|v: Value| v["error"].as_str().map(str::to_string)).unwrap_or_else(|| format!("HTTP {}", res.status));
            return Err(if reason == "invalid_grant" {
                "Netatmo no longer accepts the refresh token. Generate a new one at dev.netatmo.com and paste it here.".into()
            } else {
                format!("Netatmo refused the sign-in: {reason}. Check the client ID and secret.")
            });
        }
        let v: Value = res.json()?;
        let (Some(access), Some(next)) = (v["access_token"].as_str(), v["refresh_token"].as_str()) else {
            return Err("unexpected token reply from Netatmo".into());
        };
        tokens.insert(self.pasted.clone(), next.to_string());
        (self.sink)(Event::Remember { key: "_token".into(), value: json!({ "from": self.pasted, "token": next }) });
        // Checked after keeping the rotation: the previous token is already dead.
        if let Err(e) = check_scopes(&v, self.features) {
            self.access.lock().unwrap().clear();
            return Err(e);
        }
        *self.access.lock().unwrap() = access.to_string();
        Ok(())
    }

    fn send(&self, method: &str, path: &str, kind: Option<&str>, body: Option<&[u8]>) -> Result<crate::client::HttpResponse, String> {
        let auth = format!("Bearer {}", self.access.lock().unwrap());
        let mut headers = vec![("Authorization", auth.as_str())];
        if let Some(k) = kind {
            headers.push(("Content-Type", k));
        }
        request(&self.base, method, path, &headers, body)
    }

    /// One call, signing in again once if the access token has run out.
    fn call(&self, method: &str, path: &str, kind: Option<&str>, body: Option<&[u8]>) -> Result<Value, String> {
        let mut res = self.send(method, path, kind, body)?;
        // 403 with code 26 is throttling; a new token would not help.
        let throttled = res.status == 403 && res.json().is_ok_and(|v: Value| v["error"]["code"] == 26);
        if matches!(res.status, 401 | 403) && !throttled {
            self.refresh()?;
            res = self.send(method, path, kind, body)?;
        }
        if res.status != 200 {
            return Err(format!("Netatmo answered HTTP {}", res.status));
        }
        res.json()
    }

    fn entities(&self) -> Result<Vec<Entity>, String> {
        let mut out = Vec::new();
        if self.features.weather {
            out.extend(map(&self.call("GET", "/api/getstationsdata", None, None)?));
        }
        if self.features.homes() {
            let homes = self.call("GET", "/api/homesdata", None, None)?;
            let mut statuses = HashMap::new();
            for id in homes["body"]["homes"].as_array().into_iter().flatten().take(MAX_HOMES).filter_map(|h| h["id"].as_str()) {
                statuses.insert(id.to_string(), self.call("GET", &format!("/api/homestatus?home_id={}", percent_encode(id)), None, None)?);
            }
            let (list, targets) = map_homes(&homes, &statuses, self.features);
            out.extend(list);
            *self.targets.lock().unwrap() = targets;
        }
        Ok(out)
    }

    fn set_module(&self, home: &str, id: &str, bridge: &Option<String>, field: &str, value: Value) -> Result<(), String> {
        let mut module = json!({ "id": id });
        module[field] = value;
        if let Some(b) = bridge {
            module["bridge"] = json!(b);
        }
        let body = json!({ "home": { "id": home, "modules": [module] } }).to_string();
        let reply = self.call("POST", "/api/setstate", Some("application/json"), Some(body.as_bytes()))?;
        if reply["status"] != "ok" || reply["body"]["errors"].as_array().is_some_and(|e| !e.is_empty()) {
            return Err("Netatmo did not accept the change".into());
        }
        Ok(())
    }

    fn set_room(&self, home: &str, room: &str, temp: f64) -> Result<(), String> {
        let form = format!("home_id={}&room_id={}&mode=manual&temp={temp}", percent_encode(home), percent_encode(room));
        let reply = self.call("POST", "/api/setroomthermpoint", Some("application/x-www-form-urlencoded;charset=UTF-8"), Some(form.as_bytes()))?;
        if reply["status"] != "ok" {
            return Err("Netatmo did not accept the temperature".into());
        }
        Ok(())
    }

    fn picture(&self, url: &str) -> Result<Option<(String, Vec<u8>)>, String> {
        let res = request_public(url, "GET", "/live/snapshot_720.jpg", &[("Accept", "image/jpeg")], None)?;
        Ok((res.status == 200).then(|| (res.headers.get("content-type").cloned().unwrap_or_default(), res.body)))
    }
}

/// Thermostats, lights, sockets and cameras of every home, from `/homesdata` and `/homestatus`.
/// Only the product types on the lists above become entities: anything new stays out.
fn map_homes(homes: &Value, statuses: &HashMap<String, Value>, f: Features) -> (Vec<Entity>, HashMap<String, Target>) {
    let (mut out, mut targets) = (Vec::new(), HashMap::new());
    let safe = |s: &str| s.replace(':', "-");
    let number = |v: &Value| if v.is_number() { v.clone() } else { Value::Null };
    for home in homes["body"]["homes"].as_array().into_iter().flatten().take(MAX_HOMES) {
        let Some(home_id) = home["id"].as_str() else { continue };
        let Some(status) = statuses.get(home_id).map(|s| &s["body"]["home"]) else { continue };
        let home_name = home["name"].as_str().unwrap_or("");
        let room_name = |id: &str| home["rooms"].as_array().into_iter().flatten().find(|r| r["id"] == id).and_then(|r| r["name"].as_str());
        let about = |id: &str| home["modules"].as_array().into_iter().flatten().find(|m| m["id"] == id);
        if f.thermostats {
            for r in status["rooms"].as_array().into_iter().flatten() {
                let (Some(room), true) = (r["id"].as_str(), r["therm_setpoint_temperature"].is_number()) else { continue };
                let id = format!("room-{}-{}", safe(home_id), safe(room));
                let target = Capability { units: Some("°C".into()), min: Some(7.0), max: Some(30.0), step: Some(0.5), ..Capability::new(r["therm_setpoint_temperature"].clone(), f.control) };
                let measured = Capability { units: Some("°C".into()), ..Capability::new(number(&r["therm_measured_temperature"]), false) };
                let capabilities = BTreeMap::from([("target_temperature".to_string(), target), ("measure_temperature".to_string(), measured)]);
                out.push(Entity { id: id.clone(), name: room_name(room).unwrap_or("Room").to_string(), zone: home_name.to_string(), class: "thermostat".into(), capabilities });
                targets.insert(id, Target::Room { home: home_id.to_string(), room: room.to_string() });
            }
        }
        for m in status["modules"].as_array().into_iter().flatten() {
            let (Some(mid), Some(kind)) = (m["id"].as_str(), m["type"].as_str()) else { continue };
            let meta = about(mid);
            let name = meta.and_then(|x| x["name"].as_str()).unwrap_or(kind).to_string();
            let zone = meta.and_then(|x| x["room_id"].as_str()).and_then(room_name).unwrap_or(home_name).to_string();
            let id = format!("mod-{}", safe(mid));
            let is_light = LIGHTS.contains(&kind);
            if f.lights && (is_light || SOCKETS.contains(&kind)) && m["on"].is_boolean() {
                let mut capabilities = BTreeMap::from([("onoff".to_string(), Capability::new(m["on"].clone(), f.control))]);
                if let (true, Some(b)) = (is_light, m["brightness"].as_f64()) {
                    capabilities.insert("dim".into(), Capability { min: Some(0.0), max: Some(1.0), ..Capability::new(json!((b / 100.0).clamp(0.0, 1.0)), f.control) });
                }
                let bridge = m["bridge"].as_str().or(meta.and_then(|x| x["bridge"].as_str())).map(str::to_string);
                out.push(Entity { id: id.clone(), name, zone, class: if is_light { "light" } else { "socket" }.into(), capabilities });
                targets.insert(id, Target::Module { home: home_id.to_string(), id: mid.to_string(), bridge });
            } else if f.cameras && CAMERAS.contains(&kind) {
                let capabilities = BTreeMap::from([("camera".to_string(), Capability::new(Value::Null, false))]);
                out.push(Entity { id: id.clone(), name, zone, class: "camera".into(), capabilities });
                targets.insert(id, Target::Camera { url: m["vpn_url"].as_str().and_then(camera_url) });
            }
        }
    }
    (out, targets)
}

/// (Netatmo's key, capability, units) for each module type.
fn fields(kind: &str) -> &'static [(&'static str, &'static str, &'static str)] {
    match kind {
        "NAMain" => &[("Temperature", "measure_temperature", "°C"), ("min_temp", "temperature_min", "°C"), ("max_temp", "temperature_max", "°C"), ("Humidity", "measure_humidity", "%"), ("CO2", "measure_co2", "ppm"), ("Noise", "measure_noise", "dB"), ("Pressure", "measure_pressure", "mbar")],
        "NAModule1" => &[("Temperature", "measure_temperature", "°C"), ("min_temp", "temperature_min", "°C"), ("max_temp", "temperature_max", "°C"), ("Humidity", "measure_humidity", "%")],
        "NAModule2" => &[("WindStrength", "measure_wind_strength", "km/h"), ("GustStrength", "measure_gust_strength", "km/h"), ("WindAngle", "measure_wind_angle", "°")],
        "NAModule3" => &[("Rain", "measure_rain", "mm"), ("sum_rain_24", "meter_rain", "mm")],
        "NAModule4" => &[("Temperature", "measure_temperature", "°C"), ("min_temp", "temperature_min", "°C"), ("max_temp", "temperature_max", "°C"), ("Humidity", "measure_humidity", "%"), ("CO2", "measure_co2", "ppm")],
        _ => &[],
    }
}

/// An unreachable module has no `dashboard_data`: its readings go null and its tile stays.
fn module(m: &Value, zone: &str) -> Option<Entity> {
    let kind = m["type"].as_str()?;
    let list = fields(kind);
    if list.is_empty() {
        return None;
    }
    let capabilities: BTreeMap<String, Capability> = list.iter().map(|(key, cap, units)| {
        let reading = &m["dashboard_data"][*key];
        let value = if reading.is_number() { reading.clone() } else { Value::Null };
        ((*cap).to_string(), Capability { units: Some((*units).to_string()), ..Capability::new(value, false) })
    }).collect();
    // MAC addresses: the rest of oldpad uses ':' between integration and entity.
    let id = m["_id"].as_str()?.replace(':', "-");
    Some(Entity { id, name: m["module_name"].as_str().unwrap_or(kind).to_string(), zone: zone.to_string(), class: "sensor".into(), capabilities })
}

/// The account's own and shared stations; favourites are other people's public ones.
fn map(data: &Value) -> Vec<Entity> {
    let mut out = Vec::new();
    for d in data["body"]["devices"].as_array().into_iter().flatten().filter(|d| d["favorite"] != true) {
        let zone = d["home_name"].as_str().or(d["station_name"].as_str()).unwrap_or("");
        out.extend(module(d, zone));
        out.extend(d["modules"].as_array().into_iter().flatten().filter_map(|m| module(m, zone)));
    }
    out
}

/// Makes no call: a refresh here would burn the token the running instance or the
/// save that follows depends on, and `test` has no way to keep the new one.
fn test(_: &Map<String, Value>) -> Result<String, String> {
    Ok("Not contacted yet: Netatmo tokens work only once, so the connection is checked when you save. Its status will show any problem.".into())
}

struct Netatmo {
    api: Arc<Api>,
    entities: Arc<Mutex<Vec<Entity>>>,
    stopped: Arc<AtomicBool>,
    sink: Sink,
}

fn start(config: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let api = Arc::new(Api {
        base: API.to_string(),
        client_id: get_str(config, "client_id").to_string(),
        client_secret: get_str(config, "client_secret").to_string(),
        pasted: get_str(config, "refresh_token").to_string(),
        stored: stored_token(config),
        features: Features::from(config),
        access: Mutex::new(String::new()),
        targets: Mutex::new(HashMap::new()),
        camera_checked: Mutex::new(None),
        sink: sink.clone(),
    });
    api.refresh()?;
    let entities = Arc::new(Mutex::new(api.entities()?));
    let stopped = Arc::new(AtomicBool::new(false));
    let (shared, stop, poller, events) = (entities.clone(), stopped.clone(), api.clone(), sink.clone());
    std::thread::spawn(move || {
        let mut polled = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(2));
            if polled.elapsed() < POLL {
                continue;
            }
            polled = Instant::now();
            match poller.entities() {
                Ok(fresh) => {
                    publish(&shared, fresh, &events);
                    events(Event::Healthy(true));
                }
                Err(e) => {
                    eprintln!("netatmo: {e}");
                    events(Event::Healthy(false));
                }
            }
        }
    });
    Ok(Box::new(Netatmo { api, entities, stopped, sink }))
}

fn publish(shared: &Mutex<Vec<Entity>>, fresh: Vec<Entity>, sink: &Sink) {
    let mut list = shared.lock().unwrap();
    let same_shape = list.len() == fresh.len() && list.iter().zip(&fresh).all(|(a, b)| a.id == b.id && a.name == b.name && a.zone == b.zone);
    let changes: Vec<(String, String, Value)> = if same_shape {
        list.iter().zip(&fresh).flat_map(|(old, new)| new.capabilities.iter().filter(|(k, c)| old.capabilities.get(*k).is_none_or(|o| o.value != c.value)).map(|(k, c)| (new.id.clone(), k.clone(), c.value.clone()))).collect()
    } else {
        vec![]
    };
    *list = fresh;
    drop(list);
    if !same_shape {
        sink(Event::Reloaded);
    }
    for (entity, capability, value) in changes {
        sink(Event::Changed { entity, capability, value });
    }
}

impl Instance for Netatmo {
    fn entities(&self) -> Vec<Entity> {
        self.entities.lock().unwrap().clone()
    }
    fn set(&self, entity: &str, capability: &str, value: &Value) -> Result<(), String> {
        if !self.api.features.control {
            return Err("Control is turned off for this integration".into());
        }
        let target = self.api.targets.lock().unwrap().get(entity).cloned().ok_or("no such device")?;
        let shown = match (&target, capability) {
            (Target::Module { home, id, bridge }, "onoff") => {
                let on = value.as_bool().ok_or("expected on or off")?;
                self.api.set_module(home, id, bridge, "on", json!(on))?;
                json!(on)
            }
            (Target::Module { home, id, bridge }, "dim") => {
                let level = value.as_f64().filter(|v| (0.0..=1.0).contains(v)).ok_or("expected 0 to 1")?;
                self.api.set_module(home, id, bridge, "brightness", json!((level * 100.0).round() as i64))?;
                json!(level)
            }
            (Target::Room { home, room }, "target_temperature") => {
                let temp = value.as_f64().filter(|v| (7.0..=30.0).contains(v)).ok_or("expected 7 to 30 degrees")?;
                let temp = (temp * 2.0).round() / 2.0;
                self.api.set_room(home, room, temp)?;
                json!(temp)
            }
            _ => return Err("this device cannot do that".into()),
        };
        if let Some(c) = self.entities.lock().unwrap().iter_mut().find(|e| e.id == entity).and_then(|e| e.capabilities.get_mut(capability)) {
            c.value = shown.clone();
        }
        (self.sink)(Event::Changed { entity: entity.to_string(), capability: capability.to_string(), value: shown });
        Ok(())
    }
    fn snapshot(&self, entity: &str) -> Result<(String, Vec<u8>), String> {
        let url = || match self.api.targets.lock().unwrap().get(entity) {
            Some(Target::Camera { url }) => url.clone(),
            _ => None,
        };
        if let Some(address) = url()
            && let Some(picture) = self.api.picture(&address)?
        {
            return Ok(picture);
        }
        // The address expires after a few hours or when the camera restarts.
        let due = {
            let mut checked = self.api.camera_checked.lock().unwrap();
            let due = checked.is_none_or(|t| t.elapsed() >= CAMERA_RELOAD);
            if due {
                *checked = Some(Instant::now());
            }
            due
        };
        if due {
            self.api.entities()?;
            if let Some(address) = url()
                && let Some(picture) = self.api.picture(&address)?
            {
                return Ok(picture);
            }
        }
        Err("the camera is not available right now".into())
    }
    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_a_station_and_its_modules() {
        let list = map(&json!({"body": {"devices": [
            {"_id": "70:ee:50:00:00:01", "type": "NAMain", "module_name": "Stue", "home_name": "Hjemme",
             "dashboard_data": {"Temperature": 21.5, "CO2": 640, "Humidity": 40, "Noise": 35, "Pressure": 1012.3, "min_temp": 20.1, "max_temp": 22.0},
             "modules": [
                {"_id": "02:00:00:00:00:02", "type": "NAModule1", "module_name": "Ute", "dashboard_data": {"Temperature": -3.2, "Humidity": 80}},
                {"_id": "06:00:00:00:00:03", "type": "NAModule2", "module_name": "Vind", "dashboard_data": {"WindStrength": 12, "GustStrength": 20, "WindAngle": 180}},
                {"_id": "05:00:00:00:00:04", "type": "NAModule3", "module_name": "Regn", "reachable": false},
                {"_id": "09:00:00:00:00:05", "type": "Unknown"}
             ]},
            {"_id": "70:ee:50:00:00:09", "type": "NAMain", "favorite": true, "dashboard_data": {"Temperature": 1}}
        ]}}));
        assert_eq!(list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Stue", "Ute", "Vind", "Regn"]);
        assert_eq!((list[0].id.as_str(), list[0].zone.as_str(), list[0].class.as_str()), ("70-ee-50-00-00-01", "Hjemme", "sensor"));
        assert_eq!(list[0].capabilities["measure_co2"].value, json!(640));
        assert_eq!(list[1].capabilities["measure_temperature"].value, json!(-3.2));
        assert_eq!(list[2].capabilities["measure_wind_strength"].units.as_deref(), Some("km/h"));
        assert!(list[3].capabilities["measure_rain"].value.is_null(), "an unreachable module keeps its tile");
        assert!(list.iter().flat_map(|e| e.capabilities.values()).all(|c| !c.settable));
    }

    fn weather() -> Features {
        Features { weather: true, ..Default::default() }
    }

    #[test]
    fn a_token_holds_exactly_what_the_choices_need() {
        assert!(check_scopes(&json!({"scope": ["read_station"]}), weather()).is_ok());
        assert!(check_scopes(&json!({"scope": "read_station"}), weather()).is_ok());
        assert!(check_scopes(&json!({}), weather()).is_ok(), "a reply without scopes cannot be judged");
        let broad = check_scopes(&json!({"scope": ["read_station", "read_camera", "write_thermostat"]}), weather()).unwrap_err();
        assert!(broad.contains("read_camera, write_thermostat"), "{broad}");
        assert!(check_scopes(&json!({"scope": [7]}), weather()).is_err());
        let read = Features { weather: true, thermostats: true, ..Default::default() };
        assert!(check_scopes(&json!({"scope": ["read_station", "read_thermostat"]}), read).is_ok());
        assert!(check_scopes(&json!({"scope": ["read_station", "read_thermostat", "write_thermostat"]}), read).is_err(), "no write without control");
        assert!(check_scopes(&json!({"scope": ["read_station"]}), read).unwrap_err().contains("read_thermostat"));
        let control = Features { control: true, ..read };
        assert!(check_scopes(&json!({"scope": ["read_station", "read_thermostat"]}), control).unwrap_err().contains("write_thermostat"));
        assert!(check_scopes(&json!({"scope": ["read_station", "read_thermostat", "write_thermostat"]}), control).is_ok());
        let cams = Features { cameras: true, ..Default::default() };
        assert!(check_scopes(&json!({"scope": ["read_camera"]}), cams).unwrap_err().contains("access_camera"));
        assert!(check_scopes(&json!({"scope": ["read_camera", "access_camera"]}), cams).is_ok());
    }

    #[test]
    fn old_configs_stay_weather_only() {
        let f = Features::from(json!({"client_id": "c"}).as_object().unwrap());
        assert_eq!(f, weather());
        assert_eq!(f.allowed(), ["read_station"]);
    }

    #[test]
    fn camera_addresses_must_be_netatmos() {
        assert!(camera_url("https://prodvpn-eu-2.netatmo.net/10.255.0.1/abc/token,,").is_some());
        for bad in ["http://prodvpn.netatmo.net/a", "https://netatmo.net.evil.example/a", "https://evil.example/.netatmo.net/a", "https://192.168.1.5/a", "https://x.netatmo.net/a?b=c"] {
            assert_eq!(camera_url(bad), None, "{bad}");
        }
    }

    fn sample_homes() -> (Value, HashMap<String, Value>) {
        let homes = json!({"body": {"homes": [{"id": "h1", "name": "Hjemme",
            "rooms": [{"id": "r1", "name": "Stue"}, {"id": "r2", "name": "Gang"}],
            "modules": [
                {"id": "aa:01", "type": "NLF", "name": "Taklampe", "room_id": "r1", "bridge": "bb:01"},
                {"id": "aa:02", "type": "NLP", "name": "Stikk", "room_id": "r2", "bridge": "bb:01"},
                {"id": "aa:03", "type": "NLV", "name": "Rullegardin", "room_id": "r1"},
                {"id": "aa:04", "type": "NIS", "name": "Sirene"},
                {"id": "aa:05", "type": "NLPO", "name": "Garasjeport"},
                {"id": "aa:06", "type": "NACamera", "name": "Inne", "room_id": "r1"}
            ]}]}});
        let status = json!({"body": {"home": {"id": "h1",
            "rooms": [{"id": "r1", "therm_measured_temperature": 20.5, "therm_setpoint_temperature": 21}, {"id": "r2"}],
            "modules": [
                {"id": "aa:01", "type": "NLF", "on": true, "brightness": 40, "bridge": "bb:01"},
                {"id": "aa:02", "type": "NLP", "on": false, "bridge": "bb:01"},
                {"id": "aa:03", "type": "NLV", "current_position": 0, "target_position": 0},
                {"id": "aa:04", "type": "NIS", "status": "no_sound", "on": false},
                {"id": "aa:05", "type": "NLPO", "on": false},
                {"id": "aa:06", "type": "NACamera", "vpn_url": "https://prodvpn-eu-2.netatmo.net/secret/token,,"}
            ]}}});
        (homes, HashMap::from([("h1".to_string(), status)]))
    }

    #[test]
    fn homes_map_only_to_the_chosen_safe_devices() {
        let (homes, statuses) = sample_homes();
        let all = Features { thermostats: true, lights: true, cameras: true, control: true, ..Default::default() };
        let (list, targets) = map_homes(&homes, &statuses, all);
        assert_eq!(list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Stue", "Taklampe", "Stikk", "Inne"], "no shutter, siren or contactor");
        let lamp = &list[1];
        assert_eq!((lamp.class.as_str(), lamp.zone.as_str(), lamp.capabilities["dim"].value.as_f64()), ("light", "Stue", Some(0.4)));
        assert!(lamp.capabilities["onoff"].settable);
        assert_eq!(list[3].class, "camera");
        assert!(!serde_json::to_string(&list).unwrap().contains("secret"), "the camera address never reaches a screen");
        assert!(matches!(&targets["mod-aa-06"], Target::Camera { url: Some(u) } if u.contains("secret")));
        let (read_only, _) = map_homes(&homes, &statuses, Features { control: false, ..all });
        assert!(read_only.iter().flat_map(|e| e.capabilities.values()).all(|c| !c.settable));
        let (only_lights, _) = map_homes(&homes, &statuses, Features { lights: true, ..Default::default() });
        assert_eq!(only_lights.len(), 2);
    }

    /// A small Netatmo: answers the calls and records what was sent.
    fn fake_cloud(scope: Value) -> (String, Arc<Mutex<Vec<String>>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let (homes, statuses) = sample_homes();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut s) = conn else { continue };
                let (mut raw, mut b) = (Vec::new(), [0u8; 4096]);
                loop {
                    let n = s.read(&mut b).unwrap_or(0);
                    raw.extend(&b[..n]);
                    let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
                        if n == 0 { break }
                        continue;
                    };
                    let head = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
                    let len: usize = head.lines().find_map(|l| l.strip_prefix("content-length: ")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                    if raw.len() >= end + 4 + len || n == 0 { break }
                }
                let text = String::from_utf8_lossy(&raw).into_owned();
                let line = text.lines().next().unwrap_or("").to_string();
                let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                log.lock().unwrap().push(format!("{line} {body}"));
                let reply = if line.contains("/oauth2/token") {
                    json!({"access_token": "a1", "refresh_token": "r2", "scope": scope})
                } else if line.contains("/api/homesdata") {
                    homes.clone()
                } else if line.contains("/api/homestatus") {
                    statuses["h1"].clone()
                } else {
                    json!({"status": "ok", "body": {}})
                }.to_string();
                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
            }
        });
        (base, seen)
    }

    fn instance(base: String, features: Features, pasted: &str) -> (Netatmo, Arc<Mutex<Vec<Event>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let log = events.clone();
        let sink: Sink = Arc::new(move |e| log.lock().unwrap().push(e));
        let api = Arc::new(Api { base, client_id: "c".into(), client_secret: "s".into(), pasted: pasted.into(), stored: "r1".into(), features, access: Mutex::new(String::new()), targets: Mutex::new(HashMap::new()), camera_checked: Mutex::new(None), sink: sink.clone() });
        api.refresh().unwrap();
        let entities = Arc::new(Mutex::new(api.entities().unwrap()));
        (Netatmo { api, entities, stopped: Arc::new(AtomicBool::new(false)), sink }, events)
    }

    #[test]
    fn commands_reach_netatmo_and_update_the_screen() {
        let f = Features { thermostats: true, lights: true, control: true, ..Default::default() };
        let (base, seen) = fake_cloud(json!(["read_thermostat", "write_thermostat", "read_magellan", "write_magellan"]));
        let (n, _) = instance(base, f, "pasted-commands");
        n.set("mod-aa-01", "onoff", &json!(false)).unwrap();
        n.set("mod-aa-01", "dim", &json!(0.75)).unwrap();
        n.set("room-h1-r1", "target_temperature", &json!(22.3)).unwrap();
        let log = seen.lock().unwrap().join("\n");
        assert!(log.contains(r#""bridge":"bb:01","id":"aa:01","on":false"#), "{log}");
        assert!(log.contains(r#""brightness":75"#), "{log}");
        assert!(log.contains("home_id=h1&room_id=r1&mode=manual&temp=22.5"), "{log}");
        assert_eq!(n.entities().iter().find(|e| e.id == "room-h1-r1").unwrap().capabilities["target_temperature"].value, json!(22.5));
        assert!(n.set("room-h1-r1", "target_temperature", &json!(99)).is_err());
        assert!(n.set("mod-aa-01", "target_temperature", &json!(21)).is_err());
        assert!(n.set("mod-aa-03", "onoff", &json!(true)).is_err(), "a shutter has no target");
    }

    #[test]
    fn read_only_sends_no_commands() {
        let f = Features { lights: true, ..Default::default() };
        let (base, seen) = fake_cloud(json!(["read_magellan"]));
        let (n, _) = instance(base, f, "pasted-readonly");
        assert!(n.set("mod-aa-01", "onoff", &json!(false)).is_err());
        assert!(!seen.lock().unwrap().iter().any(|l| l.contains("/api/setstate")));
    }

    #[test]
    fn readings_must_be_numbers() {
        let e = module(&json!({"_id": "70:ee", "type": "NAMain", "dashboard_data": {"Temperature": "<img src=x>", "CO2": {"x": 1}, "Humidity": 40}}), "").unwrap();
        assert!(e.capabilities["measure_temperature"].value.is_null());
        assert!(e.capabilities["measure_co2"].value.is_null());
        assert_eq!(e.capabilities["measure_humidity"].value, json!(40));
    }

    #[test]
    fn removed_integrations_leave_no_token_in_memory() {
        let app = crate::app::tests::test_app();
        let config = json!({"refresh_token": "kept"}).as_object().unwrap().clone();
        app.store.lock().unwrap().data.integrations.push(crate::store::IntegrationConfig { id: "n".into(), kind: "netatmo".into(), name: "N".into(), config });
        {
            let mut t = TOKENS.lock().unwrap();
            t.insert("kept".into(), "rotated".into());
            t.insert("removed".into(), "rotated".into());
        }
        app.forget_unused_tokens();
        let t = TOKENS.lock().unwrap();
        assert!(t.contains_key("kept") && !t.contains_key("removed"));
    }

    #[test]
    fn a_newly_pasted_token_beats_the_remembered_one() {
        let c = |v: Value| v.as_object().unwrap().clone();
        assert_eq!(stored_token(&c(json!({"refresh_token": "a"}))), "a");
        assert_eq!(stored_token(&c(json!({"refresh_token": "a", "_token": {"from": "a", "token": "b"}}))), "b");
        assert_eq!(stored_token(&c(json!({"refresh_token": "new", "_token": {"from": "a", "token": "b"}}))), "new");
    }
}
