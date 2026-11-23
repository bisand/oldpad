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

    // --- e-ink: a picture to show, and taps that press what the picture showed
    let token = format!("Bearer {}", tablet.bearer.clone().unwrap());
    let picture = |page: u32| request(&base, "GET", &format!("/api/tablet/kindle.png?page={page}&time=14:05"), &[("Authorization", token.as_str())], None).unwrap();
    let shot = picture(0);
    assert_eq!((shot.status, shot.headers["content-type"].as_str()), (200, "image/png"));
    if let Ok(path) = std::env::var("OLDPAD_KINDLE_PREVIEW") {
        std::fs::write(path, &shot.body).unwrap();
    }
    let layout = shot.headers["x-oldpad-layout"].clone();
    let tap = json!({ "x": 100, "y": 400, "page": 0, "layout": layout });
    assert_eq!(tablet.call("POST", "/api/tablet/kindle/tap", Some(tap.clone())).0, 200);
    assert_eq!(tablet.call("GET", "/api/tablet", None).1["tiles"][0]["capabilities"]["onoff"]["value"], false, "the tap turned it off");
    assert_eq!(tablet.call("POST", "/api/tablet/kindle/tap", Some(tap)).0, 409, "that picture is out of date");
    assert_eq!(tablet.call("GET", "/api/tablet", None).1["tiles"][0]["capabilities"]["onoff"]["value"], false, "and pressed nothing");
    let layout = picture(0).headers["x-oldpad-layout"].clone();
    assert_eq!(tablet.call("POST", "/api/tablet/kindle/tap", Some(json!({ "x": 100, "y": 400, "page": 0, "layout": layout }))).0, 200);
    let mut nobody = Client { base: base.clone(), cookie: None, bearer: None };
    assert_eq!(nobody.call("GET", "/api/tablet/kindle.png", None).0, 401);
    assert_eq!(nobody.call("POST", "/api/tablet/kindle/tap", Some(json!({ "x": 1, "y": 1 }))).0, 401);

    // --- the Kindle's browser: plain pages, a cookie and forms
    let web = |method: &str, path: &str, cookie: &str, body: Option<&str>| {
        let mut headers = vec![("Cookie", cookie)];
        if body.is_some() {
            headers.push(("Content-Type", "application/x-www-form-urlencoded"));
        }
        request(&base, method, path, &headers, body.map(str::as_bytes)).unwrap()
    };
    let cookie_of = |r: &crate::client::HttpResponse| r.headers["set-cookie"].split(';').next().unwrap().to_string();
    let text = |r: &crate::client::HttpResponse| String::from_utf8_lossy(&r.body).to_string();
    let field = |html: &str, name: &str| html.split(&format!("name=\"{name}\" value=\"")).nth(1).unwrap().split('"').next().unwrap().to_string();
    assert!(text(&web("GET", "/kindle", "", None)).contains("action=\"/kindle/pair\""));
    let started = web("POST", "/kindle/pair", "", Some(""));
    assert_eq!((started.status, started.headers["location"].as_str()), (303, "/kindle"));
    let pending = cookie_of(&started);
    assert!(started.headers["set-cookie"].contains("HttpOnly"));
    let (kindle_code, kindle_pin) = { let v: Vec<&str> = pending.split('=').nth(1).unwrap().split('.').collect(); (v[2].to_string(), v[3].to_string()) };
    let pairing_page = text(&web("GET", "/kindle", &pending, None));
    assert!(pairing_page.contains(&kindle_pin), "shows the PIN");
    let qr_path = format!("/kindle/qr/{kindle_code}.png");
    assert!(pairing_page.contains(&qr_path), "shows the QR code");
    let qr = web("GET", &qr_path, &pending, None);
    assert_eq!((qr.status, qr.headers["content-type"].as_str()), (200, "image/png"));
    assert!(qr.body.len() < 100_000, "{}", qr.body.len());
    assert_eq!(web("GET", &qr_path, "", None).status, 404, "only the browser that started it");
    assert_eq!(web("GET", "/kindle/qr/AAAAAAAA.png", &pending, None).status, 404, "only its own code");
    let (s, kindle_device) = phone.call("POST", &format!("/api/admin/pair/{kindle_code}"), Some(json!({ "name": "Kindle", "pin": kindle_pin })));
    assert_eq!(s, 200);
    let kindle_id = kindle_device["id"].as_str().unwrap().to_string();
    assert_eq!(phone.call("PUT", &format!("/api/admin/devices/{kindle_id}"), Some(json!({ "tiles": [lamp, heater], "more": [plug, speaker, kitchen], "panels": { "outdoor": outdoor }, "pages": [{ "id": "p1", "name": "Evening", "icon": "moon", "items": [night] }] }))).0, 200);
    let paired = web("GET", "/kindle", &pending, None);
    assert_eq!(paired.status, 303);
    let screen = cookie_of(&paired);
    assert!(screen.starts_with("oldpad_kindle="), "{screen}");
    let page = web("GET", "/kindle", &screen, None);
    let (_, list) = phone.call("GET", "/api/admin/devices", None);
    let shown = list.as_array().unwrap().iter().find(|d| d["id"] == kindle_id.as_str()).unwrap();
    assert_eq!((&shown["online"], &shown["live"]), (&json!(true), &json!(false)), "a Kindle with no stream is online while it asks for pages");
    assert_eq!(shown["health"], Value::Null, "nothing reported yet");
    let report = json!({ "ms": 40, "drops": 9999, "errors": 1, "error": "boom\r\nx", "version": "0.7.3", "width": 1072, "height": 1226 }).to_string();
    let send = |cookie: &str| request(&base, "POST", "/kindle/health", &[("Cookie", cookie), ("Content-Type", "application/json"), ("User-Agent", "Kindle test")], Some(report.as_bytes())).unwrap().status;
    assert_eq!((send(""), send(&screen)), (401, 200), "only a paired screen reports");
    assert_eq!(tablet.call("POST", "/api/tablet/health", Some(json!({ "ms": 12 }))).0, 200);
    let (_, list) = phone.call("GET", "/api/admin/devices", None);
    let health = &list.as_array().unwrap().iter().find(|d| d["id"] == kindle_id.as_str()).unwrap()["health"];
    assert_eq!((&health["ms"], &health["drops"], &health["error"], &health["browser"], &health["width"]), (&json!(40), &json!(50), &json!("boomx"), &json!("Kindle test"), &json!(1072)), "bounded and cleaned");
    assert_eq!(list[0]["health"]["ms"], 12, "each screen has its own report");
    assert!(page.headers["content-security-policy"].contains("form-action 'self'"));
    let html = text(&page);
    if let Ok(path) = std::env::var("OLDPAD_KINDLE_HTML") {
        std::fs::write(path, &html).unwrap();
    }
    assert!(html.contains("Ceiling lamp") && html.contains("4.2°"));
    let form = |token: &str, layout: &str| format!("token={token}&layout={layout}&page=0&item=0");
    let (form_token, form_layout) = (field(&html, "token"), field(&html, "layout"));
    assert_eq!(web("POST", "/kindle/press", &screen, Some(&form("wrong", &form_layout))).status, 403);
    assert_eq!(web("POST", "/kindle/press", "", Some(&form(&form_token, &form_layout))).headers["location"], "/kindle", "no cookie, nothing pressed");
    let mut cross = vec![("Cookie", screen.as_str()), ("Content-Type", "application/x-www-form-urlencoded"), ("Origin", "https://evil.example")];
    let body = form(&form_token, &form_layout);
    assert_eq!(request(&base, "POST", "/kindle/press", &cross, Some(body.as_bytes())).unwrap().status, 403);
    cross.clear();
    let lamp_on = |t: &mut Client| t.call("GET", "/api/tablet", None).1["tiles"][0]["capabilities"]["onoff"]["value"].clone();
    assert_eq!(lamp_on(&mut tablet), true, "refused presses did nothing");
    assert_eq!(web("POST", "/kindle/press", &screen, Some(&body)).headers["location"], "/kindle?page=0");
    assert_eq!(lamp_on(&mut tablet), false);
    assert_eq!(web("POST", "/kindle/press", &screen, Some(&body)).headers["location"], "/kindle?page=0&notice=stale", "an old page presses nothing");
    assert_eq!(lamp_on(&mut tablet), false);
    let fresh = text(&web("GET", "/kindle", &screen, None));
    assert_eq!(web("POST", "/kindle/press", &screen, Some(&form(&form_token, &field(&fresh, "layout")))).status, 303);
    assert_eq!(lamp_on(&mut tablet), true);
    let menu = text(&web("GET", "/kindle", &screen, None));
    assert!(menu.contains("class=\"menu\"") && menu.contains("view=lights") && menu.contains("view=p0"));
    assert!(menu.contains("i-bulb") && web("GET", "/kindle", &screen, None).headers["content-security-policy"].contains("img-src 'self'"));
    assert!(menu.contains("src=\"/kindle/app.js\"") && web("GET", "/kindle", &screen, None).headers["content-security-policy"].contains("script-src 'self'"));
    assert_eq!(web("GET", "/kindle/app.js", "", None).status, 200);
    let sheet = web("GET", "/kindle/icons.png", "", None);
    assert_eq!((sheet.status, sheet.headers["content-type"].as_str()), (200, "image/png"));
    assert!(text(&web("GET", "/kindle/style.css", "", None)).contains(".on .i-bulb"));
    let scenes = text(&web("GET", "/kindle?view=flows", &screen, None));
    assert!(scenes.contains("Good night") && !scenes.contains("Coffee maker<"), "a view shows only its own");
    assert!(text(&web("GET", "/kindle?view=%0d%0aevil", &screen, None)).contains("Coffee maker<"), "an unknown view is home");
    let music = text(&web("GET", "/kindle?view=music", &screen, None));
    assert!(menu.contains("view=music") && music.contains("Radio One") && music.contains("i-pause") && music.contains("Favourites") && music.contains("28 %"));
    assert_eq!(music.matches("class=\"rm in\"").count(), 2, "the two rooms of the demo's group");
    let speak = |id: &str, action: &str, token: &str| web("POST", "/kindle/speaker", &screen, Some(&format!("token={token}&id={id}&action={action}")));
    assert_eq!(speak(&speaker, "pause", "wrong").status, 403);
    assert_eq!(speak(&bath, "pause", &form_token).headers["location"], "/kindle?view=music&page=0&notice=stale", "only this screen's speakers");
    assert_eq!(speak(&speaker, "pause", &form_token).headers["location"], "/kindle?view=music&page=0");
    assert!(!text(&web("GET", "/kindle?view=music", &screen, None)).contains("i-pause"));
    assert_eq!(speak(&speaker, "play", &form_token).status, 303);
    assert_eq!(speak(&speaker, "up", &form_token).status, 303);
    assert!(text(&web("GET", "/kindle?view=music", &screen, None)).contains("33 %"));
    assert_eq!(speak(&speaker, "down", &form_token).status, 303);
    let room_form = |id: &str, action: &str| format!("token={form_token}&id={id}&room={speaker}&action={action}");
    assert_eq!(web("POST", "/kindle/speaker", &screen, Some(&room_form(&kitchen, "leave"))).status, 303);
    assert_eq!(text(&web("GET", "/kindle?view=music", &screen, None)).matches("class=\"rm in\"").count(), 1, "left the group");
    assert_eq!(web("POST", "/kindle/speaker", &screen, Some(&room_form(&kitchen, "join"))).status, 303);
    assert_eq!(text(&web("GET", "/kindle?view=music", &screen, None)).matches("class=\"rm in\"").count(), 2, "back in the group");
    let hits = text(&web("GET", "/kindle?view=music&q=Radio", &screen, None));
    assert!(hits.contains("class=\"hits\"") && hits.contains("name=\"action\" value=\"fav\""), "search shows results to play");
    assert!(menu.contains("<noscript><meta http-equiv=\"refresh\""), "only a browser without scripts reloads the page");
    assert_eq!(web("GET", "/kindle/events", "", None).status, 401);
    let mut live = TcpStream::connect(base.trim_start_matches("http://")).unwrap();
    write!(live, "GET /kindle/events HTTP/1.1\r\nHost: x\r\nCookie: {screen}\r\n\r\n").unwrap();
    live.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut live_lines = BufReader::new(live.try_clone().unwrap()).lines();
    assert!(live_lines.next().unwrap().unwrap().starts_with("HTTP/1.1 200"));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(tablet.call("POST", "/api/tablet/set", Some(json!({ "id": lamp, "capability": "onoff", "value": false }))).0, 200);
    assert!(live_lines.by_ref().map_while(Result::ok).any(|l| l == "event: entity"), "the Kindle hears a change");
    assert_eq!(tablet.call("POST", "/api/tablet/set", Some(json!({ "id": lamp, "capability": "onoff", "value": true }))).0, 200);
    drop(live_lines);
    drop(live);
    let stale = format!("token={form_token}&layout=old&view=flows&page=0&item=0");
    assert_eq!(web("POST", "/kindle/press", &screen, Some(&stale)).headers["location"], "/kindle?view=flows&page=0&notice=stale", "a press stays on its view");
    assert_eq!(lamp_on(&mut tablet), true);
    assert_eq!(phone.call("DELETE", &format!("/api/admin/devices/{kindle_id}"), None).0, 200);
    assert!(text(&web("GET", "/kindle", &screen, None)).contains("action=\"/kindle/pair\""), "removing the screen signs it out");

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

    // --- doorbell: its camera may be read, without becoming a page or tile
    assert_eq!(phone.call("PUT", &dev, Some(json!({ "panels": { "outdoor": outdoor, "doorbell": heater, "door_camera": door } }))).0, 200);
    let (_, state) = tablet.call("GET", "/api/tablet", None);
    assert_eq!((state["panels"]["doorbell"]["name"].as_str().is_some(), state["panels"]["door_camera"]["class"].as_str()), (true, Some("camera")));
    assert_eq!(tablet.call("GET", &camera, None).0, 200);
    assert_eq!(phone.call("PUT", &dev, Some(json!({ "panels": { "outdoor": outdoor } }))).0, 200);
    assert_eq!(tablet.call("GET", &camera, None).0, 403, "gone with the setting");

    // --- an integration that stopped answering: told only to the screens showing it
    let (s, other) = phone.call("POST", "/api/admin/integrations", Some(json!({ "type": "demo", "name": "Elsewhere", "config": {} })));
    assert_eq!(s, 200, "{other}");
    let other = other["id"].as_str().unwrap().to_string();
    assert_eq!(tablet.call("GET", "/api/tablet", None).1["down"], json!([]));
    app.registry.force_down(&iid, Some(200));
    app.registry.force_down(&other, Some(200));
    let (_, state) = tablet.call("GET", "/api/tablet", None);
    assert_eq!((state["down"].as_array().unwrap().len(), state["down"][0]["name"].as_str()), (1, Some("Demo")));
    assert!(state["down"][0]["secs"].as_u64().unwrap() >= 200);
    let admin_says = |p: &mut Client, id: &str| p.call("GET", "/api/admin/integrations", None).1.as_array().unwrap().iter().find(|i| i["id"] == id).unwrap()["down"].as_bool();
    assert_eq!(admin_says(&mut phone, &iid), Some(true), "admin agrees with the screens");
    app.registry.force_down(&iid, Some(30));
    assert_eq!(admin_says(&mut phone, &iid), Some(false), "a short dropout is not reported");
    app.registry.force_down(&iid, None);
    assert_eq!(phone.call("DELETE", &format!("/api/admin/integrations/{other}"), None).0, 200);

    // --- history: only for what this screen shows, and only where it exists
    let (s, hist) = tablet.call("GET", &format!("/api/tablet/history?id={outdoor}"), None);
    assert_eq!((s, hist["measure_temperature"].as_array().map(Vec::len)), (200, Some(96)));
    assert_eq!(tablet.call("GET", &format!("/api/tablet/history?id={reading}"), None).0, 403, "not on this screen");
    assert_eq!(tablet.call("GET", &format!("/api/tablet/history?id={lamp}"), None).0, 502, "a lamp has no history");
    assert_eq!(stranger.call("GET", &format!("/api/tablet/history?id={outdoor}"), None).0, 401);

    // --- a sensor whose system keeps no history: oldpad records it, except while it is down
    assert_eq!(phone.call("PUT", &dev, Some(json!({ "panels": { "outdoor": outdoor, "indoor": heater } }))).0, 200);
    let heater_history = format!("/api/tablet/history?id={heater}");
    assert!(tablet.call("GET", "/api/tablet", None).1["panels"]["indoor"]["capabilities"].get("history").is_some(), "offered to the graphs page");
    assert_eq!(tablet.call("GET", &heater_history, None).0, 502, "nothing recorded yet");
    let t = crate::util::now() as i64;
    app.registry.force_down(&iid, Some(200));
    app.record_readings(t - 900);
    assert_eq!(tablet.call("GET", &heater_history, None).0, 502, "a stale value is not a reading");
    app.registry.force_down(&iid, None);
    app.record_readings(t);
    let (s, recorded) = tablet.call("GET", &heater_history, None);
    assert_eq!((s, recorded["measure_temperature"].as_array().map(Vec::len)), (200, Some(1)));
    assert_eq!(app.recorder.get(&outdoor), None, "a system with its own history is not recorded twice");
    assert_eq!(phone.call("PUT", &dev, Some(json!({ "panels": { "outdoor": outdoor } }))).0, 200);

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
    let shown = tablet.call("GET", "/api/tablet", None).1["about"].clone();
    assert_eq!((shown["version"].as_str(), shown["state"].as_str()), (Some(crate::util::VERSION), Some("dev")), "a local build is not called out of date");
    assert_eq!(phone.call("GET", "/api/admin/settings", None).1["about"], shown, "admin says the same");
    let (s, backup) = phone.call("GET", "/api/admin/backup", None);
    assert_eq!((s, backup["devices"][0]["name"].as_str(), backup.get("sessions")), (200, Some("Kitchen"), None));
    assert_eq!(phone.call("PUT", &format!("/api/admin/devices/{device_id}"), Some(json!({ "name": "Renamed" }))).0, 200);
    assert_eq!(phone.call("POST", "/api/admin/restore", Some(json!({ "not": "a backup" }))).0, 400);
    assert_eq!(phone.call("POST", "/api/admin/restore", Some(backup)).0, 200);
    assert_eq!(tablet.call("GET", "/api/tablet", None).1["device"]["name"], "Kitchen", "restored, and still paired");

    // --- a backup past the normal limit: for a signed-in admin only, within its own limit
    let (_, mut large) = phone.call("GET", "/api/admin/backup", None);
    large["padding"] = json!("x".repeat(200_000));
    let verified: Vec<u64> = app.store.lock().unwrap().data.sessions.iter().map(|s| s.verified).collect();
    app.store.lock().unwrap().data.sessions.iter_mut().for_each(|s| s.verified = 0);
    let (s, stale) = phone.call("POST", "/api/admin/restore", Some(large.clone()));
    assert_eq!((s, stale["reauth"].as_bool()), (403, Some(true)), "a stale session gets the passkey prompt");
    app.store.lock().unwrap().data.sessions.iter_mut().zip(verified).for_each(|(s, v)| s.verified = v);
    assert_eq!(phone.call("POST", "/api/admin/restore", Some(large)).0, 200);
    let status_for = |path: &str, cookie: Option<&str>, len: usize| {
        let mut c = TcpStream::connect(base.trim_start_matches("http://")).unwrap();
        let cookie = cookie.map_or(String::new(), |v| format!("Cookie: {v}\r\n"));
        write!(c, "POST {path} HTTP/1.1\r\nHost: x\r\nX-Oldpad: 1\r\nContent-Type: application/json\r\n{cookie}Content-Length: {len}\r\n\r\n").unwrap();
        let mut line = String::new();
        BufReader::new(c).read_line(&mut line).unwrap();
        line.split(' ').nth(1).unwrap().parse::<u16>().unwrap()
    };
    let (over, past) = (crate::http::MAX_BODY + 1, crate::http::MAX_RESTORE + 1);
    let cookie = phone.cookie.clone().unwrap();
    assert_eq!(status_for("/api/admin/restore", None, over), 413, "no sign-in, normal limit");
    assert_eq!(status_for("/api/admin/restore", Some("oldpad_admin=wrong"), over), 413);
    assert_eq!(status_for("/api/admin/restore", Some(&cookie), past), 413, "the larger limit holds too");
    assert_eq!(status_for("/api/admin/settings", Some(&cookie), over), 413, "other routes keep the normal limit");

    // --- removing the screen revokes its token
    assert_eq!(phone.call("DELETE", &format!("/api/admin/devices/{device_id}"), None).0, 200);
    assert_eq!(tablet.call("GET", "/api/tablet", None).0, 401);
}
