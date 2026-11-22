//! Home Assistant: live state over its WebSocket API, actions through its REST API.
//! Locks, covers and alarms are left out on purpose: a wall tablet should not open the house.

use super::{get_str, Capability, Entity, Event, Field, FieldKind, Instance, Kind, Sink};
use crate::client::{request, WebSocket};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn kind() -> Kind {
    Kind {
        id: "homeassistant",
        name: "Home Assistant",
        description: "Lights, switches, climate, sensors, media players, cameras, scenes and scripts from Home Assistant.",
        fields: vec![
            Field {
                key: "address",
                label: "Home Assistant address",
                kind: FieldKind::Url,
                required: true,
                placeholder: "http://192.168.1.60:8123",
                help: "Its address. Use https:// if it has a certificate from a public authority.",
            },
            Field {
                key: "token",
                label: "Long-lived access token",
                kind: FieldKind::Secret,
                required: true,
                placeholder: "",
                help: "Home Assistant → your profile → Security → Long-lived access tokens. The token can do what its user can, so use a separate user.",
            },
        ],
        test,
        start,
    }
}

struct Api {
    base: String,
    token: String,
}

impl Api {
    fn new(config: &Map<String, Value>) -> Self {
        Api { base: get_str(config, "address").to_string(), token: get_str(config, "token").to_string() }
    }

    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
        let auth = format!("Bearer {}", self.token);
        let body = body.map(|b| b.to_string().into_bytes());
        let res = request(&self.base, method, path, &[("Authorization", &auth)], body.as_deref())?;
        match res.status {
            200..=299 => Ok(if res.body.is_empty() { Value::Null } else { res.json()? }),
            401 | 403 => Err(format!("Home Assistant refused the token for {path} (HTTP {}).", res.status)),
            s => Err(format!("Home Assistant answered HTTP {s} for {path}: {}", String::from_utf8_lossy(&res.body).chars().take(200).collect::<String>())),
        }
    }
}

fn next(ws: &mut WebSocket, by: Instant) -> Result<Value, String> {
    while Instant::now() < by {
        if let Some(m) = ws.recv()? {
            return serde_json::from_str(&m).map_err(|e| format!("invalid JSON from Home Assistant: {e}"));
        }
    }
    Err("timed out waiting for Home Assistant".into())
}

/// An authenticated socket and the Home Assistant version.
fn open(api: &Api) -> Result<(WebSocket, String), String> {
    let mut ws = WebSocket::connect(&api.base, "/api/websocket")?;
    ws.set_poll_interval(Duration::from_secs(1));
    let by = Instant::now() + Duration::from_secs(10);
    if next(&mut ws, by)?["type"] != "auth_required" {
        return Err("this address does not look like Home Assistant".into());
    }
    ws.send(&json!({ "type": "auth", "access_token": api.token }).to_string())?;
    let reply = next(&mut ws, by)?;
    match reply["type"].as_str() {
        Some("auth_ok") => Ok((ws, reply["ha_version"].as_str().unwrap_or("?").to_string())),
        Some("auth_invalid") => Err("Home Assistant refused the access token.".into()),
        _ => Err("unexpected reply to Home Assistant sign-in".into()),
    }
}

/// Sends a command and waits for its result; events arriving meanwhile are covered by the state read after it.
fn command(ws: &mut WebSocket, msg: Value) -> Result<Value, String> {
    ws.send(&msg.to_string())?;
    let by = Instant::now() + Duration::from_secs(30);
    loop {
        let m = next(ws, by)?;
        if m["type"] == "result" && m["id"] == msg["id"] {
            return if m["success"] == true { Ok(m["result"].clone()) } else { Err(format!("Home Assistant: {} failed: {}", msg["type"], m["error"]["message"])) };
        }
    }
}

/// Which room each entity is in, and which entities the user keeps out of dashboards.
struct Rooms {
    zones: HashMap<String, String>,
    hidden: HashSet<String>,
}

impl Rooms {
    fn new(areas: &Value, devices: &Value, entities: &Value) -> Self {
        let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
        let names: HashMap<String, String> = list(areas).iter().filter_map(|a| Some((a["area_id"].as_str()?.to_string(), a["name"].as_str()?.to_string()))).collect();
        let device_area: HashMap<String, String> = list(devices).iter().filter_map(|d| Some((d["id"].as_str()?.to_string(), d["area_id"].as_str()?.to_string()))).collect();
        let (mut zones, mut hidden) = (HashMap::new(), HashSet::new());
        for e in list(entities) {
            let Some(id) = e["entity_id"].as_str() else { continue };
            if !e["hidden_by"].is_null() || !e["disabled_by"].is_null() || !e["entity_category"].is_null() {
                hidden.insert(id.to_string());
            }
            // An entity's own area overrides its device's.
            let area = e["area_id"].as_str().or_else(|| device_area.get(e["device_id"].as_str()?).map(String::as_str));
            if let Some(name) = area.and_then(|a| names.get(a)) {
                zones.insert(id.to_string(), name.clone());
            }
        }
        Rooms { zones, hidden }
    }

    fn map(&self, state: &Value) -> Option<Entity> {
        let id = state["entity_id"].as_str()?;
        if self.hidden.contains(id) {
            return None;
        }
        map_state(state, self.zones.get(id).cloned().unwrap_or_default())
    }
}

fn map_state(s: &Value, zone: String) -> Option<Entity> {
    let id = s["entity_id"].as_str()?;
    let (domain, _) = id.split_once('.')?;
    let (state, a) = (s["state"].as_str()?, &s["attributes"]);
    let mut caps = BTreeMap::new();
    let class = match domain {
        "light" => {
            caps.insert("onoff".to_string(), Capability::new(json!(state == "on"), true));
            if a["supported_color_modes"].as_array().is_some_and(|m| m.iter().any(|x| x != "onoff")) {
                let dim = a["brightness"].as_f64().unwrap_or(0.0) / 255.0;
                caps.insert("dim".to_string(), Capability { min: Some(0.0), max: Some(1.0), step: Some(0.01), ..Capability::new(json!(dim), true) });
            }
            "light"
        }
        "switch" | "input_boolean" => {
            caps.insert("onoff".to_string(), Capability::new(json!(state == "on"), true));
            "socket"
        }
        "climate" => {
            // Read-only: not every thermostat has turn_on/turn_off; the mode control switches it.
            caps.insert("onoff".to_string(), Capability::new(json!(state != "off"), false));
            if let Some(t) = a["temperature"].as_f64() {
                caps.insert(
                    "target_temperature".to_string(),
                    Capability { min: a["min_temp"].as_f64(), max: a["max_temp"].as_f64(), step: a["target_temp_step"].as_f64(), units: a["temperature_unit"].as_str().map(str::to_string), ..Capability::new(json!(t), true) },
                );
            }
            if let Some(t) = a["current_temperature"].as_f64() {
                caps.insert("measure_temperature".to_string(), Capability::new(json!(t), false));
            }
            if let Some(h) = a["current_humidity"].as_f64() {
                caps.insert("measure_humidity".to_string(), Capability::new(json!(h), false));
            }
            if let Some(modes) = a["hvac_modes"].as_array() {
                let values: Vec<Value> = modes.iter().filter_map(|m| Some(json!({ "id": m.as_str()?, "title": m.as_str()? }))).collect();
                caps.insert("thermostat_mode".to_string(), Capability { title: Some("Mode".into()), values: Some(json!(values)), ..Capability::new(json!(state), true) });
            }
            "thermostat"
        }
        "sensor" => {
            let cap = match a["device_class"].as_str()? {
                "temperature" => "measure_temperature",
                "humidity" => "measure_humidity",
                "carbon_dioxide" => "measure_co2",
                _ => return None,
            };
            let value = state.parse::<f64>().ok().filter(|v| v.is_finite()).map_or(Value::Null, |v| json!(v));
            caps.insert(cap.to_string(), Capability { units: a["unit_of_measurement"].as_str().map(str::to_string), ..Capability::new(value, false) });
            "sensor"
        }
        "media_player" => {
            caps.insert("speaker_playing".to_string(), Capability::new(json!(state == "playing"), true));
            if let Some(v) = a["volume_level"].as_f64() {
                caps.insert("volume_set".to_string(), Capability { min: Some(0.0), max: Some(1.0), ..Capability::new(json!(v), true) });
            }
            caps.insert("speaker_track".to_string(), Capability::new(a["media_title"].clone(), false));
            caps.insert("speaker_artist".to_string(), Capability::new(a["media_artist"].clone(), false));
            "speaker"
        }
        // No attributes: `entity_picture` holds an access token for the camera.
        "camera" => {
            caps.insert("camera".to_string(), Capability::new(Value::Null, false));
            "camera"
        }
        "scene" | "script" => {
            caps.insert("button".to_string(), Capability::new(Value::Null, true));
            "button"
        }
        _ => return None,
    };
    Some(Entity { id: id.to_string(), name: a["friendly_name"].as_str().unwrap_or(id).to_string(), zone, class: class.to_string(), capabilities: caps })
}

/// The Home Assistant service for a write from a tablet: (domain, service, data).
fn service_call(entity: &str, capability: &str, value: &Value) -> Result<(String, &'static str, Value), String> {
    if !entity.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
        return Err("bad id".into());
    }
    let (domain, _) = entity.split_once('.').ok_or("bad id")?;
    let flag = || value.as_bool().ok_or("expected on or off");
    let unit = || value.as_f64().filter(|v| (0.0..=1.0).contains(v)).ok_or("expected a value from 0 to 1");
    let (service, data) = match (domain, capability) {
        ("scene" | "script", "button") => ("turn_on", json!({})),
        ("light" | "switch" | "input_boolean", "onoff") => (if flag()? { "turn_on" } else { "turn_off" }, json!({})),
        ("light", "dim") => ("turn_on", json!({ "brightness_pct": (unit()? * 100.0).round() })),
        ("climate", "target_temperature") => ("set_temperature", json!({ "temperature": value.as_f64().filter(|v| v.is_finite()).ok_or("expected a temperature")? })),
        ("climate", "thermostat_mode") => ("set_hvac_mode", json!({ "hvac_mode": value.as_str().ok_or("expected a mode")? })),
        ("media_player", "speaker_playing") => (if flag()? { "media_play" } else { "media_pause" }, json!({})),
        ("media_player", "volume_set") => ("volume_set", json!({ "volume_level": unit()? })),
        _ => return Err("not supported".into()),
    };
    Ok((domain.to_string(), service, data))
}

/// Registry commands use ids 5–7; ids 1–4 are the subscriptions of a live session.
fn load(api: &Api, ws: &mut WebSocket) -> Result<(Rooms, Vec<Entity>), String> {
    let areas = command(ws, json!({ "id": 5, "type": "config/area_registry/list" }))?;
    let devices = command(ws, json!({ "id": 6, "type": "config/device_registry/list" }))?;
    let registry = command(ws, json!({ "id": 7, "type": "config/entity_registry/list" }))?;
    let rooms = Rooms::new(&areas, &devices, &registry);
    // Over REST: the full state list of a big install can exceed the WebSocket frame limit.
    let states = api.call("GET", "/api/states", None)?;
    let list = states.as_array().ok_or("unexpected state list from Home Assistant")?.iter().filter_map(|s| rooms.map(s)).collect();
    Ok((rooms, list))
}

fn test(config: &Map<String, Value>) -> Result<String, String> {
    let api = Api::new(config);
    let (mut ws, version) = open(&api)?;
    let (_, list) = load(&api, &mut ws)?;
    Ok(format!("Connected to Home Assistant {version}. {} entities usable.", list.len()))
}

struct HomeAssistant {
    api: Arc<Api>,
    entities: Arc<Mutex<Vec<Entity>>>,
    stopped: Arc<AtomicBool>,
}

fn start(config: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let api = Arc::new(Api::new(config));
    let (mut ws, _) = open(&api)?;
    let entities = Arc::new(Mutex::new(load(&api, &mut ws)?.1));
    let stopped = Arc::new(AtomicBool::new(false));
    let (a, e, stop) = (api.clone(), entities.clone(), stopped.clone());
    std::thread::spawn(move || realtime_loop(&a, &e, &sink, &stop));
    Ok(Box::new(HomeAssistant { api, entities, stopped }))
}

impl Instance for HomeAssistant {
    fn entities(&self) -> Vec<Entity> {
        self.entities.lock().unwrap().clone()
    }

    fn set(&self, entity: &str, capability: &str, value: &Value) -> Result<(), String> {
        let (domain, service, mut data) = service_call(entity, capability, value)?;
        data["entity_id"] = json!(entity);
        self.api.call("POST", &format!("/api/services/{domain}/{service}"), Some(data)).map(drop)
    }

    fn snapshot(&self, entity: &str) -> Result<(String, Vec<u8>), String> {
        if !entity.starts_with("camera.") || !entity.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
            return Err("bad id".into());
        }
        let auth = format!("Bearer {}", self.api.token);
        // Sized for a tablet; a full camera frame is too much for an old iPad.
        let res = request(&self.api.base, "GET", &format!("/api/camera_proxy/{entity}?width=960"), &[("Authorization", &auth), ("Accept", "image/*")], None)?;
        if res.status != 200 {
            return Err(format!("Home Assistant answered HTTP {} for {entity}", res.status));
        }
        Ok((res.headers.get("content-type").cloned().unwrap_or_default(), res.body))
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

/// Takes a `state_changed` event. Returns the values that changed, or None if the list changed shape.
fn apply(list: &mut Vec<Entity>, rooms: &Rooms, data: &Value) -> Option<Vec<(String, Value)>> {
    let id = data["entity_id"].as_str().unwrap_or("");
    let at = list.iter().position(|e| e.id == id);
    match (at, rooms.map(&data["new_state"])) {
        (None, None) => Some(vec![]),
        (Some(i), None) => {
            list.remove(i);
            None
        }
        (None, Some(new)) => {
            list.push(new);
            None
        }
        (Some(i), Some(new)) => {
            let old = &list[i];
            let reshaped = old.name != new.name || !old.capabilities.keys().eq(new.capabilities.keys());
            let changes = new.capabilities.iter().filter(|(k, c)| old.capabilities.get(*k).is_none_or(|o| o.value != c.value)).map(|(k, c)| (k.clone(), c.value.clone())).collect();
            list[i] = new;
            if reshaped { None } else { Some(changes) }
        }
    }
}

fn session(api: &Api, entities: &Mutex<Vec<Entity>>, sink: &Sink, stopped: &AtomicBool) -> Result<(), String> {
    let (mut ws, _) = open(api)?;
    // Subscribe before reading state, so no change falls between the two.
    for (id, event) in [(1, "state_changed"), (2, "area_registry_updated"), (3, "device_registry_updated"), (4, "entity_registry_updated")] {
        command(&mut ws, json!({ "id": id, "type": "subscribe_events", "event_type": event }))?;
    }
    let (rooms, list) = load(api, &mut ws)?;
    *entities.lock().unwrap() = list;
    sink(Event::Reloaded);
    sink(Event::Healthy(true));

    let (mut heard, mut pinged, mut ping_id) = (Instant::now(), Instant::now(), 8u64);
    while !stopped.load(Ordering::Relaxed) {
        if pinged.elapsed() >= Duration::from_secs(30) {
            ws.send(&json!({ "id": ping_id, "type": "ping" }).to_string())?;
            ping_id += 1;
            pinged = Instant::now();
        }
        let Some(msg) = ws.recv()? else {
            if heard.elapsed() >= Duration::from_secs(90) {
                return Err("no answer to pings".into());
            }
            continue;
        };
        heard = Instant::now();
        let Ok(m) = serde_json::from_str::<Value>(&msg) else { continue };
        if m["type"] != "event" {
            continue;
        }
        if m["event"]["event_type"] != "state_changed" {
            // A renamed, moved or hidden entity: reconnect, which reloads rooms and names.
            return Err("registry changed".into());
        }
        let data = &m["event"]["data"];
        let result = apply(&mut entities.lock().unwrap(), &rooms, data);
        match result {
            None => sink(Event::Reloaded),
            Some(changes) => {
                let id = data["entity_id"].as_str().unwrap_or("");
                for (capability, value) in changes {
                    sink(Event::Changed { entity: id.to_string(), capability, value });
                }
            }
        }
    }
    Ok(())
}

fn realtime_loop(api: &Api, entities: &Mutex<Vec<Entity>>, sink: &Sink, stopped: &AtomicBool) {
    let mut backoff = Duration::from_secs(2);
    while !stopped.load(Ordering::Relaxed) {
        let started = Instant::now();
        match session(api, entities, sink, stopped) {
            Ok(()) => return,
            Err(e) => {
                eprintln!("homeassistant: realtime: {e}");
                sink(Event::Healthy(false));
            }
        }
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(2);
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(Duration::from_secs(60));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rooms() -> Rooms {
        Rooms::new(
            &json!([{"area_id": "stue", "name": "Stue"}, {"area_id": "bad", "name": "Bad"}]),
            &json!([{"id": "d1", "area_id": "stue"}]),
            &json!([
                {"entity_id": "light.a", "device_id": "d1", "area_id": null, "hidden_by": null, "disabled_by": null, "entity_category": null},
                {"entity_id": "light.b", "device_id": "d1", "area_id": "bad", "hidden_by": null, "disabled_by": null, "entity_category": null},
                {"entity_id": "light.h", "device_id": "d1", "area_id": null, "hidden_by": "user", "disabled_by": null, "entity_category": null},
                {"entity_id": "sensor.diag", "device_id": "d1", "area_id": null, "hidden_by": null, "disabled_by": null, "entity_category": "diagnostic"}
            ]),
        )
    }

    fn light(id: &str, state: &str, brightness: Value) -> Value {
        json!({"entity_id": id, "state": state, "attributes": {"friendly_name": "Lampe", "supported_color_modes": ["brightness"], "brightness": brightness}})
    }

    #[test]
    fn rooms_come_from_entity_then_device_and_hidden_stay_out() {
        let r = rooms();
        assert_eq!(r.map(&light("light.a", "on", json!(255))).unwrap().zone, "Stue");
        assert_eq!(r.map(&light("light.b", "on", json!(255))).unwrap().zone, "Bad");
        assert!(r.map(&light("light.h", "on", json!(255))).is_none());
        assert!(r.map(&json!({"entity_id": "sensor.diag", "state": "1", "attributes": {"device_class": "temperature"}})).is_none());
    }

    #[test]
    fn maps_each_domain() {
        let e = map_state(&light("light.a", "on", json!(51)), String::new()).unwrap();
        assert_eq!((e.class.as_str(), &e.capabilities["onoff"].value, &e.capabilities["dim"].value), ("light", &json!(true), &json!(0.2)));
        let plain = map_state(&json!({"entity_id": "light.p", "state": "off", "attributes": {"supported_color_modes": ["onoff"]}}), String::new()).unwrap();
        assert!(!plain.capabilities.contains_key("dim"));

        let c = map_state(&json!({"entity_id": "climate.v", "state": "heat", "attributes": {"temperature": 20.0, "current_temperature": 19.5, "min_temp": 7, "max_temp": 35, "hvac_modes": ["heat", "off"]}}), String::new()).unwrap();
        assert_eq!((c.class.as_str(), c.capabilities["target_temperature"].max), ("thermostat", Some(35.0)));
        assert!(!c.capabilities["measure_temperature"].settable && !c.capabilities["onoff"].settable);
        assert_eq!(c.capabilities["thermostat_mode"].values, Some(json!([{"id": "heat", "title": "heat"}, {"id": "off", "title": "off"}])));

        let s = map_state(&json!({"entity_id": "sensor.t", "state": "unavailable", "attributes": {"device_class": "temperature"}}), String::new()).unwrap();
        assert!(s.capabilities["measure_temperature"].value.is_null());
        assert!(map_state(&json!({"entity_id": "sensor.w", "state": "5", "attributes": {"device_class": "power"}}), String::new()).is_none());

        let m = map_state(&json!({"entity_id": "media_player.s", "state": "playing", "attributes": {"volume_level": 0.5, "media_title": "Sang"}}), String::new()).unwrap();
        assert_eq!((m.class.as_str(), &m.capabilities["speaker_track"].value), ("speaker", &json!("Sang")));
        assert_eq!(map_state(&json!({"entity_id": "scene.natt", "state": "x", "attributes": {}}), String::new()).unwrap().class, "button");
        let cam = map_state(&json!({"entity_id": "camera.door", "state": "idle", "attributes": {"access_token": "secret", "entity_picture": "/api/camera_proxy/camera.door?token=secret"}}), String::new()).unwrap();
        assert_eq!(cam.class, "camera");
        assert!(!serde_json::to_string(&cam).unwrap().contains("secret"), "the camera's token stays on the server");
        for dangerous in ["lock.door", "cover.garage", "alarm_control_panel.home"] {
            assert!(map_state(&json!({"entity_id": dangerous, "state": "locked", "attributes": {}}), String::new()).is_none());
        }
    }

    #[test]
    fn writes_become_services_and_bad_values_are_refused() {
        assert_eq!(service_call("light.a", "dim", &json!(0.42)).unwrap(), ("light".to_string(), "turn_on", json!({"brightness_pct": 42.0})));
        assert_eq!(service_call("switch.a", "onoff", &json!(false)).unwrap().1, "turn_off");
        assert_eq!(service_call("climate.v", "target_temperature", &json!(21.5)).unwrap().2, json!({"temperature": 21.5}));
        assert_eq!(service_call("media_player.s", "speaker_playing", &json!(true)).unwrap().1, "media_play");
        assert_eq!(service_call("scene.natt", "button", &json!(true)).unwrap().1, "turn_on");
        assert!(service_call("light.a", "dim", &json!(2)).is_err());
        assert!(service_call("light.a", "onoff", &json!("on")).is_err());
        assert!(service_call("lock.door", "onoff", &json!(false)).is_err());
        assert!(service_call("light.a/../x", "onoff", &json!(true)).is_err());
    }

    #[test]
    fn events_update_values_or_ask_for_a_reload() {
        let r = rooms();
        let mut list = vec![r.map(&light("light.a", "off", json!(null))).unwrap()];
        let on = json!({"entity_id": "light.a", "new_state": light("light.a", "on", json!(255))});
        assert_eq!(apply(&mut list, &r, &on), Some(vec![("dim".to_string(), json!(1.0)), ("onoff".to_string(), json!(true))]));
        assert_eq!(apply(&mut list, &r, &on), Some(vec![]), "no news, no event");
        assert_eq!(apply(&mut list, &r, &json!({"entity_id": "lock.x", "new_state": {"entity_id": "lock.x", "state": "locked", "attributes": {}}})), Some(vec![]));
        assert_eq!(apply(&mut list, &r, &json!({"entity_id": "light.n", "new_state": light("light.n", "on", json!(1))})), None, "added");
        assert_eq!(list.len(), 2);
        assert_eq!(apply(&mut list, &r, &json!({"entity_id": "light.n", "new_state": null})), None, "removed");
        assert_eq!(list.len(), 1);
    }

    /// Against a real instance: OLDPAD_HA_URL=… OLDPAD_HA_TOKEN=… cargo test live_home_assistant -- --ignored
    #[test]
    #[ignore]
    fn live_home_assistant() {
        let config = json!({"address": std::env::var("OLDPAD_HA_URL").unwrap(), "token": std::env::var("OLDPAD_HA_TOKEN").unwrap()});
        let config = config.as_object().unwrap();
        println!("{}", test(config).unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let inst = start(config, Arc::new(move |e| {
            if let Event::Changed { entity, capability, value } = e {
                tx.lock().unwrap().send((entity, capability, value)).ok();
            }
        })).unwrap();
        std::thread::sleep(Duration::from_secs(3)); // let the session subscribe
        let id = "light.bed_light";
        let was = inst.entities().iter().find(|e| e.id == id).unwrap().capabilities["onoff"].value == true;
        inst.set(id, "onoff", &json!(!was)).unwrap();
        let by = Instant::now() + Duration::from_secs(10);
        loop {
            let left = by.saturating_duration_since(Instant::now());
            let (e, c, v) = rx.recv_timeout(left).expect("no change event");
            if e == id && c == "onoff" {
                assert_eq!(v, json!(!was));
                break;
            }
        }
        inst.set(id, "onoff", &json!(was)).unwrap();
        let camera = inst.entities().into_iter().find(|e| e.class == "camera").expect("a camera");
        let (kind, bytes) = inst.snapshot(&camera.id).unwrap();
        println!("{}: {kind}, {} bytes", camera.id, bytes.len());
        assert!(kind.starts_with("image/") && !bytes.is_empty());
        inst.stop();
    }
}
