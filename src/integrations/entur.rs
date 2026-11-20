//! Departures from public transport stops anywhere in Norway, from Entur's
//! open journey planner (free, no key). One entity per stop: the next departures
//! with realtime times, for the tablet's departure board.

use super::{Capability, Entity, Event, Field, FieldKind, Instance, Kind, Sink};
use crate::client::request;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn kind() -> Kind {
    Kind {
        id: "entur",
        name: "Departures (Entur)",
        description: "Next buses, trams, trains and boats from your stops, with realtime. All of Norway, free, no account needed.",
        fields: vec![
            Field { key: "stops", label: "Stops", kind: FieldKind::Text, required: true, placeholder: "Jernbanetorget, Oslo S", help: "Stop names as in the Entur or Ruter app, separated by commas, or ids like NSR:StopPlace:58366. Each becomes its own departure board." },
            Field { key: "count", label: "Departures per stop", kind: FieldKind::Number, required: false, placeholder: "8", help: "" },
        ],
        test,
        start,
    }
}

const BASE: &str = "https://api.entur.io";
// Entur asks every client to name itself; a generic name, nothing about this household.
const CLIENT: (&str, &str) = ("ET-Client-Name", "oldpad-dashboard");

fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// A stop by name or id → (id, name).
fn resolve(stop: &str) -> Result<(String, String), String> {
    let stop = stop.trim();
    if stop.starts_with("NSR:StopPlace:") && stop[14..].chars().all(|c| c.is_ascii_digit()) {
        return Ok((stop.to_string(), stop.to_string()));
    }
    let res = request(BASE, "GET", &format!("/geocoder/v1/autocomplete?text={}&layers=venue&size=1", encode(stop)), &[CLIENT], None)?;
    if res.status != 200 {
        return Err(format!("Entur's stop search answered HTTP {}", res.status));
    }
    let v = res.json()?;
    let p = &v["features"][0]["properties"];
    match (p["id"].as_str(), p["name"].as_str()) {
        (Some(id), Some(name)) if id.starts_with("NSR:StopPlace:") => Ok((id.into(), name.into())),
        _ => Err(format!("no stop called \"{stop}\"")),
    }
}

const QUERY: &str = "query($id: String!, $n: Int!) { stopPlace(id: $id) { name estimatedCalls(numberOfDepartures: $n, timeRange: 86400) { \
    expectedDepartureTime aimedDepartureTime realtime cancellation destinationDisplay { frontText } quay { publicCode } \
    serviceJourney { line { publicCode transportMode presentation { colour textColour } } } } } }";

/// The next departures: `{line, mode, to, at, aimed, rt, gone, quay, bg, fg}`, times in epoch seconds.
fn parse(v: &Value) -> Result<(String, Vec<Value>), String> {
    let sp = &v["data"]["stopPlace"];
    let name = sp["name"].as_str().ok_or_else(|| v["errors"][0]["message"].as_str().unwrap_or("no such stop").to_string())?;
    let calls = sp["estimatedCalls"].as_array().map(Vec::as_slice).unwrap_or_default();
    let time = |c: &Value, k: &str| c[k].as_str().and_then(super::spot::parse_time);
    let list = calls
        .iter()
        .filter_map(|c| {
            let line = &c["serviceJourney"]["line"];
            let hex = |k: &str| line["presentation"][k].as_str().filter(|s| s.len() == 6 && s.chars().all(|c| c.is_ascii_hexdigit())).map(|s| format!("#{s}"));
            Some(json!({
                "line": line["publicCode"].as_str().unwrap_or(""), "mode": line["transportMode"].as_str().unwrap_or(""),
                "to": c["destinationDisplay"]["frontText"].as_str().unwrap_or(""),
                "at": time(c, "expectedDepartureTime")?, "aimed": time(c, "aimedDepartureTime"),
                "rt": c["realtime"] == true, "gone": c["cancellation"] == true, "quay": c["quay"]["publicCode"].as_str(),
                "bg": hex("colour"), "fg": hex("textColour"),
            }))
        })
        .collect();
    Ok((name.to_string(), list))
}

fn fetch(id: &str, n: u32) -> Result<(String, Vec<Value>), String> {
    let body = json!({ "query": QUERY, "variables": { "id": id, "n": n } }).to_string();
    let res = request(BASE, "POST", "/journey-planner/v3/graphql", &[CLIENT, ("Content-Type", "application/json")], Some(body.as_bytes()))?;
    if res.status != 200 {
        return Err(format!("Entur answered HTTP {}", res.status));
    }
    parse(&res.json()?)
}

fn entity(id: &str, name: &str, list: Vec<Value>) -> Entity {
    Entity {
        id: format!("stop-{}", id.trim_start_matches("NSR:StopPlace:")),
        name: name.into(),
        zone: "Departures".into(),
        class: "departures".into(),
        capabilities: BTreeMap::from([("departures".to_string(), Capability::new(json!(list), false))]),
    }
}

fn config(c: &Map<String, Value>) -> Result<(Vec<String>, u32), String> {
    let stops: Vec<String> = c.get("stops").and_then(Value::as_str).unwrap_or("").split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if stops.is_empty() || stops.len() > 6 {
        return Err("give one to six stops".into());
    }
    let n = c.get("count").and_then(Value::as_f64).map_or(8, |n| n.clamp(1.0, 20.0) as u32);
    Ok((stops, n))
}

fn test(c: &Map<String, Value>) -> Result<String, String> {
    let (stops, n) = config(c)?;
    let mut out = Vec::new();
    for s in stops {
        let (id, _) = resolve(&s)?;
        let (name, list) = fetch(&id, n)?;
        out.push(format!("{name} ({id}): {} departures", list.len()));
    }
    Ok(out.join(". "))
}

struct Entur {
    entities: Arc<Mutex<Vec<Entity>>>,
    stopped: Arc<AtomicBool>,
}

fn start(c: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let (stops, n) = config(c)?;
    let ids: Vec<String> = stops.iter().map(|s| resolve(s).map(|(id, _)| id)).collect::<Result<_, _>>()?;
    let mut first = Vec::new();
    for id in &ids {
        let (name, list) = fetch(id, n)?;
        first.push(entity(id, &name, list));
    }
    let entities = Arc::new(Mutex::new(first));
    let stopped = Arc::new(AtomicBool::new(false));
    let (ents, stop) = (entities.clone(), stopped.clone());
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(30));
            for id in &ids {
                match fetch(id, n) {
                    Ok((name, list)) => {
                        let fresh = entity(id, &name, list);
                        let value = fresh.capabilities["departures"].value.clone();
                        let mut all = ents.lock().unwrap();
                        let Some(e) = all.iter_mut().find(|e| e.id == fresh.id) else { continue };
                        if e.capabilities["departures"].value != value {
                            *e = fresh;
                            drop(all);
                            sink(Event::Changed { entity: format!("stop-{}", id.trim_start_matches("NSR:StopPlace:")), capability: "departures".into(), value });
                        }
                    }
                    Err(e) => eprintln!("entur: {id}: {e}"),
                }
            }
        }
    });
    Ok(Box::new(Entur { entities, stopped }))
}

impl Instance for Entur {
    fn entities(&self) -> Vec<Entity> {
        self.entities.lock().unwrap().clone()
    }
    fn set(&self, _: &str, _: &str, _: &Value) -> Result<(), String> {
        Err("departures are read-only".into())
    }
    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_departures() {
        let v = json!({"data": {"stopPlace": {"name": "Jernbanetorget", "estimatedCalls": [
            {"expectedDepartureTime": "2026-11-20T23:55:23+01:00", "aimedDepartureTime": "2026-11-20T23:44:00+01:00", "realtime": true, "cancellation": false,
             "destinationDisplay": {"frontText": "Tonsenhagen"}, "quay": {"publicCode": "D"},
             "serviceJourney": {"line": {"publicCode": "60", "transportMode": "bus", "presentation": {"colour": "E60000", "textColour": "FFFFFF"}}}},
            {"expectedDepartureTime": "bad", "destinationDisplay": {}, "serviceJourney": {"line": {"presentation": {"colour": "red;}"}}}}
        ]}}});
        let (name, list) = parse(&v).unwrap();
        assert_eq!(name, "Jernbanetorget");
        assert_eq!(list.len(), 1, "a departure without a time is left out");
        let d = &list[0];
        assert_eq!((d["line"].as_str(), d["to"].as_str(), d["bg"].as_str(), d["rt"].as_bool()), (Some("60"), Some("Tonsenhagen"), Some("#E60000"), Some(true)));
        assert_eq!(d["at"].as_i64().unwrap() - d["aimed"].as_i64().unwrap(), 683, "11 minutes late");
        assert!(parse(&json!({"data": {"stopPlace": null}, "errors": [{"message": "nope"}]})).is_err());
        assert_eq!(encode("Oslo S/æ"), "Oslo%20S%2F%C3%A6");
    }

    #[test]
    #[ignore] // network: cargo test entur_live -- --ignored --nocapture
    fn entur_live() {
        println!("{}", test(json!({ "stops": "Jernbanetorget" }).as_object().unwrap()).unwrap());
    }
}
