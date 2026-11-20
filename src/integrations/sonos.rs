//! Sonos speakers over their local UPnP/SOAP API on port 1400. One entity per
//! room: play/pause, volume, what is playing, and the household's Sonos
//! favourites. Polled every few seconds; no cloud, no account.

use super::{Capability, Entity, Event, Field, FieldKind, Instance, Kind, Sink};
use crate::client::request;
use crate::util::{escape_xml, unescape_xml};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn kind() -> Kind {
    Kind {
        id: "sonos",
        name: "Sonos",
        description: "Play and pause, volume, what is playing and your Sonos favourites (radio stations, playlists) for each room.",
        fields: vec![Field {
            key: "host",
            label: "Address of any Sonos speaker",
            kind: FieldKind::Text,
            required: true,
            placeholder: "192.168.1.40",
            help: "Sonos app → Settings → System → About My System shows each speaker's IP. oldpad finds the other rooms from it.",
        }],
        test,
        start,
    }
}

const AV: (&str, &str) = ("/MediaRenderer/AVTransport/Control", "AVTransport");
const RC: (&str, &str) = ("/MediaRenderer/RenderingControl/Control", "RenderingControl");
const CD: (&str, &str) = ("/MediaServer/ContentDirectory/Control", "ContentDirectory");
const ZT: (&str, &str) = ("/ZoneGroupTopology/Control", "ZoneGroupTopology");

fn base(host: &str) -> String {
    format!("http://{host}:1400")
}

fn soap(host: &str, (path, service): (&str, &str), action: &str, args: &[(&str, &str)]) -> Result<String, String> {
    let urn = format!("urn:schemas-upnp-org:service:{service}:1");
    let args: String = args.iter().map(|(k, v)| format!("<{k}>{}</{k}>", escape_xml(v))).collect();
    let body = format!(
        r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action} xmlns:u="{urn}">{args}</u:{action}></s:Body></s:Envelope>"#
    );
    let soap_action = format!("\"{urn}#{action}\"");
    let headers = [("Content-Type", "text/xml; charset=\"utf-8\""), ("Accept", "text/xml"), ("SOAPACTION", soap_action.as_str())];
    let res = request(&base(host), "POST", path, &headers, Some(body.as_bytes()))?;
    let text = String::from_utf8_lossy(&res.body).into_owned();
    if res.status != 200 {
        let code = tag(&text, "errorCode").unwrap_or_default();
        return Err(format!("Sonos {action} failed (HTTP {}, UPnP error {code})", res.status));
    }
    Ok(text)
}

/// Text of the first `<name>…</name>` (or `<name attr…>…</name>`), XML-unescaped.
fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}");
    let mut from = 0;
    while let Some(i) = xml[from..].find(&open) {
        let start = from + i + open.len();
        // `<dc:title` must not match `<dc:titleSomething`.
        if matches!(xml[start..].chars().next(), Some('>') | Some(' ')) {
            let body = start + xml[start..].find('>')? + 1;
            let end = body + xml[body..].find(&format!("</{name}>"))?;
            return Some(unescape_xml(&xml[body..end]));
        }
        from = start;
    }
    None
}

fn attr(element: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let i = element.find(&key)? + key.len();
    Some(unescape_xml(&element[i..i + element[i..].find('"')?]))
}

/// Each `<name …>` start tag (attributes only) in document order.
fn elements<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name} ");
    xml.match_indices(&open).filter_map(|(i, _)| xml[i..].find('>').map(|e| &xml[i..i + e])).collect()
}

#[derive(Clone, Debug, PartialEq)]
struct Room {
    uuid: String,
    name: String,
    host: String,
    coordinator: String, // host of the group coordinator, which owns playback
    coordinator_uuid: String,
}

fn host_of(location: &str) -> Option<String> {
    Some(location.strip_prefix("http://")?.split(':').next()?.to_string())
}

fn parse_topology(state: &str) -> Vec<Room> {
    let mut rooms: Vec<Room> = Vec::new();
    for group in state.split("<ZoneGroup ").skip(1) {
        let coord_uuid = attr(&format!(" {group}"), "Coordinator").unwrap_or_default();
        let members = elements(group, "ZoneGroupMember");
        let coord_host = members.iter().find(|m| attr(m, "UUID").as_deref() == Some(&coord_uuid)).and_then(|m| host_of(&attr(m, "Location")?));
        for m in members {
            // Stereo pairs and subs list extra, invisible members for the same room.
            if attr(m, "Invisible").as_deref() == Some("1") {
                continue;
            }
            let (Some(uuid), Some(name), Some(host)) = (attr(m, "UUID"), attr(m, "ZoneName"), attr(m, "Location").and_then(|l| host_of(&l))) else { continue };
            if rooms.iter().any(|r| r.name == name) {
                continue;
            }
            rooms.push(Room { uuid, name, coordinator: coord_host.clone().unwrap_or_else(|| host.clone()), coordinator_uuid: coord_uuid.clone(), host });
        }
    }
    rooms.sort_by(|a, b| a.name.cmp(&b.name));
    rooms
}

fn topology(host: &str) -> Result<Vec<Room>, String> {
    let xml = soap(host, ZT, "GetZoneGroupState", &[])?;
    let state = tag(&xml, "ZoneGroupState").ok_or("no ZoneGroupState in reply")?;
    let rooms = parse_topology(&state);
    if rooms.is_empty() {
        return Err("this speaker reported no rooms".into());
    }
    Ok(rooms)
}

#[derive(Clone)]
struct Favourite {
    id: String,
    title: String,
    uri: String,
    meta: String,
}

fn parse_favourites(didl: &str) -> Vec<Favourite> {
    didl.split("<item ")
        .skip(1)
        .filter_map(|item| {
            Some(Favourite {
                id: attr(&format!(" {item}"), "id")?,
                title: tag(item, "dc:title")?,
                uri: tag(item, "res").unwrap_or_default(),
                // resMD is escaped once more inside the item; tag() already unescaped it once.
                meta: tag(item, "r:resMD").unwrap_or_default(),
            })
        })
        .filter(|f| !f.uri.is_empty())
        .collect()
}

fn favourites(host: &str) -> Result<Vec<Favourite>, String> {
    let args = [("ObjectID", "FV:2"), ("BrowseFlag", "BrowseDirectChildren"), ("Filter", "dc:title,res,r:resMD"), ("StartingIndex", "0"), ("RequestedCount", "100"), ("SortCriteria", "")];
    let xml = soap(host, CD, "Browse", &args)?;
    Ok(parse_favourites(&tag(&xml, "Result").unwrap_or_default()))
}

struct Now {
    playing: bool,
    volume: f64,
    title: String,
    subtitle: String,
}

/// What is playing, from the room's coordinator, and the room's own volume.
fn now_playing(room: &Room) -> Result<Now, String> {
    let state = tag(&soap(&room.coordinator, AV, "GetTransportInfo", &[("InstanceID", "0")])?, "CurrentTransportState").unwrap_or_default();
    let media = tag(&soap(&room.coordinator, AV, "GetMediaInfo", &[("InstanceID", "0")])?, "CurrentURIMetaData").unwrap_or_default();
    let track = tag(&soap(&room.coordinator, AV, "GetPositionInfo", &[("InstanceID", "0")])?, "TrackMetaData").unwrap_or_default();
    let volume = tag(&soap(&room.host, RC, "GetVolume", &[("InstanceID", "0"), ("Channel", "Master")])?, "CurrentVolume")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);

    // Radio: the station is the media title and the song is streamContent.
    // Music: the track title, with the artist underneath.
    let station = tag(&media, "dc:title").unwrap_or_default();
    let stream = tag(&track, "r:streamContent").unwrap_or_default();
    let (title, subtitle) = if !station.is_empty() {
        (station, stream)
    } else {
        (tag(&track, "dc:title").unwrap_or_default(), tag(&track, "dc:creator").unwrap_or_default())
    };
    Ok(Now { playing: matches!(state.as_str(), "PLAYING" | "TRANSITIONING"), volume: volume / 100.0, title, subtitle })
}

fn entity(room: &Room, now: &Now, favs: &[Favourite]) -> Entity {
    let ro = |v: Value| Capability::new(v, false);
    let mut caps = BTreeMap::new();
    caps.insert("speaker_playing".to_string(), Capability::new(json!(now.playing), true));
    caps.insert("volume_set".to_string(), Capability { min: Some(0.0), max: Some(1.0), ..Capability::new(json!(now.volume), true) });
    caps.insert("speaker_track".to_string(), ro(json!(now.title)));
    caps.insert("speaker_artist".to_string(), ro(json!(now.subtitle)));
    caps.insert("favourites".to_string(), ro(json!(favs.iter().map(|f| json!({ "id": f.id, "title": f.title })).collect::<Vec<_>>())));
    caps.insert("play_favourite".to_string(), Capability::new(Value::Null, true));
    Entity { id: room.uuid.clone(), name: room.name.clone(), zone: room.name.clone(), class: "speaker".into(), capabilities: caps }
}

fn test(config: &Map<String, Value>) -> Result<String, String> {
    let host = host_field(config)?;
    let rooms = topology(&host)?;
    let favs = favourites(&host).map(|f| f.len()).unwrap_or(0);
    Ok(format!("Found {} rooms ({}) and {favs} favourites.", rooms.len(), rooms.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ")))
}

fn host_field(config: &Map<String, Value>) -> Result<String, String> {
    let host = super::get_str(config, "host").trim().trim_start_matches("http://").trim_end_matches('/').trim_end_matches(":1400").to_string();
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || ".-".contains(c)) {
        return Err("enter the speaker's IP address or host name".into());
    }
    Ok(host)
}

struct State {
    rooms: Vec<Room>,
    favs: Vec<Favourite>,
    entities: Vec<Entity>,
}

struct Sonos {
    state: Arc<Mutex<State>>,
    stopped: Arc<AtomicBool>,
    poke: Arc<AtomicBool>,
}

fn refresh(host: &str, state: &Mutex<State>, sink: &Sink, full: bool) -> Result<(), String> {
    let (rooms, favs) = if full {
        (topology(host)?, favourites(host).unwrap_or_default())
    } else {
        let s = state.lock().unwrap();
        (s.rooms.clone(), s.favs.clone())
    };
    let mut fresh = Vec::new();
    for room in &rooms {
        match now_playing(room) {
            Ok(now) => fresh.push(entity(room, &now, &favs)),
            Err(e) => eprintln!("sonos: {}: {e}", room.name),
        }
    }
    let mut s = state.lock().unwrap();
    let mut changes = Vec::new();
    for e in &fresh {
        let old = s.entities.iter().find(|o| o.id == e.id);
        for (cap, c) in &e.capabilities {
            if old.and_then(|o| o.capabilities.get(cap)).map(|o| &o.value) != Some(&c.value) {
                changes.push((e.id.clone(), cap.clone(), c.value.clone()));
            }
        }
    }
    let reshaped = s.entities.len() != fresh.len();
    *s = State { rooms, favs, entities: fresh };
    drop(s);
    for (entity, capability, value) in changes {
        sink(Event::Changed { entity, capability, value });
    }
    if reshaped {
        sink(Event::Reloaded);
    }
    Ok(())
}

fn start(config: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let host = host_field(config)?;
    let state = Arc::new(Mutex::new(State { rooms: vec![], favs: vec![], entities: vec![] }));
    refresh(&host, &state, &(Arc::new(|_| {}) as Sink), true)?;
    let (stopped, poke) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let (st, stop, pk) = (state.clone(), stopped.clone(), poke.clone());
    std::thread::spawn(move || {
        let mut last_full = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            // Poll every 4 s, or right away after a command so the screen catches up.
            for _ in 0..40 {
                if pk.swap(false, Ordering::Relaxed) || stop.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let full = last_full.elapsed() > Duration::from_secs(300);
            match refresh(&host, &st, &sink, full) {
                Ok(()) if full => last_full = Instant::now(),
                Ok(()) => {}
                Err(e) => eprintln!("sonos: {e}"),
            }
        }
    });
    Ok(Box::new(Sonos { state, stopped, poke }))
}

impl Instance for Sonos {
    fn entities(&self) -> Vec<Entity> {
        self.state.lock().unwrap().entities.clone()
    }

    fn set(&self, entity: &str, capability: &str, value: &Value) -> Result<(), String> {
        let (room, fav) = {
            let s = self.state.lock().unwrap();
            let room = s.rooms.iter().find(|r| r.uuid == entity).cloned().ok_or("no such room")?;
            let fav = value.as_str().and_then(|id| s.favs.iter().find(|f| f.id == id).cloned());
            (room, fav)
        };
        match capability {
            "speaker_playing" => {
                let action = if value.as_bool().ok_or("expected true or false")? { "Play" } else { "Pause" };
                let args: &[(&str, &str)] = if action == "Play" { &[("InstanceID", "0"), ("Speed", "1")] } else { &[("InstanceID", "0")] };
                soap(&room.coordinator, AV, action, args)?;
            }
            "volume_set" => {
                let v = value.as_f64().filter(|v| (0.0..=1.0).contains(v)).ok_or("volume must be 0..1")?;
                let pct = ((v * 100.0).round() as u32).to_string();
                soap(&room.host, RC, "SetVolume", &[("InstanceID", "0"), ("Channel", "Master"), ("DesiredVolume", &pct)])?;
            }
            "play_favourite" => {
                let fav = fav.ok_or("no such favourite")?;
                let c = &room.coordinator;
                if fav.uri.starts_with("x-rincon-cpcontainer:") {
                    // Playlists and albums go through the queue; only streams can be set directly.
                    soap(c, AV, "RemoveAllTracksFromQueue", &[("InstanceID", "0")])?;
                    let args = [("InstanceID", "0"), ("EnqueuedURI", fav.uri.as_str()), ("EnqueuedURIMetaData", fav.meta.as_str()), ("DesiredFirstTrackNumberEnqueued", "0"), ("EnqueueAsNext", "0")];
                    soap(c, AV, "AddURIToQueue", &args)?;
                    let queue = format!("x-rincon-queue:{}#0", room.coordinator_uuid);
                    soap(c, AV, "SetAVTransportURI", &[("InstanceID", "0"), ("CurrentURI", &queue), ("CurrentURIMetaData", "")])?;
                } else {
                    soap(c, AV, "SetAVTransportURI", &[("InstanceID", "0"), ("CurrentURI", &fav.uri), ("CurrentURIMetaData", &fav.meta)])?;
                }
                soap(c, AV, "Play", &[("InstanceID", "0"), ("Speed", "1")])?;
            }
            _ => return Err("not supported".into()),
        }
        self.poke.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_topology() {
        let state = r#"<ZoneGroupState><ZoneGroups><ZoneGroup Coordinator="RINCON_B" ID="x"><ZoneGroupMember UUID="RINCON_A" Location="http://10.0.0.5:1400/xml/device_description.xml" ZoneName="Gang"/><ZoneGroupMember UUID="RINCON_B" Location="http://10.0.0.6:1400/xml/device_description.xml" ZoneName="Bad"><Satellite UUID="RINCON_S" Location="http://10.0.0.9:1400/x.xml" ZoneName="Bad" Invisible="1"/></ZoneGroupMember><ZoneGroupMember UUID="RINCON_C" Location="http://10.0.0.7:1400/x.xml" ZoneName="Kjøkken"/><ZoneGroupMember UUID="RINCON_D" Location="http://10.0.0.8:1400/x.xml" ZoneName="Kjøkken" Invisible="1"/></ZoneGroup><ZoneGroup Coordinator="RINCON_E" ID="y"><ZoneGroupMember UUID="RINCON_E" Location="http://10.0.0.10:1400/x.xml" ZoneName="Stue"/></ZoneGroup></ZoneGroups></ZoneGroupState>"#;
        let rooms = parse_topology(state);
        let names: Vec<_> = rooms.iter().map(|r| (r.name.as_str(), r.host.as_str(), r.coordinator.as_str())).collect();
        assert_eq!(names, vec![("Bad", "10.0.0.6", "10.0.0.6"), ("Gang", "10.0.0.5", "10.0.0.6"), ("Kjøkken", "10.0.0.7", "10.0.0.6"), ("Stue", "10.0.0.10", "10.0.0.10")]);
        assert_eq!(rooms[1].coordinator_uuid, "RINCON_B");
    }

    #[test]
    fn reads_favourites_and_metadata() {
        // As Sonos sends it inside <Result>, after one round of unescaping.
        let didl = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:r="urn:schemas-rinconnetworks-com:metadata-1-0/"><item id="FV:2/13" parentID="FV:2" restricted="false"><dc:title>NRK P1</dc:title><upnp:class>object.itemobject.item.sonos-favorite</upnp:class><res protocolInfo="x-rincon-mp3radio:*:*:*">x-rincon-mp3radio://lyd.nrk.no/p1?a=1&amp;b=2</res><r:resMD>&lt;DIDL-Lite&gt;&lt;item id="F00092020s"&gt;&lt;dc:title&gt;NRK P1&lt;/dc:title&gt;&lt;/item&gt;&lt;/DIDL-Lite&gt;</r:resMD></item><item id="FV:2/14"><dc:title>No res</dc:title></item></DIDL-Lite>"#;
        let favs = parse_favourites(didl);
        assert_eq!(favs.len(), 1);
        assert_eq!((favs[0].id.as_str(), favs[0].title.as_str()), ("FV:2/13", "NRK P1"));
        assert_eq!(favs[0].uri, "x-rincon-mp3radio://lyd.nrk.no/p1?a=1&b=2");
        assert!(favs[0].meta.starts_with("<DIDL-Lite><item id=\"F00092020s\"><dc:title>NRK P1"));
    }

    #[test]
    fn tag_does_not_match_longer_names() {
        assert_eq!(tag("<dc:titleX>no</dc:titleX><dc:title>yes</dc:title>", "dc:title").as_deref(), Some("yes"));
        assert_eq!(tag("<a b=\"1\">x &amp; y</a>", "a").as_deref(), Some("x & y"));
        assert_eq!(tag("<a>open", "a"), None);
    }

    #[test]
    #[ignore] // network: OLDPAD_TEST_SONOS=192.168.1.40 cargo test sonos_live -- --ignored --nocapture
    fn sonos_live() {
        let c = json!({ "host": std::env::var("OLDPAD_TEST_SONOS").unwrap() });
        let c = c.as_object().unwrap();
        println!("{}", test(c).unwrap());
        let host = host_field(c).unwrap();
        for r in topology(&host).unwrap() {
            let n = now_playing(&r).unwrap();
            println!("{}: playing={} vol={} {} / {}", r.name, n.playing, n.volume, n.title, n.subtitle);
        }
    }
}

/// Parses replies captured from real speakers: OLDPAD_TEST_SONOS_DIR=<dir with topology.xml, favs.xml, …>
#[cfg(test)]
#[test]
#[ignore]
fn sonos_captured() {
    let dir = std::env::var("OLDPAD_TEST_SONOS_DIR").unwrap();
    let read = |f: &str| std::fs::read_to_string(format!("{dir}/{f}")).unwrap();
    for r in parse_topology(&tag(&read("topology.xml"), "ZoneGroupState").unwrap()) {
        println!("room {} host {} coordinator {}", r.name, r.host, r.coordinator);
    }
    let favs = parse_favourites(&tag(&read("favs.xml"), "Result").unwrap());
    println!("{} favourites: {}", favs.len(), favs.iter().map(|f| f.title.as_str()).collect::<Vec<_>>().join(", "));
    if let Some(f) = favs.first() {
        println!("first: uri={} meta starts {:?}", f.uri, &f.meta.chars().take(60).collect::<String>());
    }
    let media = tag(&read("GetMediaInfo.xml"), "CurrentURIMetaData").unwrap_or_default();
    let track = tag(&read("GetPositionInfo.xml"), "TrackMetaData").unwrap_or_default();
    println!("state {:?} station {:?} stream {:?} track {:?} artist {:?} volume {:?}",
        tag(&read("GetTransportInfo.xml"), "CurrentTransportState"), tag(&media, "dc:title"), tag(&track, "r:streamContent"),
        tag(&track, "dc:title"), tag(&track, "dc:creator"), tag(&read("vol.xml"), "CurrentVolume"));
}
