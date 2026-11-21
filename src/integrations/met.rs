//! Weather forecast from MET Norway (api.met.no), free and worldwide.
//! One entity, `forecast`: the current conditions plus an hourly series the
//! tablet turns into its forecast strip in its own time zone, and MET's official
//! weather warnings (MetAlerts) for that place.

use super::{Capability, Entity, Event, Field, FieldKind, Instance, Kind, Sink};
use crate::client::request;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn kind() -> Kind {
    Kind {
        id: "met",
        name: "Weather (MET Norway)",
        description: "Forecast for any place in the world from the Norwegian Meteorological Institute. Free, no account needed.",
        fields: vec![
            Field { key: "latitude", label: "Latitude", kind: FieldKind::Number, required: true, placeholder: "59.91", help: "Right-click a place in a map app to copy its coordinates." },
            Field { key: "longitude", label: "Longitude", kind: FieldKind::Number, required: true, placeholder: "10.75", help: "" },
        ],
        test,
        start,
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
pub(super) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// "2026-11-20T10:00:00Z" → seconds since the epoch.
fn parse_time(s: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    if s.len() != 20 || !s.ends_with('Z') {
        return None;
    }
    Some(days_from_civil(n(0..4)?, n(5..7)?, n(8..10)?) * 86_400 + n(11..13)? * 3600 + n(14..16)? * 60 + n(17..19)?)
}

fn coords(config: &Map<String, Value>) -> Result<(f64, f64), String> {
    let lat = config.get("latitude").and_then(Value::as_f64).ok_or("latitude is required")?;
    let lon = config.get("longitude").and_then(Value::as_f64).ok_or("longitude is required")?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err("coordinates are out of range".into());
    }
    Ok((lat, lon))
}

fn fetch(lat: f64, lon: f64) -> Result<Entity, String> {
    // MET's terms: at most four decimals, so caches are shared between users.
    let path = format!("/weatherapi/locationforecast/2.0/compact?lat={lat:.4}&lon={lon:.4}");
    let res = request("https://api.met.no", "GET", &path, &[], None)?;
    if res.status != 200 {
        return Err(format!("api.met.no answered HTTP {}", res.status));
    }
    let v = res.json()?;
    let series = v["properties"]["timeseries"].as_array().ok_or("unexpected forecast format")?;

    let mut hours = Vec::new();
    for t in series.iter().take(60) {
        let (Some(time), Some(temp)) = (t["time"].as_str().and_then(parse_time), t["data"]["instant"]["details"]["air_temperature"].as_f64()) else { continue };
        let symbol = t["data"]["next_1_hours"]["summary"]["symbol_code"].as_str().or(t["data"]["next_6_hours"]["summary"]["symbol_code"].as_str()).unwrap_or("");
        hours.push(json!([time, temp, symbol]));
    }
    let now = &series.first().ok_or("empty forecast")?["data"];
    let cap = |v: Value| Capability::new(v, false);
    let mut caps = BTreeMap::new();
    caps.insert("measure_temperature".into(), Capability { units: Some("°C".into()), ..cap(now["instant"]["details"]["air_temperature"].clone()) });
    caps.insert("measure_wind_speed".into(), Capability { units: Some("m/s".into()), ..cap(now["instant"]["details"]["wind_speed"].clone()) });
    caps.insert("weather_symbol".into(), cap(now["next_1_hours"]["summary"]["symbol_code"].clone()));
    caps.insert("forecast".into(), cap(Value::Array(hours)));
    Ok(Entity { id: "forecast".into(), name: "Forecast".into(), zone: "Weather".into(), class: "weather".into(), capabilities: caps })
}

/// Real warnings for this place, most serious first:
/// `{id, level (2 yellow, 3 orange, 4 red), name, area, from, until, text, advice}`, times in epoch seconds.
fn parse_alerts(v: &Value) -> Vec<Value> {
    let features = v["features"].as_array().map(Vec::as_slice).unwrap_or_default();
    let mut out: Vec<(i64, i64, Value)> = features
        .iter()
        .filter(|f| f["properties"]["status"] == "Actual" && f["properties"]["type"] != "Cancel")
        .filter_map(|f| {
            let p = &f["properties"];
            // An unreadable level still warns: better yellow than nothing.
            let level = p["awareness_level"].as_str().and_then(|s| s.split(';').next()?.trim().parse::<i64>().ok()).unwrap_or(2);
            if level < 2 {
                return None;
            }
            let level = level.min(4);
            let time = |t: &Value| t.as_str().and_then(super::spot::parse_time);
            let from = time(&f["when"]["interval"][0]);
            let until = time(&f["when"]["interval"][1]).or_else(|| time(&p["eventEndingTime"]));
            let name = p["eventAwarenessName"].as_str().or(p["event"].as_str()).unwrap_or("");
            let alert = json!({ "id": p["id"], "level": level, "name": name, "area": p["area"], "from": from, "until": until, "text": p["description"], "advice": p["instruction"] });
            Some((level, from.unwrap_or(0), alert))
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    out.into_iter().take(10).map(|x| x.2).collect()
}

fn fetch_alerts(lat: f64, lon: f64) -> Result<Value, String> {
    let path = format!("/weatherapi/metalerts/2.0/current.json?lat={lat:.4}&lon={lon:.4}");
    let res = request("https://api.met.no", "GET", &path, &[], None)?;
    if res.status != 200 {
        return Err(format!("api.met.no answered HTTP {} for warnings", res.status));
    }
    let v = res.json()?;
    if !v["features"].is_array() {
        return Err("unexpected warnings format".into());
    }
    Ok(json!(parse_alerts(&v)))
}

fn test(config: &Map<String, Value>) -> Result<String, String> {
    let (lat, lon) = coords(config)?;
    let e = fetch(lat, lon)?;
    let warnings = match fetch_alerts(lat, lon) {
        Ok(a) => format!("{} weather warnings here.", a.as_array().map_or(0, Vec::len)),
        Err(e) => format!("Weather warnings are unavailable: {e}."),
    };
    Ok(format!("Got the forecast: {}°C now, {}. {warnings}", e.capabilities["measure_temperature"].value, e.capabilities["weather_symbol"].value.as_str().unwrap_or("?")))
}

struct Met {
    entity: Arc<Mutex<Entity>>,
    stopped: Arc<AtomicBool>,
}

fn start(config: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let (lat, lon) = coords(config)?;
    let mut first = fetch(lat, lon)?;
    // Warnings are extra: without them the forecast still works.
    let alerts = fetch_alerts(lat, lon).unwrap_or_else(|e| {
        eprintln!("met: {e}");
        json!([])
    });
    first.capabilities.insert("alerts".into(), Capability::new(alerts, false));
    let entity = Arc::new(Mutex::new(first));
    let stopped = Arc::new(AtomicBool::new(false));
    let (ent, stop) = (entity.clone(), stopped.clone());
    std::thread::spawn(move || {
        let mut round = 0u32;
        while !stop.load(Ordering::Relaxed) {
            // Warnings every 5 minutes. The forecast updates hourly at most, so every 20 is plenty.
            std::thread::sleep(Duration::from_secs(5 * 60));
            round += 1;
            if round.is_multiple_of(4) {
                match fetch(lat, lon) {
                    Ok(mut fresh) => {
                        let mut current = ent.lock().unwrap();
                        fresh.capabilities.insert("alerts".into(), current.capabilities["alerts"].clone());
                        *current = fresh.clone();
                        drop(current);
                        for (capability, c) in fresh.capabilities {
                            sink(Event::Changed { entity: "forecast".into(), capability, value: c.value });
                        }
                    }
                    Err(e) => eprintln!("met: {e}"),
                }
            }
            // A failed check keeps the warnings we have: an outage must not look like all clear.
            match fetch_alerts(lat, lon) {
                Ok(alerts) => {
                    let mut current = ent.lock().unwrap();
                    let c = current.capabilities.get_mut("alerts").expect("set at start");
                    if c.value != alerts {
                        c.value = alerts.clone();
                        drop(current);
                        sink(Event::Changed { entity: "forecast".into(), capability: "alerts".into(), value: alerts });
                    }
                }
                Err(e) => eprintln!("met: {e}"),
            }
        }
    });
    Ok(Box::new(Met { entity, stopped }))
}

impl Instance for Met {
    fn entities(&self) -> Vec<Entity> {
        vec![self.entity.lock().unwrap().clone()]
    }
    fn set(&self, _: &str, _: &str, _: &Value) -> Result<(), String> {
        Err("the forecast is read-only".into())
    }
    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_met_times() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-11-20T10:00:00Z"), Some(1_795_168_800));
        assert_eq!(parse_time("2024-02-29T12:30:15Z"), Some(1_709_209_815));
        assert_eq!(parse_time("2026-11-20T10:00:00+01:00"), None);
    }

    #[test]
    fn reads_warnings() {
        let alert = |id: &str, level: &str, status: &str, kind: &str| json!({
            "when": {"interval": ["2026-11-21T23:00:00+00:00", "2026-11-22T17:00:00+00:00"]},
            "properties": {"id": id, "awareness_level": level, "status": status, "type": kind, "eventAwarenessName": "Kuling", "event": "gale",
                "area": "Slettnes fyr - Vardø", "description": "Stiv kuling.", "instruction": "La båten ligge."}
        });
        let list = parse_alerts(&json!({"features": [
            alert("yellow", "2; yellow; Moderate", "Actual", "Alert"),
            alert("red", "4; red; Extreme", "Actual", "Update"),
            alert("orange", "3; orange; Severe", "Actual", "Alert"),
            alert("green", "1; green; Minor", "Actual", "Alert"),
            alert("drill", "4; red; Extreme", "Exercise", "Alert"),
            alert("test", "4; red; Extreme", "Test", "Alert"),
            alert("over", "3; orange; Severe", "Actual", "Cancel"),
            alert("odd", "?", "Actual", "Alert"),
        ]}));
        let ids: Vec<&str> = list.iter().map(|a| a["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["red", "orange", "yellow", "odd"], "most serious first, no drills or cancelled ones");
        assert_eq!(list[3]["level"], 2, "an unreadable level still warns");
        let a = &list[1];
        assert_eq!((a["name"].as_str(), a["area"].as_str(), a["advice"].as_str()), (Some("Kuling"), Some("Slettnes fyr - Vardø"), Some("La båten ligge.")));
        assert_eq!(a["until"].as_i64().unwrap() - a["from"].as_i64().unwrap(), 18 * 3600);
    }

    #[test]
    #[ignore] // network: MET_LAT=70.4 MET_LON=31.1 cargo test alerts_live -- --ignored --nocapture
    fn alerts_live() {
        let at = |k: &str, d: f64| std::env::var(k).ok().and_then(|s| s.parse().ok()).unwrap_or(d);
        println!("{}", fetch_alerts(at("MET_LAT", 59.91), at("MET_LON", 10.75)).unwrap());
    }

    #[test]
    #[ignore] // network: cargo test met_live -- --ignored --nocapture
    fn met_live() {
        let c = json!({ "latitude": 59.91, "longitude": 10.75 });
        println!("{}", test(c.as_object().unwrap()).unwrap());
    }
}
