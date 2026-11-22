//! Whether a newer release is out, from the project's public GitHub releases.
//! Only tells: oldpad never updates itself.

use crate::client::request;
use crate::util::{IS_RELEASE, VERSION};
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::Duration;

const EVERY: Duration = Duration::from_secs(6 * 3600);

#[derive(Default)]
pub struct Updates {
    /// The newest stable release, once a check has found one.
    latest: Mutex<Option<String>>,
}

impl Updates {
    /// What the settings pages show. `state`: current, available, unknown or dev.
    pub fn status(&self) -> Value {
        let latest = self.latest.lock().unwrap().clone();
        json!({
            "version": VERSION,
            "commit": option_env!("OLDPAD_COMMIT").unwrap_or(""),
            "latest": latest,
            "state": state(VERSION, IS_RELEASE, latest.as_deref()),
        })
    }

    /// A failed check keeps the last answer: being offline does not mean up to date.
    pub fn run(&self) {
        std::thread::sleep(Duration::from_secs(30));
        loop {
            match fetch() {
                Ok(Some(v)) => *self.latest.lock().unwrap() = Some(v),
                Ok(None) => {}
                Err(e) => eprintln!("updates: {e}"),
            }
            std::thread::sleep(EVERY);
        }
    }
}

/// The newest stable release, or None if there are none yet.
fn fetch() -> Result<Option<String>, String> {
    let res = request("https://api.github.com", "GET", "/repos/bisand/oldpad/releases/latest", &[("Accept", "application/vnd.github+json")], None)?;
    match res.status {
        404 => Ok(None),
        200 => {
            let tag = res.json()?["tag_name"].as_str().unwrap_or("").to_string();
            let v = tag.strip_prefix('v').filter(|v| parse(v).is_some_and(|p| !p.3)).ok_or("the latest release has no valid version")?;
            Ok(Some(v.to_string()))
        }
        s => Err(format!("GitHub answered HTTP {s}")),
    }
}

/// (major, minor, patch, is a prerelease). No leading zeros, as in semver.
fn parse(v: &str) -> Option<(u64, u64, u64, bool)> {
    let (core, pre) = v.split_once('-').map_or((v, false), |(c, p)| (c, !p.is_empty()));
    if v.contains('-') && !pre {
        return None;
    }
    let mut parts = core.split('.').map(|p| {
        let plain = !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && (p == "0" || !p.starts_with('0'));
        plain.then(|| p.parse::<u64>().ok()).flatten()
    });
    let found = (parts.next()??, parts.next()??, parts.next()??, pre);
    parts.next().is_none().then_some(found)
}

fn state(current: &str, release: bool, latest: Option<&str>) -> &'static str {
    // A build of `main` may be ahead of every release, so it cannot be called out of date.
    if !release {
        return "dev";
    }
    let (Some(c), Some(l)) = (parse(current), latest.and_then(parse)) else { return "unknown" };
    // Same numbers: the stable release is newer than its own prerelease.
    let newer = (l.0, l.1, l.2) > (c.0, c.1, c.2) || ((l.0, l.1, l.2) == (c.0, c.1, c.2) && c.3 && !l.3);
    if newer { "available" } else { "current" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        let s = |current, latest| state(current, true, Some(latest));
        assert_eq!(s("1.2.3", "1.2.3"), "current");
        assert_eq!(s("1.2.3", "1.2.4"), "available");
        assert_eq!(s("1.9.0", "1.10.0"), "available", "by number, not by text");
        assert_eq!(s("1.10.0", "1.9.0"), "current");
        assert_eq!(s("2.0.0", "1.9.9"), "current");
        assert_eq!(s("1.3.0-rc.1", "1.3.0"), "available", "the release follows its prerelease");
        assert_eq!(s("1.3.0-rc.1", "1.2.9"), "current");
        assert_eq!(s("1.2.3", "nonsense"), "unknown");
        assert_eq!(state("1.2.3", true, None), "unknown", "not checked yet is not up to date");
        assert_eq!(state("0.1.0", false, Some("9.0.0")), "dev", "a build of main is never called old");
    }

    #[test]
    fn reads_only_real_versions() {
        assert_eq!(parse("1.2.3"), Some((1, 2, 3, false)));
        assert_eq!(parse("1.2.3-rc.1"), Some((1, 2, 3, true)));
        for bad in ["", "1.2", "1.2.3.4", "01.2.3", "1.2.x", "1.2.3-", "v1.2.3", "1..3"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    /// Against GitHub: cargo test update_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn update_live() {
        println!("latest release: {:?}", fetch().unwrap());
    }
}
