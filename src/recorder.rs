//! A day of readings for sensors whose system keeps no history of its own, so the
//! graphs page works without InfluxDB. In its own file: it changes every 15 minutes
//! and does not belong in backups.

use crate::integrations::Entity;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What the graphs page can plot.
pub const GRAPHED: [&str; 4] = ["measure_temperature", "measure_humidity", "measure_co2", "measure_wind_strength"];
pub const EVERY: u64 = 900;
const KEEP: i64 = 86_400 + EVERY as i64;

type Series = BTreeMap<String, BTreeMap<String, Vec<(i64, f64)>>>;

pub struct Recorder {
    file: PathBuf,
    data: Mutex<Series>,
}

impl Recorder {
    /// A damaged file costs at most a day of graphs, so it never stops the server.
    pub fn open(dir: &Path) -> Recorder {
        let file = dir.join("history.json");
        let data = match std::fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                eprintln!("history: starting afresh, could not read {}: {e}", file.display());
                Series::new()
            }),
            Err(_) => Series::new(),
        };
        Recorder { file, data: Mutex::new(data) }
    }

    /// Adds the current readings at `t`, forgets what is over a day old, and saves.
    pub fn record(&self, t: i64, entities: &[Entity]) {
        let bytes = {
            let mut data = self.data.lock().unwrap();
            for e in entities {
                for cap in GRAPHED {
                    let Some(v) = e.capabilities.get(cap).and_then(|c| c.value.as_f64()).filter(|v| v.is_finite()) else { continue };
                    let points = data.entry(e.id.clone()).or_default().entry(cap.to_string()).or_default();
                    // A restart within the quarter replaces its point; a clock set back drops what is now "ahead".
                    points.retain(|p| p.0 < t);
                    points.push((t, (v * 10.0).round() / 10.0));
                }
            }
            for caps in data.values_mut() {
                for points in caps.values_mut() {
                    points.retain(|p| p.0 > t - KEEP);
                }
                caps.retain(|_, points| !points.is_empty());
            }
            data.retain(|_, caps| !caps.is_empty());
            serde_json::to_vec(&*data)
        };
        if let Err(e) = bytes.map_err(std::io::Error::other).and_then(|b| self.save(&b)) {
            eprintln!("history: could not save {}: {e}", self.file.display());
        }
    }

    fn save(&self, bytes: &[u8]) -> std::io::Result<()> {
        let tmp = self.file.with_extension("json.tmp");
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        std::io::Write::write_all(&mut opts.open(&tmp)?, bytes)?;
        std::fs::rename(&tmp, &self.file)
    }

    /// `{capability: [[epoch, value], …]}`, or None with nothing recorded yet.
    pub fn get(&self, id: &str) -> Option<Value> {
        self.data.lock().unwrap().get(id).map(|caps| json!(caps))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::Capability;

    fn sensor(id: &str, temp: Value) -> Entity {
        let caps = [("measure_temperature".to_string(), Capability::new(temp, false)), ("onoff".to_string(), Capability::new(json!(true), true))];
        Entity { id: id.into(), name: id.into(), zone: String::new(), class: "sensor".into(), capabilities: caps.into_iter().collect() }
    }

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("oldpad-history-{}", crate::util::random_code(8)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn keeps_a_day_and_survives_a_restart() {
        let d = dir();
        let r = Recorder::open(&d);
        assert_eq!(r.get("x:a"), None);
        r.record(1_000_000, &[sensor("x:a", json!(20.26))]);
        r.record(1_000_900, &[sensor("x:a", Value::Null)]);
        r.record(1_001_800, &[sensor("x:a", json!(21.0))]);
        assert_eq!(r.get("x:a"), Some(json!({"measure_temperature": [[1_000_000, 20.3], [1_001_800, 21.0]]})), "only what can be graphed; gaps stay gaps");

        let again = Recorder::open(&d);
        assert_eq!(again.get("x:a"), r.get("x:a"), "back after a restart");
        again.record(1_001_800, &[sensor("x:a", json!(22.0))]);
        assert_eq!(again.get("x:a").unwrap()["measure_temperature"][1], json!([1_001_800, 22.0]), "the same quarter is replaced");

        again.record(1_000_000 + KEEP + 1, &[sensor("x:b", json!(5.0))]);
        assert_eq!(again.get("x:a").unwrap()["measure_temperature"].as_array().unwrap().len(), 1, "older than a day is gone");
        again.record(1_001_800 + KEEP, &[]);
        assert_eq!(again.get("x:a"), None, "a sensor no longer recorded fades out");
    }

    #[test]
    fn a_damaged_file_only_costs_the_history() {
        let d = dir();
        std::fs::write(d.join("history.json"), b"{not json").unwrap();
        let r = Recorder::open(&d);
        assert_eq!(r.get("x:a"), None);
        r.record(1_000, &[sensor("x:a", json!(1.0))]);
        assert!(Recorder::open(&d).get("x:a").is_some());
    }
}
