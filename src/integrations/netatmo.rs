//! Netatmo weather stations, read from Netatmo's cloud. One read-only sensor per
//! module. The cloud updates its data every 10 minutes.
//!
//! Each token refresh invalidates the previous refresh token at once, so the
//! latest one is always kept, in memory and with `Event::Remember`.

use super::{get_str, Capability, Entity, Event, Field, FieldKind, Instance, Kind, Sink};
use crate::client::request;
use crate::util::percent_encode;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const API: &str = "https://api.netatmo.com";
const POLL: Duration = Duration::from_secs(5 * 60);

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
            Field { key: "refresh_token", label: "Refresh token", kind: FieldKind::Secret, required: true, placeholder: "", help: "On the app page, use the token generator with the read_station scope. Paste a new one here if Netatmo ever stops accepting it." },
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

struct Api {
    client_id: String,
    client_secret: String,
    pasted: String,
    stored: String,
    access: Mutex<String>,
    sink: Sink,
}

impl Api {
    fn refresh(&self) -> Result<(), String> {
        let mut tokens = TOKENS.lock().unwrap();
        let current = tokens.get(&self.pasted).unwrap_or(&self.stored);
        let form = format!("grant_type=refresh_token&refresh_token={}&client_id={}&client_secret={}", percent_encode(current), percent_encode(&self.client_id), percent_encode(&self.client_secret));
        let res = request(API, "POST", "/oauth2/token", &[("Content-Type", "application/x-www-form-urlencoded;charset=UTF-8")], Some(form.as_bytes()))?;
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
        *self.access.lock().unwrap() = access.to_string();
        Ok(())
    }

    fn get(&self) -> Result<crate::client::HttpResponse, String> {
        let auth = format!("Bearer {}", self.access.lock().unwrap());
        request(API, "GET", "/api/getstationsdata", &[("Authorization", &auth)], None)
    }

    fn entities(&self) -> Result<Vec<Entity>, String> {
        let mut res = self.get()?;
        // 403 with code 26 is throttling; a new token would not help.
        let throttled = res.status == 403 && res.json().is_ok_and(|v: Value| v["error"]["code"] == 26);
        if matches!(res.status, 401 | 403) && !throttled {
            self.refresh()?;
            res = self.get()?;
        }
        if res.status != 200 {
            return Err(format!("Netatmo answered HTTP {} for the station data", res.status));
        }
        Ok(map(&res.json()?))
    }
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
    let capabilities: BTreeMap<String, Capability> = list.iter().map(|(key, cap, units)| ((*cap).to_string(), Capability { units: Some((*units).to_string()), ..Capability::new(m["dashboard_data"][*key].clone(), false) })).collect();
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
    entities: Arc<Mutex<Vec<Entity>>>,
    stopped: Arc<AtomicBool>,
}

fn start(config: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let api = Api {
        client_id: get_str(config, "client_id").to_string(),
        client_secret: get_str(config, "client_secret").to_string(),
        pasted: get_str(config, "refresh_token").to_string(),
        stored: stored_token(config),
        access: Mutex::new(String::new()),
        sink: sink.clone(),
    };
    api.refresh()?;
    let entities = Arc::new(Mutex::new(api.entities()?));
    let stopped = Arc::new(AtomicBool::new(false));
    let (shared, stop) = (entities.clone(), stopped.clone());
    std::thread::spawn(move || {
        let mut polled = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(2));
            if polled.elapsed() < POLL {
                continue;
            }
            polled = Instant::now();
            match api.entities() {
                Ok(fresh) => publish(&shared, fresh, &sink),
                Err(e) => eprintln!("netatmo: {e}"),
            }
        }
    });
    Ok(Box::new(Netatmo { entities, stopped }))
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
    fn set(&self, _: &str, _: &str, _: &Value) -> Result<(), String> {
        Err("Netatmo readings are read-only".into())
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

    #[test]
    fn a_newly_pasted_token_beats_the_remembered_one() {
        let c = |v: Value| v.as_object().unwrap().clone();
        assert_eq!(stored_token(&c(json!({"refresh_token": "a"}))), "a");
        assert_eq!(stored_token(&c(json!({"refresh_token": "a", "_token": {"from": "a", "token": "b"}}))), "b");
        assert_eq!(stored_token(&c(json!({"refresh_token": "new", "_token": {"from": "a", "token": "b"}}))), "new");
    }
}
