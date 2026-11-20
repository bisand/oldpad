//! Electricity spot prices for a Norwegian price area, from hvakosterstrommen.no
//! (Nord Pool day-ahead prices, free, no key). One entity, `price`: the price now
//! and the hours ahead, for the tablet's price chart. Tomorrow's prices come out
//! around 13:00.

use super::{Capability, Entity, Event, Field, FieldKind, Instance, Kind, Sink};
use crate::client::request;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn kind() -> Kind {
    Kind {
        id: "spot",
        name: "Electricity price",
        description: "Today's and tomorrow's spot prices for your price area (Nord Pool, via hvakosterstrommen.no). Free, no account needed.",
        fields: vec![
            Field { key: "area", label: "Price area", kind: FieldKind::Text, required: true, placeholder: "NO1", help: "NO1 East (Oslo), NO2 South, NO3 Central, NO4 North, NO5 West." },
            Field { key: "extra", label: "Add per kWh (øre)", kind: FieldKind::Number, required: false, placeholder: "0", help: "Grid fee and your supplier's markup, if you want them in the price. VAT (25 %, none in NO4) is added to the spot price either way." },
        ],
        test,
        start,
    }
}

const AREAS: [&str; 5] = ["NO1", "NO2", "NO3", "NO4", "NO5"];

fn config(c: &Map<String, Value>) -> Result<(String, f64), String> {
    let area = c.get("area").and_then(Value::as_str).unwrap_or("").trim().to_uppercase();
    if !AREAS.contains(&area.as_str()) {
        return Err("price area must be NO1 to NO5".into());
    }
    let extra = c.get("extra").and_then(Value::as_f64).unwrap_or(0.0) / 100.0;
    Ok((area, extra))
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Days since 1970-01-01 → (year, month, day) (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// "2026-11-20T00:00:00+01:00" → seconds since the epoch.
fn parse_time(s: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let local = super::met::days_from_civil(n(0..4)?, n(5..7)?, n(8..10)?) * 86_400 + n(11..13)? * 3600 + n(14..16)? * 60 + n(17..19)?;
    let offset = match s.get(19..)? {
        "Z" => 0,
        o if o.len() == 6 => (n(20..22)? * 3600 + n(23..25)? * 60) * if o.starts_with('-') { -1 } else { 1 },
        _ => return None,
    };
    Some(local - offset)
}

/// Prices per period as `[start, end, kr/kWh with VAT and extra]`, from one day's file.
fn parse_day(body: &Value, vat: bool, extra: f64) -> Vec<(i64, i64, f64)> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let spot = p["NOK_per_kWh"].as_f64()?;
            let price = if vat { spot * 1.25 } else { spot } + extra;
            Some((parse_time(p["time_start"].as_str()?)?, parse_time(p["time_end"].as_str()?)?, (price * 10_000.0).round() / 10_000.0))
        })
        .collect()
}

/// Yesterday to tomorrow (by UTC date, which covers Norway's day), from three hours ago on.
fn fetch(area: &str, extra: f64, t: i64) -> Result<Vec<(i64, i64, f64)>, String> {
    let mut out = Vec::new();
    let mut ok = false;
    for day in [t / 86_400 - 1, t / 86_400, t / 86_400 + 1] {
        let (y, m, d) = civil_from_days(day);
        let res = request("https://www.hvakosterstrommen.no", "GET", &format!("/api/v1/prices/{y}/{m:02}-{d:02}_{area}.json"), &[], None)?;
        match res.status {
            200 => {
                ok = true;
                out.extend(parse_day(&res.json()?, area != "NO4", extra));
            }
            404 => {} // tomorrow before ~13:00
            s => return Err(format!("hvakosterstrommen.no answered HTTP {s}")),
        }
    }
    if !ok {
        return Err("no prices for this area yet".into());
    }
    out.sort_by_key(|p| p.0);
    out.dedup_by_key(|p| p.0);
    out.retain(|p| p.1 > t - 3 * 3600);
    Ok(out)
}

fn current(prices: &[(i64, i64, f64)], t: i64) -> Value {
    prices.iter().find(|p| p.0 <= t && t < p.1).map_or(Value::Null, |p| json!(p.2))
}

fn entity(prices: &[(i64, i64, f64)], t: i64) -> Entity {
    let series: Vec<Value> = prices.iter().map(|p| json!([p.0, p.1, p.2])).collect();
    Entity {
        id: "price".into(),
        name: "Electricity price".into(),
        zone: String::new(),
        class: "price".into(),
        capabilities: BTreeMap::from([
            ("price".to_string(), Capability { units: Some("kr/kWh".into()), ..Capability::new(current(prices, t), false) }),
            ("prices".to_string(), Capability::new(json!(series), false)),
        ]),
    }
}

fn test(c: &Map<String, Value>) -> Result<String, String> {
    let (area, extra) = config(c)?;
    let t = now();
    let prices = fetch(&area, extra, t)?;
    let ahead = prices.iter().filter(|p| p.0 > t).count();
    Ok(format!("{area} now: {} kr/kWh. {ahead} prices ahead.", current(&prices, t)))
}

struct Spot {
    entity: Arc<Mutex<Entity>>,
    stopped: Arc<AtomicBool>,
}

fn start(c: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let (area, extra) = config(c)?;
    let mut prices = fetch(&area, extra, now())?;
    let shared = Arc::new(Mutex::new(entity(&prices, now())));
    let stopped = Arc::new(AtomicBool::new(false));
    let (ent, stop) = (shared.clone(), stopped.clone());
    std::thread::spawn(move || {
        let mut fetched = now();
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(30));
            let t = now();
            // The files change once a day; every half hour finds tomorrow's soon after they're out.
            if t - fetched >= 1800 {
                match fetch(&area, extra, t) {
                    Ok(p) => (prices, fetched) = (p, t),
                    Err(e) => eprintln!("spot: {e}"),
                }
            }
            let fresh = entity(&prices, t);
            let mut e = ent.lock().unwrap();
            let changed: Vec<(String, Value)> = fresh.capabilities.iter().filter(|(k, c)| e.capabilities.get(*k).map(|o| &o.value) != Some(&c.value)).map(|(k, c)| (k.clone(), c.value.clone())).collect();
            *e = fresh;
            drop(e);
            for (capability, value) in changed {
                sink(Event::Changed { entity: "price".into(), capability, value });
            }
        }
    });
    Ok(Box::new(Spot { entity: shared, stopped }))
}

impl Instance for Spot {
    fn entities(&self) -> Vec<Entity> {
        vec![self.entity.lock().unwrap().clone()]
    }
    fn set(&self, _: &str, _: &str, _: &Value) -> Result<(), String> {
        Err("prices are read-only".into())
    }
    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_day_of_prices() {
        assert_eq!(civil_from_days(20_412), (2025, 11, 20));
        assert_eq!(civil_from_days(super::super::met::days_from_civil(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(parse_time("2026-11-20T00:00:00+01:00"), Some(1_795_129_200));
        assert_eq!(parse_time("2026-07-01T00:00:00+02:00"), parse_time("2026-06-30T22:00:00Z"));
        let day = json!([
            {"NOK_per_kWh": 1.0, "time_start": "2026-11-20T00:00:00+01:00", "time_end": "2026-11-20T01:00:00+01:00"},
            {"NOK_per_kWh": 0.5, "time_start": "2026-11-20T01:00:00+01:00", "time_end": "2026-11-20T02:00:00+01:00"},
        ]);
        let p = parse_day(&day, true, 0.4);
        assert_eq!(p.iter().map(|p| p.2).collect::<Vec<_>>(), vec![1.65, 1.025], "VAT on spot, then the add-on");
        assert_eq!(current(&p, 1_795_129_200 + 3600), json!(1.025));
        assert_eq!(current(&p, 1_795_129_200 + 7200), Value::Null);
        assert_eq!(parse_day(&day, false, 0.0)[0].2, 1.0, "NO4: no VAT");
    }

    #[test]
    #[ignore] // network: cargo test spot_live -- --ignored --nocapture
    fn spot_live() {
        println!("{}", test(json!({ "area": "NO1" }).as_object().unwrap()).unwrap());
    }
}
