//! Integrations connect oldpad to a home automation system.
//!
//! Each one is a `Kind`: a description of the fields it needs (which the admin
//! wizard turns into a form), a `test` used by the wizard's "Test connection"
//! step, and `start`, which returns a running instance. Instances report what
//! they know as `Entity` values and push changes through a `Sink`.
//!
//! To add one: write a module with a `pub fn kind() -> Kind`, add it to
//! `kinds()` below. See docs/integrations.md.

pub mod ais;
pub mod calendar;
pub mod demo;
pub mod entur;
pub mod homeassistant;
pub mod homey;
pub mod influx;
pub mod met;
pub mod smapi;
pub mod spot;
pub mod sonos;

use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)] // Number and Bool are offered to integrations; none uses them yet.
pub enum FieldKind {
    Text,
    Url,
    Secret,
    Number,
    Bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub required: bool,
    pub placeholder: &'static str,
    pub help: &'static str,
}

#[derive(Serialize, Clone, Debug)]
pub struct Capability {
    pub value: Value,
    pub settable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// What the device calls it ("Dim level", "Mode"), for the controls sheet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    /// The choices of an enum capability: `[{id, title}]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Value>,
}

impl Capability {
    pub fn new(value: Value, settable: bool) -> Self {
        Capability { value, settable, units: None, min: None, max: None, title: None, step: None, values: None }
    }
}

/// Something a tile can show or control: a light, a thermostat, a sensor, a flow.
/// Capability names follow Homey's (`onoff`, `dim` 0..1, `target_temperature`,
/// `measure_temperature`, …) so integrations map onto one shared vocabulary.
/// `button` is a write-only capability: setting it runs the thing (a Flow, a scene).
#[derive(Serialize, Clone, Debug)]
pub struct Entity {
    pub id: String, // unique within its integration instance
    pub name: String,
    pub zone: String,
    /// light, socket, thermostat, sensor, button, speaker, other
    pub class: String,
    pub capabilities: BTreeMap<String, Capability>,
}

pub enum Event {
    /// One capability changed value.
    Changed { entity: String, capability: String, value: Value },
    /// The entity list changed shape (added, removed, renamed).
    Reloaded,
    /// Keep `value` in this integration's stored config under `key`, which must
    /// start with `_`: state the integration earned (a service token), never
    /// shown to the browser and kept when the admin edits the fields.
    Remember { key: String, value: Value },
}

pub type Sink = Arc<dyn Fn(Event) + Send + Sync>;

pub trait Instance: Send + Sync {
    fn entities(&self) -> Vec<Entity>;
    fn set(&self, entity: &str, capability: &str, value: &Value) -> Result<(), String>;
    /// Things to play on `entity` matching `query`: `[{id, title, sub, kind}]`. The ids
    /// work with `play_favourite` and `add_favourite`.
    fn search(&self, _entity: &str, _query: &str) -> Result<Vec<Value>, String> {
        Err("search is not supported here".into())
    }
    /// An image the instance named in a capability (album art): `(content type, bytes)`.
    fn image(&self, _key: &str) -> Result<(String, Vec<u8>), String> {
        Err("no images here".into())
    }
    /// The latest picture from a camera entity: `(content type, bytes)`. Private, unlike
    /// `image`: only reached through the signed-in route, for screens assigned that camera.
    fn snapshot(&self, _entity: &str) -> Result<(String, Vec<u8>), String> {
        Err("no camera here".into())
    }
    /// Accounts the admin can connect, e.g. music services: `[{id, name, connected}]`.
    fn accounts(&self) -> Vec<Value> {
        vec![]
    }
    /// Starts connecting `account` and returns the URL where the admin signs in.
    /// The instance finishes in the background and keeps the result with `Event::Remember`.
    fn connect(&self, _account: &str) -> Result<String, String> {
        Err("nothing to connect".into())
    }
    /// Stop background work. Called before the instance is dropped.
    fn stop(&self);
}

pub struct Kind {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub fields: Vec<Field>,
    pub test: fn(&Map<String, Value>) -> Result<String, String>,
    pub start: fn(&Map<String, Value>, Sink) -> Result<Box<dyn Instance>, String>,
}

pub fn kinds() -> Vec<Kind> {
    vec![homey::kind(), homeassistant::kind(), influx::kind(), met::kind(), sonos::kind(), ais::kind(), spot::kind(), entur::kind(), calendar::kind(), demo::kind()]
}

/// Checks a submitted config against the kind's fields and returns it cleaned:
/// unknown keys dropped, types enforced. Secrets left empty keep `previous`.
pub fn validate(kind: &Kind, input: &Map<String, Value>, previous: Option<&Map<String, Value>>) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    for f in &kind.fields {
        let mut v = input.get(f.key).cloned().unwrap_or(Value::Null);
        if f.kind == FieldKind::Secret && v.as_str().is_none_or(str::is_empty) {
            v = previous.and_then(|p| p.get(f.key)).cloned().unwrap_or(Value::Null);
        }
        let v = match (f.kind, v) {
            (_, Value::Null) => Value::Null,
            (FieldKind::Bool, Value::Bool(b)) => Value::Bool(b),
            (FieldKind::Number, Value::Number(n)) => Value::Number(n),
            (FieldKind::Number, Value::String(s)) if s.trim().is_empty() => Value::Null,
            (FieldKind::Number, Value::String(s)) => s
                .trim()
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
                .ok_or_else(|| format!("{} must be a number", f.label))?,
            (FieldKind::Text | FieldKind::Secret, Value::String(s)) => {
                if s.len() > 2048 {
                    return Err(format!("{} is too long", f.label));
                }
                if s.trim().is_empty() { Value::Null } else { Value::String(s.trim().to_string()) }
            }
            (FieldKind::Url, Value::String(s)) => {
                let s = s.trim().trim_end_matches('/').to_string();
                if s.is_empty() {
                    Value::Null
                } else if !(s.starts_with("http://") || s.starts_with("https://")) || s.len() > 2048 {
                    return Err(format!("{} must start with http:// or https://", f.label));
                } else {
                    Value::String(s)
                }
            }
            _ => return Err(format!("{} has the wrong type", f.label)),
        };
        if v.is_null() {
            if f.required {
                return Err(format!("{} is required", f.label));
            }
        } else {
            out.insert(f.key.to_string(), v);
        }
    }
    // State the integration remembered survives edits of the fields.
    for (k, v) in previous.into_iter().flatten().filter(|(k, _)| k.starts_with('_')) {
        out.insert(k.clone(), v.clone());
    }
    Ok(out)
}

/// The config as the admin UI may see it: secrets replaced by a flag.
pub fn redact(kind: &Kind, config: &Map<String, Value>) -> Map<String, Value> {
    let mut out = config.clone();
    out.retain(|k, _| !k.starts_with('_'));
    for f in kind.fields.iter().filter(|f| f.kind == FieldKind::Secret) {
        if out.contains_key(f.key) {
            out.insert(f.key.to_string(), Value::Bool(true));
        }
    }
    out
}

pub fn get_str<'a>(config: &'a Map<String, Value>, key: &str) -> &'a str {
    config.get(key).and_then(Value::as_str).unwrap_or("")
}

struct Running {
    instance: Option<Arc<dyn Instance>>,
    status: String,
    /// Bumped on every (re)start, so a stale start thread knows to give up.
    generation: u64,
}

#[cfg(not(test))]
const RETRY_FIRST: std::time::Duration = std::time::Duration::from_secs(30);
#[cfg(test)]
const RETRY_FIRST: std::time::Duration = std::time::Duration::from_millis(20);
const RETRY_MAX: std::time::Duration = std::time::Duration::from_secs(600);
const MAX_SNAPSHOT: usize = 3 * 1024 * 1024; // an old iPad holds it in memory as base64

/// Running instances, keyed by integration id. Entity ids seen by the rest of
/// oldpad are "<integration id>:<entity id>".
pub struct Registry {
    pub kinds: Vec<Kind>,
    running: Mutex<HashMap<String, Running>>,
}

impl Registry {
    pub fn new() -> Self {
        Registry { kinds: kinds(), running: Mutex::new(HashMap::new()) }
    }

    pub fn kind(&self, id: &str) -> Option<&Kind> {
        self.kinds.iter().find(|k| k.id == id)
    }

    /// Starts an instance in the background; `start` may block on the network.
    /// Starts an instance in the background; `start` may block on the network.
    /// A failed start is retried with backoff (30 s doubling to 10 min), so a
    /// device that is offline when oldpad boots is picked up once it is back.
    pub fn start(self: &Arc<Self>, id: String, kind_id: String, config: Map<String, Value>, sink: Sink) {
        self.stop(&id);
        let generation = {
            let mut running = self.running.lock().unwrap();
            let generation = running.values().map(|r| r.generation).max().unwrap_or(0) + 1;
            running.insert(id.clone(), Running { instance: None, status: "starting".into(), generation });
            generation
        };
        let reg = Arc::clone(self);
        std::thread::spawn(move || {
            let Some(kind) = reg.kind(&kind_id) else { return };
            let mut backoff = RETRY_FIRST;
            loop {
                let result = (kind.start)(&config, sink.clone());
                let mut running = reg.running.lock().unwrap();
                // The integration may have been deleted or restarted while we connected.
                let Some(slot) = running.get_mut(&id).filter(|s| s.generation == generation) else {
                    if let Ok(inst) = result {
                        inst.stop();
                    }
                    return;
                };
                match result {
                    Ok(inst) => {
                        slot.instance = Some(Arc::from(inst));
                        slot.status = "running".into();
                        drop(running);
                        sink(Event::Reloaded);
                        return;
                    }
                    Err(e) => {
                        eprintln!("integration {id}: {e} (retrying in {} s)", backoff.as_secs());
                        slot.status = format!("error: {e}");
                    }
                }
                drop(running);
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(RETRY_MAX);
            }
        });
    }

    pub fn stop(&self, id: &str) {
        if let Some(r) = self.running.lock().unwrap().remove(id)
            && let Some(inst) = r.instance {
                inst.stop();
            }
    }

    pub fn status(&self, id: &str) -> String {
        self.running.lock().unwrap().get(id).map(|r| r.status.clone()).unwrap_or_else(|| "stopped".into())
    }

    fn instance(&self, id: &str) -> Option<Arc<dyn Instance>> {
        self.running.lock().unwrap().get(id).and_then(|r| r.instance.clone())
    }

    /// All entities, with ids prefixed by their integration id.
    pub fn entities(&self) -> Vec<Entity> {
        let instances: Vec<(String, Arc<dyn Instance>)> = self
            .running
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(id, r)| r.instance.clone().map(|i| (id.clone(), i)))
            .collect();
        let mut out = Vec::new();
        for (id, inst) in instances {
            for mut e in inst.entities() {
                e.id = format!("{id}:{}", e.id);
                out.push(e);
            }
        }
        out.sort_by(|a, b| (&a.zone, &a.name).cmp(&(&b.zone, &b.name)));
        out
    }

    pub fn entity(&self, full_id: &str) -> Option<Entity> {
        let (iid, eid) = full_id.split_once(':')?;
        let mut e = self.instance(iid)?.entities().into_iter().find(|e| e.id == eid)?;
        e.id = full_id.to_string();
        Some(e)
    }

    pub fn set(&self, full_id: &str, capability: &str, value: &Value) -> Result<(), String> {
        let (iid, eid) = full_id.split_once(':').ok_or("bad entity id")?;
        let inst = self.instance(iid).ok_or("integration is not running")?;
        let entity = inst.entities().into_iter().find(|e| e.id == eid).ok_or("no such entity")?;
        let cap = entity.capabilities.get(capability).ok_or("no such capability")?;
        if !cap.settable {
            return Err("capability is read-only".into());
        }
        // The lock is not held here: `set` talks to the network.
        inst.set(eid, capability, value)
    }

    pub fn image(&self, id: &str, key: &str) -> Result<(String, Vec<u8>), String> {
        self.instance(id).ok_or("integration is not running")?.image(key)
    }

    /// A camera's picture. Only plain image types: it ends up in a `data:` URL on the tablet.
    pub fn snapshot(&self, full_id: &str) -> Result<(String, Vec<u8>), String> {
        let (iid, eid) = full_id.split_once(':').ok_or("bad entity id")?;
        let inst = self.instance(iid).ok_or("integration is not running")?;
        if !inst.entities().iter().any(|e| e.id == eid && e.capabilities.contains_key("camera")) {
            return Err("not a camera".into());
        }
        let (kind, bytes) = inst.snapshot(eid)?;
        let kind = match kind.split(';').next().unwrap_or("").trim().to_ascii_lowercase().as_str() {
            // Home Assistant says image/jpg.
            "image/jpeg" | "image/jpg" => "image/jpeg",
            "image/png" => "image/png",
            "image/gif" => "image/gif",
            _ => return Err("the camera did not send a picture".into()),
        }
        .to_string();
        if bytes.is_empty() || bytes.len() > MAX_SNAPSHOT {
            return Err("the camera's picture is empty or too large".into());
        }
        Ok((kind, bytes))
    }

    pub fn accounts(&self, id: &str) -> Vec<Value> {
        self.instance(id).map(|i| i.accounts()).unwrap_or_default()
    }

    pub fn connect(&self, id: &str, account: &str) -> Result<String, String> {
        self.instance(id).ok_or("integration is not running")?.connect(account)
    }

    pub fn search(&self, full_id: &str, query: &str) -> Result<Vec<Value>, String> {
        let (iid, eid) = full_id.split_once(':').ok_or("bad entity id")?;
        self.instance(iid).ok_or("integration is not running")?.search(eid, query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn kind() -> Kind {
        Kind {
            id: "t",
            name: "T",
            description: "",
            fields: vec![
                Field { key: "url", label: "Address", kind: FieldKind::Url, required: true, placeholder: "", help: "" },
                Field { key: "key", label: "Key", kind: FieldKind::Secret, required: true, placeholder: "", help: "" },
                Field { key: "n", label: "N", kind: FieldKind::Number, required: false, placeholder: "", help: "" },
            ],
            test: |_| Ok(String::new()),
            start: |_, _| Err(String::new()),
        }
    }

    fn map(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn failed_starts_are_retried() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static TRIES: AtomicUsize = AtomicUsize::new(0);
        let mut k = kind();
        k.id = "flaky";
        k.start = |c, s| {
            if TRIES.fetch_add(1, Ordering::SeqCst) < 2 {
                return Err("not yet".into());
            }
            (super::demo::kind().start)(c, s)
        };
        let reg = Arc::new(Registry { kinds: vec![k], running: Mutex::new(HashMap::new()) });
        reg.start("x".into(), "flaky".into(), Map::new(), Arc::new(|_| {}));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while reg.status("x") != "running" {
            assert!(std::time::Instant::now() < deadline, "never recovered: {}", reg.status("x"));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(TRIES.load(Ordering::SeqCst), 3);
        assert!(!reg.entities().is_empty());
    }

    #[test]
    fn validates_and_keeps_secrets() {
        let k = kind();
        let ok = validate(&k, &map(json!({"url": " http://h/ ", "key": "s3", "n": "2", "junk": 1})), None).unwrap();
        assert_eq!(Value::Object(ok.clone()), json!({"url": "http://h", "key": "s3", "n": 2.0}));
        // Editing without retyping the secret keeps the old one.
        let edited = validate(&k, &map(json!({"url": "http://h2", "key": ""})), Some(&ok)).unwrap();
        assert_eq!(edited["key"], "s3");
        assert_eq!(redact(&k, &edited)["key"], true);
        // Remembered state (a service token) survives edits, can't be set from the form, and never leaves.
        let mut stored = edited.clone();
        stored.insert("_token".into(), json!({"t": "secret"}));
        let again = validate(&k, &map(json!({"url": "http://h3", "key": "", "_token": "forged", "_other": 1})), Some(&stored)).unwrap();
        assert_eq!((again["_token"].clone(), again.get("_other")), (json!({"t": "secret"}), None));
        assert!(!redact(&k, &again).contains_key("_token"));
        assert!(validate(&k, &map(json!({"url": "ftp://h", "key": "x"})), None).is_err());
        assert!(validate(&k, &map(json!({"key": "x"})), None).is_err());
        assert!(validate(&k, &map(json!({"url": "http://h", "key": "x", "n": "two"})), None).is_err());
    }
}
