//! Weather forecast from MET Norway (api.met.no), free and worldwide.
//! One entity, `forecast`: the current conditions plus an hourly series the
//! tablet turns into its forecast strip in its own time zone.

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

fn test(config: &Map<String, Value>) -> Result<String, String> {
    let (lat, lon) = coords(config)?;
    let e = fetch(lat, lon)?;
    Ok(format!("Got the forecast: {}°C now, {}.", e.capabilities["measure_temperature"].value, e.capabilities["weather_symbol"].value.as_str().unwrap_or("?")))
}

struct Met {
    entity: Arc<Mutex<Entity>>,
    stopped: Arc<AtomicBool>,
}

fn start(config: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let (lat, lon) = coords(config)?;
    let entity = Arc::new(Mutex::new(fetch(lat, lon)?));
    let stopped = Arc::new(AtomicBool::new(false));
    let (ent, stop) = (entity.clone(), stopped.clone());
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            // MET updates hourly at most; 20 minutes keeps us well inside their limits.
            std::thread::sleep(Duration::from_secs(20 * 60));
            match fetch(lat, lon) {
                Ok(fresh) => {
                    *ent.lock().unwrap() = fresh.clone();
                    for (capability, c) in fresh.capabilities {
                        sink(Event::Changed { entity: "forecast".into(), capability, value: c.value });
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
    #[ignore] // network: cargo test met_live -- --ignored --nocapture
    fn met_live() {
        let c = json!({ "latitude": 59.91, "longitude": 10.75 });
        println!("{}", test(c.as_object().unwrap()).unwrap());
    }
}
