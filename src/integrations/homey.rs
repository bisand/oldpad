//! Homey Pro (2023 and later) over its local API. See docs/homey-protocol.md.

use super::{get_str, Capability, Entity, Event, Field, FieldKind, Instance, Kind, Sink};
use crate::client::{request, WebSocket};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn kind() -> Kind {
    Kind {
        id: "homey",
        name: "Homey Pro",
        description: "Lights, sockets, thermostats, sensors and Flows from a Homey Pro on your network.",
        fields: vec![
            Field {
                key: "address",
                label: "Homey address",
                kind: FieldKind::Url,
                required: true,
                placeholder: "http://192.168.1.50",
                help: "Homey app → Settings → General shows the local IP address. Give Homey a fixed address in your router.",
            },
            Field {
                key: "token",
                label: "API key",
                kind: FieldKind::Secret,
                required: true,
                placeholder: "",
                help: "my.homey.app → Settings → API keys → New API key. Tick: View devices, Control devices, View zones, View Flows, Start Flows. Nothing else.",
            },
        ],
        test,
        start,
    }
}

struct Api {
    base: String,
    auth: String,
}

impl Api {
    fn new(config: &Map<String, Value>) -> Self {
        Api { base: get_str(config, "address").to_string(), auth: format!("Bearer {}", get_str(config, "token")) }
    }

    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
        let body = body.map(|b| b.to_string().into_bytes());
        let res = request(&self.base, method, path, &[("Authorization", &self.auth)], body.as_deref())?;
        match res.status {
            200..=299 => Ok(if res.body.is_empty() { Value::Null } else { res.json()? }),
            401 | 403 => Err(format!("Homey refused the API key for {path} (HTTP {}). Check the key and its permissions.", res.status)),
            s => Err(format!("Homey answered HTTP {s} for {path}: {}", String::from_utf8_lossy(&res.body).chars().take(200).collect::<String>())),
        }
    }

    fn homey_id(&self) -> Result<String, String> {
        let res = request(&self.base, "GET", "/api/manager/system/ping", &[], None)?;
        res.headers.get("x-homey-id").cloned().ok_or_else(|| "this address does not look like a Homey (no X-Homey-ID)".into())
    }
}

/// Homey returns collections as objects keyed by id; accept arrays too.
fn items(v: &Value) -> Vec<&Value> {
    match v {
        Value::Object(m) => m.values().collect(),
        Value::Array(a) => a.iter().collect(),
        _ => vec![],
    }
}

fn test(config: &Map<String, Value>) -> Result<String, String> {
    let api = Api::new(config);
    let id = api.homey_id()?;
    let devices = api.call("GET", "/api/manager/devices/device", None)?;
    Ok(format!("Connected to Homey {id}. {} devices visible.", items(&devices).len()))
}

/// Homey titles are a string in the Homey's language, or sometimes `{en, nb, …}`.
fn text(v: &Value) -> Option<String> {
    v.as_str().or_else(|| v["en"].as_str()).map(str::to_string)
}

fn map_device(d: &Value, zones: &HashMap<String, String>) -> Option<Entity> {
    let id = d["id"].as_str()?.to_string();
    let mut capabilities = BTreeMap::new();
    if let Some(caps) = d["capabilitiesObj"].as_object() {
        for (cid, c) in caps {
            capabilities.insert(
                cid.clone(),
                Capability {
                    value: c["value"].clone(),
                    settable: c["setable"].as_bool().unwrap_or(false),
                    units: c["units"].as_str().map(str::to_string),
                    min: c["min"].as_f64(),
                    max: c["max"].as_f64(),
                    title: text(&c["title"]),
                    step: c["step"].as_f64(),
                    values: c["values"].as_array().map(|vs| {
                        vs.iter().filter_map(|v| Some(json!({ "id": v["id"].as_str()?, "title": text(&v["title"]).unwrap_or_default() }))).collect()
                    }),
                },
            );
        }
    }
    Some(Entity {
        id,
        name: d["name"].as_str().unwrap_or("?").to_string(),
        zone: zones.get(d["zone"].as_str().unwrap_or("")).cloned().unwrap_or_default(),
        class: d["class"].as_str().unwrap_or("other").to_string(),
        capabilities,
    })
}

fn map_flow(f: &Value, prefix: &str) -> Option<Entity> {
    // Only Flows that start with "This Flow is started" can be run from outside.
    if f["triggerable"].as_bool() != Some(true) || f["enabled"].as_bool() == Some(false) {
        return None;
    }
    Some(Entity {
        id: format!("{prefix}{}", f["id"].as_str()?),
        name: f["name"].as_str().unwrap_or("Flow").to_string(),
        zone: "Flows".into(),
        class: "button".into(),
        capabilities: BTreeMap::from([("button".to_string(), Capability::new(Value::Null, true))]),
    })
}

fn load(api: &Api) -> Result<Vec<Entity>, String> {
    let zones: HashMap<String, String> = items(&api.call("GET", "/api/manager/zones/zone", None)?)
        .into_iter()
        .filter_map(|z| Some((z["id"].as_str()?.to_string(), z["name"].as_str()?.to_string())))
        .collect();
    let mut out: Vec<Entity> = items(&api.call("GET", "/api/manager/devices/device", None)?)
        .into_iter()
        .filter_map(|d| map_device(d, &zones))
        .collect();
    // Flows are optional: a key without the Flow permissions still gives devices.
    if let Ok(flows) = api.call("GET", "/api/manager/flow/flow", None) {
        out.extend(items(&flows).into_iter().filter_map(|f| map_flow(f, "flow-")));
    }
    if let Ok(flows) = api.call("GET", "/api/manager/flow/advancedflow", None) {
        out.extend(items(&flows).into_iter().filter_map(|f| map_flow(f, "aflow-")));
    }
    Ok(out)
}

struct Homey {
    api: Arc<Api>,
    entities: Arc<Mutex<Vec<Entity>>>,
    stopped: Arc<AtomicBool>,
}

fn start(config: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let api = Arc::new(Api::new(config));
    let homey_id = api.homey_id()?;
    let entities = Arc::new(Mutex::new(load(&api)?));
    let stopped = Arc::new(AtomicBool::new(false));

    let (a, e, s, stop) = (api.clone(), entities.clone(), sink, stopped.clone());
    std::thread::spawn(move || realtime_loop(&a, &homey_id, &e, &s, &stop));
    Ok(Box::new(Homey { api, entities, stopped }))
}

impl Instance for Homey {
    fn entities(&self) -> Vec<Entity> {
        self.entities.lock().unwrap().clone()
    }

    fn set(&self, entity: &str, capability: &str, value: &Value) -> Result<(), String> {
        let enc = |s: &str| s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        // Sub-capabilities have a dot: `measure_temperature.inside`.
        if !enc(entity) || !enc(&capability.replace('.', "")) {
            return Err("bad id".into());
        }
        if let Some(id) = entity.strip_prefix("aflow-") {
            return self.api.call("POST", &format!("/api/manager/flow/advancedflow/{id}/trigger"), Some(json!({}))).map(drop);
        }
        if let Some(id) = entity.strip_prefix("flow-") {
            return self.api.call("POST", &format!("/api/manager/flow/flow/{id}/trigger"), Some(json!({}))).map(drop);
        }
        self.api
            .call("PUT", &format!("/api/manager/devices/device/{entity}/capability/{capability}"), Some(json!({ "value": value })))
            .map(drop)
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

/// A parsed socket.io v2 packet: `4` + type + optional namespace + optional ack id + JSON.
#[derive(Debug, PartialEq)]
pub struct Packet {
    pub kind: char,
    pub nsp: String,
    pub ack: Option<u64>,
    pub data: Value,
}

pub fn parse_packet(msg: &str) -> Option<Packet> {
    let rest = msg.strip_prefix('4')?;
    let mut chars = rest.chars();
    let kind = chars.next()?;
    let mut rest = chars.as_str();
    let mut nsp = "/".to_string();
    if rest.starts_with('/') {
        let end = rest.find(',').unwrap_or(rest.len());
        nsp = rest[..end].to_string();
        rest = rest.get(end + 1..).unwrap_or("");
    }
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let ack = if digits > 0 { rest[..digits].parse().ok() } else { None };
    let json = &rest[digits..];
    let data = if json.is_empty() { Value::Null } else { serde_json::from_str(json).ok()? };
    Some(Packet { kind, nsp, ack, data })
}

fn emit(nsp: &str, ack: u64, args: Value) -> String {
    if nsp == "/" { format!("42{ack}{args}") } else { format!("42{nsp},{ack}{args}") }
}

/// Waits for a packet matching `pred`, answering pings meanwhile.
fn wait_for(ws: &mut WebSocket, deadline: Instant, pred: impl Fn(&Packet) -> bool) -> Result<Packet, String> {
    while Instant::now() < deadline {
        if let Some(msg) = ws.recv()?
            && let Some(p) = parse_packet(&msg).filter(|p| pred(p)) {
                return Ok(p);
            }
    }
    Err("timed out waiting for Homey".into())
}

fn session(api: &Api, homey_id: &str, entities: &Mutex<Vec<Entity>>, sink: &Sink, stopped: &AtomicBool) -> Result<(), String> {
    let mut ws = WebSocket::connect(&api.base, "/socket.io/?EIO=3&transport=websocket")?;
    ws.set_poll_interval(Duration::from_secs(1));
    let deadline = || Instant::now() + Duration::from_secs(10);

    // Engine.IO open packet: 0{"pingInterval":…}
    let open_by = deadline();
    let ping_every = loop {
        if Instant::now() >= open_by {
            return Err("Homey sent no socket.io open packet".into());
        }
        if let Some(open) = ws.recv()?.as_deref().and_then(|m| m.strip_prefix('0')) {
            let v: Value = serde_json::from_str(open).unwrap_or_default();
            break Duration::from_millis(v["pingInterval"].as_u64().unwrap_or(25_000));
        }
    };

    let token = api.auth.trim_start_matches("Bearer ");
    ws.send(&emit("/", 1, json!(["handshakeClient", { "token": token, "homeyId": homey_id }])))?;
    let ack = wait_for(&mut ws, deadline(), |p| p.kind == '3' && p.ack == Some(1))?;
    if !ack.data[0].is_null() {
        return Err(format!("Homey refused the realtime handshake: {}", ack.data[0]));
    }
    let nsp = ack.data[1]["namespace"].as_str().ok_or("no namespace in handshake reply")?.to_string();
    ws.send(&format!("40{nsp}"))?;
    wait_for(&mut ws, deadline(), |p| p.kind == '0' && p.nsp == nsp)?;

    // Subscribe to the devices manager (for added/removed devices) and each device.
    let ids: Vec<String> = entities.lock().unwrap().iter().filter(|e| !e.id.contains("flow-")).map(|e| e.id.clone()).collect();
    let mut uris = vec!["homey:manager:devices".to_string()];
    uris.extend(ids.iter().map(|id| format!("homey:device:{id}")));
    for (i, uri) in uris.iter().enumerate() {
        ws.send(&emit(&nsp, 10 + i as u64, json!(["subscribe", uri])))?;
    }
    eprintln!("homey: realtime connected, {} subscriptions", uris.len());

    let mut last_ping = Instant::now();
    while !stopped.load(Ordering::Relaxed) {
        if last_ping.elapsed() >= ping_every {
            ws.send("2")?;
            last_ping = Instant::now();
        }
        let Some(msg) = ws.recv()? else { continue };
        let Some(p) = parse_packet(&msg) else { continue };
        if crate::util::verbose() {
            // Event data can be large (whole device objects); the start says enough.
            let shown: String = p.data.to_string().chars().take(300).collect();
            eprintln!("homey: <- type {} {} {shown}", p.kind, p.nsp);
        }
        if p.kind == '1' && p.nsp == nsp {
            return Err("Homey closed the namespace".into());
        }
        if p.kind != '2' {
            continue;
        }
        // ["homey:device:<id>", "capability", {capabilityId, value}] or ["homey:manager:devices", "device.create", …]
        let (uri, event, data) = (p.data[0].as_str().unwrap_or(""), p.data[1].as_str().unwrap_or(""), &p.data[2]);
        if let Some(id) = uri.strip_prefix("homey:device:").filter(|_| event == "capability") {
            let (Some(cap), value) = (data["capabilityId"].as_str(), data["value"].clone()) else { continue };
            let mut list = entities.lock().unwrap();
            if let Some(c) = list.iter_mut().find(|e| e.id == id).and_then(|e| e.capabilities.get_mut(cap)) {
                c.value = value.clone();
                drop(list);
                sink(Event::Changed { entity: id.to_string(), capability: cap.to_string(), value });
            }
        } else if uri == "homey:manager:devices" && matches!(event, "device.create" | "device.delete") {
            // A new or removed device: reconnect, which reloads and resubscribes.
            return Err("device list changed".into());
        }
    }
    Ok(())
}

fn realtime_loop(api: &Api, homey_id: &str, entities: &Mutex<Vec<Entity>>, sink: &Sink, stopped: &AtomicBool) {
    let mut backoff = Duration::from_secs(2);
    let mut first = true;
    while !stopped.load(Ordering::Relaxed) {
        if !first {
            // Reload after every reconnect: values may have changed while we were away.
            match load(api) {
                Ok(list) => {
                    *entities.lock().unwrap() = list;
                    sink(Event::Reloaded);
                }
                Err(e) => eprintln!("homey: reload failed: {e}"),
            }
        }
        first = false;
        let started = Instant::now();
        match session(api, homey_id, entities, sink, stopped) {
            Ok(()) => return,
            Err(e) => eprintln!("homey: realtime: {e}"),
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

    #[test]
    fn parses_socket_io_packets() {
        assert_eq!(
            parse_packet(r#"431[null,{"namespace":"/api/abc"}]"#).unwrap(),
            Packet { kind: '3', nsp: "/".into(), ack: Some(1), data: json!([null, {"namespace": "/api/abc"}]) }
        );
        let p = parse_packet(r#"42/api/abc,["homey:device:d1","capability",{"capabilityId":"onoff","value":true}]"#).unwrap();
        assert_eq!((p.kind, p.nsp.as_str(), p.ack), ('2', "/api/abc", None));
        assert_eq!(p.data[2]["value"], true);
        assert_eq!(parse_packet("40/api/abc").unwrap().nsp, "/api/abc");
        assert_eq!(parse_packet("40").unwrap().kind, '0');
        assert!(parse_packet("3").is_none());
        assert_eq!(emit("/api/x", 12, json!(["subscribe", "u"])), r#"42/api/x,12["subscribe","u"]"#);
        assert_eq!(emit("/", 1, json!(["a"])), r#"421["a"]"#);
    }

    #[test]
    fn maps_devices_and_flows() {
        let zones = HashMap::from([("z1".to_string(), "Stue".to_string())]);
        let d = json!({"id": "d1", "name": "Taklampe", "zone": "z1", "class": "light",
            "capabilitiesObj": {"onoff": {"value": true, "setable": true}, "dim": {"value": 0.4, "setable": true, "min": 0, "max": 1, "title": "Dimme", "step": 0.01},
                "thermostat_mode": {"value": "heat", "setable": true, "title": {"en": "Mode"}, "values": [{"id": "heat", "title": "Varme"}, {"id": "off", "title": {"en": "Off"}}]}}});
        let e = map_device(&d, &zones).unwrap();
        assert_eq!((e.zone.as_str(), e.class.as_str()), ("Stue", "light"));
        assert!(e.capabilities["onoff"].settable);
        assert_eq!(e.capabilities["dim"].max, Some(1.0));
        assert_eq!((e.capabilities["dim"].title.as_deref(), e.capabilities["dim"].step), (Some("Dimme"), Some(0.01)));
        assert_eq!(e.capabilities["thermostat_mode"].title.as_deref(), Some("Mode"));
        assert_eq!(e.capabilities["thermostat_mode"].values, Some(json!([{"id": "heat", "title": "Varme"}, {"id": "off", "title": "Off"}])));

        assert!(map_flow(&json!({"id": "f1", "name": "God natt", "triggerable": true}), "flow-").is_some());
        assert!(map_flow(&json!({"id": "f2", "name": "Auto", "triggerable": false}), "flow-").is_none());
        assert!(map_flow(&json!({"id": "f3", "triggerable": true, "enabled": false}), "flow-").is_none());
    }
}
