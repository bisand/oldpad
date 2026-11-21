//! Calendars from iCal (.ics) addresses: Google, iCloud, Outlook and most others,
//! including many municipal waste collection calendars. One read-only entity per
//! calendar: the coming events with repeats expanded, for the tablet's agenda.

use super::met::days_from_civil;
use super::spot::civil_from_days;
use super::{get_str, Capability, Entity, Event, Field, FieldKind, Instance, Kind, Sink};
use crate::client::request;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DAY: i64 = 86_400;
const MAX_EVENTS: usize = 200;
const MAX_PERIODS: i64 = 40_000; // over a century of a daily repeat
const REFRESH: Duration = Duration::from_secs(15 * 60);

// shortcut: floating times and TZIDs the file does not define are read as Central European time; add a time zone field when someone elsewhere needs it.
const DEFAULT_ZONE: &[u8] = b"BEGIN:VTIMEZONE\nTZID:default\nBEGIN:STANDARD\nDTSTART:19701025T030000\nTZOFFSETFROM:+0200\nTZOFFSETTO:+0100\nRRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\nEND:STANDARD\nBEGIN:DAYLIGHT\nDTSTART:19700329T020000\nTZOFFSETFROM:+0100\nTZOFFSETTO:+0200\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\nEND:DAYLIGHT\nEND:VTIMEZONE\n";

pub fn kind() -> Kind {
    Kind {
        id: "calendar",
        name: "Calendar (iCal)",
        description: "Coming events from Google, iCloud, Outlook or any other calendar with an iCal address.",
        fields: vec![
            Field {
                key: "urls",
                label: "Calendar addresses",
                kind: FieldKind::Secret,
                required: true,
                placeholder: "https://calendar.google.com/calendar/ical/…/basic.ics",
                help: "The calendar's secret or public iCal address (https:// or webcal://). Separate several with commas. Anyone with a secret address can read that calendar, so treat it like a password.",
            },
            Field { key: "days", label: "Days ahead", kind: FieldKind::Number, required: false, placeholder: "14", help: "" },
        ],
        test,
        start,
    }
}

// ------------------------------------------------------------------ parsing

struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, key: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// Splits on `sep`, leaving separators inside double quotes alone.
fn split_unquoted(s: &str, sep: char) -> Vec<&str> {
    let (mut out, mut from, mut quoted) = (Vec::new(), 0, false);
    for (i, c) in s.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if c == sep && !quoted {
            out.push(&s[from..i]);
            from = i + 1;
        }
    }
    out.push(&s[from..]);
    out
}

fn parse_prop(line: &str) -> Option<Prop> {
    let (mut quoted, mut colon) = (false, None);
    for (i, c) in line.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if c == ':' && !quoted {
            colon = Some(i);
            break;
        }
    }
    let colon = colon?;
    let head = split_unquoted(&line[..colon], ';');
    let params = head[1..].iter().filter_map(|p| p.split_once('=')).map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string())).collect();
    Some(Prop { name: head[0].to_ascii_uppercase(), params, value: line[colon + 1..].to_string() })
}

/// Unfolds on bytes first: a fold may fall in the middle of a UTF-8 character.
fn props(bytes: &[u8]) -> Vec<Prop> {
    let mut lines: Vec<Vec<u8>> = Vec::new();
    for raw in bytes.split(|&b| b == b'\n') {
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        if matches!(raw.first(), Some(b' ' | b'\t')) {
            if let Some(last) = lines.last_mut() {
                last.extend(&raw[1..]);
            }
        } else if !raw.is_empty() {
            lines.push(raw.to_vec());
        }
    }
    lines.iter().filter_map(|l| parse_prop(String::from_utf8_lossy(l).trim_start_matches('\u{feff}'))).collect()
}

fn text(s: &str) -> String {
    let (mut out, mut chars) = (String::new(), s.chars());
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push(' '),
            Some(x) => out.push(x),
            None => {}
        }
    }
    out.trim().to_string()
}

#[derive(Clone, PartialEq, Debug)]
enum Zone {
    Utc,
    Named(String),
    Floating,
}

/// A wall-clock time, as seconds since 1970 read as if it were UTC, and what zone it is in.
#[derive(Clone, Debug)]
struct Stamp {
    local: i64,
    zone: Zone,
    date: bool,
}

fn stamp(v: &str, tz: Option<&str>) -> Option<Stamp> {
    let n = |r: std::ops::Range<usize>| v.get(r)?.parse::<u32>().ok().map(i64::from);
    let (month, day) = (n(4..6)?, n(6..8)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let days = days_from_civil(n(0..4)?, month, day);
    if v.len() == 8 {
        return Some(Stamp { local: days * DAY, zone: Zone::Floating, date: true });
    }
    if v.as_bytes()[8] != b'T' {
        return None;
    }
    let local = days * DAY + n(9..11)? * 3600 + n(11..13)? * 60 + n(13..15)?;
    let zone = match v.get(15..)? {
        "Z" => Zone::Utc,
        "" => tz.map_or(Zone::Floating, |t| Zone::Named(t.to_string())),
        _ => return None,
    };
    Some(Stamp { local, zone, date: false })
}

fn prop_stamp(p: &Prop) -> Option<Stamp> {
    stamp(p.value.trim(), p.param("TZID"))
}

/// "PT1H30M", "P1D", "P2W" → seconds.
fn duration(v: &str) -> Option<i64> {
    let (sign, rest) = match v.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, v.strip_prefix('+').unwrap_or(v)),
    };
    let (mut total, mut n) = (0i64, 0i64);
    for c in rest.strip_prefix('P')?.chars() {
        match c {
            'T' => {}
            '0'..='9' => n = n.checked_mul(10)?.checked_add(c as i64 - '0' as i64)?,
            'W' | 'D' | 'H' | 'M' | 'S' => {
                let unit = match c { 'W' => 7 * DAY, 'D' => DAY, 'H' => 3600, 'M' => 60, _ => 1 };
                total = total.checked_add(n.checked_mul(unit)?)?;
                n = 0;
            }
            _ => return None,
        }
    }
    Some(sign * total)
}

// ------------------------------------------------------------------ repeats

#[derive(Clone, Copy, PartialEq, Debug)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug)]
struct Rule {
    freq: Freq,
    interval: i64,
    count: Option<i64>,
    until: Option<Stamp>,
    by_day: Vec<(i64, i64)>, // (nth of the month, 0 = every; weekday, 0 = Monday)
    by_month: Vec<i64>,
    by_month_day: Vec<i64>,
    set_pos: Vec<i64>,
    week_start: i64,
}

fn weekday_id(s: &str) -> Option<i64> {
    ["MO", "TU", "WE", "TH", "FR", "SA", "SU"].iter().position(|d| *d == s).map(|i| i as i64)
}

fn weekday(days: i64) -> i64 {
    (days + 3).rem_euclid(7) // 1970-01-01 was a Thursday
}

/// None for what is not handled (BYYEARDAY, BYWEEKNO, BYHOUR, …): the event then shows once.
fn parse_rule(v: &str) -> Option<Rule> {
    let mut r = Rule { freq: Freq::Daily, interval: 1, count: None, until: None, by_day: vec![], by_month: vec![], by_month_day: vec![], set_pos: vec![], week_start: 0 };
    let mut freq = None;
    let numbers = |s: &str| s.split(',').map(|x| x.parse::<i64>().ok()).collect::<Option<Vec<_>>>();
    for part in v.trim().split(';').filter(|p| !p.is_empty()) {
        let (k, val) = part.split_once('=')?;
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => freq = Some(match val { "DAILY" => Freq::Daily, "WEEKLY" => Freq::Weekly, "MONTHLY" => Freq::Monthly, "YEARLY" => Freq::Yearly, _ => return None }),
            "INTERVAL" => r.interval = val.parse().ok().filter(|n| (1..=10_000).contains(n))?,
            "COUNT" => r.count = Some(val.parse().ok()?),
            "UNTIL" => r.until = Some(stamp(val, None)?),
            "BYDAY" => {
                for d in val.split(',') {
                    let at = d.len().checked_sub(2)?;
                    let nth = if at == 0 { 0 } else { d.get(..at)?.parse().ok()? };
                    r.by_day.push((nth, weekday_id(d.get(at..)?)?));
                }
            }
            "BYMONTH" => r.by_month = numbers(val)?,
            "BYMONTHDAY" => r.by_month_day = numbers(val)?,
            "BYSETPOS" => r.set_pos = numbers(val)?,
            "WKST" => r.week_start = weekday_id(val)?,
            _ => return None,
        }
    }
    r.freq = freq?;
    if r.freq == Freq::Yearly && r.by_month.is_empty() && !(r.by_day.is_empty() && r.by_month_day.is_empty()) {
        return None;
    }
    Some(r)
}

fn month_days(r: &Rule, y: i64, m: i64, start_day: i64) -> Vec<i64> {
    let first = days_from_civil(y, m, 1);
    let next = if m == 12 { days_from_civil(y + 1, 1, 1) } else { days_from_civil(y, m + 1, 1) };
    let n = next - first;
    if r.by_day.is_empty() && r.by_month_day.is_empty() {
        return if start_day <= n { vec![first + start_day - 1] } else { vec![] }; // no 31st: skipped, as RFC 5545 says
    }
    (first..next)
        .filter(|&d| {
            let dom = d - first + 1;
            (r.by_month_day.is_empty() || r.by_month_day.iter().any(|&x| x == dom || (x < 0 && n + x + 1 == dom)))
                && (r.by_day.is_empty()
                    || r.by_day.iter().any(|&(nth, wd)| weekday(d) == wd && (nth == 0 || (nth > 0 && (dom - 1) / 7 + 1 == nth) || (nth < 0 && (n - dom) / 7 + 1 == -nth))))
        })
        .collect()
}

/// The wall-clock times of a repeat, up to `limit`. Wall clock so 09:00 stays 09:00 across daylight saving.
fn expand(r: &Rule, start: i64, limit: i64, to_epoch: &dyn Fn(i64) -> i64) -> Vec<i64> {
    let (day0, time) = (start.div_euclid(DAY), start.rem_euclid(DAY));
    let (y0, m0, d0) = civil_from_days(day0);
    let until = r.until.as_ref().map(|u| match (&u.zone, u.date) {
        (Zone::Utc, _) => u.local,
        (_, true) => to_epoch(u.local + DAY - 1),
        _ => to_epoch(u.local),
    });
    let month_ok = |d: i64| r.by_month.is_empty() || r.by_month.contains(&civil_from_days(d).1);
    let (mut out, mut seen) = (Vec::new(), 0);
    for k in 0..MAX_PERIODS {
        let step = k * r.interval;
        let (period, mut days): (i64, Vec<i64>) = match r.freq {
            Freq::Daily => {
                let d = day0 + step;
                let dom = civil_from_days(d).2;
                let keep = month_ok(d) && (r.by_day.is_empty() || r.by_day.iter().any(|x| x.1 == weekday(d))) && (r.by_month_day.is_empty() || r.by_month_day.contains(&dom));
                (d, if keep { vec![d] } else { vec![] })
            }
            Freq::Weekly => {
                let w = day0 - (weekday(day0) - r.week_start).rem_euclid(7) + step * 7;
                let wanted = |d: i64| if r.by_day.is_empty() { weekday(d) == weekday(day0) } else { r.by_day.iter().any(|x| x.1 == weekday(d)) };
                (w, (w..w + 7).filter(|&d| wanted(d) && month_ok(d)).collect())
            }
            Freq::Monthly => {
                let t = y0 * 12 + m0 - 1 + step;
                let (y, m) = (t.div_euclid(12), t.rem_euclid(12) + 1);
                (days_from_civil(y, m, 1), if month_ok(days_from_civil(y, m, 1)) { month_days(r, y, m, d0) } else { vec![] })
            }
            Freq::Yearly => {
                let y = y0 + step;
                let months = if r.by_month.is_empty() { vec![m0] } else { r.by_month.clone() };
                (days_from_civil(y, 1, 1), months.iter().filter(|m| (1..=12).contains(*m)).flat_map(|&m| month_days(r, y, m, d0)).collect())
            }
        };
        if period * DAY > limit {
            break;
        }
        days.sort_unstable();
        if !r.set_pos.is_empty() {
            let n = days.len() as i64;
            days = r.set_pos.iter().filter_map(|&p| days.get((if p > 0 { p - 1 } else { n + p }) as usize).copied()).collect();
            days.sort_unstable();
        }
        for d in days {
            let t = d * DAY + time;
            if t < start {
                continue;
            }
            if t > limit || until.is_some_and(|u| to_epoch(t) > u) || r.count.is_some_and(|c| seen >= c) {
                return out;
            }
            seen += 1;
            out.push(t);
        }
    }
    out
}

// ------------------------------------------------------------------ time zones

/// (wall clock of the change, offset from then on), sorted.
type Table = Vec<(i64, i64)>;

fn offset(v: &str) -> Option<i64> {
    let sign = if v.starts_with('-') { -1 } else { 1 };
    let d = v.get(1..)?;
    let n = |r: std::ops::Range<usize>| d.get(r)?.parse::<i64>().ok();
    Some(sign * (n(0..2)? * 3600 + n(2..4)? * 60 + n(4..6).unwrap_or(0)))
}

fn zone_table(observances: &[Vec<Prop>], horizon: i64) -> Table {
    let mut table = Vec::new();
    let mut earliest: Option<(i64, i64)> = None;
    for o in observances {
        let get = |name: &str| o.iter().find(|p| p.name == name);
        let (Some(from), Some(to), Some(start)) = (get("TZOFFSETFROM").and_then(|p| offset(p.value.trim())), get("TZOFFSETTO").and_then(|p| offset(p.value.trim())), get("DTSTART").and_then(prop_stamp)) else { continue };
        if earliest.is_none_or(|e| start.local < e.0) {
            earliest = Some((start.local, from));
        }
        match get("RRULE").and_then(|p| parse_rule(&p.value)) {
            Some(rule) => table.extend(expand(&rule, start.local, horizon, &|t| t - from).into_iter().map(|t| (t, to))),
            None => table.push((start.local, to)),
        }
    }
    table.sort_unstable();
    if let Some((_, from)) = earliest {
        table.insert(0, (i64::MIN, from));
    }
    table
}

struct Zones {
    tables: HashMap<String, Table>,
    fallback: Table,
}

impl Zones {
    fn epoch(&self, local: i64, zone: &Zone, date: bool) -> i64 {
        if date || *zone == Zone::Utc {
            return local;
        }
        let table = match zone {
            Zone::Named(n) => self.tables.get(n).filter(|t| !t.is_empty()).unwrap_or(&self.fallback),
            _ => &self.fallback,
        };
        let i = table.partition_point(|x| x.0 <= local);
        local - if i == 0 { 0 } else { table[i - 1].1 }
    }
}

// ------------------------------------------------------------------ events

struct Sections {
    name: Option<String>,
    zones: Vec<(String, Vec<Vec<Prop>>)>,
    events: Vec<Vec<Prop>>,
}

fn sections(bytes: &[u8]) -> Sections {
    let mut out = Sections { name: None, zones: vec![], events: vec![] };
    let (mut stack, mut cur): (Vec<String>, Vec<Prop>) = (vec![], vec![]);
    for p in props(bytes) {
        if p.name == "BEGIN" {
            let section = p.value.trim().to_ascii_uppercase();
            if section == "VTIMEZONE" {
                out.zones.push((String::new(), vec![]));
            } else if matches!(section.as_str(), "VEVENT" | "STANDARD" | "DAYLIGHT") {
                cur.clear();
            }
            stack.push(section);
        } else if p.name == "END" {
            match stack.pop().as_deref() {
                Some("VEVENT") => out.events.push(std::mem::take(&mut cur)),
                Some("STANDARD" | "DAYLIGHT") => {
                    if let Some(z) = out.zones.last_mut() {
                        z.1.push(std::mem::take(&mut cur));
                    }
                }
                _ => {}
            }
        } else {
            match stack.last().map(String::as_str) {
                Some("VEVENT" | "STANDARD" | "DAYLIGHT") => cur.push(p),
                Some("VTIMEZONE") if p.name == "TZID" => {
                    if let Some(z) = out.zones.last_mut() {
                        z.0 = p.value.trim().to_string();
                    }
                }
                Some("VCALENDAR") if p.name == "X-WR-CALNAME" => out.name = Some(text(&p.value)),
                _ => {}
            }
        }
    }
    out
}

/// The calendar's name and its events from `now` − 2 days to `now` + `days`, sorted:
/// `{t, at, end, all, place}`, times in epoch seconds. All-day events are UTC midnights,
/// so the tablet reads their date the same in any time zone.
fn agenda(bytes: &[u8], now: i64, days: i64) -> (Option<String>, Vec<Value>) {
    let parsed = sections(bytes);
    let (from, to) = (now - 2 * DAY, now + days * DAY);
    let horizon = to + 2 * DAY;
    let tables = parsed.zones.iter().map(|(id, o)| (id.clone(), zone_table(o, horizon))).collect();
    let default = sections(DEFAULT_ZONE);
    let zones = Zones { tables, fallback: zone_table(&default.zones[0].1, horizon) };

    let get = |e: &'_ Vec<Prop>, name: &str| e.iter().position(|p| p.name == name);
    let moved: HashSet<(String, i64)> = parsed
        .events
        .iter()
        .filter_map(|e| {
            let id = get(e, "RECURRENCE-ID").and_then(|i| prop_stamp(&e[i]))?;
            let uid = e[get(e, "UID")?].value.trim().to_string();
            Some((uid, zones.epoch(id.local, &id.zone, id.date)))
        })
        .collect();

    let mut out: Vec<(i64, Value)> = Vec::new();
    for e in &parsed.events {
        let value = |name: &str| get(e, name).map(|i| e[i].value.trim());
        if value("STATUS").is_some_and(|s| s.eq_ignore_ascii_case("CANCELLED")) {
            continue;
        }
        let Some(start) = get(e, "DTSTART").and_then(|i| prop_stamp(&e[i])) else { continue };
        let at = |local: i64| zones.epoch(local, &start.zone, start.date);
        let length = match (get(e, "DTEND").and_then(|i| prop_stamp(&e[i])), value("DURATION").and_then(duration)) {
            (Some(end), _) => (zones.epoch(end.local, &end.zone, end.date) - at(start.local)).max(0),
            (None, Some(d)) => d.max(0),
            _ if start.date => DAY,
            _ => 0,
        };
        let is_override = get(e, "RECURRENCE-ID").is_some();
        let uid = value("UID").unwrap_or("").to_string();
        let exceptions: HashSet<i64> = e
            .iter()
            .filter(|p| p.name == "EXDATE")
            .flat_map(|p| p.value.split(',').filter_map(|v| stamp(v.trim(), p.param("TZID"))))
            .map(|s| zones.epoch(s.local, &s.zone, s.date))
            .collect();
        let rule = if is_override { None } else { value("RRULE").and_then(parse_rule) };
        let times = match &rule {
            Some(r) => expand(r, start.local, horizon, &at),
            None => vec![start.local],
        };
        // A private event still blocks time on the wall, without saying what it is.
        let private = value("CLASS").is_some_and(|c| c.eq_ignore_ascii_case("PRIVATE") || c.eq_ignore_ascii_case("CONFIDENTIAL"));
        let (title, place) = if private { (None, None) } else { (value("SUMMARY").map(text), value("LOCATION").map(text).filter(|s| !s.is_empty())) };
        for local in times {
            let begin = at(local);
            let end = begin + length;
            if end < from || begin >= to || exceptions.contains(&begin) || (!is_override && moved.contains(&(uid.clone(), begin))) {
                continue;
            }
            out.push((begin, json!({ "t": title, "at": begin, "end": end, "all": start.date, "place": place })));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1["t"].as_str().cmp(&b.1["t"].as_str())));
    out.truncate(MAX_EVENTS);
    (parsed.name.filter(|n| !n.is_empty()), out.into_iter().map(|x| x.1).collect())
}

// ------------------------------------------------------------------ fetching

/// Splits an address into what `request` wants. webcal:// is https:// by convention.
fn split_url(url: &str) -> Result<(String, String), String> {
    let url = match url.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_string(),
    };
    let scheme_end = url.find("://").ok_or("a calendar address must start with https:// or webcal://")?;
    if !matches!(&url[..scheme_end], "http" | "https") || url.chars().any(|c| c.is_control() || c == ' ') {
        return Err("a calendar address must start with https:// or webcal://".into());
    }
    Ok(match url[scheme_end + 3..].find('/') {
        Some(i) => (url[..scheme_end + 3 + i].to_string(), url[scheme_end + 3 + i..].to_string()),
        None => (url, "/".to_string()),
    })
}

/// Errors never include the address: it may be a secret.
fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let (mut base, mut path) = split_url(url)?;
    for _ in 0..4 {
        let res = request(&base, "GET", &path, &[("Accept", "text/calendar, */*")], None)?;
        match res.status {
            200 if res.body.windows(15).any(|w| w.eq_ignore_ascii_case(b"BEGIN:VCALENDAR")) => return Ok(res.body),
            200 => return Err("that address did not return an iCal calendar".into()),
            301 | 302 | 303 | 307 | 308 => {
                let to = res.headers.get("location").ok_or("calendar redirect without a destination")?;
                let (b, p) = if to.starts_with('/') { split_url(&format!("{base}{to}"))? } else { split_url(to)? };
                if base.starts_with("https://") && !b.starts_with("https://") {
                    return Err("the calendar redirected to an unencrypted address".into());
                }
                (base, path) = (b, p);
            }
            401 | 403 | 404 => return Err(format!("the calendar address was refused (HTTP {}). It may be private or out of date.", res.status)),
            s => return Err(format!("the calendar server answered HTTP {s}")),
        }
    }
    Err("too many calendar redirects".into())
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// A stable id that does not reveal the address.
fn calendar_id(url: &str) -> String {
    let hash = Sha256::digest(url.as_bytes());
    format!("cal-{}", hash[..8].iter().map(|b| format!("{b:02x}")).collect::<String>())
}

fn load(url: &str, days: i64) -> Result<Entity, String> {
    let (name, events) = agenda(&fetch(url)?, now(), days);
    Ok(Entity {
        id: calendar_id(url),
        name: name.unwrap_or_else(|| "Calendar".into()),
        zone: "Calendar".into(),
        class: "calendar".into(),
        capabilities: BTreeMap::from([("events".to_string(), Capability::new(json!(events), false))]),
    })
}

fn config(c: &Map<String, Value>) -> Result<(Vec<String>, i64), String> {
    let urls: Vec<String> = get_str(c, "urls").split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if urls.is_empty() || urls.len() > 8 {
        return Err("give one to eight calendar addresses".into());
    }
    for u in &urls {
        split_url(u)?;
    }
    Ok((urls, c.get("days").and_then(Value::as_f64).map_or(14, |d| d.clamp(1.0, 60.0) as i64)))
}

fn test(c: &Map<String, Value>) -> Result<String, String> {
    let (urls, days) = config(c)?;
    let mut out = Vec::new();
    for url in &urls {
        let e = load(url, days)?;
        out.push(format!("{}: {} events in the next {days} days", e.name, e.capabilities["events"].value.as_array().map_or(0, Vec::len)));
    }
    Ok(out.join(". "))
}

struct Calendar {
    entities: Arc<Mutex<Vec<Entity>>>,
    stopped: Arc<AtomicBool>,
}

fn start(c: &Map<String, Value>, sink: Sink) -> Result<Box<dyn Instance>, String> {
    let (urls, days) = config(c)?;
    let first = urls.iter().map(|u| load(u, days)).collect::<Result<Vec<_>, _>>()?;
    let entities = Arc::new(Mutex::new(first));
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
            for url in &urls {
                // A failed refresh keeps showing what we have.
                let fresh = match load(url, days) {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("calendar: {e}");
                        continue;
                    }
                };
                let mut all = ents.lock().unwrap();
                let Some(e) = all.iter_mut().find(|e| e.id == fresh.id) else { continue };
                let renamed = e.name != fresh.name;
                let changed = e.capabilities["events"].value != fresh.capabilities["events"].value;
                let (id, value) = (fresh.id.clone(), fresh.capabilities["events"].value.clone());
                *e = fresh;
                drop(all);
                if renamed {
                    sink(Event::Reloaded);
                } else if changed {
                    sink(Event::Changed { entity: id, capability: "events".into(), value });
                }
            }
        }
    });
    Ok(Box::new(Calendar { entities, stopped }))
}

impl Instance for Calendar {
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

    const OSLO: &str = "BEGIN:VTIMEZONE\r\nTZID:Europe/Oslo\r\nBEGIN:DAYLIGHT\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\nDTSTART:19700329T020000\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r\nEND:DAYLIGHT\r\nBEGIN:STANDARD\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nDTSTART:19701025T030000\r\nRRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n";

    fn utc(y: i64, m: i64, d: i64, h: i64, min: i64) -> i64 {
        days_from_civil(y, m, d) * DAY + h * 3600 + min * 60
    }

    fn cal(body: &str) -> Vec<u8> {
        format!("BEGIN:VCALENDAR\r\nX-WR-CALNAME:Familie\r\n{OSLO}{body}END:VCALENDAR\r\n").into_bytes()
    }

    fn event(lines: &str) -> String {
        format!("BEGIN:VEVENT\r\nUID:u1\r\n{lines}\r\nEND:VEVENT\r\n")
    }

    fn times(v: &[Value]) -> Vec<i64> {
        v.iter().map(|e| e["at"].as_i64().unwrap()).collect()
    }

    #[test]
    fn reads_folds_escapes_and_quoted_params() {
        let body = "BEGIN:VEVENT\r\nUID:u1\r\nSUMMARY:Fotball\\, trening p\u{e5} \r\n banen\r\nLOCATION;ALTREP=\"http://a:b/\":Hallen\\nOslo\r\nDTSTART:20261122T120000Z\r\nDTEND:20261122T133000Z\r\nBEGIN:VALARM\r\nSUMMARY:not the title\r\nEND:VALARM\r\nEND:VEVENT\r\n";
        let (name, list) = agenda(&cal(body), utc(2026, 11, 21, 12, 0), 14);
        assert_eq!(name.as_deref(), Some("Familie"));
        assert_eq!(list[0]["t"], "Fotball, trening p\u{e5} banen");
        assert_eq!(list[0]["place"], "Hallen Oslo");
        assert_eq!(list[0]["end"].as_i64().unwrap() - list[0]["at"].as_i64().unwrap(), 5400);
        // A fold may split a UTF-8 character.
        let raw = b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:\xC3\r\n \xA5\r\nDTSTART:20261122T120000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        assert_eq!(agenda(raw, utc(2026, 11, 21, 12, 0), 14).1[0]["t"], "\u{e5}");
    }

    #[test]
    fn time_zones_follow_daylight_saving() {
        let now = utc(2026, 3, 1, 0, 0);
        let (_, winter) = agenda(&cal(&event("DTSTART;TZID=Europe/Oslo:20260310T090000")), now, 60);
        assert_eq!(times(&winter), vec![utc(2026, 3, 10, 8, 0)]);
        let (_, summer) = agenda(&cal(&event("DTSTART;TZID=Europe/Oslo:20260401T090000")), now, 60);
        assert_eq!(times(&summer), vec![utc(2026, 4, 1, 7, 0)]);
        // A weekly 09:00 stays 09:00 when the clocks change on 29 March.
        let (_, weekly) = agenda(&cal(&event("DTSTART;TZID=Europe/Oslo:20260325T090000\r\nRRULE:FREQ=WEEKLY;COUNT=2")), now, 60);
        assert_eq!(times(&weekly), vec![utc(2026, 3, 25, 8, 0), utc(2026, 4, 1, 7, 0)]);
        // No VTIMEZONE for this id, and a floating time: Central European by default.
        let (_, unknown) = agenda(&cal(&event("DTSTART;TZID=W. Europe Standard Time:20260401T090000")), now, 60);
        assert_eq!(times(&unknown), vec![utc(2026, 4, 1, 7, 0)]);
        let (_, floating) = agenda(&cal(&event("DTSTART:20260310T090000")), now, 60);
        assert_eq!(times(&floating), vec![utc(2026, 3, 10, 8, 0)]);
    }

    #[test]
    fn all_day_events_keep_their_date() {
        let (_, list) = agenda(&cal(&event("DTSTART;VALUE=DATE:20261123\r\nDTEND;VALUE=DATE:20261125\r\nSUMMARY:Ferie")), utc(2026, 11, 21, 12, 0), 14);
        assert_eq!((list[0]["at"].as_i64(), list[0]["end"].as_i64(), list[0]["all"].as_bool()), (Some(utc(2026, 11, 23, 0, 0)), Some(utc(2026, 11, 25, 0, 0)), Some(true)));
        let (_, one) = agenda(&cal(&event("DTSTART;VALUE=DATE:20261123")), utc(2026, 11, 21, 12, 0), 14);
        assert_eq!(one[0]["end"].as_i64().unwrap() - one[0]["at"].as_i64().unwrap(), DAY);
    }

    #[test]
    fn repeats() {
        let days = |rule: &str, start: i64, limit: i64| -> Vec<(i64, i64, i64)> {
            expand(&parse_rule(rule).unwrap(), start, limit, &|t| t).into_iter().map(|t| civil_from_days(t.div_euclid(DAY))).collect()
        };
        let (jan, end) = (utc(2026, 1, 1, 10, 0), utc(2026, 12, 31, 0, 0));
        assert_eq!(days("FREQ=DAILY;INTERVAL=2;COUNT=3", jan, end), vec![(2026, 1, 1), (2026, 1, 3), (2026, 1, 5)]);
        assert_eq!(days("FREQ=WEEKLY;BYDAY=MO,TH;COUNT=3", jan, end), vec![(2026, 1, 1), (2026, 1, 5), (2026, 1, 8)]);
        assert_eq!(days("FREQ=MONTHLY;BYDAY=2TU;COUNT=2", utc(2026, 1, 13, 10, 0), end), vec![(2026, 1, 13), (2026, 2, 10)]);
        assert_eq!(days("FREQ=MONTHLY;BYDAY=-1FR;COUNT=2", utc(2026, 1, 30, 10, 0), end), vec![(2026, 1, 30), (2026, 2, 27)]);
        assert_eq!(days("FREQ=MONTHLY;BYMONTHDAY=-1;COUNT=2", utc(2026, 1, 31, 10, 0), end), vec![(2026, 1, 31), (2026, 2, 28)]);
        assert_eq!(days("FREQ=MONTHLY;COUNT=3", utc(2026, 1, 31, 10, 0), end), vec![(2026, 1, 31), (2026, 3, 31), (2026, 5, 31)], "no 31 February");
        assert_eq!(days("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=2", utc(2026, 1, 30, 10, 0), end), vec![(2026, 1, 30), (2026, 2, 27)]);
        assert_eq!(days("FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU;COUNT=2", utc(2026, 3, 29, 2, 0), utc(2028, 1, 1, 0, 0)), vec![(2026, 3, 29), (2027, 3, 28)]);
        assert_eq!(days("FREQ=WEEKLY;UNTIL=20260108T100000Z", jan, end).len(), 2);
        assert_eq!(days("FREQ=WEEKLY;UNTIL=20260108", jan, end).len(), 2, "a date includes that whole day");
        assert_eq!(days("FREQ=YEARLY;COUNT=2", utc(2024, 2, 29, 10, 0), utc(2029, 1, 1, 0, 0)), vec![(2024, 2, 29), (2028, 2, 29)]);
        assert!(parse_rule("FREQ=MONTHLY;BYWEEKNO=3").is_none());
        assert!(parse_rule("FREQ=HOURLY").is_none());
        assert!(parse_rule("FREQ=DAILY;INTERVAL=0").is_none());
    }

    #[test]
    fn exceptions_moves_and_cancellations() {
        let now = utc(2026, 11, 21, 12, 0);
        let base = "DTSTART;TZID=Europe/Oslo:20261123T090000\r\nRRULE:FREQ=DAILY;COUNT=4\r\nEXDATE;TZID=Europe/Oslo:20261124T090000\r\nSUMMARY:Daglig";
        let moved = "BEGIN:VEVENT\r\nUID:u1\r\nRECURRENCE-ID;TZID=Europe/Oslo:20261125T090000\r\nDTSTART;TZID=Europe/Oslo:20261125T150000\r\nSUMMARY:Flyttet\r\nEND:VEVENT\r\n";
        let gone = "BEGIN:VEVENT\r\nUID:u1\r\nRECURRENCE-ID:20261126T080000Z\r\nDTSTART;TZID=Europe/Oslo:20261126T090000\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\n";
        let (_, list) = agenda(&cal(&format!("{}{moved}{gone}", event(base))), now, 14);
        assert_eq!(times(&list), vec![utc(2026, 11, 23, 8, 0), utc(2026, 11, 25, 14, 0)]);
        assert_eq!(list[1]["t"], "Flyttet");
        assert!(agenda(&cal(&event("DTSTART:20261123T090000Z\r\nSTATUS:CANCELLED")), now, 14).1.is_empty());
    }

    #[test]
    fn private_events_show_no_details_and_the_window_holds() {
        let now = utc(2026, 11, 21, 12, 0);
        let (_, list) = agenda(&cal(&event("DTSTART:20261123T090000Z\r\nSUMMARY:Legetime\r\nLOCATION:Sykehuset\r\nCLASS:PRIVATE")), now, 14);
        assert!(list[0]["t"].is_null() && list[0]["place"].is_null());
        assert!(agenda(&cal(&event("DTSTART:20270101T090000Z")), now, 14).1.is_empty(), "too far ahead");
        assert!(agenda(&cal(&event("DTSTART:20200101T090000Z")), now, 14).1.is_empty(), "long gone");
        assert_eq!(agenda(&cal(&event("DTSTART:20000103T090000Z\r\nRRULE:FREQ=WEEKLY")), now, 14).1.len(), 2, "old repeats still run");
    }

    #[test]
    fn addresses() {
        assert_eq!(split_url("webcal://p1.icloud.com/published/2/abc?x=1").unwrap(), ("https://p1.icloud.com".to_string(), "/published/2/abc?x=1".to_string()));
        assert_eq!(split_url("https://a.example").unwrap().1, "/");
        assert!(split_url("ftp://a.example/x").is_err());
        assert!(split_url("https://a.example/x\r\nHost: evil").is_err());
        assert!(!calendar_id("https://a.example/secret").contains("secret"));
        assert_eq!(duration("PT1H30M"), Some(5400));
        assert_eq!(duration("P1W"), Some(7 * DAY));
        assert_eq!(duration("x"), None);
    }

    #[test]
    fn serves_and_follows_a_redirect() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/old.ics", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for reply in ["HTTP/1.1 302 Found\r\nLocation: /new.ics\r\nContent-Length: 0\r\n\r\n", "HTTP/1.1 200 OK\r\nContent-Length: 15\r\n\r\nBEGIN:VCALENDAR"] {
                let (mut s, _) = listener.accept().unwrap();
                let mut b = [0u8; 1024];
                let _ = s.read(&mut b).unwrap();
                s.write_all(reply.as_bytes()).unwrap();
            }
        });
        assert_eq!(fetch(&url).unwrap(), b"BEGIN:VCALENDAR");
    }
}
