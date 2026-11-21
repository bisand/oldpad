//! Calendars from a CalDAV account (read-only). One entity per calendar, in the same
//! shape as the iCal integration. The password goes only to the configured HTTPS origin:
//! redirects and addresses the server names elsewhere are refused.

use super::calendar::{agenda, now, split_url, DAY};
use super::spot::civil_from_days;
use super::{get_str, Capability, Entity, Event, Field, FieldKind, Instance, Kind, Sink};
use crate::client::request;
use crate::util::{b64_encode, percent_encode, sha256_hex, unescape_xml};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const REFRESH: Duration = Duration::from_secs(15 * 60);
const MAX_CALENDARS: usize = 16;
const BAD_LOGIN: &str = "the server refused the username or password";

const PRINCIPAL: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:current-user-principal/></d:prop></d:propfind>"#;
const HOME: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><c:calendar-home-set/></d:prop></d:propfind>"#;
const LIST: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:resourcetype/><d:displayname/><c:supported-calendar-component-set/></d:prop></d:propfind>"#;

pub fn kind() -> Kind {
    Kind {
        id: "caldav",
        name: "Calendar (CalDAV)",
        description: "Coming events from a CalDAV account, such as Domeneshop. Read-only.",
        fields: vec![
            Field { key: "address", label: "CalDAV server", kind: FieldKind::Url, required: true, placeholder: "https://caldav.domeneshop.no", help: "Must be https://." },
            Field { key: "username", label: "Username", kind: FieldKind::Text, required: true, placeholder: "oldpad@example.com", help: "Use a dedicated account with your calendars shared to it, not your own mailbox." },
            Field { key: "password", label: "Password", kind: FieldKind::Secret, required: true, placeholder: "", help: "Stored on the oldpad server and included in backups." },
            Field { key: "days", label: "Days ahead", kind: FieldKind::Number, required: false, placeholder: "14", help: "" },
        ],
        test,
        start,
    }
}

// ------------------------------------------------------------------ XML

/// The inner text of every element with this local name, whatever its namespace prefix.
fn elements<'a>(xml: &'a str, local: &str) -> Vec<&'a str> {
    let (mut out, mut pos) = (Vec::new(), 0);
    while let Some(i) = xml[pos..].find('<') {
        let start = pos + i + 1;
        let Some(len) = xml[start..].find('>') else { break };
        let tag = &xml[start..start + len];
        pos = start + len + 1;
        if tag.starts_with(['/', '?', '!']) {
            continue;
        }
        let name = tag.trim_end_matches('/').split_whitespace().next().unwrap_or("");
        if name.rsplit(':').next() != Some(local) {
            continue;
        }
        if tag.ends_with('/') {
            out.push("");
            continue;
        }
        let Some(end) = xml[pos..].find(&format!("</{name}")) else { continue };
        out.push(&xml[pos..pos + end]);
        pos += end;
    }
    out
}

fn text(s: &str) -> String {
    let s = s.trim();
    match s.strip_prefix("<![CDATA[").and_then(|x| x.strip_suffix("]]>")) {
        Some(data) => data.to_string(),
        None => unescape_xml(s),
    }
}

/// A response's properties the server actually has (status 200).
fn found(response: &str) -> String {
    elements(response, "propstat").into_iter().filter(|p| elements(p, "status").first().is_some_and(|s| s.contains(" 200"))).collect()
}

/// Accepts only a path on our own origin, so the password cannot be sent anywhere else.
fn same_origin(origin: &str, href: &str) -> Option<String> {
    let href = href.trim();
    let path = href.strip_prefix(origin).unwrap_or(href);
    (path.starts_with('/') && !path.starts_with("//") && !path.chars().any(|c| c.is_control() || c == ' ')).then(|| path.to_string())
}

// ------------------------------------------------------------------ protocol

struct Dav {
    origin: String,
    auth: String,
    user: String,
}

enum Failure {
    Login,
    Missing,
    Other(String),
}

impl From<Failure> for String {
    fn from(f: Failure) -> String {
        match f {
            Failure::Login => BAD_LOGIN.into(),
            Failure::Missing => "the server has no calendars at that address".into(),
            Failure::Other(e) => e,
        }
    }
}

impl Dav {
    fn call(&self, method: &str, path: &str, depth: &str, body: &str) -> Result<String, Failure> {
        let mut path = path.to_string();
        for _ in 0..4 {
            let headers = [("Authorization", self.auth.as_str()), ("Depth", depth), ("Content-Type", "application/xml; charset=utf-8"), ("Accept", "application/xml, text/xml")];
            let res = request(&self.origin, method, &path, &headers, Some(body.as_bytes())).map_err(Failure::Other)?;
            match res.status {
                200 | 207 => return Ok(String::from_utf8_lossy(&res.body).into_owned()),
                301 | 302 | 303 | 307 | 308 => {
                    let to = res.headers.get("location").ok_or(Failure::Missing)?;
                    path = same_origin(&self.origin, to).ok_or_else(|| Failure::Other("the server redirected to another address; not sending the password there".into()))?;
                }
                401 => return Err(Failure::Login),
                403 | 404 | 405 => return Err(Failure::Missing),
                s => return Err(Failure::Other(format!("the CalDAV server answered HTTP {s}"))),
            }
        }
        Err(Failure::Other("too many redirects".into()))
    }

    /// The property's `href` on the first response that has it.
    fn href(&self, path: &str, body: &str, property: &str) -> Result<Option<String>, Failure> {
        let xml = self.call("PROPFIND", path, "0", body)?;
        let href = elements(&xml, "response").iter().find_map(|r| {
            let props = found(r);
            let inner = *elements(&props, property).first()?;
            same_origin(&self.origin, &text(elements(inner, "href").first()?))
        });
        Ok(href)
    }

    /// Calendars directly under `home`: (path, name).
    fn list(&self, home: &str) -> Result<Vec<(String, String)>, Failure> {
        let xml = self.call("PROPFIND", home, "1", LIST)?;
        let mut out = Vec::new();
        for r in elements(&xml, "response") {
            let props = found(r);
            let is_calendar = elements(&props, "resourcetype").first().is_some_and(|t| !elements(t, "calendar").is_empty());
            let components = elements(&props, "supported-calendar-component-set");
            let has_events = components.first().is_none_or(|c| c.trim().is_empty() || c.contains("VEVENT"));
            let Some(path) = elements(r, "href").first().and_then(|h| same_origin(&self.origin, &text(h))) else { continue };
            if is_calendar && has_events && out.len() < MAX_CALENDARS {
                let name = elements(&props, "displayname").first().map(|n| text(n)).filter(|n| !n.is_empty());
                out.push((path, name.unwrap_or_else(|| "Calendar".into())));
            }
        }
        Ok(out)
    }

    /// Standard discovery, then the usual places for servers that do not advertise it.
    fn calendars(&self, start: &str) -> Result<Vec<(String, String)>, String> {
        let user = percent_encode(&self.user);
        let candidates = [start.to_string(), "/.well-known/caldav".into(), "/".into(), format!("/principals/{user}/"), format!("/principals/users/{user}/")];
        for path in &candidates {
            let principal = match self.href(path, PRINCIPAL, "current-user-principal") {
                Ok(p) => p.unwrap_or_else(|| path.clone()),
                Err(Failure::Login) => return Err(BAD_LOGIN.into()),
                Err(_) => continue,
            };
            match self.href(&principal, HOME, "calendar-home-set") {
                Ok(Some(home)) => {
                    let list = self.list(&home).map_err(String::from)?;
                    if !list.is_empty() {
                        return Ok(list);
                    }
                }
                Err(Failure::Login) => return Err(BAD_LOGIN.into()),
                _ => {}
            }
        }
        // The address may already be the calendar home.
        match self.list(start) {
            Ok(list) if !list.is_empty() => Ok(list),
            Err(Failure::Login) => Err(BAD_LOGIN.into()),
            // Some servers answer 404 rather than 401 to a bad login.
            _ => Err("no calendars found. Check the username and password, and that a calendar is shared with this account".into()),
        }
    }

    fn entity(&self, path: &str, name: &str, days: i64) -> Result<Entity, String> {
        let t = now();
        let query = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><c:calendar-data/></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{}" end="{}"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#,
            stamp(t - 2 * DAY),
            stamp(t + (days + 2) * DAY)
        );
        let xml = self.call("REPORT", path, "1", &query).map_err(String::from)?;
        let data: String = elements(&xml, "calendar-data").into_iter().map(text).collect::<Vec<_>>().join("\r\n");
        let (_, events) = agenda(data.as_bytes(), t, days);
        Ok(Entity {
            id: format!("dav-{}", &sha256_hex(path)[..16]),
            name: name.to_string(),
            zone: "Calendar".into(),
            class: "calendar".into(),
            capabilities: BTreeMap::from([("events".to_string(), Capability::new(json!(events), false))]),
        })
    }

    fn load(&self, start: &str, days: i64) -> Result<Vec<Entity>, String> {
        self.calendars(start)?.iter().map(|(path, name)| self.entity(path, name, days)).collect()
    }
}

fn stamp(t: i64) -> String {
    let (y, m, d) = civil_from_days(t.div_euclid(DAY));
    let s = t.rem_euclid(DAY);
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", s / 3600, s / 60 % 60, s % 60)
}

fn config(c: &Map<String, Value>) -> Result<(Dav, String, i64), String> {
    let (origin, path) = split_url(get_str(c, "address"))?;
    if !origin.starts_with("https://") {
        return Err("the CalDAV server must use https://, so the password is encrypted".into());
    }
    let (user, password) = (get_str(c, "username"), get_str(c, "password"));
    if user.is_empty() || password.is_empty() || user.contains(':') || user.chars().any(char::is_control) {
        return Err("enter a valid username and password".into());
    }
    let auth = format!("Basic {}", b64_encode(format!("{user}:{password}").as_bytes()));
    let days = c.get("days").and_then(Value::as_f64).map_or(14, |d| d.clamp(1.0, 60.0) as i64);
    Ok((Dav { origin, auth, user: user.to_string() }, path, days))
}

fn test(c: &Map<String, Value>) -> Result<String, String> {
    let (dav, path, days) = config(c)?;
    let list = dav.load(&path, days)?;
    Ok(list.iter().map(|e| format!("{}: {} events in the next {days} days", e.name, e.capabilities["events"].value.as_array().map_or(0, Vec::len))).collect::<Vec<_>>().join(". "))
}

struct CalDav {
    entities: Arc<Mutex<Vec<Entity>>>,
    stopped: Arc<AtomicBool>,
}

fn start(c: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let (dav, path, days) = config(c)?;
    let entities = Arc::new(Mutex::new(dav.load(&path, days)?));
    let stopped = Arc::new(AtomicBool::new(false));
    let (ents, stop) = (entities.clone(), stopped.clone());
    std::thread::spawn(move || {
        let mut last = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(1));
            if last.elapsed() < REFRESH {
                continue;
            }
            last = Instant::now();
            // A failed refresh keeps showing what we have.
            let fresh = match dav.load(&path, days) {
                Ok(list) => list,
                Err(e) => {
                    eprintln!("caldav: {e}");
                    continue;
                }
            };
            let mut all = ents.lock().unwrap();
            let same_shape = all.len() == fresh.len() && all.iter().zip(&fresh).all(|(a, b)| a.id == b.id && a.name == b.name);
            let changed: Vec<(String, Value)> = fresh.iter().zip(all.iter()).filter(|(a, b)| a.capabilities["events"].value != b.capabilities["events"].value).map(|(a, _)| (a.id.clone(), a.capabilities["events"].value.clone())).collect();
            *all = fresh;
            drop(all);
            if !same_shape {
                sink(Event::Reloaded);
            } else {
                for (entity, value) in changed {
                    sink(Event::Changed { entity, capability: "events".into(), value });
                }
            }
        }
    });
    Ok(Box::new(CalDav { entities, stopped }))
}

impl Instance for CalDav {
    fn entities(&self) -> Vec<Entity> {
        self.entities.lock().unwrap().clone()
    }
    fn set(&self, _: &str, _: &str, _: &Value) -> Result<(), String> {
        Err("calendars are read-only".into())
    }
    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn multi(body: &str) -> String {
        format!(r#"<?xml version="1.0"?><D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">{body}</D:multistatus>"#)
    }

    fn prop(href: &str, props: &str) -> String {
        format!("<D:response><D:href>{href}</D:href><D:propstat><D:prop>{props}</D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>")
    }

    /// A small CalDAV server. Returns its origin; a wrong password gets 401.
    fn server(auth: String, redirect_to: Option<String>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut s) = conn else { continue };
                let mut raw = Vec::new();
                let mut b = [0u8; 4096];
                loop {
                    let n = s.read(&mut b).unwrap_or(0);
                    raw.extend(&b[..n]);
                    let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
                        if n == 0 { break }
                        continue;
                    };
                    let head = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
                    let len: usize = head.lines().find_map(|l| l.strip_prefix("content-length: ")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                    if raw.len() >= end + 4 + len || n == 0 { break }
                }
                let req = String::from_utf8_lossy(&raw).into_owned();
                let mut first = req.lines().next().unwrap_or("").split(' ');
                let (method, path) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
                let ok = req.lines().any(|l| l == format!("Authorization: {auth}"));
                let t = now() + 3600;
                let ics = format!("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:1\r\nDTSTART:{}\r\nSUMMARY:Tannlege &amp; kontroll\r\nEND:VEVENT\r\nEND:VCALENDAR", stamp(t));
                let (status, extra, body) = if !ok {
                    ("401 Unauthorized", String::new(), String::new())
                } else if let (Some(to), "/") = (&redirect_to, path) {
                    ("302 Found", format!("Location: {to}\r\n"), String::new())
                } else {
                    match (method, path) {
                        ("PROPFIND", "/") => ("207 Multi-Status", String::new(), multi(&prop("/", "<D:current-user-principal><D:href>/principals/me/</D:href></D:current-user-principal>"))),
                        ("PROPFIND", "/principals/me/") => ("207 Multi-Status", String::new(), multi(&prop("/principals/me/", "<C:calendar-home-set><D:href>/cal/me/</D:href></C:calendar-home-set>"))),
                        ("PROPFIND", "/cal/me/") => ("207 Multi-Status", String::new(), multi(&[
                            prop("/cal/me/", "<D:resourcetype><D:collection/></D:resourcetype>"),
                            prop("/cal/me/family/", r#"<D:resourcetype><D:collection/><C:calendar/></D:resourcetype><D:displayname>Familie</D:displayname><C:supported-calendar-component-set><C:comp name="VEVENT"/></C:supported-calendar-component-set>"#),
                            prop("/cal/me/tasks/", r#"<D:resourcetype><D:collection/><C:calendar/></D:resourcetype><D:displayname>Tasks</D:displayname><C:supported-calendar-component-set><C:comp name="VTODO"/></C:supported-calendar-component-set>"#),
                            prop("https://evil.example/steal/", "<D:resourcetype><C:calendar/></D:resourcetype>"),
                        ].concat())),
                        ("REPORT", "/cal/me/family/") => ("207 Multi-Status", String::new(), multi(&prop("/cal/me/family/1.ics", &format!("<C:calendar-data><![CDATA[{}]]></C:calendar-data>", ics.replace("&amp;", "&"))))),
                        _ => ("404 Not Found", String::new(), String::new()),
                    }
                };
                let _ = write!(s, "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        origin
    }

    fn dav(origin: &str, password: &str) -> Dav {
        Dav { origin: origin.into(), auth: format!("Basic {}", b64_encode(format!("me:{password}").as_bytes())), user: "me".into() }
    }

    #[test]
    fn finds_calendars_and_reads_events() {
        let good = dav("", "secret").auth;
        let origin = server(good, None);
        let list = dav(&origin, "secret").load("/", 14).unwrap();
        assert_eq!(list.len(), 1, "task lists and other origins are left out");
        assert_eq!(list[0].name, "Familie");
        let events = list[0].capabilities["events"].value.as_array().unwrap();
        assert_eq!((events.len(), events[0]["t"].as_str()), (1, Some("Tannlege & kontroll")));
    }

    #[test]
    fn wrong_password_is_reported_plainly() {
        let good = dav("", "secret").auth;
        let origin = server(good, None);
        assert_eq!(dav(&origin, "wrong").load("/", 14).err().as_deref(), Some(BAD_LOGIN));
    }

    #[test]
    fn password_never_follows_a_redirect_elsewhere() {
        let good = dav("", "secret").auth;
        let origin = server(good, Some("https://evil.example/".into()));
        assert!(matches!(dav(&origin, "secret").call("PROPFIND", "/", "0", PRINCIPAL), Err(Failure::Other(e)) if e.contains("another address")));
    }

    #[test]
    fn only_same_origin_paths_are_accepted() {
        let o = "https://cal.example";
        assert_eq!(same_origin(o, "/a/b/").as_deref(), Some("/a/b/"));
        assert_eq!(same_origin(o, "https://cal.example/a/").as_deref(), Some("/a/"));
        for bad in ["https://cal.example.evil/a", "https://evil/a", "//evil/a", "a/b", "/a b", "/a\r\nX: y"] {
            assert_eq!(same_origin(o, bad), None, "{bad}");
        }
    }

    #[test]
    fn config_needs_https_and_a_login() {
        let c = |a: &str, u: &str, p: &str| json!({ "address": a, "username": u, "password": p }).as_object().unwrap().clone();
        assert!(config(&c("https://cal.example", "me", "pw")).is_ok());
        assert!(config(&c("http://cal.example", "me", "pw")).is_err());
        assert!(config(&c("https://cal.example", "a:b", "pw")).is_err());
        assert!(config(&c("https://cal.example", "me", "")).is_err());
    }
}
