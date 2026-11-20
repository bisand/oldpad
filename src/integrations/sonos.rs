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
            label: "Address of a Sonos speaker (optional)",
            kind: FieldKind::Text,
            required: false,
            placeholder: "found automatically",
            help: "Leave empty and oldpad finds your speakers on the network. If it cannot (some Docker and Kubernetes setups block discovery), enter one or more speaker IPs from Sonos app → Settings → System → About My System. From one speaker oldpad learns all rooms, and keeps up when their addresses change.",
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

/// Asks the network for Sonos speakers the way the Sonos app does (SSDP
/// multicast) and returns the addresses that answered. Empty where multicast
/// does not reach, such as a Kubernetes pod network.
fn discover(wait: Duration) -> Vec<String> {
    use std::net::UdpSocket;
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else { return vec![] };
    let _ = sock.set_multicast_ttl_v4(2);
    let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
    let msg = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\r\n";
    let mut found = Vec::new();
    let until = Instant::now() + wait;
    let mut next_send = Instant::now();
    let mut buf = [0u8; 2048];
    while Instant::now() < until {
        if Instant::now() >= next_send {
            // UDP is lossy: ask three times during the window.
            let _ = sock.send_to(msg.as_bytes(), "239.255.255.250:1900");
            next_send = Instant::now() + wait / 3;
        }
        if let Ok((n, _)) = sock.recv_from(&mut buf) {
            if let Some(host) = parse_ssdp(&String::from_utf8_lossy(&buf[..n])) {
                if !found.contains(&host) {
                    found.push(host);
                }
            }
        }
    }
    found
}

/// The speaker's host from an SSDP reply, if the reply is from a Sonos player.
fn parse_ssdp(reply: &str) -> Option<String> {
    let mut location = None;
    let mut sonos = false;
    for line in reply.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        match k.trim().to_ascii_lowercase().as_str() {
            "location" => location = host_of(v.trim()),
            "st" | "server" if v.contains("ZonePlayer") || v.contains("Sonos") => sonos = true,
            _ => {}
        }
    }
    location.filter(|_| sonos)
}

/// The household's current rooms, from the first known speaker that answers,
/// or from discovery when none does.
fn find_topology(known: &[String]) -> Result<(Vec<Room>, bool), String> {
    for host in known {
        if let Ok(rooms) = topology(host) {
            return Ok((rooms, false));
        }
    }
    for host in discover(Duration::from_secs(3)) {
        if let Ok(rooms) = topology(&host) {
            return Ok((rooms, true));
        }
    }
    Err(if known.is_empty() {
        "no Sonos speakers answered on the network. Enter a speaker's IP address instead.".into()
    } else {
        format!("none of the known speakers ({}) answered, and discovery found none", known.join(", "))
    })
}

fn test(config: &Map<String, Value>) -> Result<String, String> {
    let seeds = seeds(config)?;
    let (rooms, discovered) = find_topology(&seeds)?;
    let favs = favourites(&rooms[0].host).map(|f| f.len()).unwrap_or(0);
    let how = if discovered { " (found automatically)" } else { "" };
    Ok(format!("Found {} rooms{how}: {}. {favs} favourites.", rooms.len(), rooms.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ")))
}

/// The admin's speaker addresses: none, one, or several separated by commas.
fn seeds(config: &Map<String, Value>) -> Result<Vec<String>, String> {
    super::get_str(config, "host")
        .split([',', ' '])
        .map(|h| h.trim().trim_start_matches("http://").trim_end_matches('/').trim_end_matches(":1400"))
        .filter(|h| !h.is_empty())
        .map(|h| {
            if h.chars().all(|c| c.is_ascii_alphanumeric() || ".-".contains(c)) {
                Ok(h.to_string())
            } else {
                Err(format!("\"{h}\" is not an IP address or host name"))
            }
        })
        .collect()
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

/// Speakers to ask for the topology: every room we know of, then the admin's seeds.
fn known_hosts(state: &Mutex<State>, seeds: &[String]) -> Vec<String> {
    let mut hosts: Vec<String> = state.lock().unwrap().rooms.iter().map(|r| r.host.clone()).collect();
    for s in seeds {
        if !hosts.contains(s) {
            hosts.push(s.clone());
        }
    }
    hosts
}

/// Returns whether every room answered; a room that did not may have moved.
fn refresh(seeds: &[String], state: &Mutex<State>, sink: &Sink, full: bool) -> Result<bool, String> {
    let (rooms, favs) = if full {
        let (rooms, _) = find_topology(&known_hosts(state, seeds))?;
        let favs = favourites(&rooms[0].host).unwrap_or_else(|_| state.lock().unwrap().favs.clone());
        (rooms, favs)
    } else {
        let s = state.lock().unwrap();
        (s.rooms.clone(), s.favs.clone())
    };
    let mut fresh = Vec::new();
    let mut all_answered = true;
    for room in &rooms {
        match now_playing(room) {
            Ok(now) => fresh.push(entity(room, &now, &favs)),
            Err(e) => {
                all_answered = false;
                eprintln!("sonos: {}: {e}", room.name);
                // Keep showing the last known state rather than dropping the room.
                if let Some(old) = state.lock().unwrap().entities.iter().find(|e| e.id == room.uuid) {
                    fresh.push(old.clone());
                }
            }
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
    Ok(all_answered)
}

fn start(config: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let seeds = seeds(config)?;
    let state = Arc::new(Mutex::new(State { rooms: vec![], favs: vec![], entities: vec![] }));
    refresh(&seeds, &state, &(Arc::new(|_| {}) as Sink), true)?;
    let rooms: Vec<String> = state.lock().unwrap().rooms.iter().map(|r| format!("{} {}", r.name, r.host)).collect();
    eprintln!("sonos: {} rooms: {}", rooms.len(), rooms.join(", "));
    let (stopped, poke) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let (st, stop, pk) = (state.clone(), stopped.clone(), poke.clone());
    std::thread::spawn(move || {
        let mut last_full = Instant::now();
        let mut lost = false;
        while !stop.load(Ordering::Relaxed) {
            // Poll every 4 s, or right away after a command so the screen catches up.
            for _ in 0..40 {
                if pk.swap(false, Ordering::Relaxed) || stop.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            // Re-read the topology every 5 minutes, and at once when a room stops
            // answering: that is how a speaker's new address is learned.
            let full = lost || last_full.elapsed() > Duration::from_secs(300);
            match refresh(&seeds, &st, &sink, full) {
                Ok(all) => {
                    if full {
                        last_full = Instant::now();
                    }
                    lost = !all && !full;
                }
                Err(e) => {
                    eprintln!("sonos: {e}");
                    lost = false; // the 5-minute timer retries; don't hammer
                }
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
    fn reads_ssdp_replies() {
        let sonos = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age = 1800\r\nLOCATION: http://192.168.1.44:1400/xml/device_description.xml\r\nSERVER: Linux UPnP/1.0 Sonos/80.1-55014 (ZPS27)\r\nST: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\r\n";
        assert_eq!(parse_ssdp(sonos).as_deref(), Some("192.168.1.44"));
        let tv = "HTTP/1.1 200 OK\r\nLOCATION: http://192.168.1.9:8080/desc.xml\r\nST: upnp:rootdevice\r\nSERVER: SomeTV\r\n\r\n";
        assert_eq!(parse_ssdp(tv), None);
    }

    #[test]
    fn seed_addresses() {
        let c = |h: &str| json!({ "host": h }).as_object().unwrap().clone();
        assert_eq!(seeds(&c("")).unwrap(), Vec::<String>::new());
        assert_eq!(seeds(&c("192.168.1.4, http://192.168.1.5:1400/")).unwrap(), vec!["192.168.1.4", "192.168.1.5"]);
        assert!(seeds(&c("1.2.3.4/evil")).is_err());
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
        let (rooms, _) = find_topology(&seeds(c).unwrap()).unwrap();
        for r in rooms {
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
