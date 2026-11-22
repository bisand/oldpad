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
        // A plug the user told Homey has a lamp in it is a light here too.
        class: if d["virtualClass"] == "light" { "light" } else { d["class"].as_str().unwrap_or("other") }.to_string(),
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
    let zones_raw = api.call("GET", "/api/manager/zones/zone", None)?;
    let zones: HashMap<String, String> = items(&zones_raw).into_iter().filter_map(|z| Some((z["id"].as_str()?.to_string(), z["name"].as_str()?.to_string()))).collect();
    let devices = api.call("GET", "/api/manager/devices/device", None)?;
    let mut out: Vec<Entity> = items(&devices).into_iter().filter_map(|d| map_device(d, &zones)).collect();
    let house = House::new(&zones_raw, &devices);
    out.extend(load_flows(api, &house));
    Ok(out)
}

/// What the zoneLights HomeyScript (docs/homey-zone-lights.md) would switch, so a
/// Flow that runs it can show On while any of its lights is on.
struct House {
    zones: Vec<(String, String, Option<String>)>, // id, name, parent
    lights: Vec<(String, String, bool)>,          // device id, zone id, on
    switches: Vec<((String, String, bool), String)>, // any device with on/off, by its loose name
}

/// Zone names may hold non-breaking spaces; compare loosely, as the script does.
fn loose(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

impl House {
    fn new(zones: &Value, devices: &Value) -> Self {
        let zones = items(zones).into_iter().filter_map(|z| Some((z["id"].as_str()?.to_string(), z["name"].as_str()?.to_string(), z["parent"].as_str().map(str::to_string)))).collect();
        let state = |d: &Value| Some((d["id"].as_str()?.to_string(), d["zone"].as_str()?.to_string(), d["capabilitiesObj"]["onoff"]["value"].as_bool()?));
        let usable: Vec<&Value> = items(devices).into_iter().filter(|d| d["available"] != false).collect();
        let lights = usable
            .iter()
            .filter(|d| d["class"] == "light" || d["virtualClass"] == "light")
            .filter(|d| !d["driverId"].as_str().unwrap_or("").contains("com.swttt.devicegroups"))
            .filter_map(|d| state(d))
            .collect();
        let switches = usable.iter().filter_map(|d| Some((state(d)?, loose(d["name"].as_str()?)))).collect();
        House { zones, lights, switches }
    }

    /// What "toggle: Zone, Device, …" switches: the lights in each zone (sub-zones included),
    /// and each named device. None if a name is neither, as the script then does nothing.
    fn members(&self, argument: &str) -> Option<Vec<&(String, String, bool)>> {
        let (_, list) = argument.split_once(':')?;
        let mut inside: Vec<&str> = Vec::new();
        let mut out = Vec::new();
        for want in list.split(',').map(loose).filter(|s| !s.is_empty()) {
            if let Some(z) = self.zones.iter().find(|z| loose(&z.1) == want) {
                inside.push(&z.0);
            } else {
                out.push(&self.switches.iter().find(|s| s.1 == want)?.0);
            }
        }
        while let Some(child) = self.zones.iter().find(|z| !inside.contains(&z.0.as_str()) && z.2.as_deref().is_some_and(|p| inside.contains(&p))) {
            inside.push(&child.0);
        }
        let zoned: Vec<_> = self.lights.iter().filter(|l| inside.contains(&l.1.as_str()) && !out.iter().any(|o| o.0 == l.0)).collect();
        out.extend(zoned);
        Some(out)
    }
}

/// The argument a Flow gives the zoneLights script, from either kind of Flow.
fn zone_argument(f: &Value) -> Option<String> {
    let cards: Vec<&Value> = f["cards"].as_object().map(|c| c.values().collect()).unwrap_or_else(|| f["actions"].as_array().map(|a| a.iter().collect()).unwrap_or_default());
    cards.into_iter().find_map(|c| {
        let runs = c["args"]["script"]["name"] == "zoneLights" || c["args"]["code"].as_str().is_some_and(|code| code.contains("zoneLights"));
        c["args"]["argument"].as_str().filter(|_| runs).map(str::to_string)
    })
}

/// Gives a lights Flow a read-only `onoff` (any light on) and its `members`.
fn group(e: &mut Entity, members: &[&(String, String, bool)]) {
    let ids: Vec<&String> = members.iter().map(|m| &m.0).collect();
    e.capabilities.insert("onoff".into(), Capability::new(json!(members.iter().any(|m| m.2)), false));
    e.capabilities.insert("members".into(), Capability::new(json!(ids), false));
}

/// A light changed: the lights Flows it belongs to, with their new state if that changed.
fn regroup(list: &mut [Entity], device: &str) -> Vec<(String, Value)> {
    let on = |list: &[Entity], id: &str| list.iter().find(|e| e.id == id).and_then(|e| e.capabilities.get("onoff")).is_some_and(|c| c.value == true);
    let mut changed = Vec::new();
    for i in 0..list.len() {
        let Some(ids) = list[i].capabilities.get("members").and_then(|c| c.value.as_array()).cloned() else { continue };
        if !ids.iter().any(|m| m == device) {
            continue;
        }
        let any = json!(ids.iter().filter_map(Value::as_str).any(|m| on(list, m)));
        let c = list[i].capabilities.get_mut("onoff").unwrap();
        if c.value != any {
            c.value = any.clone();
            changed.push((list[i].id.clone(), any));
        }
    }
    changed
}

/// Zone id → (name, parent).
fn load_zones(api: &Api) -> Result<HashMap<String, (String, Value)>, String> {
    Ok(items(&api.call("GET", "/api/manager/zones/zone", None)?)
        .into_iter()
        .filter_map(|z| Some((z["id"].as_str()?.to_string(), (z["name"].as_str()?.to_string(), z["parent"].clone()))))
        .collect())
}

/// Zones report motion ("active") all the time; only a new name or parent matters here.
fn zone_reshaped(zones: &HashMap<String, (String, Value)>, event: &str, z: &Value) -> bool {
    event != "zone.update" || zones.get(z["id"].as_str().unwrap_or("")).is_none_or(|(name, parent)| z["name"] != name.as_str() || z["parent"] != *parent)
}

/// Takes a renamed or moved device's new name, room and kind. None if nothing changed,
/// else whether it moved room (or became or stopped being a light), which lights Flows care about.
fn apply_update(list: &mut [Entity], device: &Value, zones: &HashMap<String, String>) -> Option<bool> {
    let new = map_device(device, zones)?;
    let e = list.iter_mut().find(|e| e.id == new.id)?;
    if (&e.name, &e.zone, &e.class) == (&new.name, &new.zone, &new.class) {
        return None;
    }
    let moved = (&e.zone, &e.class) != (&new.zone, &new.class);
    (e.name, e.zone, e.class) = (new.name, new.zone, new.class);
    Some(moved)
}

/// The Flows that can be started. Optional: a key without the Flow permissions still gives devices.
fn load_flows(api: &Api, house: &House) -> Vec<Entity> {
    let mut out = Vec::new();
    for (path, prefix) in [("/api/manager/flow/flow", "flow-"), ("/api/manager/flow/advancedflow", "aflow-")] {
        let Ok(flows) = api.call("GET", path, None) else { continue };
        for f in items(&flows) {
            let Some(mut e) = map_flow(f, prefix) else { continue };
            if let Some(members) = zone_argument(f).and_then(|a| house.members(&a)) {
                group(&mut e, &members);
            }
            out.push(e);
        }
    }
    out
}

fn is_flow(e: &Entity) -> bool {
    e.id.starts_with("flow-") || e.id.starts_with("aflow-")
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
    let ids: Vec<String> = entities.lock().unwrap().iter().filter(|e| !is_flow(e)).map(|e| e.id.clone()).collect();
    // The Flow and zone managers too, so new Flows and renamed rooms show up without a restart.
    let mut uris = vec!["homey:manager:devices".to_string(), "homey:manager:flow".to_string(), "homey:manager:zones".to_string()];
    let tree = load_zones(api)?;
    let zones: HashMap<String, String> = tree.iter().map(|(id, (name, _))| (id.clone(), name.clone())).collect();
    uris.extend(ids.iter().map(|id| format!("homey:device:{id}")));
    for (i, uri) in uris.iter().enumerate() {
        ws.send(&emit(&nsp, 10 + i as u64, json!(["subscribe", uri])))?;
    }
    eprintln!("homey: realtime connected, {} subscriptions", uris.len());
    sink(Event::Healthy(true));

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
                let flows = if cap == "onoff" { regroup(&mut list, id) } else { vec![] };
                drop(list);
                sink(Event::Changed { entity: id.to_string(), capability: cap.to_string(), value });
                for (flow, on) in flows {
                    sink(Event::Changed { entity: flow, capability: "onoff".into(), value: on });
                }
            }
        } else if uri == "homey:manager:flow" && event.contains("flow.") {
            // Flows don't report runs, only edits; reload only when the startable list really changed.
            let fresh: Vec<Entity> = load(api)?.into_iter().filter(is_flow).collect();
            let mut list = entities.lock().unwrap();
            let names = |l: &mut dyn Iterator<Item = &Entity>| l.map(|e| (e.id.clone(), e.name.clone(), e.capabilities.get("members").map(|c| c.value.clone()))).collect::<Vec<_>>();
            if names(&mut list.iter().filter(|e| is_flow(e))) != names(&mut fresh.iter()) {
                list.retain(|e| !is_flow(e));
                list.extend(fresh);
                drop(list);
                eprintln!("homey: flows changed");
                sink(Event::Reloaded);
            }
        } else if uri == "homey:manager:devices" && event == "device.update" {
            // Sent on renames and moves, not on value changes; also when a device comes and goes.
            match apply_update(&mut entities.lock().unwrap(), data, &zones) {
                // A move changes which lights Flows hold it: reconnect, which reloads them.
                Some(true) => return Err(format!("{} moved", data["name"])),
                Some(false) => {
                    eprintln!("homey: {} renamed", data["name"]);
                    sink(Event::Reloaded);
                }
                None => {}
            }
        } else if uri == "homey:manager:zones" && event.starts_with("zone.") && zone_reshaped(&tree, event, data) {
            // A renamed or new room: reconnect, which reloads names and rooms.
            return Err("zones changed".into());
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
            Err(e) => {
                eprintln!("homey: realtime: {e}");
                sink(Event::Healthy(false));
            }
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
    fn zone_motion_is_not_a_change() {
        let tree = HashMap::from([("s".to_string(), ("Stue".to_string(), json!("o")))]);
        assert!(!zone_reshaped(&tree, "zone.update", &json!({"id": "s", "name": "Stue", "parent": "o", "active": true})));
        assert!(zone_reshaped(&tree, "zone.update", &json!({"id": "s", "name": "Stua", "parent": "o"})));
        assert!(zone_reshaped(&tree, "zone.update", &json!({"id": "s", "name": "Stue", "parent": "h"})));
        assert!(zone_reshaped(&tree, "zone.create", &json!({"id": "n", "name": "Ny", "parent": "h"})));
    }

    #[test]
    fn lights_flows_follow_their_zones() {
        let zones = json!({
            "h": {"id": "h", "name": "Hjem", "parent": null},
            "o": {"id": "o", "name": "Andre etasje", "parent": "h"},
            "s": {"id": "s", "name": "Lys\u{a0}Stue", "parent": "o"},
            "n": {"id": "n", "name": "Første etasje", "parent": "h"},
        });
        let light = |id: &str, zone: &str, on: bool| json!({"id": id, "zone": zone, "class": "light", "driverId": "homey:app:nl.philips.hue:bulb", "capabilitiesObj": {"onoff": {"value": on}}});
        let devices = json!({
            "a": light("a", "s", false),
            "b": {"id": "b", "zone": "o", "class": "socket", "virtualClass": "light", "driverId": "homey:app:se.nexa:x", "capabilitiesObj": {"onoff": {"value": false}}},
            "c": light("c", "n", true),
            "dead": {"id": "dead", "zone": "s", "class": "light", "available": false, "capabilitiesObj": {"onoff": {"value": true}}},
            "grp": {"id": "grp", "zone": "s", "class": "light", "driverId": "homey:app:com.swttt.devicegroups:light", "capabilitiesObj": {"onoff": {"value": true}}},
            "plug": {"id": "plug", "name": "Plug", "zone": "s", "class": "socket", "capabilitiesObj": {"onoff": {"value": true}}},
        });
        let house = House::new(&zones, &devices);
        let ids = |arg: &str| house.members(arg).map(|m| m.iter().map(|l| l.0.clone()).collect::<Vec<_>>());
        assert_eq!(ids("toggle: andre  etasje"), Some(vec!["a".into(), "b".into()]), "sub-zones, plug lamps; not dead, groups or plain plugs");
        assert_eq!(ids("off: Lys Stue, Første etasje").map(|v| v.len()), Some(2));
        assert_eq!(ids("toggle: Nowhere"), None);
        assert_eq!(ids("on: Første etasje, Plug"), Some(vec!["plug".into(), "c".into()]), "a device by name, light or not");
        assert_eq!(ids("toggle: Stue, Gone"), None, "an unknown name makes the script do nothing");

        let flow = json!({"id": "f", "name": "Lys: Andre etasje", "triggerable": true, "cards": {
            "1": {"type": "start"},
            "2": {"id": "homey:app:com.athom.homeyscript:runCodeWithArg_v2", "args": {"code": "… name: 'zoneLights' …", "argument": "toggle: Andre etasje"}}}});
        let mut e = map_flow(&flow, "aflow-").unwrap();
        group(&mut e, &house.members(&zone_argument(&flow).unwrap()).unwrap());
        assert_eq!(e.capabilities["onoff"].value, false);
        assert!(!e.capabilities["onoff"].settable, "tapping runs the Flow");

        let zone_names = HashMap::new();
        let mut list: Vec<Entity> = ["a", "b"].iter().filter_map(|id| map_device(&devices[*id], &zone_names)).collect();
        list.push(e);
        assert_eq!(list[1].class, "light");
        list[1].capabilities.get_mut("onoff").unwrap().value = json!(true);
        assert_eq!(regroup(&mut list, "b"), vec![("aflow-f".to_string(), json!(true))]);
        assert_eq!(regroup(&mut list, "b"), vec![], "no news, no event");
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

        let mut list = vec![e];
        assert_eq!(apply_update(&mut list, &d, &zones), None, "nothing changed");
        let moved = json!({"id": "d1", "name": "Taklampe stue", "zone": "z2", "class": "light", "capabilitiesObj": {}});
        let zones = HashMap::from([("z2".to_string(), "Kjøkken".to_string())]);
        assert_eq!(apply_update(&mut list, &moved, &zones), Some(true), "renamed and moved");
        assert_eq!((list[0].name.as_str(), list[0].zone.as_str()), ("Taklampe stue", "Kjøkken"));
        assert_eq!(list[0].capabilities.len(), 3, "keeps its capabilities and values");

        assert!(map_flow(&json!({"id": "f1", "name": "God natt", "triggerable": true}), "flow-").is_some());
        assert!(map_flow(&json!({"id": "f2", "name": "Auto", "triggerable": false}), "flow-").is_none());
        assert!(map_flow(&json!({"id": "f3", "triggerable": true, "enabled": false}), "flow-").is_none());
    }
}
