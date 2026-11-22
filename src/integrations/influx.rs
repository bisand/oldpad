//! InfluxDB 1.x: readings such as indoor and outdoor temperature, as already
//! collected for Grafana. Each value of one tag (e.g. `place`) becomes an
//! entity; mapped fields become read-only capabilities, plus the temperature's
//! low and high over the last 24 hours.

use super::{get_str, Capability, Entity, Event, Field, FieldKind, Instance, Kind, Sink};
use crate::client::request;
use crate::util::{b64_encode, percent_encode};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEFAULT_FIELDS: &str = "Temperature=measure_temperature, Humidity=measure_humidity, CO2=measure_co2, WindStrength=measure_wind_strength, Pressure=measure_pressure";

pub fn kind() -> Kind {
    let f = |key, label, kind, required, placeholder, help| Field { key, label, kind, required, placeholder, help };
    Kind {
        id: "influxdb",
        name: "InfluxDB",
        description: "Indoor and outdoor readings (temperature, humidity, CO₂, wind) from an InfluxDB 1.x database, for example what Grafana already shows.",
        fields: vec![
            f("address", "InfluxDB address", FieldKind::Url, true, "http://influxdb:8086", "The HTTP API, usually port 8086."),
            f("database", "Database", FieldKind::Text, true, "home", ""),
            f("username", "Username", FieldKind::Text, false, "", "Only if the database requires a login."),
            f("password", "Password", FieldKind::Secret, false, "", ""),
            f("measurement", "Measurement", FieldKind::Text, true, "weather", "Where the readings are stored."),
            f("tag", "One entity per value of tag", FieldKind::Text, true, "place", "Each value of this tag (e.g. Living room, Terrace) becomes something a screen can show."),
            f("filter", "Only where", FieldKind::Text, false, "location='home'", "Optional extra conditions: tag='value', joined with AND."),
            f("fields", "Fields", FieldKind::Text, false, DEFAULT_FIELDS, "Which fields to read and what they are. Wind is in km/h."),
        ],
        test,
        start,
    }
}

fn ident(s: &str) -> Result<String, String> {
    if s.is_empty() || s.len() > 64 || !s.chars().all(|c| c.is_alphanumeric() || " _-.".contains(c)) {
        return Err(format!("\"{s}\" is not a valid InfluxDB name"));
    }
    Ok(format!("\"{s}\""))
}

/// Accepts only `tag='value' AND tag2='value'`, so the admin's filter cannot
/// turn into arbitrary InfluxQL.
fn parse_filter(s: &str) -> Result<String, String> {
    let mut parts = Vec::new();
    for cond in s.split(" AND ").map(str::trim).filter(|c| !c.is_empty()) {
        let (k, v) = cond.split_once('=').ok_or_else(|| format!("filter \"{cond}\" must look like tag='value'"))?;
        let v = v.trim();
        let inner = v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')).ok_or("filter values need single quotes")?;
        if inner.contains(['\'', '\\']) {
            return Err("filter values cannot contain quotes or backslashes".into());
        }
        parts.push(format!("{} = '{inner}'", ident(k.trim())?));
    }
    Ok(parts.join(" AND "))
}

fn parse_fields(s: &str) -> Result<Vec<(String, String)>, String> {
    let s = if s.trim().is_empty() { DEFAULT_FIELDS } else { s };
    s.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (field, cap) = p.split_once('=').ok_or_else(|| format!("\"{p}\" must look like Field=capability"))?;
            ident(field.trim())?;
            let cap = cap.trim();
            if !cap.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
                return Err(format!("\"{cap}\" is not a capability name"));
            }
            Ok((field.trim().to_string(), cap.to_string()))
        })
        .collect()
}

struct Query {
    base: String,
    database: String,
    auth: Option<String>,
    measurement: String,
    tag: String,
    filter: String,
    fields: Vec<(String, String)>,
}

impl Query {
    fn new(c: &Map<String, Value>) -> Result<Query, String> {
        let user = get_str(c, "username");
        Ok(Query {
            base: get_str(c, "address").to_string(),
            database: get_str(c, "database").to_string(),
            auth: (!user.is_empty()).then(|| format!("Basic {}", b64_encode(format!("{user}:{}", get_str(c, "password")).as_bytes()))),
            measurement: ident(get_str(c, "measurement"))?,
            tag: ident(get_str(c, "tag"))?,
            filter: parse_filter(get_str(c, "filter"))?,
            fields: parse_fields(get_str(c, "fields"))?,
        })
    }

    fn run(&self, q: &str) -> Result<Value, String> {
        let path = format!("/query?db={}&epoch=s&q={}", percent_encode(&self.database), percent_encode(q));
        let auth = self.auth.as_deref().map(|a| ("Authorization", a));
        let res = request(&self.base, "GET", &path, auth.as_slice(), None)?;
        if res.status == 401 {
            return Err("InfluxDB refused the username or password".into());
        }
        let v = res.json()?;
        if let Some(e) = v["error"].as_str().or(v["results"][0]["error"].as_str()) {
            return Err(format!("InfluxDB: {e}"));
        }
        if res.status != 200 {
            return Err(format!("InfluxDB answered HTTP {}", res.status));
        }
        Ok(v)
    }

    fn where_(&self, window: &str) -> String {
        match self.filter.as_str() {
            "" => format!("WHERE time > now() - {window}"),
            f => format!("WHERE time > now() - {window} AND {f}"),
        }
    }

    /// 24 hours in 15-minute averages for every entity. One query, so the tag value of
    /// an entity never becomes part of InfluxQL.
    fn history(&self) -> Result<Value, String> {
        let selects: Vec<String> = self.fields.iter().map(|(f, _)| format!("mean(\"{f}\") AS \"{f}\"")).collect();
        let v = self.run(&format!("SELECT {} FROM {} {} GROUP BY time(15m), {} fill(none)", selects.join(", "), self.measurement, self.where_("24h"), self.tag))?;
        Ok(parse_history(&v, self.tag.trim_matches('"'), &self.fields))
    }

    fn load(&self) -> Result<Vec<Entity>, String> {
        let selects: Vec<String> = self.fields.iter().map(|(f, _)| format!("last(\"{f}\") AS \"{f}\"")).collect();
        let latest = self.run(&format!("SELECT {} FROM {} {} GROUP BY {}", selects.join(", "), self.measurement, self.where_("2h"), self.tag))?;
        let range = self.run(&format!(
            "SELECT min(\"Temperature\") AS lo, max(\"Temperature\") AS hi FROM {} {} GROUP BY {}",
            self.measurement,
            self.where_("24h"),
            self.tag
        ));
        let tag_name = self.tag.trim_matches('"');
        let mut out: BTreeMap<String, Entity> = BTreeMap::new();

        for s in latest["results"][0]["series"].as_array().into_iter().flatten() {
            let Some(name) = s["tags"][tag_name].as_str().filter(|n| !n.is_empty()) else { continue };
            let cols: Vec<&str> = s["columns"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            let row = &s["values"][0];
            let mut caps = BTreeMap::new();
            for (field, cap) in &self.fields {
                let Some(i) = cols.iter().position(|c| c == field) else { continue };
                if let Some(v) = row[i].as_f64() {
                    let mut c = Capability::new(json!((v * 10.0).round() / 10.0), false);
                    c.units = units(cap).map(str::to_string);
                    caps.insert(cap.clone(), c);
                }
            }
            if !caps.is_empty() {
                caps.insert("history".into(), Capability::new(Value::Null, false));
                out.insert(name.to_string(), Entity { id: name.to_string(), name: name.to_string(), zone: String::new(), class: "sensor".into(), capabilities: caps });
            }
        }
        // The 24-hour range is a nicety: a failure there must not hide the readings.
        if let Ok(range) = range {
            for s in range["results"][0]["series"].as_array().into_iter().flatten() {
                let Some(e) = s["tags"][tag_name].as_str().and_then(|n| out.get_mut(n)) else { continue };
                for (i, cap) in [(1, "temperature_min"), (2, "temperature_max")] {
                    if let Some(v) = s["values"][0][i].as_f64() {
                        let mut c = Capability::new(json!((v * 10.0).round() / 10.0), false);
                        c.units = Some("°C".into());
                        e.capabilities.insert(cap.into(), c);
                    }
                }
            }
        }
        Ok(out.into_values().collect())
    }
}

/// `{entity: {capability: [[epoch, value], …]}}`, leaving out gaps.
fn parse_history(v: &Value, tag: &str, fields: &[(String, String)]) -> Value {
    let mut out = Map::new();
    for s in v["results"][0]["series"].as_array().into_iter().flatten() {
        let Some(name) = s["tags"][tag].as_str().filter(|n| !n.is_empty()) else { continue };
        let cols: Vec<&str> = s["columns"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let rows = s["values"].as_array().map(Vec::as_slice).unwrap_or_default();
        let mut caps = Map::new();
        for (field, cap) in fields {
            let Some(i) = cols.iter().position(|c| c == field) else { continue };
            let points: Vec<Value> = rows.iter().filter_map(|r| Some(json!([r[0].as_i64()?, (r[i].as_f64()? * 10.0).round() / 10.0]))).collect();
            if !points.is_empty() {
                caps.insert(cap.clone(), Value::Array(points));
            }
        }
        out.insert(name.to_string(), Value::Object(caps));
    }
    Value::Object(out)
}

fn units(cap: &str) -> Option<&'static str> {
    Some(match cap {
        "measure_temperature" => "°C",
        "measure_humidity" => "%",
        "measure_co2" => "ppm",
        "measure_wind_strength" => "km/h",
        "measure_pressure" => "mbar",
        _ => return None,
    })
}

fn test(config: &Map<String, Value>) -> Result<String, String> {
    let list = Query::new(config)?.load()?;
    if list.is_empty() {
        return Err("Connected, but found no readings in the last two hours. Check the measurement, tag and filter.".into());
    }
    let names: Vec<&str> = list.iter().map(|e| e.name.as_str()).collect();
    Ok(format!("Found {} with recent readings: {}.", list.len(), names.join(", ")))
}

struct Influx {
    entities: Arc<Mutex<Vec<Entity>>>,
    stopped: Arc<AtomicBool>,
    query: Arc<Query>,
    /// Screens share one answer for 5 minutes, so they cannot load the database.
    history: Mutex<Option<(Instant, Value)>>,
}

fn start(config: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let query = Arc::new(Query::new(config)?);
    let q = query.clone();
    let entities = Arc::new(Mutex::new(q.load()?));
    let stopped = Arc::new(AtomicBool::new(false));
    let (ents, stop) = (entities.clone(), stopped.clone());
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(60));
            match q.load() {
                Ok(fresh) => {
                    let mut changes = Vec::new();
                    let mut list = ents.lock().unwrap();
                    for e in &fresh {
                        let old = list.iter().find(|o| o.id == e.id);
                        for (cap, c) in &e.capabilities {
                            if old.and_then(|o| o.capabilities.get(cap)).map(|o| &o.value) != Some(&c.value) {
                                changes.push((e.id.clone(), cap.clone(), c.value.clone()));
                            }
                        }
                    }
                    let reshaped = fresh.len() != list.len();
                    *list = fresh;
                    drop(list);
                    for (entity, capability, value) in changes {
                        sink(Event::Changed { entity, capability, value });
                    }
                    if reshaped {
                        sink(Event::Reloaded);
                    }
                }
                Err(e) => eprintln!("influxdb: {e}"),
            }
        }
    });
    Ok(Box::new(Influx { entities, stopped, query, history: Mutex::new(None) }))
}

impl Instance for Influx {
    fn entities(&self) -> Vec<Entity> {
        self.entities.lock().unwrap().clone()
    }
    fn set(&self, _: &str, _: &str, _: &Value) -> Result<(), String> {
        Err("readings are read-only".into())
    }
    fn history(&self, entity: &str) -> Result<Value, String> {
        let mut cache = self.history.lock().unwrap();
        if !cache.as_ref().is_some_and(|(at, _)| at.elapsed() < Duration::from_secs(300)) {
            *cache = Some((Instant::now(), self.query.history()?));
        }
        Ok(cache.as_ref().map_or(Value::Null, |(_, all)| all.get(entity).cloned().unwrap_or_else(|| json!({}))))
    }
    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_cannot_inject() {
        assert_eq!(parse_filter("location='home' AND floor='1'").unwrap(), "\"location\" = 'home' AND \"floor\" = '1'");
        assert_eq!(parse_filter("").unwrap(), "");
        assert!(parse_filter("location='x' OR 1=1").is_err());
        assert!(parse_filter("location='x'; DROP DATABASE home").is_err());
        assert!(parse_filter("location=x").is_err());
        assert!(parse_filter("a\"b='x'").is_err());
        assert!(ident("weather\"; DROP").is_err());
    }

    #[test]
    fn history_keeps_each_place_and_skips_gaps() {
        let fields = parse_fields("Temperature=measure_temperature, CO2=measure_co2, Gone=measure_pressure").unwrap();
        let v = json!({"results": [{"series": [
            {"tags": {"place": "Stue"}, "columns": ["time", "Temperature", "CO2", "Gone"], "values": [[100, 21.26, null, null], [1000, null, 612.0, null], [1900, 21.0, 640.0, null]]},
            {"tags": {"place": ""}, "columns": ["time", "Temperature"], "values": [[100, 1.0]]},
        ]}]});
        assert_eq!(parse_history(&v, "place", &fields), json!({"Stue": {
            "measure_temperature": [[100, 21.3], [1900, 21.0]],
            "measure_co2": [[1000, 612.0], [1900, 640.0]],
        }}));
        assert_eq!(parse_history(&json!({"results": [{}]}), "place", &fields), json!({}), "nothing stored yet");
    }

    #[test]
    fn field_map() {
        let f = parse_fields("").unwrap();
        assert!(f.contains(&("CO2".to_string(), "measure_co2".to_string())));
        assert!(parse_fields("Temp=Measure").is_err());
        assert!(parse_fields("Temp").is_err());
    }
}

/// Against a real server: OLDPAD_TEST_INFLUX=http://localhost:18086 OLDPAD_TEST_INFLUX_DB=home cargo test influx_live -- --ignored --nocapture
#[cfg(test)]
#[test]
#[ignore]
fn influx_live() {
    let c = json!({
        "address": std::env::var("OLDPAD_TEST_INFLUX").unwrap(),
        "database": std::env::var("OLDPAD_TEST_INFLUX_DB").unwrap(),
        "measurement": "weather", "tag": "place",
        "filter": std::env::var("OLDPAD_TEST_INFLUX_FILTER").unwrap_or_default(),
    });
    let c = c.as_object().unwrap();
    println!("{}", test(c).unwrap());
    for e in Query::new(c).unwrap().load().unwrap() {
        println!("{}: {}", e.name, e.capabilities.iter().map(|(k, v)| format!("{k}={}", v.value)).collect::<Vec<_>>().join(" "));
    }
}
