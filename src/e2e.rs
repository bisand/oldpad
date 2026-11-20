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
    std::thread::spawn(move || crate::serve(server, listener));

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

    let (s, approved) = phone.call("POST", &format!("/api/admin/pair/{}", pair["code"].as_str().unwrap()), Some(json!({ "name": "Kitchen" })));
    assert_eq!(s, 200);
    let device_id = approved["id"].as_str().unwrap().to_string();
    let (_, got) = tablet.call_with("GET", &poll, None, &[("X-Pair-Secret", &secret)]);
    assert_eq!(got["status"], "approved");
    tablet.bearer = Some(got["token"].as_str().unwrap().to_string());

    // --- phone picks two tiles; tablet sees them
    let (lamp, heater, plug) = (format!("{iid}:lamp"), format!("{iid}:heater"), format!("{iid}:plug"));
    assert_eq!(phone.call("PUT", &format!("/api/admin/devices/{device_id}"), Some(json!({ "tiles": [lamp, heater] }))).0, 200);
    let (s, state) = tablet.call("GET", "/api/tablet", None);
    assert_eq!(s, 200);
    assert_eq!(state["device"]["name"], "Kitchen");
    assert_eq!(state["tiles"][0]["capabilities"]["onoff"]["value"], true);
    assert_eq!(state["tiles"][0]["zone"], "Stue");

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

    // --- a tablet controls its own tiles only; read-only values stay read-only
    assert_eq!(tablet.call("POST", "/api/tablet/set", Some(json!({ "id": plug, "capability": "onoff", "value": true }))).0, 403);
    assert_eq!(tablet.call("POST", "/api/tablet/set", Some(json!({ "id": heater, "capability": "measure_temperature", "value": 30 }))).0, 502);

    // --- removing the screen revokes its token
    assert_eq!(phone.call("DELETE", &format!("/api/admin/devices/{device_id}"), None).0, 200);
    assert_eq!(tablet.call("GET", "/api/tablet", None).0, 401);
}
