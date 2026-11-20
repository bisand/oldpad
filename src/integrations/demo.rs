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
        description: "Pretend lights, a heater and a scene. For trying oldpad without any hardware.",
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
        entity("outdoor", "Outdoor", "Garden", "sensor", vec![("measure_temperature", temp(4.2, false))]),
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
        let cap = e.capabilities.get_mut(capability).ok_or("no such capability")?;
        let ok = match capability {
            "onoff" => value.is_boolean(),
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

    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}
