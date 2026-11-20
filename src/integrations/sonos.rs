//! Sonos speakers over their local UPnP/SOAP API on port 1400. One entity per
//! room: play/pause, volume, what is playing, and the household's Sonos
//! favourites. Polled every few seconds; no cloud, no account.

use super::smapi::{self, Account, SPOTIFY_URL};
use super::{Capability, Entity, Event, Field, FieldKind, Instance, Kind, Sink};
use crate::client::request;
use crate::util::{escape_xml, percent_encode, unescape_xml};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn kind() -> Kind {
    Kind {
        id: "sonos",
        name: "Sonos",
        description: "Play and pause, volume, what is playing and your Sonos favourites (radio stations, playlists) for each room.",
        fields: vec![Field {
            key: "host",
            label: "Address of a Sonos speaker",
            kind: FieldKind::Text,
            required: false,
            placeholder: "found automatically",
            help: "Leave empty and oldpad finds your speakers on the network. It asks the network, then scans it for speakers. If neither works (in Docker's default network, for one), enter one or more speaker IPs from Sonos app → Settings → System → About My System. From one speaker oldpad learns all rooms, and keeps up when their addresses change.",
        }],
        test,
        start,
    }
}

const AV: (&str, &str) = ("/MediaRenderer/AVTransport/Control", "AVTransport");
const RC: (&str, &str) = ("/MediaRenderer/RenderingControl/Control", "RenderingControl");
const CD: (&str, &str) = ("/MediaServer/ContentDirectory/Control", "ContentDirectory");
const ZT: (&str, &str) = ("/ZoneGroupTopology/Control", "ZoneGroupTopology");
const DP: (&str, &str) = ("/DeviceProperties/Control", "DeviceProperties");
const SP: (&str, &str) = ("/SystemProperties/Control", "SystemProperties");

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
pub(super) fn tag(xml: &str, name: &str) -> Option<String> {
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
            // Two rooms may share a name; only the same speaker twice is a duplicate.
            if rooms.iter().any(|r| r.uuid == uuid) {
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
    sub: String, // shown under a search result
    art: String, // cover or logo URL, or empty
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
                sub: String::new(),
                art: tag(item, "upnp:albumArtURI").unwrap_or_default(),
            })
        })
        .filter(|f| !f.uri.is_empty())
        .collect()
}

fn favourites(host: &str) -> Result<Vec<Favourite>, String> {
    let args = [("ObjectID", "FV:2"), ("BrowseFlag", "BrowseDirectChildren"), ("Filter", "dc:title,res,r:resMD,upnp:albumArtURI"), ("StartingIndex", "0"), ("RequestedCount", "100"), ("SortCriteria", "")];
    let xml = soap(host, CD, "Browse", &args)?;
    let mut favs = parse_favourites(&tag(&xml, "Result").unwrap_or_default());
    for f in &mut favs {
        f.art = art_url(&f.art, host);
    }
    Ok(favs)
}

/// An image URL the screens can be given: speakers hand out their own as a path.
fn art_url(art: &str, host: &str) -> String {
    // TuneIn logos come at 60 px through Sonos's resizer; ask for more.
    let art = if art.starts_with('/') { format!("http://{host}:1400{art}") } else { art.replace("?w=60&", "?w=200&") };
    if art.starts_with("http://") || art.starts_with("https://") { art } else { String::new() }
}

/// oldpad's own order for the favourites, and which go on the front page.
/// Sonos keeps no order that other apps can change, so oldpad keeps one.
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
struct Arrangement {
    order: Vec<String>,
    front: Vec<String>,
}

/// The favourites in the admin's order, each with whether it goes on the
/// front page. Before any arranging: Sonos's order, all on the front page.
/// Favourites added later go last, off the front page.
fn arrange<'a>(favs: &'a [Favourite], a: Option<&Arrangement>) -> Vec<(&'a Favourite, bool)> {
    let Some(a) = a else { return favs.iter().map(|f| (f, true)).collect() };
    let mut out: Vec<_> = favs.iter().map(|f| (f, a.front.contains(&f.id))).collect();
    out.sort_by_key(|(f, _)| a.order.iter().position(|id| *id == f.id).unwrap_or(usize::MAX));
    out
}

const DIDL: &str = r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" xmlns:r="urn:schemas-rinconnetworks-com:metadata-1-0/" xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/">"#;

/// Radio stations matching `query`, from the open radio-browser.info directory.
fn search_radio(query: &str) -> Result<Vec<Favourite>, String> {
    let path = format!("/json/stations/search?name={}&limit=40&hidebroken=true&order=clickcount&reverse=true", percent_encode(query));
    let res = request("https://all.api.radio-browser.info", "GET", &path, &[("Accept", "application/json")], None)?;
    if res.status != 200 {
        return Err(format!("radio search failed (HTTP {})", res.status));
    }
    let list: Vec<Value> = serde_json::from_slice(&res.body).map_err(|e| format!("radio search: {e}"))?;
    Ok(parse_stations(&list))
}

fn parse_stations(list: &[Value]) -> Vec<Favourite> {
    let mut out: Vec<Favourite> = Vec::new();
    for s in list {
        let (Some(url), Some(name), Some(uuid)) = (s["url_resolved"].as_str(), s["name"].as_str(), s["stationuuid"].as_str()) else { continue };
        let codec = s["codec"].as_str().unwrap_or("");
        // Sonos plays these as radio streams; the directory lists many others.
        if !(url.starts_with("http://") || url.starts_with("https://")) || !matches!(codec, "MP3" | "AAC" | "AAC+") {
            continue;
        }
        let title = name.trim().to_string();
        // The directory holds many copies of popular stations: keep the most played.
        if title.is_empty() || out.iter().any(|f| f.title.eq_ignore_ascii_case(&title)) {
            continue;
        }
        let meta = format!(
            r#"{DIDL}<item id="R:0/0/0" parentID="R:0/0" restricted="true"><dc:title>{}</dc:title><upnp:class>object.item.audioItem.audioBroadcast</upnp:class><desc id="cdudn" nameSpace="urn:schemas-rinconnetworks-com:metadata-1-0/">SA_RINCON65031_</desc></item></DIDL-Lite>"#,
            escape_xml(&title)
        );
        let sub = [s["country"].as_str().unwrap_or(""), s["tags"].as_str().unwrap_or("").split(',').next().unwrap_or("")]
            .iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" · ");
        out.push(Favourite { id: format!("radio:{uuid}"), title, uri: format!("x-rincon-mp3radio://{url}"), meta, sub, art: String::new() });
    }
    out.truncate(20);
    out
}

/// Which of the household's Spotify accounts plays (`sn=` in its favourites); the most used one.
fn spotify_sn(favs: &[Favourite]) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for f in favs.iter().filter(|f| f.uri.contains("sid=9&")) {
        let Some(sn) = f.uri.split("sn=").nth(1).map(|v| v.split('&').next().unwrap_or("").to_string()) else { continue };
        match counts.iter_mut().find(|(s, _)| *s == sn) {
            Some((_, n)) => *n += 1,
            None => counts.push((sn, 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1));
    // shortcut: with no Spotify favourite yet, guess the first account; add one in the Sonos app if playing fails.
    counts.into_iter().next().map_or("1".into(), |(sn, _)| sn)
}

/// A Spotify search result as something Sonos can play, the way its own favourites are written.
fn spotify_item(item: &smapi::Item, sn: &str) -> Favourite {
    let enc = item.id.replace(':', "%3a");
    let (uri, item_id, class) = match item.kind {
        "track" => (format!("x-sonos-spotify:{enc}?sid=9&flags=8224&sn={sn}"), format!("00032020{enc}"), "object.item.audioItem.musicTrack"),
        "album" => (format!("x-rincon-cpcontainer:1004206c{enc}?sid=9&flags=8300&sn={sn}"), format!("1004206c{enc}"), "object.container.album.musicAlbum"),
        _ => (format!("x-rincon-cpcontainer:1006206c{enc}?sid=9&flags=8300&sn={sn}"), format!("1006206c{enc}"), "object.container.playlistContainer"),
    };
    let meta = format!(
        r#"{DIDL}<item id="{item_id}" parentID="" restricted="true"><dc:title>{}</dc:title><upnp:class>{class}</upnp:class><desc id="cdudn" nameSpace="urn:schemas-rinconnetworks-com:metadata-1-0/">SA_RINCON2311_X_#Svc2311-0-Token</desc></item></DIDL-Lite>"#,
        escape_xml(&item.title)
    );
    Favourite { id: item.id.clone(), title: item.title.clone(), uri, meta, sub: item.sub.clone(), art: String::new() }
}

/// A Sonos favourite for `f`, as CreateObject wants it in FV:2.
fn favourite_didl(f: &Favourite) -> String {
    let scheme = f.uri.split(':').next().unwrap_or("");
    format!(
        r#"{DIDL}<item id="" restricted="false" parentID="FV:2"><dc:title>{}</dc:title><upnp:class>object.itemobject.item.sonos-favorite</upnp:class><r:ordinal>-1</r:ordinal><res protocolInfo="{scheme}:*:*:*">{}</res><r:type>instantPlay</r:type><r:description>{}</r:description><r:resMD>{}</r:resMD></item></DIDL-Lite>"#,
        escape_xml(&f.title), escape_xml(&f.uri), if f.uri.contains("sid=9&") { "Spotify" } else { "Radio" }, escape_xml(&f.meta)
    )
}

struct Now {
    playing: bool,
    volume: f64,
    title: String,
    subtitle: String,
    art: String, // absolute image URL, or empty
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
    // The track's cover, else the station's logo; speakers give their own as a path.
    let art = [tag(&track, "upnp:albumArtURI"), tag(&media, "upnp:albumArtURI")].into_iter().flatten().find(|u| !u.is_empty()).unwrap_or_default();
    let art = art_url(&art, &room.coordinator);
    Ok(Now { playing: matches!(state.as_str(), "PLAYING" | "TRANSITIONING"), volume: volume / 100.0, title, subtitle, art })
}

/// An opaque name for an image URL: the screen asks for this, never for a URL.
fn art_key(url: &str) -> String {
    if url.is_empty() { String::new() } else { crate::util::sha256_hex(url)[..16].to_string() }
}

fn entity(room: &Room, now: &Now, favs: &[(&Favourite, bool)]) -> Entity {
    let ro = |v: Value| Capability::new(v, false);
    let mut caps = BTreeMap::new();
    caps.insert("speaker_playing".to_string(), Capability::new(json!(now.playing), true));
    caps.insert("volume_set".to_string(), Capability { min: Some(0.0), max: Some(1.0), ..Capability::new(json!(now.volume), true) });
    caps.insert("speaker_track".to_string(), ro(json!(now.title)));
    caps.insert("speaker_artist".to_string(), ro(json!(now.subtitle)));
    caps.insert("favourites".to_string(), ro(json!(favs.iter().map(|(f, front)| json!({ "id": f.id, "title": f.title, "art": art_key(&f.art), "front": front })).collect::<Vec<_>>())));
    caps.insert("arrange_favourites".to_string(), Capability::new(Value::Null, true));
    caps.insert("remove_favourite".to_string(), Capability::new(Value::Null, true));
    caps.insert("play_favourite".to_string(), Capability::new(Value::Null, true));
    caps.insert("speaker_art".to_string(), ro(json!(art_key(&now.art))));
    caps.insert("add_favourite".to_string(), Capability::new(Value::Null, true));
    // The room whose group this one plays with; itself when it plays alone or leads.
    caps.insert("speaker_group".to_string(), Capability::new(json!(room.coordinator_uuid), true));
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

/// The /24 to scan: around OLDPAD_LAN_IP if set (in Kubernetes, the node's
/// address), else around this machine's own address. Private networks only.
fn scan_base(lan_ip: Option<&str>, own: Option<Ipv4Addr>) -> Option<[u8; 3]> {
    let ip = lan_ip.and_then(|s| s.trim().parse::<Ipv4Addr>().ok()).or(own)?;
    let o = ip.octets();
    ip.is_private().then_some([o[0], o[1], o[2]])
}

/// Discovery without multicast: every address in the /24 that accepts a
/// connection on Sonos's port 1400. The caller confirms each one is a speaker.
fn scan() -> Vec<String> {
    use std::net::{SocketAddr, TcpStream, UdpSocket};
    // Connecting a UDP socket sends nothing; it only picks the outgoing address.
    let own = UdpSocket::bind("0.0.0.0:0").ok().and_then(|s| s.connect("192.0.2.1:9").ok().and(s.local_addr().ok())).and_then(|a| match a.ip() {
        std::net::IpAddr::V4(v4) => Some(v4),
        _ => None,
    });
    let Some([a, b, c]) = scan_base(std::env::var("OLDPAD_LAN_IP").ok().as_deref(), own) else { return vec![] };
    let found = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for d in 1..=254u8 {
            let found = &found;
            let _ = std::thread::Builder::new().stack_size(64 * 1024).spawn_scoped(s, move || {
                let addr = SocketAddr::from(([a, b, c, d], 1400));
                if TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok() {
                    found.lock().unwrap().push(addr.ip().to_string());
                }
            });
        }
    });
    found.into_inner().unwrap()
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
    let discovered = discover(Duration::from_secs(3));
    let hosts = if discovered.is_empty() { scan() } else { discovered };
    for host in hosts {
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
    let how = if discovered { " on the network" } else { "" };
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
    /// Images the screens may fetch through oldpad: key → URL. Recent ones only.
    art: Vec<(String, String)>,
    arranged: Option<Arrangement>,
}

struct Sonos {
    state: Arc<Mutex<State>>,
    stopped: Arc<AtomicBool>,
    poke: Arc<AtomicBool>,
    regroup: Arc<AtomicUsize>,
    found: Mutex<Vec<Favourite>>, // recent search results, playable and saveable by id
    spotify: Arc<Mutex<Option<Account>>>,
    sink: Sink,
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
    let arranged = state.lock().unwrap().arranged.clone();
    let ordered = arrange(&favs, arranged.as_ref());
    let mut arts: Vec<(String, String)> = favs.iter().map(|f| (art_key(&f.art), f.art.clone())).collect();
    let mut fresh = Vec::new();
    let mut all_answered = true;
    for room in &rooms {
        match now_playing(room) {
            Ok(now) => {
                arts.push((art_key(&now.art), now.art.clone()));
                fresh.push(entity(room, &now, &ordered))
            }
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
    let mut art = arts.into_iter().filter(|(k, _)| !k.is_empty()).collect::<Vec<_>>();
    for old in std::mem::take(&mut s.art) {
        if art.len() < 50 && !art.iter().any(|(k, _)| *k == old.0) {
            art.push(old);
        }
    }
    drop(ordered);
    // Keep the arrangement as it is now: the admin may have changed it meanwhile.
    let arranged = s.arranged.take();
    *s = State { rooms, favs, entities: fresh, art, arranged };
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
    let arranged = config.get("_favourites").and_then(|v| serde_json::from_value::<Arrangement>(v.clone()).ok());
    let state = Arc::new(Mutex::new(State { rooms: vec![], favs: vec![], entities: vec![], art: vec![], arranged }));
    refresh(&seeds, &state, &(Arc::new(|_| {}) as Sink), true)?;
    let rooms: Vec<String> = state.lock().unwrap().rooms.iter().map(|r| format!("{} {}", r.name, r.host)).collect();
    eprintln!("sonos: {} rooms: {}", rooms.len(), rooms.join(", "));
    let (stopped, poke) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let regroup = Arc::new(AtomicUsize::new(0));
    let (st, stop, pk, rg, sk) = (state.clone(), stopped.clone(), poke.clone(), regroup.clone(), sink.clone());
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
            // After a regrouping, re-read it for a few polls: Sonos takes a moment to settle.
            let settling = rg.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1)).is_ok();
            let full = lost || settling || last_full.elapsed() > Duration::from_secs(300);
            match refresh(&seeds, &st, &sk, full) {
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
    let spotify = config.get("_spotify").and_then(|v| serde_json::from_value::<Account>(v.clone()).ok());
    Ok(Box::new(Sonos { state, stopped, poke, regroup, found: Mutex::new(vec![]), spotify: Arc::new(Mutex::new(spotify)), sink }))
}

impl Instance for Sonos {
    fn entities(&self) -> Vec<Entity> {
        self.state.lock().unwrap().entities.clone()
    }

    fn search(&self, entity: &str, query: &str) -> Result<Vec<Value>, String> {
        let sn = {
            let s = self.state.lock().unwrap();
            if !s.rooms.iter().any(|r| r.uuid == entity) {
                return Err("no such room".into());
            }
            spotify_sn(&s.favs)
        };
        let account = self.spotify.lock().unwrap().clone();
        // Radio and each Spotify category at once: one slow answer shouldn't stack up.
        let (radio, music, account_after) = std::thread::scope(|scope| {
            let radio = scope.spawn(|| search_radio(query));
            let music = account.clone().map(|mut acc| {
                scope.spawn(move || {
                    let mut items = Vec::new();
                    let mut errors = Vec::new();
                    for category in ["track", "playlist", "album"] {
                        match smapi::search(SPOTIFY_URL, &mut acc, category, query, 6) {
                            Ok(found) => items.extend(found),
                            Err(e) => errors.push(e),
                        }
                    }
                    (items, errors, acc)
                })
            });
            let radio = radio.join().unwrap_or_else(|_| Err("radio search failed".into()));
            let (items, errors, acc) = music.map(|h| h.join().unwrap_or_else(|_| (vec![], vec!["Spotify search failed".into()], account.clone().unwrap()))).map_or((vec![], vec![], None), |(i, e, a)| (i, e, Some(a)));
            (radio, (items, errors), acc)
        });
        // Spotify may have handed out a fresh token on the way: keep it.
        if account_after.is_some() && account_after != account {
            *self.spotify.lock().unwrap() = account_after.clone();
            (self.sink)(Event::Remember { key: "_spotify".into(), value: json!(account_after) });
        }
        let (items, errors) = music;
        for e in &errors {
            eprintln!("sonos: {e}");
        }
        if items.is_empty() {
            if let Err(e) = &radio {
                return Err(errors.into_iter().next().unwrap_or_else(|| e.clone()));
            }
        }
        let mut results: Vec<(Favourite, &str)> = items.iter().map(|i| (spotify_item(i, &sn), i.kind)).collect();
        results.extend(radio.unwrap_or_default().into_iter().map(|f| (f, "radio")));
        let mut found = self.found.lock().unwrap();
        found.retain(|f| !results.iter().any(|(r, _)| r.id == f.id));
        found.splice(0..0, results.iter().map(|(f, _)| f.clone()));
        found.truncate(300);
        Ok(results.iter().map(|(f, kind)| json!({ "id": f.id, "title": f.title, "sub": f.sub, "kind": kind })).collect())
    }

    fn image(&self, key: &str) -> Result<(String, Vec<u8>), String> {
        let url = self.state.lock().unwrap().art.iter().find(|(k, _)| k == key).map(|(_, u)| u.clone()).ok_or("no such image")?;
        // Logo services hand out redirects to their image CDN; follow a few.
        let mut url = url;
        let mut res = request(&url, "GET", "", &[("Accept", "image/*")], None)?;
        for _ in 0..3 {
            let Some(next) = res.headers.get("location").filter(|_| matches!(res.status, 301 | 302 | 303 | 307 | 308)) else { break };
            if !(next.starts_with("http://") || next.starts_with("https://")) {
                break;
            }
            url = next.clone();
            res = request(&url, "GET", "", &[("Accept", "image/*")], None)?;
        }
        let kind = res.headers.get("content-type").cloned().unwrap_or_default();
        if res.status != 200 || !kind.starts_with("image/") || res.body.len() > 2_000_000 {
            return Err(format!("image not available (HTTP {})", res.status));
        }
        Ok((kind, res.body))
    }

    fn accounts(&self) -> Vec<Value> {
        vec![json!({ "id": "spotify", "name": "Spotify", "connected": self.spotify.lock().unwrap().is_some() })]
    }

    fn connect(&self, account: &str) -> Result<String, String> {
        if account != "spotify" {
            return Err("only Spotify can be connected so far".into());
        }
        let host = self.state.lock().unwrap().rooms.first().map(|r| r.host.clone()).ok_or("no speakers found")?;
        let household = tag(&soap(&host, DP, "GetHouseholdID", &[])?, "CurrentHouseholdID").ok_or("the speaker did not say which household it is in")?;
        let device = tag(&soap(&host, SP, "GetString", &[("VariableName", "R_TrialZPSerial")])?, "StringValue").ok_or("the speaker did not give its serial number")?;
        let (url, code) = smapi::app_link(SPOTIFY_URL, &device, &household)?;
        let (slot, sink) = (self.spotify.clone(), self.sink.clone());
        std::thread::spawn(move || {
            // The admin has a few minutes to sign in.
            for _ in 0..120 {
                std::thread::sleep(Duration::from_secs(5));
                match smapi::device_token(SPOTIFY_URL, &device, &household, &code) {
                    Ok(Some(acc)) => {
                        eprintln!("sonos: Spotify connected");
                        *slot.lock().unwrap() = Some(acc.clone());
                        sink(Event::Remember { key: "_spotify".into(), value: json!(acc) });
                        return;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("sonos: Spotify sign-in: {e}");
                        return;
                    }
                }
            }
            eprintln!("sonos: Spotify sign-in timed out");
        });
        Ok(url)
    }

    fn set(&self, entity: &str, capability: &str, value: &Value) -> Result<(), String> {
        let (room, fav) = {
            let s = self.state.lock().unwrap();
            let room = s.rooms.iter().find(|r| r.uuid == entity).cloned().ok_or("no such room")?;
            let fav = value.as_str().and_then(|id| s.favs.iter().chain(self.found.lock().unwrap().iter()).find(|f| f.id == id).cloned());
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
                if fav.uri.starts_with("x-rincon-cpcontainer:") || fav.uri.starts_with("x-sonos-spotify:") {
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
            "add_favourite" => {
                let f = fav.ok_or("no such station")?;
                soap(&room.host, CD, "CreateObject", &[("ContainerID", "FV:2"), ("Elements", &favourite_didl(&f))])?;
                self.regroup.store(1, Ordering::Relaxed); // the next full refresh reads the new list
            }
            "arrange_favourites" => {
                let a: Arrangement = serde_json::from_value(value.clone()).map_err(|_| "expected the order and the front page favourites")?;
                if a.order.len() > 500 || a.front.len() > 500 {
                    return Err("too many favourites".into());
                }
                self.state.lock().unwrap().arranged = Some(a.clone());
                (self.sink)(Event::Remember { key: "_favourites".into(), value: json!(a) });
            }
            "remove_favourite" => {
                // Only Sonos favourites, never a search result: this deletes it for the household.
                let id = value.as_str().filter(|id| id.starts_with("FV:2/") && self.state.lock().unwrap().favs.iter().any(|f| f.id == *id)).ok_or("no such favourite")?;
                soap(&room.host, CD, "DestroyObject", &[("ObjectID", id)])?;
                self.regroup.store(1, Ordering::Relaxed); // the next full refresh reads the new list
            }
            "speaker_group" => {
                let target = value.as_str().ok_or("expected a room")?;
                if target == room.uuid {
                    soap(&room.host, AV, "BecomeCoordinatorOfStandaloneGroup", &[("InstanceID", "0")])?;
                } else {
                    // Join the target's group by following its coordinator.
                    let lead = self.state.lock().unwrap().rooms.iter().find(|r| r.uuid == target).map(|r| r.coordinator_uuid.clone()).ok_or("no such room")?;
                    let uri = format!("x-rincon:{lead}");
                    soap(&room.host, AV, "SetAVTransportURI", &[("InstanceID", "0"), ("CurrentURI", &uri), ("CurrentURIMetaData", "")])?;
                }
                self.regroup.store(3, Ordering::Relaxed);
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
    fn arranges_favourites() {
        let fav = |id: &str| Favourite { id: id.into(), title: id.into(), uri: String::new(), meta: String::new(), sub: String::new(), art: String::new() };
        let favs = [fav("a"), fav("b"), fav("c"), fav("new")];
        let ids = |v: Vec<(&Favourite, bool)>| v.into_iter().map(|(f, front)| format!("{}{}", f.id, if front { "*" } else { "" })).collect::<Vec<_>>();
        assert_eq!(ids(arrange(&favs, None)), ["a*", "b*", "c*", "new*"]);
        let a = Arrangement { order: vec!["c".into(), "gone".into(), "a".into(), "b".into()], front: vec!["c".into(), "b".into()] };
        assert_eq!(ids(arrange(&favs, Some(&a))), ["c*", "a", "b*", "new"]);
        assert_eq!(art_url("/getaa?s=1&u=x", "10.0.0.5"), "http://10.0.0.5:1400/getaa?s=1&u=x");
        assert_eq!(art_url("https://x.cloudfront.net/image?w=60&image=a", "h"), "https://x.cloudfront.net/image?w=200&image=a");
        assert_eq!(art_url("file:///etc/passwd", "h"), "");
    }

    #[test]
    fn reads_ssdp_replies() {
        let sonos = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age = 1800\r\nLOCATION: http://192.168.1.44:1400/xml/device_description.xml\r\nSERVER: Linux UPnP/1.0 Sonos/80.1-55014 (ZPS27)\r\nST: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\r\n";
        assert_eq!(parse_ssdp(sonos).as_deref(), Some("192.168.1.44"));
        let tv = "HTTP/1.1 200 OK\r\nLOCATION: http://192.168.1.9:8080/desc.xml\r\nST: upnp:rootdevice\r\nSERVER: SomeTV\r\n\r\n";
        assert_eq!(parse_ssdp(tv), None);
    }

    #[test]
    fn reads_radio_stations() {
        let list: Vec<Value> = serde_json::from_str(r#"[
            {"stationuuid":"a","name":"NRK P1 ","url_resolved":"https://cdn.example/p1_mp3_h","codec":"MP3","country":"Norway","tags":"news,talk"},
            {"stationuuid":"b","name":"nrk p1","url_resolved":"https://mirror.example/p1","codec":"MP3"},
            {"stationuuid":"c","name":"Video","url_resolved":"https://x.example/v.m3u8","codec":"UNKNOWN"},
            {"stationuuid":"d","name":"Rock & <Roll>","url_resolved":"http://r.example/s","codec":"AAC+"}
        ]"#).unwrap();
        let s = parse_stations(&list);
        assert_eq!(s.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), vec!["radio:a", "radio:d"]);
        assert_eq!((s[0].title.as_str(), s[0].sub.as_str(), s[0].uri.as_str()), ("NRK P1", "Norway · news", "x-rincon-mp3radio://https://cdn.example/p1_mp3_h"));
        assert!(s[1].meta.contains("<dc:title>Rock &amp; &lt;Roll&gt;</dc:title>"));
        // The favourite's metadata is escaped once more inside it, as Sonos stores it.
        let didl = favourite_didl(&s[1]);
        assert!(didl.contains(r#"<res protocolInfo="x-rincon-mp3radio:*:*:*">x-rincon-mp3radio://http://r.example/s</res>"#));
        assert!(didl.contains("&lt;dc:title&gt;Rock &amp;amp; &amp;lt;Roll&amp;gt;&lt;/dc:title&gt;"));
    }

    #[test]
    fn plays_spotify_results_like_its_favourites() {
        let fav = |uri: &str| Favourite { id: "x".into(), title: "x".into(), uri: uri.into(), meta: String::new(), sub: String::new(), art: String::new() };
        let favs = [fav("x-rincon-cpcontainer:1006206cspotify%3aplaylist%3a1?sid=9&flags=8300&sn=4"), fav("x-sonos-spotify:spotify%3atrack%3a1?sid=9&flags=8224&sn=2"), fav("x-rincon-cpcontainer:1006206cspotify%3aplaylist%3a2?sid=9&flags=8300&sn=2"), fav("x-rincon-mp3radio://x?sn=7")];
        assert_eq!(spotify_sn(&favs), "2");
        assert_eq!(spotify_sn(&[]), "1");
        let track = spotify_item(&smapi::Item { id: "spotify:track:2Wf".into(), title: "Take <on> Me".into(), sub: "a-ha".into(), kind: "track" }, "2");
        assert_eq!(track.uri, "x-sonos-spotify:spotify%3atrack%3a2Wf?sid=9&flags=8224&sn=2");
        assert!(track.meta.contains(r#"<item id="00032020spotify%3atrack%3a2Wf""#) && track.meta.contains("<dc:title>Take &lt;on&gt; Me</dc:title>"));
        let list = spotify_item(&smapi::Item { id: "spotify:playlist:37i".into(), title: "Mix".into(), sub: String::new(), kind: "playlist" }, "2");
        assert_eq!(list.uri, "x-rincon-cpcontainer:1006206cspotify%3aplaylist%3a37i?sid=9&flags=8300&sn=2");
        assert!(favourite_didl(&list).contains("<r:description>Spotify</r:description>"));
    }

    #[test]
    fn scan_network() {
        let own = Some(Ipv4Addr::new(10, 42, 0, 105));
        assert_eq!(scan_base(Some("192.168.174.21"), own), Some([192, 168, 174]));
        assert_eq!(scan_base(None, own), Some([10, 42, 0]));
        assert_eq!(scan_base(Some("nonsense"), own), Some([10, 42, 0]));
        assert_eq!(scan_base(Some("8.8.8.8"), own), None); // never sweep a public network
        assert_eq!(scan_base(None, None), None);
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
