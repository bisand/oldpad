//! Ships nearby, from an AIS receiver's NMEA stream (`!AIVDM` lines over TCP,
//! e.g. socat in front of rtl_ais, or AIS-catcher). One entity, `ships`: the
//! nearest ships within a radius of home, for the tablet's radar card.

use super::{Capability, Entity, Event, Field, FieldKind, Instance, Kind, Sink};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn kind() -> Kind {
    Kind {
        id: "ais",
        name: "Ships nearby (AIS)",
        description: "Boats and ships around you, from your own AIS receiver.",
        fields: vec![
            Field { key: "address", label: "NMEA stream", kind: FieldKind::Text, required: true, placeholder: "192.168.1.20:12123", help: "host:port of a TCP port that sends !AIVDM lines, e.g. socat in front of rtl_ais, or AIS-catcher." },
            Field { key: "latitude", label: "Home latitude", kind: FieldKind::Number, required: true, placeholder: "59.91", help: "The middle of the radar. Right-click a place in a map app to copy its coordinates." },
            Field { key: "longitude", label: "Home longitude", kind: FieldKind::Number, required: true, placeholder: "10.75", help: "" },
            Field { key: "radius", label: "Radius (km)", kind: FieldKind::Number, required: false, placeholder: "20", help: "Ships further away are left out. 20 km if empty." },
            Field { key: "forget", label: "Hide after (minutes)", kind: FieldKind::Number, required: false, placeholder: "20", help: "How long a ship stays after its last report. Anchored ships report every 3 minutes, small boats less often. Ships unheard for 5 minutes are shown faded." },
        ],
        test,
        start,
    }
}

const SHOWN: usize = 25; // nearest ships sent to screens

struct Config {
    address: String,
    home: (f64, f64),
    radius: f64,
    forget: Duration, // a moored ship reports every 3 min, class B less often
}

fn config(c: &Map<String, Value>) -> Result<Config, String> {
    let address = c.get("address").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if address.rsplit_once(':').and_then(|(_, p)| p.parse::<u16>().ok()).is_none() {
        return Err("the NMEA stream needs host:port".into());
    }
    let lat = c.get("latitude").and_then(Value::as_f64).ok_or("latitude is required")?;
    let lon = c.get("longitude").and_then(Value::as_f64).ok_or("longitude is required")?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err("coordinates are out of range".into());
    }
    let radius = c.get("radius").and_then(Value::as_f64).filter(|r| *r > 0.0 && *r <= 500.0).unwrap_or(20.0);
    let forget = Duration::from_secs(c.get("forget").and_then(Value::as_f64).filter(|m| *m >= 1.0 && *m <= 24.0 * 60.0).unwrap_or(20.0) as u64 * 60);
    Ok(Config { address, home: (lat, lon), radius, forget })
}

fn connect(address: &str) -> Result<BufReader<TcpStream>, String> {
    let addr = address.to_socket_addrs().map_err(|e| format!("{address}: {e}"))?.next().ok_or("no address")?;
    let s = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| format!("cannot reach {address}: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok();
    Ok(BufReader::new(s))
}

fn test(c: &Map<String, Value>) -> Result<String, String> {
    let cfg = config(c)?;
    let mut r = connect(&cfg.address)?;
    let until = Instant::now() + Duration::from_secs(30);
    let mut line = String::new();
    while Instant::now() < until {
        line.clear();
        if r.read_line(&mut line).is_ok() && line.starts_with("!AIVDM") {
            return Ok("Connected, and hearing ships.".into());
        }
    }
    Err("Connected, but no AIS heard for 30 seconds. Is the receiver running?".into())
}

// ------------------------------------------------------------------ decoding

/// The payload's bits, one per byte (AIS packs six bits in each character).
fn bits(payload: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() * 6);
    for c in payload.bytes() {
        let mut v = c.wrapping_sub(48);
        if v > 40 {
            v -= 8;
        }
        for i in (0..6).rev() {
            out.push((v >> i) & 1);
        }
    }
    out
}

fn uint(b: &[u8], start: usize, len: usize) -> Option<u64> {
    let s = b.get(start..start + len)?;
    Some(s.iter().fold(0, |acc, bit| acc << 1 | u64::from(*bit)))
}

fn int(b: &[u8], start: usize, len: usize) -> Option<i64> {
    let u = uint(b, start, len)? as i64;
    Some(if u >> (len - 1) & 1 == 1 { u - (1 << len) } else { u })
}

/// Six-bit ASCII, trailing '@' and spaces trimmed.
fn text(b: &[u8], start: usize, len: usize) -> Option<String> {
    let s: String = (0..len / 6).filter_map(|i| uint(b, start + i * 6, 6)).map(|v| char::from(if v < 32 { v as u8 + 64 } else { v as u8 })).collect();
    let s = s.trim_end_matches(['@', ' ']).trim().to_string();
    (!s.is_empty() && b.len() >= start + len).then_some(s)
}

#[derive(Debug, PartialEq)]
enum Msg {
    Position { mmsi: u32, lat: f64, lon: f64, sog: Option<f64>, cog: Option<f64>, heading: Option<f64> },
    Static { mmsi: u32, name: Option<String>, kind: Option<u8>, destination: Option<String> },
}

fn position(b: &[u8], mmsi: u32, sog: usize, lon: usize, lat: usize, cog: usize, hdg: usize) -> Option<Msg> {
    let (x, y) = (int(b, lon, 28)? as f64 / 600_000.0, int(b, lat, 27)? as f64 / 600_000.0);
    if !(-180.0..=180.0).contains(&x) || !(-90.0..=90.0).contains(&y) {
        return None; // 181 / 91: not available
    }
    Some(Msg::Position {
        mmsi,
        lat: y,
        lon: x,
        sog: uint(b, sog, 10).filter(|v| *v < 1023).map(|v| v as f64 / 10.0),
        cog: uint(b, cog, 12).filter(|v| *v < 3600).map(|v| v as f64 / 10.0),
        heading: uint(b, hdg, 9).filter(|v| *v < 360).map(|v| v as f64),
    })
}

/// The messages that say where a ship is, or what it is.
fn decode(payload: &str) -> Vec<Msg> {
    let b = bits(payload);
    let (Some(kind), Some(mmsi)) = (uint(&b, 0, 6), uint(&b, 8, 30).map(|m| m as u32)) else { return vec![] };
    match kind {
        1..=3 => position(&b, mmsi, 50, 61, 89, 116, 128).into_iter().collect(),
        18 => position(&b, mmsi, 46, 57, 85, 112, 124).into_iter().collect(),
        19 => {
            let mut out: Vec<Msg> = position(&b, mmsi, 46, 57, 85, 112, 124).into_iter().collect();
            out.push(Msg::Static { mmsi, name: text(&b, 143, 120), kind: uint(&b, 263, 8).map(|v| v as u8), destination: None });
            out
        }
        5 => vec![Msg::Static { mmsi, name: text(&b, 112, 120), kind: uint(&b, 232, 8).map(|v| v as u8), destination: text(&b, 302, 120) }],
        24 => match uint(&b, 38, 2) {
            Some(0) => vec![Msg::Static { mmsi, name: text(&b, 40, 120), kind: None, destination: None }],
            Some(1) => vec![Msg::Static { mmsi, name: None, kind: uint(&b, 40, 8).map(|v| v as u8), destination: None }],
            _ => vec![],
        },
        _ => vec![],
    }
}

/// Puts multi-part sentences back together. Returns whole payloads.
#[derive(Default)]
struct Assembler {
    parts: HashMap<String, Vec<Option<String>>>,
}

impl Assembler {
    fn line(&mut self, line: &str) -> Option<String> {
        let line = line.trim();
        let (body, sum) = line.strip_prefix('!')?.rsplit_once('*')?;
        let calc = body.bytes().fold(0u8, |a, c| a ^ c);
        if u8::from_str_radix(sum.get(..2)?, 16).ok()? != calc {
            return None;
        }
        let f: Vec<&str> = body.split(',').collect();
        if f.len() < 7 || !f[0].ends_with("VDM") {
            return None;
        }
        let (count, num): (usize, usize) = (f[1].parse().ok()?, f[2].parse().ok()?);
        if count == 1 {
            return Some(f[5].to_string());
        }
        if count > 9 || num == 0 || num > count {
            return None;
        }
        if self.parts.len() > 50 {
            self.parts.clear(); // lost fragments; don't keep them forever
        }
        let key = format!("{}{}", f[3], f[4]);
        let slots = self.parts.entry(key.clone()).or_insert_with(|| vec![None; count]);
        if slots.len() != count || num == 1 {
            *slots = vec![None; count];
        }
        slots[num - 1] = Some(f[5].to_string());
        if slots.iter().all(Option::is_some) {
            return self.parts.remove(&key).map(|s| s.into_iter().flatten().collect());
        }
        None
    }
}

// ------------------------------------------------------------------ ships

#[derive(Default)]
struct Ship {
    name: Option<String>,
    kind: Option<u8>,
    destination: Option<String>,
    at: Option<(f64, f64, Option<f64>, Option<f64>, Option<f64>)>, // lat, lon, sog, cog, heading
    seen: Option<Instant>,
}

/// Distance in km and bearing in degrees from `a` to `b`.
fn distance(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (la1, lo1, la2, lo2) = (a.0.to_radians(), a.1.to_radians(), b.0.to_radians(), b.1.to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2) + la1.cos() * la2.cos() * ((lo2 - lo1) / 2.0).sin().powi(2);
    let d = 2.0 * 6371.0 * h.sqrt().asin();
    let y = (lo2 - lo1).sin() * la2.cos();
    let x = la1.cos() * la2.sin() - la1.sin() * la2.cos() * (lo2 - lo1).cos();
    (d, (y.atan2(x).to_degrees() + 360.0) % 360.0)
}

/// Applies a message; true if it taught us a ship's name or type.
fn apply(ships: &mut HashMap<u32, Ship>, msg: Msg, now: Instant) -> bool {
    match msg {
        Msg::Position { mmsi, lat, lon, sog, cog, heading } => {
            let s = ships.entry(mmsi).or_default();
            s.at = Some((lat, lon, sog, cog, heading));
            s.seen = Some(now);
            false
        }
        Msg::Static { mmsi, name, kind, destination } => {
            if ships.len() > 2000 && !ships.contains_key(&mmsi) {
                return false;
            }
            let s = ships.entry(mmsi).or_default();
            let news = (name.is_some() && name != s.name) || (kind.is_some() && kind != s.kind);
            s.name = name.or(s.name.take());
            s.kind = kind.or(s.kind);
            s.destination = destination.or(s.destination.take());
            news
        }
    }
}

/// Ships only say their name every six minutes; remember the ones heard, so a restart
/// doesn't leave the screen full of numbers. The most recently seen 500 (about 15 kB).
fn names(ships: &HashMap<u32, Ship>) -> Value {
    let mut known: Vec<(&u32, &Ship)> = ships.iter().filter(|(_, s)| s.name.is_some() || s.kind.is_some()).collect();
    known.sort_by_key(|(_, s)| std::cmp::Reverse(s.seen));
    Value::Object(known.into_iter().take(500).map(|(m, s)| (m.to_string(), json!([s.name, s.kind]))).collect())
}

fn remembered(c: &Map<String, Value>) -> HashMap<u32, Ship> {
    let Some(map) = c.get("_names").and_then(Value::as_object) else { return HashMap::new() };
    map.iter()
        .filter_map(|(m, v)| Some((m.parse().ok()?, Ship { name: v[0].as_str().map(str::to_string), kind: v[1].as_u64().map(|k| k as u8), ..Default::default() })))
        .collect()
}

/// The nearest ships within the radius, as the screen gets them.
fn nearby(ships: &mut HashMap<u32, Ship>, cfg: &Config, now: Instant) -> Vec<Value> {
    ships.retain(|_, s| s.seen.is_none_or(|t| now.duration_since(t) < cfg.forget));
    let mut list: Vec<(f64, Value)> = ships
        .iter()
        .filter_map(|(mmsi, s)| {
            let (lat, lon, sog, cog, heading) = s.at?;
            let (d, brg) = distance(cfg.home, (lat, lon));
            (d <= cfg.radius).then(|| {
                let r1 = |v: f64| (v * 10.0).round() / 10.0;
                (d, json!({
                    "mmsi": mmsi, "name": s.name, "type": s.kind, "dest": s.destination,
                    "km": r1(d), "brg": brg.round(), "lat": (lat * 1e5).round() / 1e5, "lon": (lon * 1e5).round() / 1e5, "kn": sog.map(r1), "cog": cog.map(f64::round), "hdg": heading,
                    "age": s.seen.map(|t| now.duration_since(t).as_secs() / 60 * 60),
                }))
            })
        })
        .collect();
    list.sort_by(|a, b| a.0.total_cmp(&b.0));
    list.into_iter().take(SHOWN).map(|(_, v)| v).collect()
}

fn entity(list: &[Value], cfg: &Config) -> Entity {
    Entity {
        id: "ships".into(),
        name: "Ships nearby".into(),
        zone: String::new(),
        class: "ais".into(),
        capabilities: BTreeMap::from([
            ("ships".to_string(), Capability::new(json!(list), false)),
            ("radius".to_string(), Capability { units: Some("km".into()), ..Capability::new(json!(cfg.radius), false) }),
            ("home".to_string(), Capability::new(json!([cfg.home.0, cfg.home.1]), false)),
        ]),
    }
}

struct Ais {
    entity: Arc<Mutex<Entity>>,
    stopped: Arc<AtomicBool>,
    home: (f64, f64),
    radius: f64,
}

// ------------------------------------------------------------------ map tiles

/// Kartverket's open map tiles (CC BY 4.0): the nautical chart, and a grey map for night.
const LAYERS: [&str; 3] = ["sjokartraster", "topograatone", "topo"];

/// Web Mercator tile coordinates (fractional) of a place at zoom `z`.
fn tile_of((lat, lon): (f64, f64), z: u32) -> (f64, f64) {
    let n = f64::from(1u32 << z);
    let lat = lat.to_radians();
    ((lon + 180.0) / 360.0 * n, (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * n)
}

/// `layer-z-y-x` → the tile's path, if it is one the map around home would show.
/// Anything else is refused: this is not an open map proxy.
fn tile_path(key: &str, home: (f64, f64), radius_km: f64) -> Option<String> {
    let mut p = key.splitn(4, '-');
    let (layer, z, y, x) = (p.next()?, p.next()?.parse::<u32>().ok()?, p.next()?.parse::<u32>().ok()?, p.next()?.parse::<u32>().ok()?);
    if !LAYERS.contains(&layer) || !(6..=15).contains(&z) {
        return None;
    }
    let (hx, hy) = tile_of(home, z);
    let tile_km = 40_075.0 * home.0.to_radians().cos() / f64::from(1u32 << z);
    let reach = (radius_km * 3.0 / tile_km).ceil() + 2.0;
    ((f64::from(x) - hx).abs() <= reach && (f64::from(y) - hy).abs() <= reach).then(|| format!("/v1/wmts/1.0.0/{layer}/default/webmercator/{z}/{y}/{x}.png"))
}

fn start(c: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let cfg = config(c)?;
    let (cfg_home, cfg_radius) = (cfg.home, cfg.radius);
    let entity = Arc::new(Mutex::new(entity(&[], &cfg)));
    let stopped = Arc::new(AtomicBool::new(false));
    let (ent, stop, known) = (entity.clone(), stopped.clone(), remembered(c));
    std::thread::spawn(move || run(&cfg, known, &ent, &sink, &stop));
    Ok(Box::new(Ais { entity, stopped, home: cfg_home, radius: cfg_radius }))
}

/// Reads the stream for as long as it lasts, then reconnects; publishes every 10 s.
fn run(cfg: &Config, mut ships: HashMap<u32, Ship>, entity: &Mutex<Entity>, sink: &Sink, stopped: &AtomicBool) {
    let (mut learned, mut saved) = (false, Instant::now());
    let mut asm = Assembler::default();
    let mut backoff = Duration::from_secs(2);
    let mut published = Instant::now();
    while !stopped.load(Ordering::Relaxed) {
        let mut r = match connect(&cfg.address) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("ais: {e}");
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(60));
                continue;
            }
        };
        eprintln!("ais: connected to {}", cfg.address);
        let mut line = String::new();
        let mut heard = Instant::now();
        while !stopped.load(Ordering::Relaxed) {
            line.clear();
            match r.read_line(&mut line) {
                Ok(0) => break, // closed
                Ok(_) => {
                    heard = Instant::now();
                    backoff = Duration::from_secs(2);
                    for msg in asm.line(&line).map(|p| decode(&p)).unwrap_or_default() {
                        learned |= apply(&mut ships, msg, heard);
                    }
                }
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    // A quiet sea is normal for a while; a silent socket for 10 minutes is not.
                    if heard.elapsed() > Duration::from_secs(600) {
                        eprintln!("ais: nothing heard for 10 minutes, reconnecting");
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("ais: {e}");
                    break;
                }
            }
            if learned && saved.elapsed() >= Duration::from_secs(600) {
                (learned, saved) = (false, Instant::now());
                sink(Event::Remember { key: "_names".into(), value: names(&ships) });
            }
            if published.elapsed() >= Duration::from_secs(10) {
                published = Instant::now();
                let list = json!(nearby(&mut ships, cfg, published));
                let mut e = entity.lock().unwrap();
                let c = e.capabilities.get_mut("ships").unwrap();
                if c.value != list {
                    c.value = list.clone();
                    drop(e);
                    sink(Event::Changed { entity: "ships".into(), capability: "ships".into(), value: list });
                }
            }
        }
        std::thread::sleep(backoff);
    }
}

impl Instance for Ais {
    fn entities(&self) -> Vec<Entity> {
        vec![self.entity.lock().unwrap().clone()]
    }

    fn set(&self, _: &str, _: &str, _: &Value) -> Result<(), String> {
        Err("ships can only be watched".into())
    }

    fn image(&self, key: &str) -> Result<(String, Vec<u8>), String> {
        let path = tile_path(key, self.home, self.radius).ok_or("not a map tile near home")?;
        let res = crate::client::request("https://cache.kartverket.no", "GET", &path, &[], None)?;
        if res.status != 200 {
            return Err(format!("Kartverket answered HTTP {}", res.status));
        }
        Ok(("image/png".into(), res.body))
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_positions_and_names() {
        // Well-known samples from the AIS decoding literature (gpsd's test set).
        let mut asm = Assembler::default();
        let p = asm.line("!AIVDM,1,1,,B,177KQJ5000G?tO`K>RA1wUbN0TKH,0*5C\r\n").unwrap();
        match &decode(&p)[..] {
            [Msg::Position { mmsi, lat, lon, sog, cog, heading }] => {
                assert_eq!(*mmsi, 477553000);
                assert!((lat - 47.5828).abs() < 0.001 && (lon + 122.3458).abs() < 0.001, "{lat} {lon}");
                assert_eq!((*sog, *cog, *heading), (Some(0.0), Some(51.0), Some(181.0)));
            }
            other => panic!("{other:?}"),
        }
        assert!(asm.line("!AIVDM,1,1,,B,177KQJ5000G?tO`K>RA1wUbN0TKH,0*5D").is_none(), "bad checksum");

        // Type 5 in two parts: name, type and destination.
        assert!(asm.line("!AIVDM,2,1,1,A,55?MbV02;H;s<HtKR20EHE:0@T4@Dn2222222216L961O5Gf0NSQEp6ClRp8,0*1C").is_none());
        let p = asm.line("!AIVDM,2,2,1,A,88888888880,2*25").unwrap();
        assert_eq!(decode(&p), vec![Msg::Static { mmsi: 351759000, name: Some("EVER DIADEM".into()), kind: Some(70), destination: Some("NEW YORK".into()) }]);

        // Class B position.
        let p = asm.line("!AIVDM,1,1,,A,B52K>;h00Fc>jpUlNV@ikwpUoP06,0*4C").unwrap();
        assert!(matches!(decode(&p)[..], [Msg::Position { mmsi: 338087471, .. }]));
    }

    #[test]
    fn serves_only_map_tiles_near_home() {
        let home = (59.91, 10.75); // Oslo
        let (x, y) = tile_of(home, 11);
        assert_eq!((x as u32, y as u32), (1085, 595));
        let (x, y) = (x as u32, y as u32);
        assert_eq!(tile_path(&format!("sjokartraster-11-{y}-{x}"), home, 25.0), Some(format!("/v1/wmts/1.0.0/sjokartraster/default/webmercator/11/{y}/{x}.png")));
        assert!(tile_path(&format!("sjokartraster-11-{y}-{}", x + 200), home, 25.0).is_none(), "far away");
        assert!(tile_path(&format!("osm-11-{y}-{x}"), home, 25.0).is_none(), "unknown layer");
        assert!(tile_path("topo-18-0-0", home, 25.0).is_none(), "zoom out of range");
        assert!(tile_path(&format!("topo-11-{y}-{x}/../x"), home, 25.0).is_none());
    }

    #[test]
    fn keeps_the_nearest_within_the_radius() {
        let cfg = Config { address: String::new(), home: (59.0, 10.0), radius: 20.0, forget: Duration::from_secs(20 * 60) };
        let now = Instant::now();
        let mut ships = HashMap::new();
        apply(&mut ships, Msg::Position { mmsi: 1, lat: 59.1, lon: 10.0, sog: Some(12.0), cog: Some(90.0), heading: None }, now);
        apply(&mut ships, Msg::Position { mmsi: 2, lat: 59.05, lon: 10.0, sog: None, cog: None, heading: None }, now);
        apply(&mut ships, Msg::Position { mmsi: 3, lat: 60.0, lon: 10.0, sog: None, cog: None, heading: None }, now); // 111 km off
        apply(&mut ships, Msg::Static { mmsi: 1, name: Some("COLOR FANTASY".into()), kind: Some(60), destination: None }, now);
        let list = nearby(&mut ships, &cfg, now);
        let mut again = remembered(&Map::from_iter([("_names".to_string(), names(&ships))]));
        assert_eq!(again.remove(&1).and_then(|s| s.name).as_deref(), Some("COLOR FANTASY"), "names survive a restart");
        assert_eq!(list.iter().map(|s| s["mmsi"].as_u64().unwrap()).collect::<Vec<_>>(), vec![2, 1]);
        assert_eq!((list[1]["name"].as_str(), list[1]["km"].as_f64(), list[1]["brg"].as_f64()), (Some("COLOR FANTASY"), Some(11.1), Some(0.0)));
        let later = now + cfg.forget;
        assert!(nearby(&mut ships, &cfg, later).is_empty(), "forgotten when silent");
    }
}
