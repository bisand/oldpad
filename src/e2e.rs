//! End to end over real HTTP: a phone sets up an admin passkey, signs in, adds
//! the demo integration and approves a tablet; the tablet pairs, reads its
//! tiles, opens the live event stream and flips a light.

use crate::app::tests::test_app;
use crate::client::request;
use crate::webauthn::tests::{assertion_for, register_for};
use crate::webauthn::Rp;
use p256::ecdsa::SigningKey;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

struct Client {
    base: String,
    cookie: Option<String>,
    bearer: Option<String>,
}

impl Client {
    fn call(&mut self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        self.call_with(method, path, body, &[("X-Oldpad", "1")])
    }

    fn call_with(&mut self, method: &str, path: &str, body: Option<Value>, extra: &[(&str, &str)]) -> (u16, Value) {
        let mut headers: Vec<(&str, String)> = extra.iter().map(|(k, v)| (*k, v.to_string())).collect();
        if let Some(c) = &self.cookie {
            headers.push(("Cookie", c.clone()));
        }
        if let Some(t) = &self.bearer {
            headers.push(("Authorization", format!("Bearer {t}")));
        }
        let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let body = body.map(|b| b.to_string().into_bytes());
        let res = request(&self.base, method, path, &refs, body.as_deref()).unwrap();
        if let Some(set) = res.headers.get("set-cookie") {
            self.cookie = Some(set.split(';').next().unwrap().to_string());
        }
        (res.status, res.json().unwrap_or(Value::Null))
    }
}

#[test]
fn phone_and_tablet_end_to_end() {
    let app = test_app();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let setup_code = app.setup_code.lock().unwrap().clone().unwrap();
    let server = app.clone();
    std::thread::spawn(move || crate::serve(server, listener, false));

    let rp = Rp { id: "localhost", origin: "http://localhost:8080" };
    let key = SigningKey::from_slice(&[3u8; 32]).unwrap();
    let mut phone = Client { base: base.clone(), cookie: None, bearer: None };

    // --- first-time setup with the setup code, then a passkey
    let (s, opts) = phone.call("POST", "/api/admin/register/options", Some(json!({ "name": "Test", "setupCode": "WRONG123" })));
    assert_eq!((s, opts["error"].is_string()), (403, true));
    let (s, opts) = phone.call("POST", "/api/admin/register/options", Some(json!({ "name": "Test", "setupCode": setup_code })));
    assert_eq!(s, 200, "{opts}");
    let challenge = opts["challenge"].as_str().unwrap();
    let credential = register_for(&rp, &key, challenge);
    let (s, _) = phone.call("POST", "/api/admin/register/verify", Some(json!({ "challenge": challenge, "credential": credential })));
    assert_eq!(s, 200);
    assert_eq!(phone.call("GET", "/api/admin/status", None).1["admin"]["name"], "Test");
    assert!(app.setup_code.lock().unwrap().is_none(), "setup code is spent");

    // --- sign out, sign back in with the passkey
    phone.call("POST", "/api/admin/logout", Some(json!({})));
    assert_eq!(phone.call("GET", "/api/admin/devices", None).0, 401);
    let (_, opts) = phone.call("POST", "/api/admin/login/options", Some(json!({})));
    let challenge = opts["challenge"].as_str().unwrap();
    let assertion = assertion_for(&rp, &key, challenge, rp.origin, 0x05, 0);
    assert_eq!(phone.call("POST", "/api/admin/login/verify", Some(json!({ "challenge": challenge, "credential": assertion }))).0, 200);

    // --- inviting a second admin: one use, then gone; the last admin can't be removed
    let (s, inv) = phone.call("POST", "/api/admin/invites", Some(json!({})));
    assert_eq!(s, 200);
    let code = inv["url"].as_str().unwrap().rsplit('/').next().unwrap().to_string();
    let mut guest = Client { base: base.clone(), cookie: None, bearer: None };
    assert_eq!(guest.call("POST", "/api/admin/register/options", Some(json!({ "name": "Kari", "invite": "NOPE" }))).0, 410);
    let (s, opts) = guest.call("POST", "/api/admin/register/options", Some(json!({ "name": "Kari", "invite": code })));
    assert_eq!(s, 200, "{opts}");
    let challenge = opts["challenge"].as_str().unwrap();
    let credential = register_for(&rp, &key, challenge);
    assert_eq!(guest.call("POST", "/api/admin/register/verify", Some(json!({ "challenge": challenge, "credential": credential }))).0, 200);
    assert_eq!(guest.call("GET", "/api/admin/status", None).1["admin"]["name"], "Kari");
    assert_eq!(guest.call("POST", "/api/admin/register/options", Some(json!({ "name": "Again", "invite": code }))).0, 200, "signed in now: adds a passkey to Kari");
    let mut other = Client { base: base.clone(), cookie: None, bearer: None };
    assert_eq!(other.call("POST", "/api/admin/register/options", Some(json!({ "name": "Ola", "invite": code }))).0, 410, "used up");
    let (_, people) = phone.call("GET", "/api/admin/people", None);
    let kari = people["people"].as_array().unwrap().iter().find(|p| p["name"] == "Kari").unwrap()["id"].as_str().unwrap().to_string();
    let me = people["people"].as_array().unwrap().iter().find(|p| p["me"] == true).unwrap()["id"].as_str().unwrap().to_string();
    assert_eq!(phone.call("DELETE", &format!("/api/admin/people/{kari}"), None).0, 200);
    assert_eq!(guest.call("GET", "/api/admin/devices", None).0, 401, "her session ended with her");
    assert_eq!(phone.call("DELETE", &format!("/api/admin/people/{me}"), None).0, 409);

    // --- writes need the custom header, and must not come from another origin
    assert_eq!(phone.call_with("POST", "/api/admin/logout", Some(json!({})), &[]).0, 403);
    assert_eq!(phone.call_with("POST", "/api/admin/logout", Some(json!({})), &[("X-Oldpad", "1"), ("Origin", "https://evil.example")]).0, 403);

    // --- add the demo integration through the wizard's endpoints
    let (s, kinds) = phone.call("GET", "/api/admin/kinds", None);
    assert_eq!(s, 200);
    assert!(kinds.as_array().unwrap().iter().any(|k| k["id"] == "homey"));
    let (_, test) = phone.call("POST", "/api/admin/integrations/test", Some(json!({ "type": "demo", "config": {} })));
    assert_eq!(test["ok"], true);
    let (s, created) = phone.call("POST", "/api/admin/integrations", Some(json!({ "type": "demo", "name": "Demo", "config": { "zone": "Stue" } })));
    assert_eq!(s, 200);
    let iid = created["id"].as_str().unwrap().to_string();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.registry.status(&iid) != "running" {
        assert!(Instant::now() < deadline, "demo never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    let (_, entities) = phone.call("GET", "/api/admin/entities", None);
    assert!(entities.as_array().unwrap().len() >= 5);

    // --- tablet pairs; phone approves the code from the QR
    let mut tablet = Client { base: base.clone(), cookie: None, bearer: None };
    let (s, pair) = tablet.call("POST", "/api/pair", Some(json!({})));
    assert_eq!(s, 200);
    assert!(pair["qr"].as_str().unwrap().starts_with("<svg"));
    assert!(pair["url"].as_str().unwrap().ends_with(&format!("/admin#/pair/{}", pair["code"].as_str().unwrap())));
    let poll = format!("/api/pair/{}", pair["id"].as_str().unwrap());
    let secret = pair["secret"].as_str().unwrap().to_string();
    assert_eq!(tablet.call_with("GET", &poll, None, &[("X-Pair-Secret", &secret)]).1["status"], "pending");

    let (s, approved) = phone.call("POST", &format!("/api/admin/pair/{}", pair["code"].as_str().unwrap()), Some(json!({ "name": "Kitchen", "pin": pair["pin"] })));
    assert_eq!(s, 200);
    let device_id = approved["id"].as_str().unwrap().to_string();
    let (_, got) = tablet.call_with("GET", &poll, None, &[("X-Pair-Secret", &secret)]);
    assert_eq!(got["status"], "approved");
    tablet.bearer = Some(got["token"].as_str().unwrap().to_string());

    // --- phone picks two tiles; tablet sees them
    let (lamp, heater, plug, reading, outdoor) =
        (format!("{iid}:lamp"), format!("{iid}:heater"), format!("{iid}:plug"), format!("{iid}:reading"), format!("{iid}:outdoor"));
    let (speaker, kitchen, bath) = (format!("{iid}:speaker"), format!("{iid}:kitchen"), format!("{iid}:bath"));
    let night = format!("{iid}:night");
    let layout = json!({ "tiles": [lamp, heater], "more": [plug, speaker, kitchen], "panels": { "outdoor": outdoor },
        "pages": [{ "id": "p1", "name": "Evening", "icon": "moon", "items": ["card:clock", night] }] });
    for bad in [json!([{ "id": "p1", "name": "X", "icon": "nope", "items": [] }]), json!([{ "id": "p1", "name": "X", "icon": "moon", "items": ["card:secret"] }]), json!([{ "id": "p1", "name": " ", "icon": "moon", "items": [] }])] {
        assert_eq!(phone.call("PUT", &format!("/api/admin/devices/{device_id}"), Some(json!({ "pages": bad }))).0, 400);
    }
    assert_eq!(phone.call("PUT", &format!("/api/admin/devices/{device_id}"), Some(layout)).0, 200);
    let (s, state) = tablet.call("GET", "/api/tablet", None);
    assert_eq!(s, 200);
    assert_eq!(state["device"]["name"], "Kitchen");
    assert_eq!(state["tiles"][0]["capabilities"]["onoff"]["value"], true);
    assert_eq!(state["tiles"][0]["zone"], "Stue");
    assert_eq!(state["panels"]["outdoor"]["capabilities"]["measure_temperature"]["value"], 4.2);
    assert!(state["panels"]["indoor"].is_null());
    assert_eq!(state["more"][0]["name"], "Coffee maker");
    assert_eq!((state["pages"][0]["name"].as_str(), state["pages"][0]["items"][0]["card"].as_str()), (Some("Evening"), Some("clock")));
    assert_eq!(state["pages"][0]["items"][1]["name"], "Good night");

    // --- names: a screen can call things its own name, wherever they show
    let dev = format!("/api/admin/devices/{device_id}");
    assert_eq!(phone.call("PUT", &dev, Some(json!({ "names": { night.clone(): "x".repeat(33) } }))).0, 400);
    assert_eq!(phone.call("PUT", &dev, Some(json!({ "names": { night.clone(): " Natta ", plug.clone(): "" } }))).0, 200);
    let (_, state) = tablet.call("GET", "/api/tablet", None);
    assert_eq!((state["pages"][0]["items"][1]["name"].as_str(), state["more"][0]["name"].as_str()), (Some("Natta"), Some("Coffee maker")), "trimmed; empty means its own name");
    assert_eq!(phone.call("GET", "/api/admin/devices", None).1[0]["names"], json!({ night.clone(): "Natta" }));

    // --- cameras: only for a signed-in screen that was given the camera
    let door = format!("{iid}:door");
    let camera = format!("/api/tablet/camera?id={door}");
    assert_eq!(tablet.call("GET", &camera, None).0, 403, "not on this screen");
    assert_eq!(phone.call("PUT", &dev, Some(json!({ "more": [plug, speaker, kitchen, door] }))).0, 200);
    let (s, shot) = tablet.call("GET", &camera, None);
    assert_eq!((s, shot["type"].as_str()), (200, Some("image/png")));
    assert!(shot["data"].as_str().unwrap().starts_with("iVBOR"));
    let mut stranger = Client { base: base.clone(), cookie: None, bearer: None };
    assert_eq!(stranger.call("GET", &camera, None).0, 401, "no sign-in, no picture");
    assert_eq!(stranger.call("GET", &format!("/api/image/{iid}/door"), None).0, 401, "images need a paired screen");
    let image_token = tablet.call("GET", "/api/tablet", None).1["device"]["imageToken"].as_str().unwrap().to_string();
    assert_eq!(stranger.call("GET", &format!("/api/image/{iid}/door?t={image_token}"), None).0, 404, "a camera is not on the image route");
    assert_eq!(tablet.call("GET", &format!("/api/tablet/camera?id={iid}:lamp"), None).0, 502, "on this screen, but a lamp has no picture");
    assert_eq!(phone.call("PUT", &dev, Some(json!({ "more": [plug, speaker, kitchen] }))).0, 200);

    // --- live stream: flip the lamp, the event arrives on the stream
    let (_, ticket) = tablet.call("POST", "/api/tablet/ticket", Some(json!({})));
    let mut sse = TcpStream::connect(base.trim_start_matches("http://")).unwrap();
    write!(sse, "GET /api/events?ticket={} HTTP/1.1\r\nHost: x\r\n\r\n", ticket["ticket"].as_str().unwrap()).unwrap();
    sse.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut lines = BufReader::new(sse.try_clone().unwrap()).lines();
    assert!(lines.next().unwrap().unwrap().starts_with("HTTP/1.1 200"));
    std::thread::sleep(Duration::from_millis(100)); // let the server register the stream

    let (s, _) = tablet.call("POST", "/api/tablet/set", Some(json!({ "id": lamp, "capability": "onoff", "value": false })));
    assert_eq!(s, 200);
    let event = lines.by_ref().map(Result::unwrap).find(|l| l.starts_with("data: {\"capability\"")).unwrap();
    let data: Value = serde_json::from_str(event.trim_start_matches("data: ")).unwrap();
    assert_eq!((data["id"].as_str(), data["value"].as_bool()), (Some(lamp.as_str()), Some(false)));

    // --- a screen controls what is configured for it only; read-only values stay read-only
    assert_eq!(tablet.call("POST", "/api/tablet/set", Some(json!({ "id": plug, "capability": "onoff", "value": true }))).0, 200);
    assert_eq!(tablet.call("POST", "/api/tablet/set", Some(json!({ "id": reading, "capability": "onoff", "value": true }))).0, 403);
    assert_eq!(tablet.call("POST", "/api/tablet/set", Some(json!({ "id": night, "capability": "button", "value": true }))).0, 200); // via its page
    assert_eq!(tablet.call("POST", "/api/tablet/set", Some(json!({ "id": heater, "capability": "measure_temperature", "value": 30 }))).0, 502);

    // --- grouping: only with rooms this screen controls, and the room id is passed on bare
    let mut group = |room: &str, to: &str| tablet.call("POST", "/api/tablet/set", Some(json!({ "id": room, "capability": "speaker_group", "value": to }))).0;
    assert_eq!(group(&kitchen, &bath), 403);
    assert_eq!(group(&kitchen, "other:speaker"), 403);
    assert_eq!(group(&kitchen, &kitchen), 200); // leave
    assert_eq!(group(&kitchen, &speaker), 200); // join
    let (_, state) = tablet.call("GET", "/api/tablet", None);
    let room = state["more"].as_array().unwrap().iter().find(|e| e["id"] == kitchen.as_str()).unwrap().clone();
    assert_eq!(room["capabilities"]["speaker_group"]["value"], "speaker");

    // --- favourites: the admin arranges and removes them for the household; screens can't
    assert_eq!(tablet.call("POST", "/api/tablet/set", Some(json!({ "id": kitchen, "capability": "remove_favourite", "value": "f0" }))).0, 403);
    let arranged = json!({ "order": ["f2", "f0"], "front": ["f2"] });
    assert_eq!(phone.call("POST", "/api/admin/set", Some(json!({ "id": kitchen, "capability": "arrange_favourites", "value": arranged }))).0, 200);
    assert_eq!(phone.call("POST", "/api/admin/set", Some(json!({ "id": kitchen, "capability": "remove_favourite", "value": "f1" }))).0, 200);
    let (_, state) = tablet.call("GET", "/api/tablet", None);
    let room = state["more"].as_array().unwrap().iter().find(|e| e["id"] == speaker.as_str()).unwrap().clone();
    let favs: Vec<String> = room["capabilities"]["favourites"]["value"].as_array().unwrap().iter().map(|f| format!("{}{}", f["id"].as_str().unwrap(), if f["front"] == true { "*" } else { "" })).collect();
    assert_eq!(favs, ["f2*", "f0", "f3", "f4", "f5"]);
    assert_eq!(guest.call("POST", "/api/admin/set", Some(json!({ "id": kitchen, "capability": "remove_favourite", "value": "f2" }))).0, 401);

    // --- an admin can look at the screen (read only); nobody else can
    let (s, seen) = phone.call("GET", &format!("/api/tablet?preview={device_id}"), None);
    assert_eq!((s, seen["device"]["name"].as_str()), (200, Some("Kitchen")));
    assert_eq!(phone.call("POST", &format!("/api/tablet/set?preview={device_id}"), Some(json!({ "id": lamp, "capability": "onoff", "value": true }))).0, 403);
    let mut stranger = Client { base: base.clone(), cookie: None, bearer: None };
    assert_eq!(stranger.call("GET", &format!("/api/tablet?preview={device_id}"), None).0, 401);
    let (_, list) = phone.call("GET", "/api/admin/devices", None);
    assert_eq!(list[0]["online"], true); // the event stream above is still open
    assert_eq!(phone.call("POST", &format!("/api/admin/devices/{device_id}/reload"), Some(json!({}))).0, 200);
    assert!(lines.by_ref().map(Result::unwrap).any(|l| l == "event: refresh"));

    // --- a second pad shares the layout, then a third takes over its place
    let pair_pad = |phone: &mut Client, extra: Value| {
        let mut pad = Client { base: base.clone(), cookie: None, bearer: None };
        let (_, p) = pad.call("POST", "/api/pair", Some(json!({})));
        let mut body = json!({ "name": "Hall", "pin": p["pin"] });
        body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let (s, ok) = phone.call("POST", &format!("/api/admin/pair/{}", p["code"].as_str().unwrap()), Some(body));
        assert_eq!(s, 200, "{ok}");
        let poll = format!("/api/pair/{}", p["id"].as_str().unwrap());
        let (_, got) = pad.call_with("GET", &poll, None, &[("X-Pair-Secret", p["secret"].as_str().unwrap())]);
        pad.bearer = Some(got["token"].as_str().unwrap().to_string());
        (pad, ok["id"].as_str().unwrap().to_string())
    };
    let (mut hall, hall_id) = pair_pad(&mut phone, json!({ "follow": device_id }));
    let tiles = |c: &mut Client| c.call("GET", "/api/tablet", None).1["tiles"].clone();
    assert!(!tiles(&mut tablet).as_array().unwrap().is_empty());
    assert_eq!(tiles(&mut hall), tiles(&mut tablet), "shares the layout");
    assert_eq!(phone.call("PUT", &format!("/api/admin/devices/{hall_id}"), Some(json!({ "tiles": [] }))).0, 409, "edit the shared layout, not a copy");
    let (mut hall2, same) = pair_pad(&mut phone, json!({ "replace": hall_id }));
    assert_eq!(same, hall_id);
    assert_eq!(hall.call("GET", "/api/tablet", None).0, 401, "the replaced token is dead");
    assert_eq!(tiles(&mut hall2), tiles(&mut tablet), "the replacement keeps the layout");
    assert_eq!(phone.call("DELETE", &format!("/api/admin/devices/{hall_id}"), None).0, 200);

    // --- settings reach the screens; a backup restores what it holds
    assert_eq!(phone.call("PUT", "/api/admin/settings", Some(json!({ "day_start": 25, "day_end": 21, "theme": "auto", "language": "auto" }))).0, 400);
    assert_eq!(phone.call("PUT", "/api/admin/settings", Some(json!({ "day_start": 6, "day_end": 22, "theme": "night", "language": "nb" }))).0, 200);
    assert_eq!(tablet.call("GET", "/api/tablet", None).1["settings"]["theme"], "night");
    let (s, backup) = phone.call("GET", "/api/admin/backup", None);
    assert_eq!((s, backup["devices"][0]["name"].as_str(), backup.get("sessions")), (200, Some("Kitchen"), None));
    assert_eq!(phone.call("PUT", &format!("/api/admin/devices/{device_id}"), Some(json!({ "name": "Renamed" }))).0, 200);
    assert_eq!(phone.call("POST", "/api/admin/restore", Some(json!({ "not": "a backup" }))).0, 400);
    assert_eq!(phone.call("POST", "/api/admin/restore", Some(backup)).0, 200);
    assert_eq!(tablet.call("GET", "/api/tablet", None).1["device"]["name"], "Kitchen", "restored, and still paired");

    // --- removing the screen revokes its token
    assert_eq!(phone.call("DELETE", &format!("/api/admin/devices/{device_id}"), None).0, 200);
    assert_eq!(tablet.call("GET", "/api/tablet", None).0, 401);
}
