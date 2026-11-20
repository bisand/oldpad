//! A fake house, for trying oldpad without hardware and as the smallest
//! example of an integration.

use super::{Capability, Entity, Event, Field, FieldKind, Instance, Kind, Sink};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn kind() -> Kind {
    Kind {
        id: "demo",
        name: "Demo house",
        description: "Pretend lights, a heater, sensors, a speaker and a scene. For trying oldpad without any hardware.",
        fields: vec![Field {
            key: "zone",
            label: "Room name",
            kind: FieldKind::Text,
            required: false,
            placeholder: "Living room",
            help: "Shown under each tile.",
        }],
        test: |_| Ok("The demo house is always reachable.".into()),
        start,
    }
}

const STATIONS: [&str; 6] = ["Jazz Radio", "Jazz FM", "Classic Rock", "Rock Antenne", "News 24", "Lo-fi Beats"];

struct Demo {
    entities: Arc<Mutex<Vec<Entity>>>,
    stopped: Arc<AtomicBool>,
    sink: Sink,
}

fn entity(id: &str, name: &str, zone: &str, class: &str, caps: Vec<(&str, Capability)>) -> Entity {
    Entity {
        id: id.into(),
        name: name.into(),
        zone: zone.into(),
        class: class.into(),
        capabilities: caps.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>(),
    }
}

/// A room that plays with `group`'s room; rooms in one group share what plays.
fn speaker(id: &str, room: &str, group: &str, playing: bool, volume: f64, track: &str) -> Entity {
    let favs: Vec<Value> = ["Radio One", "Radio Two", "Jazz", "Classical", "Morning mix", "Focus"].iter().enumerate().map(|(i, t)| json!({ "id": format!("f{i}"), "title": t })).collect();
    entity(id, room, room, "speaker", vec![
        ("speaker_playing", Capability::new(json!(playing), true)),
        ("volume_set", Capability { min: Some(0.0), max: Some(1.0), ..Capability::new(json!(volume), true) }),
        ("speaker_track", Capability::new(json!(track), false)),
        ("speaker_artist", Capability::new(json!(if track.is_empty() { "" } else { "Morning news" }), false)),
        ("favourites", Capability::new(json!(favs), false)),
        ("play_favourite", Capability::new(Value::Null, true)),
        ("add_favourite", Capability::new(Value::Null, true)),
        ("speaker_group", Capability::new(json!(group), true)),
    ])
}

fn start(config: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let zone = match super::get_str(config, "zone") {
        "" => "Living room",
        z => z,
    };
    let temp = |v: f64, settable| Capability { units: Some("°C".into()), min: Some(5.0), max: Some(30.0), ..Capability::new(json!(v), settable) };
    let entities = Arc::new(Mutex::new(vec![
        entity("lamp", "Ceiling lamp", zone, "light", vec![("onoff", Capability::new(json!(true), true)), ("dim", Capability::new(json!(0.6), true))]),
        entity("reading", "Reading lamp", zone, "light", vec![("onoff", Capability::new(json!(false), true))]),
        entity("heater", "Heater", zone, "thermostat", vec![("target_temperature", temp(21.5, true)), ("measure_temperature", temp(20.8, false))]),
        entity("plug", "Coffee maker", "Kitchen", "socket", vec![("onoff", Capability::new(json!(false), true))]),
        entity("indoor", "Indoor", zone, "sensor", vec![
            ("measure_temperature", temp(21.4, false)),
            ("temperature_min", temp(19.8, false)),
            ("temperature_max", temp(22.1, false)),
            ("measure_humidity", Capability::new(json!(42), false)),
            ("measure_co2", Capability::new(json!(612), false)),
        ]),
        entity("outdoor", "Outdoor", "Garden", "sensor", vec![
            ("measure_temperature", temp(4.2, false)),
            ("temperature_min", temp(1.3, false)),
            ("temperature_max", temp(6.0, false)),
            ("measure_humidity", Capability::new(json!(87), false)),
            ("measure_wind_strength", Capability::new(json!(10.8), false)),
        ]),
        speaker("speaker", zone, "speaker", true, 0.28, "Radio One"),
        speaker("kitchen", "Kitchen", "speaker", true, 0.2, "Radio One"),
        speaker("bath", "Bathroom", "bath", false, 0.15, ""),
        entity("night", "Good night", "Scenes", "button", vec![("button", Capability::new(Value::Null, true))]),
    ]));
    let stopped = Arc::new(AtomicBool::new(false));

    // Let the indoor temperature drift towards the setpoint, so live updates can be seen.
    let (ents, stop, s) = (entities.clone(), stopped.clone(), sink.clone());
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(10));
            let mut list = ents.lock().unwrap();
            let Some(h) = list.iter_mut().find(|e| e.id == "heater") else { continue };
            let target = h.capabilities["target_temperature"].value.as_f64().unwrap_or(21.0);
            let cur = h.capabilities.get_mut("measure_temperature").unwrap();
            let now = cur.value.as_f64().unwrap_or(20.0);
            let next = ((now + (target - now).clamp(-0.1, 0.1)) * 10.0).round() / 10.0;
            if next != now {
                cur.value = json!(next);
                drop(list);
                s(Event::Changed { entity: "heater".into(), capability: "measure_temperature".into(), value: json!(next) });
            }
        }
    });

    Ok(Box::new(Demo { entities, stopped, sink }))
}

impl Instance for Demo {
    fn entities(&self) -> Vec<Entity> {
        self.entities.lock().unwrap().clone()
    }

    fn set(&self, entity: &str, capability: &str, value: &Value) -> Result<(), String> {
        let mut list = self.entities.lock().unwrap();
        let e = list.iter_mut().find(|e| e.id == entity).ok_or("no such entity")?;
        if capability == "button" {
            eprintln!("demo: ran {}", e.name);
            return Ok(());
        }
        if capability == "speaker_group" {
            let target = value.as_str().ok_or("expected a room")?.to_string();
            let speakers = |l: &Vec<Entity>| l.iter().filter(|e| e.capabilities.contains_key("speaker_group")).map(|e| e.id.clone()).collect::<Vec<_>>();
            let lead = |l: &Vec<Entity>, id: &str| l.iter().find(|e| e.id == id).map(|e| e.capabilities["speaker_group"].value.clone());
            let old = lead(&list, entity).ok_or("no such room")?;
            let new = if target == entity { json!(entity) } else { lead(&list, &target).ok_or("no such room")? };
            let mut changes = vec![(entity.to_string(), new.clone())];
            if old == json!(entity) {
                // The leader left or moved: the rest of its group follows its first remaining member.
                let rest: Vec<String> = speakers(&list).into_iter().filter(|id| id != entity && lead(&list, id) == Some(old.clone())).collect();
                if let Some(first) = rest.first() {
                    changes.extend(rest.iter().map(|id| (id.clone(), json!(first))));
                }
            }
            for (id, v) in &changes {
                list.iter_mut().find(|e| e.id == *id).unwrap().capabilities.get_mut("speaker_group").unwrap().value = v.clone();
            }
            drop(list);
            for (id, v) in changes {
                (self.sink)(Event::Changed { entity: id, capability: "speaker_group".into(), value: v });
            }
            return Ok(());
        }
        if capability == "add_favourite" {
            let i = value.as_str().and_then(|v| v.strip_prefix("radio:")).and_then(|i| i.parse::<usize>().ok()).filter(|i| *i < STATIONS.len()).ok_or("no such station")?;
            let mut changed = vec![];
            for e in list.iter_mut().filter(|e| e.capabilities.contains_key("favourites")) {
                let favs = e.capabilities.get_mut("favourites").unwrap();
                let arr = favs.value.as_array_mut().unwrap();
                if !arr.iter().any(|f| f["id"] == *value) {
                    arr.push(json!({ "id": value, "title": STATIONS[i] }));
                }
                changed.push((e.id.clone(), favs.value.clone()));
            }
            drop(list);
            for (id, v) in changed {
                (self.sink)(Event::Changed { entity: id, capability: "favourites".into(), value: v });
            }
            return Ok(());
        }
        if capability == "play_favourite" {
            let favs = e.capabilities["favourites"].value.clone();
            let station = value.as_str().and_then(|v| v.strip_prefix("radio:")).and_then(|i| i.parse::<usize>().ok()).and_then(|i| STATIONS.get(i));
            let title = favs.as_array().into_iter().flatten().find(|f| f["id"] == *value).map(|f| f["title"].clone()).or(station.map(|t| json!(t))).ok_or("no such favourite")?;
            e.capabilities.get_mut("speaker_track").unwrap().value = title.clone();
            e.capabilities.get_mut("speaker_playing").unwrap().value = json!(true);
            drop(list);
            (self.sink)(Event::Changed { entity: entity.into(), capability: "speaker_track".into(), value: title });
            (self.sink)(Event::Changed { entity: entity.into(), capability: "speaker_playing".into(), value: json!(true) });
            return Ok(());
        }
        let cap = e.capabilities.get_mut(capability).ok_or("no such capability")?;
        let ok = match capability {
            "onoff" | "speaker_playing" => value.is_boolean(),
            "volume_set" => value.as_f64().is_some_and(|v| (0.0..=1.0).contains(&v)),
            "dim" => value.as_f64().is_some_and(|v| (0.0..=1.0).contains(&v)),
            _ => value.as_f64().is_some_and(|v| cap.min.is_none_or(|m| v >= m) && cap.max.is_none_or(|m| v <= m)),
        };
        if !ok {
            return Err(format!("invalid value for {capability}"));
        }
        cap.value = value.clone();
        drop(list);
        (self.sink)(Event::Changed { entity: entity.into(), capability: capability.into(), value: value.clone() });
        Ok(())
    }

    fn search(&self, _entity: &str, query: &str) -> Result<Vec<Value>, String> {
        let q = query.to_lowercase();
        Ok(STATIONS.iter().enumerate().filter(|(_, t)| t.to_lowercase().contains(&q)).map(|(i, t)| json!({ "id": format!("radio:{i}"), "title": t, "sub": "Demo", "kind": "radio" })).collect())
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}
