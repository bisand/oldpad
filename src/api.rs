use crate::app::{App, Challenge, PAIRING_TTL};
use crate::http::{Request, Response};
use crate::integrations::{redact, validate};
use crate::store::{Admin, Credential, IntegrationConfig};
use crate::util::{b64url_encode, constant_eq, escape_xml, now, random_bytes, random_code};
use crate::webauthn::{self, Rp};
use qrcodegen::{QrCode, QrCodeEcc};
use serde_json::{json, Value};
use std::sync::Arc;

const TABLET_HTML: &str = include_str!("../web/tablet.html");
const TABLET_JS: &str = include_str!("../web/tablet.js");
const TABLET_CSS: &str = include_str!("../web/tablet.css");
const ADMIN_HTML: &str = include_str!("../web/admin.html");
const ADMIN_JS: &str = include_str!("../web/admin.js");
const ADMIN_CSS: &str = include_str!("../web/admin.css");

const CSP: &str = "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

fn asset(content_type: &str, body: &str) -> Response {
    Response::new(200, content_type, body).header("Cache-Control", "no-cache").header("Content-Security-Policy", CSP)
}

pub fn handle(app: &Arc<App>, req: &Request) -> Response {
    // Every state-changing call must carry a custom header. Browsers will not
    // send one cross-site without a CORS preflight, which we never answer.
    if req.method != "GET" && req.header("x-oldpad") != Some("1") {
        return Response::error(403, "Missing X-Oldpad header");
    }
    let seg = req.segments();
    let s: Vec<&str> = seg.iter().map(String::as_str).collect();
    let m = req.method.as_str();
    match (m, s.as_slice()) {
        ("GET", []) => asset("text/html; charset=utf-8", TABLET_HTML),
        ("GET", ["tablet.js"]) => asset("text/javascript; charset=utf-8", TABLET_JS),
        ("GET", ["tablet.css"]) => asset("text/css; charset=utf-8", TABLET_CSS),
        ("GET", ["admin"]) => asset("text/html; charset=utf-8", ADMIN_HTML),
        ("GET", ["admin.js"]) => asset("text/javascript; charset=utf-8", ADMIN_JS),
        ("GET", ["admin.css"]) => asset("text/css; charset=utf-8", ADMIN_CSS),
        ("GET", ["healthz"]) => Response::new(200, "text/plain", "ok"),
        (_, ["api", "admin", rest @ ..]) => admin(app, req, m, rest).unwrap_or_else(|e| e),
        (_, ["api", rest @ ..]) => tablet(app, req, m, rest).unwrap_or_else(|e| e),
        _ => Response::error(404, "Not found"),
    }
}

// ---------------------------------------------------------------- tablet

fn qr_svg(text: &str) -> String {
    let Ok(qr) = QrCode::encode_text(text, QrCodeEcc::Medium) else { return String::new() };
    let (size, border) = (qr.size(), 4);
    let mut path = String::new();
    for y in 0..size {
        for x in 0..size {
            if qr.get_module(x, y) {
                path.push_str(&format!("M{},{}h1v1h-1z", x + border, y + border));
            }
        }
    }
    let dim = size + border * 2;
    format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {dim} {dim}" shape-rendering="crispEdges"><rect width="100%" height="100%" fill="#fff"/><path d="{path}" fill="#000"/><title>{}</title></svg>"##, escape_xml(text))
}

fn tablet(app: &Arc<App>, req: &Request, m: &str, s: &[&str]) -> Result<Response, Response> {
    match (m, s) {
        ("POST", ["pair"]) => {
            let p = app.new_pairing().map_err(|e| Response::error(429, e))?;
            let url = format!("{}/admin#/pair/{}", app.cfg.public_url, p.code);
            Ok(Response::ok(json!({
                "id": p.id, "secret": p.secret, "code": p.code, "url": url, "qr": qr_svg(&url), "expiresIn": PAIRING_TTL,
            })))
        }
        ("GET", ["pair", id]) => Ok(Response::ok(app.poll_pairing(id, req.header("x-pair-secret").unwrap_or("")))),
        ("GET", ["events"]) => {
            let device = app.redeem_ticket(req.query.get("ticket").map(String::as_str).unwrap_or("")).ok_or_else(|| Response::error(401, "Invalid ticket"))?;
            let (tx, rx) = std::sync::mpsc::channel();
            app.add_client(&device, tx);
            Ok(Response::stream(rx))
        }
        _ => {
            let device = app.device_from(req).ok_or_else(|| Response::error(401, "Not paired"))?;
            match (m, s) {
                ("GET", ["tablet"]) => {
                    let get = |id: &String| app.registry.entity(id).map(|e| json!(e)).unwrap_or_else(|| json!({ "id": id, "missing": true }));
                    let p = &device.panels;
                    let panel = |id: &Option<String>| id.as_ref().map(get).unwrap_or(Value::Null);
                    Ok(Response::ok(json!({
                        "device": { "id": device.id, "name": device.name },
                        "tiles": device.tiles.iter().map(get).collect::<Vec<_>>(),
                        "more": device.more.iter().map(get).collect::<Vec<_>>(),
                        "panels": { "indoor": panel(&p.indoor), "outdoor": panel(&p.outdoor), "wind": panel(&p.wind), "weather": panel(&p.weather), "speaker": panel(&p.speaker) },
                    })))
                }
                ("POST", ["tablet", "ticket"]) => Ok(Response::ok(json!({ "ticket": app.new_ticket(&device.id) }))),
                ("POST", ["tablet", "set"]) => {
                    let body = req.json()?;
                    let id = body["id"].as_str().unwrap_or("");
                    // A screen controls what is configured for it and nothing else.
                    if !device.allowed().contains(id) {
                        return Err(Response::error(403, "Not configured for this screen"));
                    }
                    app.registry
                        .set(id, body["capability"].as_str().unwrap_or(""), &body["value"])
                        .map(|_| Response::ok(json!({ "ok": true })))
                        .map_err(|e| Response::error(502, &e))
                }
                _ => Err(Response::error(404, "Not found")),
            }
        }
    }
}

// ---------------------------------------------------------------- admin

fn rp(app: &App) -> Rp<'_> {
    Rp { id: &app.cfg.rp_id, origin: &app.cfg.public_url }
}

fn str_field<'a>(v: &'a Value, key: &str, max: usize) -> Result<&'a str, Response> {
    let s = v[key].as_str().map(str::trim).unwrap_or("");
    if s.is_empty() || s.len() > max {
        return Err(Response::error(400, &format!("{key} is required (max {max} characters)")));
    }
    Ok(s)
}

fn admin(app: &Arc<App>, req: &Request, m: &str, s: &[&str]) -> Result<Response, Response> {
    if m != "GET" {
        // Belt and braces next to SameSite=Strict: refuse writes from any other origin.
        if let Some(origin) = req.header("origin")
            && origin != app.cfg.public_url {
                return Err(Response::error(403, "Wrong origin"));
            }
    }
    let current = app.admin_from(req);
    let needs_setup = app.store.lock().unwrap().data.admins.is_empty();

    // Routes that work without a session.
    match (m, s) {
        ("GET", ["status"]) => {
            return Ok(Response::ok(json!({
                "setupNeeded": needs_setup,
                "admin": current.as_ref().map(|a| json!({ "name": a.name, "passkeys": a.credentials.len() })),
                "publicUrl": app.cfg.public_url,
            })));
        }
        ("POST", ["register", "options"]) => {
            let body = req.json()?;
            let (admin_id, name, user_handle) = if let Some(a) = &current {
                (Some(a.id.clone()), a.name.clone(), a.user_handle.clone())
            } else if needs_setup {
                let code = app.setup_code.lock().unwrap().clone().unwrap_or_default();
                if !constant_eq(&code, &body["setupCode"].as_str().unwrap_or("").trim().to_ascii_uppercase()) {
                    return Err(Response::error(403, "Wrong setup code. It is printed in the server log at startup."));
                }
                (None, str_field(&body, "name", 64)?.to_string(), b64url_encode(&random_bytes::<16>()))
            } else {
                return Err(Response::error(401, "Sign in first"));
            };
            let challenge = app.new_challenge(Challenge::Register { admin_id, name: name.clone(), user_handle: user_handle.clone() });
            let exclude: Vec<String> = current.iter().flat_map(|a| a.credentials.iter().map(|c| c.id.clone())).collect();
            return Ok(Response::ok(webauthn::registration_options(&rp(app), &challenge, &user_handle, &name, &exclude)));
        }
        ("POST", ["register", "verify"]) => {
            let body = req.json()?;
            let challenge = body["challenge"].as_str().unwrap_or("");
            let Some(Challenge::Register { admin_id, name, user_handle }) = app.take_challenge(challenge) else {
                return Err(Response::error(400, "Challenge expired. Try again."));
            };
            let cred = webauthn::verify_registration(&rp(app), challenge, &body["credential"]).map_err(|e| Response::error(400, &e))?;
            let cred = Credential { id: cred.id, public_key: cred.public_key, counter: cred.counter, created: now() };
            let mut store = app.store.lock().unwrap();
            let admin_id = match admin_id {
                Some(id) => {
                    let a = store.data.admins.iter_mut().find(|a| a.id == id).ok_or_else(|| Response::error(404, "Admin gone"))?;
                    a.credentials.push(cred);
                    id
                }
                None => {
                    if !store.data.admins.is_empty() {
                        return Err(Response::error(409, "Setup is already done"));
                    }
                    let id = random_code(10);
                    store.data.admins.push(Admin { id: id.clone(), name, user_handle, credentials: vec![cred] });
                    *app.setup_code.lock().unwrap() = None;
                    id
                }
            };
            store.save();
            drop(store);
            let cookie = app.new_session(&admin_id);
            return Ok(Response::ok(json!({ "ok": true })).header("Set-Cookie", &cookie));
        }
        ("POST", ["login", "options"]) => {
            let challenge = app.new_challenge(Challenge::Login);
            return Ok(Response::ok(webauthn::authentication_options(&rp(app), &challenge)));
        }
        ("POST", ["login", "verify"]) => {
            let body = req.json()?;
            let challenge = body["challenge"].as_str().unwrap_or("");
            let Some(Challenge::Login) = app.take_challenge(challenge) else {
                return Err(Response::error(400, "Challenge expired. Try again."));
            };
            let cred_id = body["credential"]["id"].as_str().unwrap_or("");
            let mut store = app.store.lock().unwrap();
            let (admin_id, cred) = store
                .data
                .admins
                .iter_mut()
                .find_map(|a| {
                    let id = a.id.clone();
                    a.credentials.iter_mut().find(|c| c.id == cred_id).map(|c| (id, c))
                })
                .ok_or_else(|| Response::error(401, "Unknown passkey"))?;
            cred.counter = webauthn::verify_authentication(&rp(app), challenge, &body["credential"], &cred.public_key, cred.counter)
                .map_err(|e| Response::error(401, &e))?;
            store.save();
            drop(store);
            let cookie = app.new_session(&admin_id);
            return Ok(Response::ok(json!({ "ok": true })).header("Set-Cookie", &cookie));
        }
        ("POST", ["logout"]) => {
            let cookie = app.end_session(req);
            return Ok(Response::ok(json!({ "ok": true })).header("Set-Cookie", &cookie));
        }
        _ => {}
    }

    if current.is_none() {
        return Err(Response::error(401, "Sign in first"));
    }

    match (m, s) {
        ("GET", ["pair", code]) => {
            let list = app.pairings.lock().unwrap();
            let p = list.iter().find(|p| constant_eq(&p.code, &code.to_ascii_uppercase()) && p.expires > now());
            Ok(Response::ok(json!({ "status": match p { None => "expired", Some(p) if p.token.is_some() => "approved", _ => "pending" } })))
        }
        ("POST", ["pair", code]) => {
            let body = req.json()?;
            let device = app.approve_pairing(code, str_field(&body, "name", 64)?).map_err(|e| Response::error(410, e))?;
            Ok(Response::ok(json!({ "id": device.id })))
        }
        ("GET", ["devices"]) => {
            let store = app.store.lock().unwrap();
            let list: Vec<Value> = store
                .data
                .devices
                .iter()
                .map(|d| json!({ "id": d.id, "name": d.name, "created": d.created, "lastSeen": d.last_seen, "expires": d.expires, "tiles": d.tiles, "panels": d.panels, "more": d.more }))
                .collect();
            Ok(Response::ok(json!(list)))
        }
        ("PUT", ["devices", id]) => {
            let body = req.json()?;
            let mut store = app.store.lock().unwrap();
            let d = store.data.devices.iter_mut().find(|d| d.id == *id).ok_or_else(|| Response::error(404, "No such device"))?;
            if body.get("name").is_some() {
                d.name = str_field(&body, "name", 64)?.to_string();
            }
            let ids = |v: &Value, max: usize| -> Result<Option<Vec<String>>, Response> {
                let Some(list) = v.as_array() else { return Ok(None) };
                let list: Vec<String> = list.iter().filter_map(Value::as_str).map(str::to_string).collect();
                if list.len() > max || list.iter().any(|t| t.len() > 200) {
                    return Err(Response::error(400, "Too many entities"));
                }
                Ok(Some(list))
            };
            if let Some(tiles) = ids(&body["tiles"], 24)? {
                d.tiles = tiles;
            }
            if let Some(more) = ids(&body["more"], 200)? {
                d.more = more;
            }
            if body.get("panels").is_some() {
                let panels: crate::store::Panels = serde_json::from_value(body["panels"].clone()).map_err(|_| Response::error(400, "Bad panels"))?;
                if panels.ids().any(|id| id.len() > 200) {
                    return Err(Response::error(400, "Bad panels"));
                }
                d.panels = panels;
            }
            let device = d.clone();
            store.save();
            drop(store);
            app.update_client_tiles(&device);
            app.notify_device(&device.id, "reload");
            Ok(Response::ok(json!({ "ok": true })))
        }
        ("DELETE", ["devices", id]) => {
            let mut store = app.store.lock().unwrap();
            store.data.devices.retain(|d| d.id != *id);
            store.save();
            drop(store);
            app.notify_device(id, "revoked");
            Ok(Response::ok(json!({ "ok": true })))
        }
        ("GET", ["kinds"]) => Ok(Response::ok(json!(app
            .registry
            .kinds
            .iter()
            .map(|k| json!({ "id": k.id, "name": k.name, "description": k.description, "fields": k.fields }))
            .collect::<Vec<_>>()))),
        ("GET", ["integrations"]) => {
            let list = app.store.lock().unwrap().data.integrations.clone();
            Ok(Response::ok(json!(list
                .iter()
                .filter_map(|i| {
                    let kind = app.registry.kind(&i.kind)?;
                    Some(json!({ "id": i.id, "type": i.kind, "typeName": kind.name, "name": i.name, "config": redact(kind, &i.config), "status": app.registry.status(&i.id) }))
                })
                .collect::<Vec<_>>())))
        }
        ("POST", ["integrations", "test"]) => {
            let body = req.json()?;
            let (kind, config) = integration_input(app, &body, None)?;
            Ok(Response::ok(match (kind.test)(&config) {
                Ok(msg) => json!({ "ok": true, "message": msg }),
                Err(e) => json!({ "ok": false, "message": e }),
            }))
        }
        ("POST", ["integrations"]) | ("PUT", ["integrations", _]) => {
            let body = req.json()?;
            let path_id = if let [_, id] = s { Some(*id) } else { None };
            if path_id.is_some_and(|id| !app.store.lock().unwrap().data.integrations.iter().any(|i| i.id == id)) {
                return Err(Response::error(404, "No such integration"));
            }
            let (kind, config) = integration_input(app, &body, path_id)?;
            let name = str_field(&body, "name", 64)?.to_string();
            let mut store = app.store.lock().unwrap();
            let id = match s {
                [_, id] => {
                    let i = store.data.integrations.iter_mut().find(|i| i.id == *id).ok_or_else(|| Response::error(404, "No such integration"))?;
                    i.name = name;
                    i.config = config.clone();
                    id.to_string()
                }
                _ => {
                    let id = random_code(6).to_ascii_lowercase();
                    store.data.integrations.push(IntegrationConfig { id: id.clone(), kind: kind.id.into(), name, config: config.clone() });
                    id
                }
            };
            store.save();
            drop(store);
            app.registry.start(id.clone(), kind.id.into(), config, app.sink(id.clone()));
            Ok(Response::ok(json!({ "id": id })))
        }
        ("DELETE", ["integrations", id]) => {
            let mut store = app.store.lock().unwrap();
            store.data.integrations.retain(|i| i.id != *id);
            store.save();
            drop(store);
            app.registry.stop(id);
            Ok(Response::ok(json!({ "ok": true })))
        }
        ("GET", ["entities"]) => Ok(Response::ok(json!(app.registry.entities()))),
        _ => Err(Response::error(404, "Not found")),
    }
}

/// Resolves {type | id, config} into a validated config, reusing stored
/// secrets when an existing integration is edited.
fn integration_input<'a>(app: &'a App, body: &Value, id: Option<&str>) -> Result<(&'a crate::integrations::Kind, serde_json::Map<String, Value>), Response> {
    let existing = id.or(body["id"].as_str()).and_then(|id| app.store.lock().unwrap().data.integrations.iter().find(|i| i.id == id).cloned());
    let kind_id = existing.as_ref().map(|i| i.kind.clone()).or_else(|| body["type"].as_str().map(str::to_string)).unwrap_or_default();
    let kind = app.registry.kind(&kind_id).ok_or_else(|| Response::error(400, "Unknown integration type"))?;
    let input = body["config"].as_object().cloned().unwrap_or_default();
    let config = validate(kind, &input, existing.as_ref().map(|i| &i.config)).map_err(|e| Response::error(400, &e))?;
    Ok((kind, config))
}
