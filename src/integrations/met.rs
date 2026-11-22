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

/// One forecast hour: `[time, temperature, symbol, wind m/s, wind from degrees]`.
fn hour(t: &Value) -> Option<Value> {
    let details = &t["data"]["instant"]["details"];
    let (time, temp) = (t["time"].as_str().and_then(parse_time)?, details["air_temperature"].as_f64()?);
    let symbol = t["data"]["next_1_hours"]["summary"]["symbol_code"].as_str().or(t["data"]["next_6_hours"]["summary"]["symbol_code"].as_str()).unwrap_or("");
    Some(json!([time, temp, symbol, details["wind_speed"], details["wind_from_direction"]]))
}

/// The sun's height above the horizon in degrees at an epoch second (USNO's low-precision formulas).
fn sun_altitude(t: i64, lat: f64, lon: f64) -> f64 {
    let d = t as f64 / 86_400.0 - 10_957.5; // days since J2000
    let g = (357.529 + 0.985_600_28 * d).to_radians();
    let l = (280.459 + 0.985_647_36 * d + 1.915 * g.sin() + 0.020 * (2.0 * g).sin()).to_radians();
    let e = (23.439 - 0.000_000_36 * d).to_radians();
    let ra = (e.cos() * l.sin()).atan2(l.cos());
    let dec = (e.sin() * l.sin()).asin();
    let hour_angle = ((18.697_374_558 + 24.065_709_824_419_08 * d) * 15.0 + lon).to_radians() - ra;
    let lat = lat.to_radians();
    (lat.sin() * dec.sin() + lat.cos() * dec.cos() * hour_angle.cos()).asin().to_degrees()
}

/// When the sun is up over the next three days: `{until, spans: [[rise, set], …]}`, to the minute.
/// Stepping through the minutes needs no special case for midnight sun or polar night.
fn daylight(from: i64, lat: f64, lon: f64) -> Value {
    let until = from + 3 * 86_400;
    let (mut spans, mut rose) = (Vec::new(), None);
    for t in (from..until).step_by(60) {
        // -0.833°: the top of the disc, seen through the atmosphere.
        match (sun_altitude(t, lat, lon) >= -0.833, rose) {
            (true, None) => rose = Some(t),
            (false, Some(a)) => {
                spans.push([a, t]);
                rose = None;
            }
            _ => {}
        }
    }
    if let Some(a) = rose {
        spans.push([a, until]);
    }
    json!({ "until": until, "spans": spans })
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

    let hours: Vec<Value> = series.iter().take(60).filter_map(hour).collect();
    let now = &series.first().ok_or("empty forecast")?["data"];
    let cap = |v: Value| Capability::new(v, false);
    let mut caps = BTreeMap::new();
    caps.insert("measure_temperature".into(), Capability { units: Some("°C".into()), ..cap(now["instant"]["details"]["air_temperature"].clone()) });
    caps.insert("measure_wind_speed".into(), Capability { units: Some("m/s".into()), ..cap(now["instant"]["details"]["wind_speed"].clone()) });
    caps.insert("weather_symbol".into(), cap(now["next_1_hours"]["summary"]["symbol_code"].clone()));
    caps.insert("forecast".into(), cap(Value::Array(hours)));
    caps.insert("daylight".into(), cap(daylight(crate::util::now() as i64, lat, lon)));
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
    fn an_hour_carries_its_wind() {
        let t = json!({ "time": "2026-11-21T14:00:00Z", "data": { "instant": { "details": { "air_temperature": 2.5, "wind_speed": 7.4, "wind_from_direction": 225.0 } }, "next_1_hours": { "summary": { "symbol_code": "fair_day" } } } });
        let h = hour(&t).unwrap();
        assert_eq!((&h[1], &h[2], &h[3], &h[4]), (&json!(2.5), &json!("fair_day"), &json!(7.4), &json!(225.0)));
        let calm = json!({ "time": "2026-11-21T14:00:00Z", "data": { "instant": { "details": { "air_temperature": 1.0 } } } });
        assert!(hour(&calm).unwrap()[3].is_null(), "a missing wind stays empty");
    }

    #[test]
    fn parses_met_times() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-11-20T10:00:00Z"), Some(1_795_168_800));
        assert_eq!(parse_time("2024-02-29T12:30:15Z"), Some(1_709_209_815));
        assert_eq!(parse_time("2026-11-20T10:00:00+01:00"), None);
    }

    #[test]
    fn daylight_matches_met_and_handles_the_poles() {
        let t = |s: &str| parse_time(s).unwrap();
        // Times from MET's Sunrise API: Oslo, Sydney, Los Angeles, Tromsø.
        for (lat, lon, from, met_rise, met_set) in [
            (59.91, 10.75, "2026-06-21T00:00:00Z", "2026-06-21T01:53:00Z", "2026-06-21T20:43:00Z"),
            (-33.87, 151.21, "2026-03-19T12:00:00Z", "2026-03-19T19:57:00Z", "2026-03-20T08:06:00Z"),
            (34.05, -118.24, "2026-09-23T06:00:00Z", "2026-09-23T13:41:00Z", "2026-09-24T01:48:00Z"),
            (69.65, 18.96, "2026-04-10T00:00:00Z", "2026-04-10T03:06:00Z", "2026-04-10T18:26:00Z"),
        ] {
            let d = daylight(t(from), lat, lon);
            let (rise, set) = (d["spans"][0][0].as_i64().unwrap(), d["spans"][0][1].as_i64().unwrap());
            assert!((rise - t(met_rise)).abs() <= 120, "{lat}: rise is {}s off", rise - t(met_rise));
            assert!((set - t(met_set)).abs() <= 120, "{lat}: set is {}s off", set - t(met_set));
            assert_eq!(d["spans"].as_array().unwrap().len(), 3);
        }

        let from = t("2026-06-21T00:00:00Z");
        assert_eq!(daylight(from, 78.22, 15.65)["spans"], json!([[from, from + 3 * 86_400]]), "midnight sun in Longyearbyen");
        assert_eq!(daylight(t("2026-12-21T00:00:00Z"), 78.22, 15.65)["spans"], json!([]), "polar night");
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
