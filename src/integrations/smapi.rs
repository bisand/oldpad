//! Sonos music services (SMAPI): what the Sonos app uses to search Spotify.
//! Connecting is Sonos's "AppLink": the admin opens a link and signs in, and
//! the service hands this household a token. Only Spotify is wired up so far;
//! other services speak the same protocol but name their items differently.

use crate::client::request;
use crate::util::escape_xml;
use serde::{Deserialize, Serialize};

pub const SPOTIFY_URL: &str = "https://spotify-v5.ws.sonos.com/smapi";

/// A connected service account, kept in the integration's stored config.
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Account {
    pub token: String,
    pub key: String,
    /// The speaker serial and household the link was made for; the token is bound to them.
    pub device: String,
    pub household: String,
}

#[derive(Debug, PartialEq)]
pub struct Item {
    pub id: String, // e.g. spotify:track:2WfaOiMkCvy7F5fcp2zZ8L
    pub title: String,
    pub sub: String,
    pub kind: &'static str, // track, album or playlist
    pub art: String,        // cover URL, or empty
}

fn call(url: &str, device: &str, household: &str, login: Option<&Account>, action: &str, body: &str) -> Result<String, String> {
    let login = login.map_or(String::new(), |a| {
        format!("<loginToken><token>{}</token><key>{}</key><householdId>{}</householdId></loginToken>", escape_xml(&a.token), escape_xml(&a.key), escape_xml(household))
    });
    let env = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Header><credentials xmlns="http://www.sonos.com/Services/1.1"><deviceId>{}</deviceId><deviceProvider>Sonos</deviceProvider>{login}</credentials></s:Header><s:Body><{action} xmlns="http://www.sonos.com/Services/1.1">{body}</{action}></s:Body></s:Envelope>"#,
        escape_xml(device)
    );
    let soap_action = format!("\"http://www.sonos.com/Services/1.1#{action}\"");
    let headers = [("Content-Type", "text/xml; charset=\"utf-8\""), ("SOAPACTION", soap_action.as_str())];
    let res = request(url, "POST", "", &headers, Some(env.as_bytes()))?;
    // Faults come back as HTTP 500 with a body worth reading.
    Ok(strip_ns(&String::from_utf8_lossy(&res.body)))
}

/// `<ns2:title>` → `<title>`: services pick their own namespace prefixes.
fn strip_ns(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..=i]);
        rest = &rest[i + 1..];
        if let Some(r) = rest.strip_prefix('/') {
            out.push('/');
            rest = r;
        }
        let name_end = rest.find(|c: char| c == ' ' || c == '>' || c == '/').unwrap_or(rest.len());
        if let Some(colon) = rest[..name_end].find(':') {
            rest = &rest[colon + 1..];
        }
    }
    out.push_str(rest);
    out
}

fn tag(xml: &str, name: &str) -> Option<String> {
    super::sonos::tag(xml, name)
}

/// Starts linking: returns the sign-in URL and the code to collect the token with.
pub fn app_link(url: &str, device: &str, household: &str) -> Result<(String, String), String> {
    let xml = call(url, device, household, None, "getAppLink", &format!("<householdId>{}</householdId>", escape_xml(household)))?;
    match (tag(&xml, "regUrl"), tag(&xml, "linkCode")) {
        (Some(reg), Some(code)) if reg.starts_with("https://") => Ok((reg, code)),
        _ => Err(tag(&xml, "faultstring").unwrap_or_else(|| "the service did not offer a sign-in link".into())),
    }
}

/// The account once the admin has signed in; `None` while still waiting.
pub fn device_token(url: &str, device: &str, household: &str, code: &str) -> Result<Option<Account>, String> {
    let body = format!("<householdId>{}</householdId><linkCode>{}</linkCode><linkDeviceId>{}</linkDeviceId>", escape_xml(household), escape_xml(code), escape_xml(device));
    let xml = call(url, device, household, None, "getDeviceAuthToken", &body)?;
    if let (Some(token), Some(key)) = (tag(&xml, "authToken"), tag(&xml, "privateKey")) {
        return Ok(Some(Account { token, key, device: device.into(), household: household.into() }));
    }
    let fault = tag(&xml, "faultstring").unwrap_or_default();
    if fault.contains("RETRY") || fault.is_empty() { Ok(None) } else { Err(fault) }
}

/// Searches one category (track, album, playlist). Refreshes the token in
/// `account` when the service asks; the caller saves it if it changed.
pub fn search(url: &str, account: &mut Account, category: &str, term: &str, count: u32) -> Result<Vec<Item>, String> {
    let body = format!("<id>{}</id><term>{}</term><index>0</index><count>{count}</count>", escape_xml(category), escape_xml(term));
    for _ in 0..2 {
        let xml = call(url, &account.device, &account.household, Some(account), "search", &body)?;
        if xml.contains("TokenRefreshRequired") {
            match (tag(&xml, "authToken"), tag(&xml, "privateKey")) {
                (Some(token), Some(key)) => {
                    account.token = token;
                    account.key = key;
                    continue;
                }
                _ => return Err("Spotify asked to sign in again".into()),
            }
        }
        if let Some(fault) = tag(&xml, "faultstring") {
            return Err(format!("Spotify: {fault}"));
        }
        return Ok(parse_search(&xml));
    }
    Err("Spotify keeps asking for a new token".into())
}

pub fn parse_search(xml: &str) -> Vec<Item> {
    let mut out = Vec::new();
    for (open, close) in [("<mediaMetadata>", "</mediaMetadata>"), ("<mediaCollection>", "</mediaCollection>")] {
        for part in xml.split(open).skip(1) {
            let item = part.split(close).next().unwrap_or("");
            let (Some(id), Some(title)) = (tag(item, "id"), tag(item, "title")) else { continue };
            let kind = match id.split(':').nth(1) {
                Some("track") => "track",
                Some("album") => "album",
                Some("playlist") => "playlist",
                _ => continue, // artists and the like need browsing, not playing
            };
            let sub = tag(item, "artist").unwrap_or_default();
            let art = tag(item, "albumArtURI").unwrap_or_default();
            out.push(Item { id, title, sub, kind, art });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_namespaces() {
        assert_eq!(strip_ns(r#"<?xml version="1.0"?><SOAP-ENV:Body><ns2:title a="1">x</ns2:title><br/></SOAP-ENV:Body>"#), r#"<?xml version="1.0"?><Body><title a="1">x</title><br/></Body>"#);
    }

    #[test]
    fn reads_search_results() {
        let xml = strip_ns(r#"<ns2:searchResult><ns2:mediaMetadata><ns2:id>spotify:track:2Wf</ns2:id><ns2:itemType>track</ns2:itemType><ns2:title>Take on Me</ns2:title><ns2:trackMetadata><ns2:artist>a-ha</ns2:artist><ns2:albumArtURI>https://i.scdn.co/image/ab67</ns2:albumArtURI></ns2:trackMetadata></ns2:mediaMetadata><ns2:mediaCollection><ns2:id>spotify:playlist:37i</ns2:id><ns2:itemType>playlist</ns2:itemType><ns2:title>Dinner &#127837; 2026</ns2:title></ns2:mediaCollection><ns2:mediaCollection><ns2:id>spotify:artist:9</ns2:id><ns2:title>a-ha</ns2:title></ns2:mediaCollection></ns2:searchResult>"#);
        let items = parse_search(&xml);
        assert_eq!(items, vec![
            Item { id: "spotify:track:2Wf".into(), title: "Take on Me".into(), sub: "a-ha".into(), kind: "track", art: "https://i.scdn.co/image/ab67".into() },
            Item { id: "spotify:playlist:37i".into(), title: "Dinner 🍝 2026".into(), sub: "".into(), kind: "playlist", art: "".into() },
        ]);
    }
}
