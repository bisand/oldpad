use crate::app::{App, Challenge, PairTarget, PAIRING_TTL};
use crate::http::{Request, Response};
use crate::integrations::{redact, validate, FieldKind};
use crate::store::{Admin, Credential, IntegrationConfig};
use crate::util::{b64_encode, b64url_encode, constant_eq, escape_xml, now, random_bytes, random_code};
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

const ADMIN_ONLY: [&str; 2] = ["arrange_favourites", "remove_favourite"];

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
    if app.cfg.admin_bind.is_some() && !req.admin_port && matches!(s.as_slice(), ["admin" | "admin.js" | "admin.css"] | ["api", "admin", ..]) {
        return Response::error(404, "Not found");
    }
    match (m, s.as_slice()) {
        ("GET", []) => asset("text/html; charset=utf-8", TABLET_HTML),
        ("GET", ["tablet.js"]) => asset("text/javascript; charset=utf-8", TABLET_JS),
        ("GET", ["tablet.css"]) => asset("text/css; charset=utf-8", TABLET_CSS),
        ("GET", ["admin"]) => asset("text/html; charset=utf-8", ADMIN_HTML),
        ("GET", ["admin.js"]) => asset("text/javascript; charset=utf-8", ADMIN_JS),
        ("GET", ["admin.css"]) => asset("text/css; charset=utf-8", ADMIN_CSS),
        ("GET", ["healthz"]) => Response::new(200, "text/plain", "ok"),
        // Album art and map tiles. img tags can't send a token, so screens add `?t=`; admins have their cookie.
        ("GET", ["api", "image", iid, key]) => {
            if !app.can_fetch_image(req) {
                return Response::error(401, "Not paired");
            }
            match app.registry.image(iid, key) {
                Ok((kind, bytes)) => Response::new(200, &kind, bytes).header("Cache-Control", "private, max-age=86400").header("Content-Security-Policy", "default-src 'none'; sandbox"),
                Err(e) => Response::error(404, &e),
            }
        }
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
    // The admin port offers admins a preview of a screen, and nothing a tablet uses.
    if req.admin_port && matches!(s, ["pair", ..] | ["events"]) {
        return Err(Response::error(404, "Not found"));
    }
    match (m, s) {
        ("POST", ["pair"]) => {
            let p = app.new_pairing().map_err(|e| Response::error(429, e))?;
            let url = format!("{}/admin#/pair/{}", app.cfg.public_url, p.code);
            Ok(Response::ok(json!({
                "id": p.id, "secret": p.secret, "code": p.code, "pin": p.pin, "url": url, "qr": qr_svg(&url), "expiresIn": PAIRING_TTL,
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
            // An admin may look at a screen as it is (read only): `?preview=<screen id>`.
            let preview = req.query.get("preview").filter(|_| app.admin_from(req).is_some()).and_then(|id| {
                let store = app.store.lock().unwrap();
                let d = store.data.devices.iter().find(|d| d.id == *id).map(|d| store.data.effective(d));
                d
            });
            if preview.is_some() && m != "GET" {
                return Err(Response::error(403, "Preview is read only"));
            }
            let device = preview.or_else(|| app.device_from(req).filter(|_| !req.admin_port)).ok_or_else(|| Response::error(401, "Not paired"))?;
            match (m, s) {
                ("GET", ["tablet"]) => {
                    let get = |id: &String| {
                        let Some(mut e) = app.registry.entity(id) else { return json!({ "id": id, "missing": true }) };
                        if let Some(name) = device.names.get(id) {
                            e.name = name.clone();
                        }
                        json!(e)
                    };
                    let p = &device.panels;
                    let panel = |id: &Option<String>| id.as_ref().map(get).unwrap_or(Value::Null);
                    Ok(Response::ok(json!({
                        "device": { "id": device.id, "name": device.name, "favRows": device.fav_rows.unwrap_or(1), "imageToken": App::image_token(&device) },
                        "settings": app.store.lock().unwrap().data.settings,
                        "tiles": device.tiles.iter().map(get).collect::<Vec<_>>(),
                        "more": device.more.iter().map(get).collect::<Vec<_>>(),
                        "pages": device.pages.iter().map(|p| json!({
                            "id": p.id, "name": p.name, "icon": p.icon,
                            "items": p.items.iter().map(|i| if i.starts_with("card:") { json!({ "card": &i[5..] }) } else { get(i) }).collect::<Vec<_>>(),
                        })).collect::<Vec<_>>(),
                        "panels": { "indoor": panel(&p.indoor), "outdoor": panel(&p.outdoor), "wind": panel(&p.wind), "weather": panel(&p.weather), "speaker": panel(&p.speaker) },
                    })))
                }
                // Base64 in JSON, not an img URL: a camera must need the screen's sign-in.
                ("GET", ["tablet", "camera"]) => {
                    let id = req.query.get("id").map(String::as_str).unwrap_or("");
                    if !device.allowed().contains(id) {
                        return Err(Response::error(403, "Not configured for this screen"));
                    }
                    let (kind, bytes) = app.registry.snapshot(id).map_err(|e| Response::error(502, &e))?;
                    Ok(Response::ok(json!({ "type": kind, "data": b64_encode(&bytes) })))
                }
                ("POST", ["tablet", "ticket"]) => Ok(Response::ok(json!({ "ticket": app.new_ticket(&device.id) }))),
                ("POST", ["tablet", "search"]) => {
                    let body = req.json()?;
                    let id = body["id"].as_str().unwrap_or("");
                    let query = body["query"].as_str().unwrap_or("").trim();
                    if !device.allowed().contains(id) {
                        return Err(Response::error(403, "Not configured for this screen"));
                    }
                    if query.is_empty() || query.chars().count() > 100 {
                        return Err(Response::error(400, "Search for 1 to 100 characters"));
                    }
                    app.registry.search(id, query).map(|r| Response::ok(json!({ "results": r }))).map_err(|e| Response::error(502, &e))
                }
                ("POST", ["tablet", "set"]) => {
                    let body = req.json()?;
                    let id = body["id"].as_str().unwrap_or("");
                    // A screen controls what is configured for it and nothing else.
                    let allowed = device.allowed();
                    if !allowed.contains(id) {
                        return Err(Response::error(403, "Not configured for this screen"));
                    }
                    let capability = body["capability"].as_str().unwrap_or("");
                    // Removing and arranging favourites changes them for the whole household: admin only.
                    if ADMIN_ONLY.contains(&capability) {
                        return Err(Response::error(403, "Only the admin page can do that"));
                    }
                    let mut value = body["value"].clone();
                    // Grouping names a second speaker: the screen must control that one too,
                    // and it must belong to the same integration.
                    if capability == "speaker_group" {
                        let target = value.as_str().unwrap_or("");
                        let same = matches!((id.split_once(':'), target.split_once(':')), (Some((a, _)), Some((b, _))) if a == b);
                        if !same || !allowed.contains(target) {
                            return Err(Response::error(403, "Not configured for this screen"));
                        }
                        value = json!(target.split_once(':').map_or("", |(_, e)| e));
                    }
                    app.registry
                        .set(id, capability, &value)
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

/// For what would hand over the house: secrets, new admins, removing someone's access.
fn require_fresh(app: &App, req: &Request) -> Result<(), Response> {
    if app.session_is_fresh(req) {
        return Ok(());
    }
    Err(Response::json(403, &json!({ "error": "Confirm with your passkey first.", "reauth": true })))
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
            let invite = body["invite"].as_str().map(str::to_string);
            let (admin_id, name, user_handle) = if let Some(a) = &current {
                require_fresh(app, req)?;
                (Some(a.id.clone()), a.name.clone(), a.user_handle.clone())
            } else if invite.as_deref().is_some_and(|c| app.invite_valid(c, false)) {
                (None, str_field(&body, "name", 64)?.to_string(), b64url_encode(&random_bytes::<16>()))
            } else if invite.is_some() {
                return Err(Response::error(410, "This invitation has expired or was already used. Ask for a new one."));
            } else if needs_setup {
                if !app.check_setup_code(&body["setupCode"].as_str().unwrap_or("").trim().to_ascii_uppercase()) {
                    return Err(Response::error(403, "Wrong setup code. It is printed in the server log at startup."));
                }
                (None, str_field(&body, "name", 64)?.to_string(), b64url_encode(&random_bytes::<16>()))
            } else {
                return Err(Response::error(401, "Sign in first"));
            };
            let invite = if current.is_none() { invite } else { None };
            let challenge = app.new_challenge(Challenge::Register { admin_id, name: name.clone(), user_handle: user_handle.clone(), invite });
            let exclude: Vec<String> = current.iter().flat_map(|a| a.credentials.iter().map(|c| c.id.clone())).collect();
            return Ok(Response::ok(webauthn::registration_options(&rp(app), &challenge, &user_handle, &name, &exclude)));
        }
        ("POST", ["register", "verify"]) => {
            let body = req.json()?;
            let challenge = body["challenge"].as_str().unwrap_or("");
            let Some(Challenge::Register { admin_id, name, user_handle, invite }) = app.take_challenge(challenge) else {
                return Err(Response::error(400, "Challenge expired. Try again."));
            };
            let cred = webauthn::verify_registration(&rp(app), challenge, &body["credential"]).map_err(|e| Response::error(400, &e))?;
            let cred = Credential { id: cred.id, public_key: cred.public_key, counter: cred.counter, created: now(), last_used: now() };
            let mut store = app.store.lock().unwrap();
            let admin_id = match admin_id {
                Some(id) => {
                    let a = store.data.admins.iter_mut().find(|a| a.id == id).ok_or_else(|| Response::error(404, "Admin gone"))?;
                    a.credentials.push(cred);
                    id
                }
                None => {
                    // Joining by invitation, or creating the very first admin.
                    let invited = invite.as_deref().is_some_and(|c| app.invite_valid(c, true));
                    if !invited && !store.data.admins.is_empty() {
                        return Err(Response::error(409, "Setup is already done"));
                    }
                    let id = random_code(10);
                    store.data.admins.push(Admin { id: id.clone(), name, user_handle, credentials: vec![cred] });
                    id
                }
            };
            store.save().map_err(|e| Response::error(500, e))?;
            drop(store);
            // Only once the first admin is on disk, so a failed save leaves setup possible.
            *app.setup_code.lock().unwrap() = None;
            let cookie = app.new_session(&admin_id).map_err(|e| Response::error(500, e))?;
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
            cred.last_used = now();
            store.save().map_err(|e| Response::error(500, e))?;
            drop(store);
            // A confirmation replaces the session it confirms instead of piling up another.
            let _ = app.end_session(req);
            let cookie = app.new_session(&admin_id).map_err(|e| Response::error(500, e))?;
            return Ok(Response::ok(json!({ "ok": true })).header("Set-Cookie", &cookie));
        }
        ("POST", ["logout"]) => {
            let cookie = app.end_session(req).map_err(|e| Response::error(500, e))?;
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
            require_fresh(app, req)?;
            let body = req.json()?;
            let target = match (body["replace"].as_str(), body["follow"].as_str()) {
                (Some(id), None) => PairTarget::Replace(id),
                (None, Some(id)) => PairTarget::Follow(id),
                (None, None) => PairTarget::New,
                _ => return Err(Response::error(400, "Choose one: replace or follow")),
            };
            let replacing = matches!(target, PairTarget::Replace(_));
            let name = if replacing { "" } else { str_field(&body, "name", 64)? };
            let device = app.approve_pairing(code, body["pin"].as_str().unwrap_or(""), name, target).map_err(|e| Response::error(if e == crate::store::SAVE_FAILED { 500 } else { 410 }, e))?;
            if replacing {
                app.end_device_streams(&device.id);
            }
            Ok(Response::ok(json!({ "id": device.id })))
        }
        ("GET", ["devices"]) => {
            let store = app.store.lock().unwrap();
            let list: Vec<Value> = store
                .data
                .devices
                .iter()
                .map(|d| store.data.effective(d))
                .map(|d| json!({ "id": d.id, "name": d.name, "follows": d.follows, "online": app.online(&d.id), "created": d.created, "lastSeen": d.last_seen, "expires": d.expires, "tiles": d.tiles, "panels": d.panels, "more": d.more, "pages": d.pages, "favRows": d.fav_rows.unwrap_or(1), "names": d.names }))
                .collect();
            Ok(Response::ok(json!(list)))
        }
        ("PUT", ["devices", id]) => {
            let body = req.json()?;
            let mut store = app.store.lock().unwrap();
            let following = store.data.devices.iter().find(|d| d.id == *id).ok_or_else(|| Response::error(404, "No such device"))?.follows.is_some();
            let will_follow = body.get("follows").map_or(following, Value::is_string);
            if will_follow && ["tiles", "more", "panels", "names", "favRows", "pages"].iter().any(|k| body.get(k).is_some()) {
                return Err(Response::error(409, "This screen uses another screen's layout. Edit that one."));
            }
            if let Some(f) = body.get("follows") {
                if !f.is_null() && !f.is_string() {
                    return Err(Response::error(400, "Bad follows"));
                }
                store.data.set_follows(id, f.as_str()).map_err(|e| Response::error(409, e))?;
            }
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
            if let Some(names) = body.get("names") {
                let names: std::collections::BTreeMap<String, String> = serde_json::from_value(names.clone()).map_err(|_| Response::error(400, "Bad names"))?;
                let names: std::collections::BTreeMap<String, String> = names.into_iter().map(|(id, n)| (id, n.trim().to_string())).filter(|(_, n)| !n.is_empty()).collect();
                if names.len() > 300 || names.iter().any(|(id, n)| id.len() > 200 || n.chars().count() > 32) {
                    return Err(Response::error(400, "Names: up to 32 characters"));
                }
                d.names = names;
            }
            if let Some(rows) = body.get("favRows") {
                d.fav_rows = Some(rows.as_u64().filter(|r| *r <= 3).ok_or_else(|| Response::error(400, "Favourite rows: 0 to 3"))? as u8);
            }
            if body.get("pages").is_some() {
                let pages: Vec<crate::store::Page> = serde_json::from_value(body["pages"].clone()).map_err(|_| Response::error(400, "Bad pages"))?;
                let bad = pages.len() > 12
                    || pages.iter().any(|p| {
                        p.name.trim().is_empty() || p.name.chars().count() > 32 || p.id.is_empty() || p.id.len() > 16
                            || !crate::store::PAGE_ICONS.contains(&p.icon.as_str())
                            || p.items.len() > 48
                            || p.items.iter().any(|i| i.len() > 200 || (i.starts_with("card:") && !crate::store::PAGE_CARDS.contains(&i.as_str())))
                    });
                if bad {
                    return Err(Response::error(400, "Pages: up to 12, each with a name (max 32 characters), a known icon and up to 48 items"));
                }
                d.pages = pages.into_iter().map(|p| crate::store::Page { name: p.name.trim().to_string(), ..p }).collect();
            }
            let device = d.clone();
            store.save().map_err(|e| Response::error(500, e))?;
            drop(store);
            app.layout_changed(&device.id);
            Ok(Response::ok(json!({ "ok": true })))
        }
        ("POST", ["devices", id, "reload"]) => {
            if !app.store.lock().unwrap().data.devices.iter().any(|d| d.id == *id) {
                return Err(Response::error(404, "No such device"));
            }
            app.notify_device(id, "refresh");
            Ok(Response::ok(json!({ "ok": app.online(id) })))
        }
        ("DELETE", ["devices", id]) => {
            let mut store = app.store.lock().unwrap();
            store.data.remove_device(id);
            store.save().map_err(|e| Response::error(500, e))?;
            drop(store);
            app.notify_device(id, "revoked");
            app.drop_revoked_clients();
            Ok(Response::ok(json!({ "ok": true })))
        }
        ("GET", ["people"]) => {
            let me = current.as_ref().map(|a| a.id.clone()).unwrap_or_default();
            let store = app.store.lock().unwrap();
            let people: Vec<Value> = store.data.admins.iter().map(|a| json!({
                "id": a.id, "name": a.name, "me": a.id == me,
                "passkeys": a.credentials.iter().map(|c| json!({ "id": c.id, "created": c.created, "lastUsed": c.last_used })).collect::<Vec<_>>(),
            })).collect();
            drop(store);
            app.invite_valid("", false); // drops expired ones
            let invites: Vec<u64> = app.invites.lock().unwrap().iter().map(|(_, e)| *e).collect();
            Ok(Response::ok(json!({ "people": people, "invites": invites })))
        }
        ("POST", ["invites"]) => {
            require_fresh(app, req)?;
            let code = random_code(10);
            let expires = now() + 24 * 3600;
            let mut invites = app.invites.lock().unwrap();
            if invites.len() >= 10 {
                return Err(Response::error(429, "Too many open invitations. Cancel some first."));
            }
            invites.push((code.clone(), expires));
            drop(invites);
            let url = format!("{}/admin#/invite/{code}", app.cfg.public_url);
            Ok(Response::ok(json!({ "url": url, "qr": qr_svg(&url), "expires": expires })))
        }
        ("DELETE", ["invites"]) => {
            app.invites.lock().unwrap().clear();
            Ok(Response::ok(json!({ "ok": true })))
        }
        ("DELETE", ["passkeys", cred_id]) => {
            require_fresh(app, req)?;
            let mut store = app.store.lock().unwrap();
            let a = store.data.admins.iter_mut().find(|a| a.credentials.iter().any(|c| c.id == *cred_id)).ok_or_else(|| Response::error(404, "No such passkey"))?;
            if a.credentials.len() == 1 {
                return Err(Response::error(409, &format!("That is {}'s only passkey. Remove the person instead.", a.name)));
            }
            a.credentials.retain(|c| c.id != *cred_id);
            // A lost phone may still be signed in: end that person's other sessions.
            let owner = a.id.clone();
            let mine = req.cookie("oldpad_admin").map(|t| crate::util::sha256_hex(&t));
            store.data.sessions.retain(|s| s.admin_id != owner || mine.as_ref() == Some(&s.hash));
            store.save().map_err(|e| Response::error(500, e))?;
            Ok(Response::ok(json!({ "ok": true })))
        }
        ("DELETE", ["people", id]) => {
            require_fresh(app, req)?;
            let mut store = app.store.lock().unwrap();
            if !store.data.admins.iter().any(|a| a.id == *id) {
                return Err(Response::error(404, "No such person"));
            }
            if store.data.admins.len() == 1 {
                return Err(Response::error(409, "oldpad needs at least one admin."));
            }
            store.data.admins.retain(|a| a.id != *id);
            store.data.sessions.retain(|s| s.admin_id != *id);
            store.save().map_err(|e| Response::error(500, e))?;
            Ok(Response::ok(json!({ "ok": true })))
        }
        ("GET", ["settings"]) => {
            let store = app.store.lock().unwrap();
            Ok(Response::ok(json!({
                "settings": store.data.settings,
                "version": env!("CARGO_PKG_VERSION"),
                "commit": option_env!("OLDPAD_COMMIT").unwrap_or(""),
                "publicUrl": app.cfg.public_url,
                "counts": { "screens": store.data.devices.len(), "integrations": store.data.integrations.len(), "people": store.data.admins.len() },
            })))
        }
        ("PUT", ["settings"]) => {
            let settings: crate::store::Settings = serde_json::from_value(req.json()?).map_err(|_| Response::error(400, "Bad settings"))?;
            if !settings.valid() {
                return Err(Response::error(400, "Bad settings"));
            }
            let mut store = app.store.lock().unwrap();
            store.data.settings = settings;
            store.save().map_err(|e| Response::error(500, e))?;
            drop(store);
            // Language and theme are applied when a screen loads: have them all reload.
            app.notify_all("refresh");
            Ok(Response::ok(json!({ "ok": true })))
        }
        ("GET", ["backup"]) => {
            require_fresh(app, req)?;
            let store = app.store.lock().unwrap();
            let d = &store.data;
            // Everything but sign-in sessions. It holds integration secrets: the admin is told so.
            let backup = json!({ "oldpad": 1, "version": env!("CARGO_PKG_VERSION"), "exported": now(), "admins": d.admins, "devices": d.devices, "integrations": d.integrations, "settings": d.settings });
            Ok(Response::ok(backup).header("Content-Disposition", "attachment; filename=\"oldpad-backup.json\""))
        }
        ("POST", ["restore"]) => {
            require_fresh(app, req)?;
            // shortcut: request bodies are capped at 64 KB; raise http::MAX_BODY if a household outgrows it.
            let b = req.json()?;
            if b["oldpad"] != 1 {
                return Err(Response::error(400, "This is not an oldpad backup"));
            }
            let parse = |k: &str| b[k].clone();
            let admins: Vec<crate::store::Admin> = serde_json::from_value(parse("admins")).map_err(|_| Response::error(400, "Backup: bad admins"))?;
            let devices: Vec<crate::store::Device> = serde_json::from_value(parse("devices")).map_err(|_| Response::error(400, "Backup: bad screens"))?;
            let integrations: Vec<IntegrationConfig> = serde_json::from_value(parse("integrations")).map_err(|_| Response::error(400, "Backup: bad integrations"))?;
            let settings: crate::store::Settings = serde_json::from_value(parse("settings")).unwrap_or_default();
            if integrations.iter().any(|i| app.registry.kind(&i.kind).is_none()) || !settings.valid() {
                return Err(Response::error(400, "Backup: unknown integration type or bad settings"));
            }
            let mut store = app.store.lock().unwrap();
            let old: Vec<String> = store.data.integrations.iter().map(|i| i.id.clone()).collect();
            // People in the backup are added; nobody signed in now is locked out.
            for a in admins {
                if !store.data.admins.iter().any(|x| x.id == a.id) {
                    store.data.admins.push(a);
                }
            }
            store.data.devices = devices;
            store.data.clean_follows();
            store.data.integrations = integrations;
            store.data.settings = settings;
            store.save().map_err(|e| Response::error(500, e))?;
            drop(store);
            for id in old {
                app.registry.stop(&id);
            }
            app.start_integrations();
            app.forget_unused_tokens();
            app.notify_all("refresh");
            app.drop_revoked_clients();
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
                    Some(json!({ "id": i.id, "type": i.kind, "typeName": kind.name, "name": i.name, "config": redact(kind, &i.config), "status": app.registry.status(&i.id), "accounts": app.registry.accounts(&i.id) }))
                })
                .collect::<Vec<_>>())))
        }
        ("POST", ["integrations", "test"]) => {
            let body = req.json()?;
            let (kind, config) = integration_input(app, req, &body, None)?;
            Ok(Response::ok(match (kind.test)(&config) {
                Ok(msg) => json!({ "ok": true, "message": msg }),
                Err(e) => json!({ "ok": false, "message": e }),
            }))
        }
        ("POST", ["integrations", id, "restart"]) => {
            let i = app.store.lock().unwrap().data.integrations.iter().find(|i| i.id == *id).cloned().ok_or_else(|| Response::error(404, "No such integration"))?;
            app.registry.start(i.id.clone(), i.kind, i.config, app.sink(i.id));
            Ok(Response::ok(json!({ "ok": true })))
        }
        ("POST", ["integrations", id, "connect"]) => {
            let body = req.json()?;
            let url = app.registry.connect(id, body["account"].as_str().unwrap_or("")).map_err(|e| Response::error(502, &e))?;
            Ok(Response::ok(json!({ "url": url })))
        }
        ("POST", ["integrations"]) | ("PUT", ["integrations", _]) => {
            let body = req.json()?;
            let path_id = if let [_, id] = s { Some(*id) } else { None };
            if path_id.is_some_and(|id| !app.store.lock().unwrap().data.integrations.iter().any(|i| i.id == id)) {
                return Err(Response::error(404, "No such integration"));
            }
            let (kind, config) = integration_input(app, req, &body, path_id)?;
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
            store.save().map_err(|e| Response::error(500, e))?;
            drop(store);
            app.registry.start(id.clone(), kind.id.into(), config, app.sink(id.clone()));
            app.forget_unused_tokens();
            Ok(Response::ok(json!({ "id": id })))
        }
        ("DELETE", ["integrations", id]) => {
            let mut store = app.store.lock().unwrap();
            store.data.integrations.retain(|i| i.id != *id);
            store.save().map_err(|e| Response::error(500, e))?;
            drop(store);
            app.registry.stop(id);
            app.forget_unused_tokens();
            Ok(Response::ok(json!({ "ok": true })))
        }
        ("GET", ["entities"]) => Ok(Response::ok(json!(app.registry.entities()))),
        ("POST", ["set"]) => {
            let body = req.json()?;
            app.registry
                .set(body["id"].as_str().unwrap_or(""), body["capability"].as_str().unwrap_or(""), &body["value"])
                .map(|_| Response::ok(json!({ "ok": true })))
                .map_err(|e| Response::error(502, &e))
        }
        _ => Err(Response::error(404, "Not found")),
    }
}

/// Resolves {type | id, config} into a validated config, reusing stored
/// secrets when an existing integration is edited.
fn integration_input<'a>(app: &'a App, req: &Request, body: &Value, id: Option<&str>) -> Result<(&'a crate::integrations::Kind, serde_json::Map<String, Value>), Response> {
    let existing = id.or(body["id"].as_str()).and_then(|id| app.store.lock().unwrap().data.integrations.iter().find(|i| i.id == id).cloned());
    let kind_id = existing.as_ref().map(|i| i.kind.clone()).or_else(|| body["type"].as_str().map(str::to_string)).unwrap_or_default();
    let kind = app.registry.kind(&kind_id).ok_or_else(|| Response::error(400, "Unknown integration type"))?;
    let input = body["config"].as_object().cloned().unwrap_or_default();
    let config = validate(kind, &input, existing.as_ref().map(|i| &i.config)).map_err(|e| Response::error(400, &e))?;
    // Pointing a stored key at a new address would send the key there.
    if let Some(old) = &existing {
        let keeps_secret = kind.fields.iter().any(|f| f.kind == FieldKind::Secret && config.get(f.key).is_some() && config.get(f.key) == old.config.get(f.key));
        let moved = kind.fields.iter().any(|f| f.kind == FieldKind::Url && config.get(f.key) != old.config.get(f.key));
        if keeps_secret && moved {
            require_fresh(app, req)?;
        }
    }
    Ok((kind, config))
}
