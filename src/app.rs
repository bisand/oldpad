use crate::http::Request;
use crate::integrations::{Event, Registry, Sink};
use crate::store::{Admin, Device, Store};
use crate::util::{constant_eq, now, random_code, random_pin, random_token, sha256_hex};
use std::sync::atomic::{AtomicUsize, Ordering};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, Weak};

pub struct Config {
    pub bind: String,
    /// A second port for the admin only, the one to expose. Unset: one port serves everything.
    pub admin_bind: Option<String>,
    pub data_dir: PathBuf,
    /// The address the admin's phone uses. Passkeys are bound to it.
    pub public_url: String,
    pub rp_id: String,
    pub device_ttl: u64,
    pub admin_ttl: u64,
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        let var = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
        let public_url = var("OLDPAD_PUBLIC_URL", "http://localhost:8080").trim_end_matches('/').to_string();
        let host = public_url
            .split_once("://")
            .map(|(_, rest)| rest.split(['/', ':']).next().unwrap_or(""))
            .filter(|h| !h.is_empty())
            .ok_or("OLDPAD_PUBLIC_URL must look like https://dash.example.com")?;
        if !public_url.starts_with("https://") && host != "localhost" {
            return Err("OLDPAD_PUBLIC_URL must be https:// (passkeys need a secure origin); only localhost may use http://".into());
        }
        let days = |k: &str, d: &str| var(k, d).parse::<u64>().map(|n| n * 86_400).map_err(|_| format!("{k} must be a number of days"));
        Ok(Config {
            bind: var("OLDPAD_BIND", "0.0.0.0:8080"),
            admin_bind: std::env::var("OLDPAD_ADMIN_BIND").ok().filter(|b| !b.trim().is_empty()),
            data_dir: PathBuf::from(var("OLDPAD_DATA_DIR", "data")),
            rp_id: host.to_string(),
            public_url,
            device_ttl: days("OLDPAD_DEVICE_TTL_DAYS", "90")?,
            admin_ttl: days("OLDPAD_ADMIN_TTL_DAYS", "30")?,
        })
    }

    pub fn secure_cookies(&self) -> bool {
        self.public_url.starts_with("https://")
    }
}

pub const PAIRING_TTL: u64 = 300;
const MAX_PENDING_PAIRINGS: usize = 10;
const MAX_PIN_MISSES: u8 = 3;
const TOUCH_INTERVAL: u64 = 3600;
const MAX_CHALLENGES: usize = 1000;
/// Wrong setup codes before the code is replaced (the new one is in the server log).
const MAX_SETUP_ATTEMPTS: usize = 5;
/// How recently a passkey must have been used for backups, new admins and the like.
const FRESH_AUTH: u64 = 300;

pub struct Pairing {
    pub id: String,
    pub secret: String,
    pub code: String,
    /// Shown on the screen but not in its link, so a forwarded link alone cannot be approved.
    pub pin: String,
    pub misses: u8,
    pub expires: u64,
    /// Set once an admin approves; handed to the tablet exactly once.
    pub token: Option<String>,
}

#[derive(Clone, Copy)]
pub enum PairTarget<'a> {
    New,
    Replace(&'a str),
    Follow(&'a str),
}

pub enum Challenge {
    /// `invite` is set when someone joins through an admin's invitation.
    Register { admin_id: Option<String>, name: String, user_handle: String, invite: Option<String> },
    Login,
}

struct Client {
    device_id: String,
    tiles: HashSet<String>,
    tx: Sender<String>,
}

pub struct App {
    pub cfg: Config,
    pub store: Mutex<Store>,
    pub registry: Arc<Registry>,
    pub pairings: Mutex<Vec<Pairing>>,
    pub challenges: Mutex<HashMap<String, (Challenge, u64)>>,
    tickets: Mutex<HashMap<String, (String, u64)>>,
    clients: Mutex<Vec<Client>>,
    /// Printed at startup while no admin exists; needed to register the first passkey.
    pub setup_code: Mutex<Option<String>>,
    setup_failures: AtomicUsize,
    /// Invitations for another admin: (code, expires). One use each.
    pub invites: Mutex<Vec<(String, u64)>>,
}

impl App {
    pub fn new(cfg: Config, store: Store) -> Arc<App> {
        let setup_code = store.data.admins.is_empty().then(|| random_code(8));
        Arc::new(App {
            cfg,
            store: Mutex::new(store),
            registry: Arc::new(Registry::new()),
            pairings: Mutex::new(Vec::new()),
            challenges: Mutex::new(HashMap::new()),
            tickets: Mutex::new(HashMap::new()),
            clients: Mutex::new(Vec::new()),
            setup_code: Mutex::new(setup_code),
            setup_failures: AtomicUsize::new(0),
            invites: Mutex::new(Vec::new()),
        })
    }

    pub fn start_integrations(self: &Arc<Self>) {
        let list = self.store.lock().unwrap().data.integrations.clone();
        for i in list {
            self.registry.start(i.id.clone(), i.kind, i.config, self.sink(i.id));
        }
    }

    /// Routes an integration's events to the tablets that show the entity.
    pub fn sink(self: &Arc<Self>, integration_id: String) -> Sink {
        let app: Weak<App> = Arc::downgrade(self);
        Arc::new(move |event| {
            let Some(app) = app.upgrade() else { return };
            match event {
                Event::Changed { entity, capability, value } => {
                    let id = format!("{integration_id}:{entity}");
                    let msg = sse("entity", &json!({ "id": id, "capability": capability, "value": value }));
                    app.send_where(|c| c.tiles.contains(&id), &msg);
                }
                Event::Reloaded => app.send_where(|_| true, &sse("reload", &json!({}))),
                Event::Remember { key, value } => {
                    if !key.starts_with('_') {
                        return;
                    }
                    let mut store = app.store.lock().unwrap();
                    if let Some(i) = store.data.integrations.iter_mut().find(|i| i.id == integration_id) {
                        i.config.insert(key, value);
                        if let Err(e) = store.save() {
                            eprintln!("integration {integration_id}: could not save: {e}");
                        }
                    }
                }
            }
        })
    }

    fn send_where(&self, pred: impl Fn(&Client) -> bool, msg: &str) {
        // A failed send means the stream is gone; drop the client.
        self.clients.lock().unwrap().retain(|c| !pred(c) || c.tx.send(msg.to_string()).is_ok());
    }

    /// Whether the screen has its live connection open right now.
    pub fn online(&self, device_id: &str) -> bool {
        self.clients.lock().unwrap().iter().any(|c| c.device_id == device_id)
    }

    pub fn notify_all(&self, event: &str) {
        self.send_where(|_| true, &sse(event, &json!({})));
    }

    pub fn notify_device(&self, device_id: &str, event: &str) {
        self.send_where(|c| c.device_id == device_id, &sse(event, &json!({})));
    }

    /// Cuts off whoever held this screen's previous token: open streams and unused tickets.
    pub fn end_device_streams(&self, device_id: &str) {
        self.notify_device(device_id, "revoked");
        self.clients.lock().unwrap().retain(|c| c.device_id != device_id);
        self.tickets.lock().unwrap().retain(|_, (id, _)| id != device_id);
    }

    /// Closes the streams of screens that no longer exist.
    pub fn drop_revoked_clients(&self) {
        let ids: HashSet<String> = self.store.lock().unwrap().data.devices.iter().map(|d| d.id.clone()).collect();
        self.clients.lock().unwrap().retain(|c| ids.contains(&c.device_id));
    }

    pub fn add_client(&self, device: &Device, tx: Sender<String>) {
        let mut clients = self.clients.lock().unwrap();
        // shortcut: a fixed cap per device; raise it if one tablet ever needs several streams.
        let open = clients.iter().filter(|c| c.device_id == device.id).count();
        if open >= 3 {
            clients.retain(|c| c.device_id != device.id);
        }
        clients.push(Client { device_id: device.id.clone(), tiles: device.allowed(), tx });
    }

    /// Tells a screen, and every screen sharing its layout, to show the new one.
    pub fn layout_changed(&self, id: &str) {
        let group: Vec<Device> = {
            let store = self.store.lock().unwrap();
            store.data.devices.iter().filter(|d| d.id == id || d.follows.as_deref() == Some(id)).map(|d| store.data.effective(d)).collect()
        };
        for d in &group {
            self.update_client_tiles(d);
            self.notify_device(&d.id, "reload");
        }
    }

    pub fn update_client_tiles(&self, device: &Device) {
        for c in self.clients.lock().unwrap().iter_mut().filter(|c| c.device_id == device.id) {
            c.tiles = device.allowed();
        }
    }

    // ---------- admin sessions ----------

    /// With a separate admin port, the plain-HTTP tablet port never accepts an admin session.
    pub fn admin_from(&self, req: &Request) -> Option<Admin> {
        if self.cfg.admin_bind.is_some() && !req.admin_port {
            return None;
        }
        let hash = sha256_hex(&req.cookie("oldpad_admin")?);
        let store = self.store.lock().unwrap();
        let session = store.data.sessions.iter().find(|s| constant_eq(&s.hash, &hash) && s.expires > now())?;
        store.data.admins.iter().find(|a| a.id == session.admin_id).cloned()
    }

    /// Whether this session used a passkey within the last few minutes.
    pub fn session_is_fresh(&self, req: &Request) -> bool {
        let Some(token) = req.cookie("oldpad_admin") else { return false };
        let hash = sha256_hex(&token);
        let t = now();
        self.store.lock().unwrap().data.sessions.iter().any(|s| constant_eq(&s.hash, &hash) && s.expires > t && s.verified + FRESH_AUTH > t)
    }

    /// Fails closed when there is no code. Repeated misses replace it, so it cannot be guessed.
    pub fn check_setup_code(&self, given: &str) -> bool {
        let mut code = self.setup_code.lock().unwrap();
        let Some(current) = code.as_deref() else { return false };
        if constant_eq(current, given) {
            self.setup_failures.store(0, Ordering::Relaxed);
            return true;
        }
        if self.setup_failures.fetch_add(1, Ordering::Relaxed) + 1 >= MAX_SETUP_ATTEMPTS {
            self.setup_failures.store(0, Ordering::Relaxed);
            let fresh = random_code(8);
            eprintln!("  too many wrong setup codes. New setup code: {fresh}");
            *code = Some(fresh);
        }
        false
    }

    /// Creates a session and returns the Set-Cookie value.
    pub fn new_session(&self, admin_id: &str) -> Result<String, &'static str> {
        let token = random_token();
        let mut store = self.store.lock().unwrap();
        let t = now();
        store.data.sessions.retain(|s| s.expires > t);
        store.data.sessions.push(crate::store::Session { hash: sha256_hex(&token), admin_id: admin_id.into(), expires: t + self.cfg.admin_ttl, verified: t });
        store.save()?;
        Ok(self.cookie(&token, self.cfg.admin_ttl))
    }

    /// Whether `code` is a live invitation; `take` also uses it up.
    pub fn invite_valid(&self, code: &str, take: bool) -> bool {
        let mut invites = self.invites.lock().unwrap();
        let t = now();
        invites.retain(|(_, exp)| *exp > t);
        let found = invites.iter().position(|(c, _)| !code.is_empty() && constant_eq(c, &code.trim().to_ascii_uppercase()));
        if let (Some(i), true) = (found, take) {
            invites.remove(i);
        }
        found.is_some()
    }

    pub fn end_session(&self, req: &Request) -> Result<String, &'static str> {
        if let Some(token) = req.cookie("oldpad_admin") {
            let hash = sha256_hex(&token);
            let mut store = self.store.lock().unwrap();
            store.data.sessions.retain(|s| s.hash != hash);
            store.save()?;
        }
        Ok(self.cookie("", 0))
    }

    fn cookie(&self, value: &str, max_age: u64) -> String {
        let secure = if self.cfg.secure_cookies() { "; Secure" } else { "" };
        format!("oldpad_admin={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}")
    }

    // ---------- device tokens ----------

    pub fn device_from(&self, req: &Request) -> Option<Device> {
        let hash = sha256_hex(req.bearer()?);
        let mut store = self.store.lock().unwrap();
        let t = now();
        let device = store.data.devices.iter_mut().find(|d| constant_eq(&d.token_hash, &hash) && d.expires > t)?;
        // Sliding expiry: a tablet in daily use never expires. Saved at most hourly.
        let touch = t - device.last_seen > TOUCH_INTERVAL;
        if touch {
            device.last_seen = t;
            device.expires = t + self.cfg.device_ttl;
        }
        let d = device.clone();
        if touch {
            // Bookkeeping, not the tablet's change: a failure is logged and the tablet carries on.
            let _ = store.save();
        }
        Some(store.data.effective(&d))
    }

    /// What a screen puts on image URLs, since img tags cannot send its token.
    /// Derived from the device, so it dies with the pairing and proves nothing else.
    pub fn image_token(device: &Device) -> String {
        sha256_hex(&format!("image:{}", device.token_hash))
    }

    pub fn can_fetch_image(&self, req: &Request) -> bool {
        if self.admin_from(req).is_some() {
            return true;
        }
        let Some(given) = req.query.get("t").filter(|_| !req.admin_port) else { return false };
        let t = now();
        self.store.lock().unwrap().data.devices.iter().any(|d| d.expires > t && constant_eq(&Self::image_token(d), given))
    }

    pub fn new_ticket(&self, device_id: &str) -> String {
        let ticket = random_token();
        let mut tickets = self.tickets.lock().unwrap();
        let t = now();
        tickets.retain(|_, (_, exp)| *exp > t);
        tickets.insert(ticket.clone(), (device_id.into(), t + 60));
        ticket
    }

    /// Event streams cannot send headers, so they authenticate with a
    /// one-time ticket instead of putting the long-lived token in a URL.
    pub fn redeem_ticket(&self, ticket: &str) -> Option<Device> {
        let (device_id, exp) = self.tickets.lock().unwrap().remove(ticket)?;
        if exp <= now() {
            return None;
        }
        let store = self.store.lock().unwrap();
        store.data.devices.iter().find(|d| d.id == device_id && d.expires > now()).map(|d| store.data.effective(d))
    }

    // ---------- pairing ----------

    pub fn new_pairing(&self) -> Result<Pairing, &'static str> {
        let mut list = self.pairings.lock().unwrap();
        let t = now();
        list.retain(|p| p.expires > t);
        // Anyone can start one: the oldest gives way, so strangers cannot hold every slot.
        if list.iter().filter(|p| p.token.is_none()).count() >= MAX_PENDING_PAIRINGS
            && let Some(oldest) = list.iter().enumerate().filter(|(_, p)| p.token.is_none()).min_by_key(|(_, p)| p.expires).map(|(i, _)| i) {
                list.remove(oldest);
            }
        let p = Pairing { id: random_token(), secret: random_token(), code: random_code(8), pin: random_pin(), misses: 0, expires: t + PAIRING_TTL, token: None };
        list.push(Pairing { id: p.id.clone(), secret: p.secret.clone(), code: p.code.clone(), pin: p.pin.clone(), misses: 0, expires: p.expires, token: None });
        Ok(p)
    }

    /// Admin approval: creates the device and parks its token for the tablet to collect.
    /// `Replace` takes over a screen (same layout, new token); `Follow` adds one that shows another's layout.
    pub fn approve_pairing(&self, code: &str, pin: &str, name: &str, target: PairTarget) -> Result<Device, &'static str> {
        let mut list = self.pairings.lock().unwrap();
        let i = list
            .iter()
            .position(|p| constant_eq(&p.code, &code.to_ascii_uppercase()) && p.expires > now())
            .ok_or("This code has expired or does not exist. Start pairing again on the tablet.")?;
        if list[i].token.is_some() {
            return Err("This tablet is already approved.");
        }
        if !constant_eq(&list[i].pin, pin.trim()) {
            list[i].misses += 1;
            if list[i].misses >= MAX_PIN_MISSES {
                list.remove(i);
                return Err("Wrong number too many times. Start pairing again on the tablet.");
            }
            return Err("That number does not match the screen.");
        }
        let p = &mut list[i];
        let token = random_token();
        let t = now();
        let mut store = self.store.lock().unwrap();
        if let PairTarget::Follow(id) = target
            && !store.data.devices.iter().any(|d| d.id == id && d.follows.is_none()) {
                return Err("That layout no longer exists.");
            }
        let device = match target {
            PairTarget::Replace(id) => {
                let d = store.data.devices.iter_mut().find(|d| d.id == id).ok_or("That screen no longer exists.")?;
                d.token_hash = sha256_hex(&token);
                d.last_seen = t;
                d.expires = t + self.cfg.device_ttl;
                d.clone()
            }
            PairTarget::New | PairTarget::Follow(_) => {
                let d = Device {
                    id: random_code(10),
                    name: name.into(),
                    token_hash: sha256_hex(&token),
                    created: t,
                    last_seen: t,
                    expires: t + self.cfg.device_ttl,
                    tiles: Vec::new(),
                    panels: Default::default(),
                    more: Vec::new(),
                    pages: Vec::new(),
                    fav_rows: None,
                    names: Default::default(),
                    follows: if let PairTarget::Follow(id) = target { Some(id.to_string()) } else { None },
                };
                store.data.devices.push(d.clone());
                d
            }
        };
        store.save()?;
        p.token = Some(token);
        Ok(device)
    }

    /// The tablet polls this. Returns the token once, then forgets the pairing.
    pub fn poll_pairing(&self, id: &str, secret: &str) -> Value {
        let mut list = self.pairings.lock().unwrap();
        let Some(i) = list.iter().position(|p| constant_eq(&p.id, id) && constant_eq(&p.secret, secret)) else {
            return json!({ "status": "expired" });
        };
        if list[i].expires <= now() {
            list.remove(i);
            return json!({ "status": "expired" });
        }
        match list[i].token.is_some() {
            true => json!({ "status": "approved", "token": list.remove(i).token }),
            false => json!({ "status": "pending" }),
        }
    }

    // ---------- passkey challenges ----------

    pub fn new_challenge(&self, c: Challenge) -> String {
        let id = random_token();
        let mut map = self.challenges.lock().unwrap();
        let t = now();
        map.retain(|_, (_, exp)| *exp > t);
        // Anyone can ask for a login challenge: the oldest give way rather than memory growing.
        while map.len() >= MAX_CHALLENGES {
            let Some(oldest) = map.iter().min_by_key(|(_, (_, exp))| *exp).map(|(k, _)| k.clone()) else { break };
            map.remove(&oldest);
        }
        map.insert(id.clone(), (c, t + 180));
        id
    }

    pub fn take_challenge(&self, id: &str) -> Option<Challenge> {
        let (c, exp) = self.challenges.lock().unwrap().remove(id)?;
        (exp > now()).then_some(c)
    }
}

pub fn sse(event: &str, data: &Value) -> String {
    format!("event: {event}\ndata: {data}\n\n")
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn test_app() -> Arc<App> {
        let dir = std::env::temp_dir().join(format!("oldpad-app-{}", random_code(8)));
        let cfg = Config {
            bind: "127.0.0.1:0".into(),
            admin_bind: None,
            data_dir: dir.clone(),
            public_url: "http://localhost:8080".into(),
            rp_id: "localhost".into(),
            device_ttl: 90 * 86_400,
            admin_ttl: 86_400,
        };
        App::new(cfg, Store::open(&dir).unwrap())
    }

    fn bearer(token: &str) -> Request {
        Request {
            method: "GET".into(),
            path: "/".into(),
            query: Default::default(),
            headers: HashMap::from([("authorization".into(), format!("Bearer {token}"))]),
            body: vec![],
            peer: "t".into(),
            admin_port: false,
        }
    }

    #[test]
    fn pairing_hands_out_the_token_once() {
        let app = test_app();
        let p = app.new_pairing().unwrap();
        assert_eq!(app.poll_pairing(&p.id, &p.secret)["status"], "pending");
        assert_eq!(app.poll_pairing(&p.id, "wrong")["status"], "expired");
        app.approve_pairing(&p.code.to_lowercase(), &p.pin, "Kitchen", PairTarget::New).unwrap();
        assert!(app.approve_pairing(&p.code, &p.pin, "Again", PairTarget::New).is_err());

        let first = app.poll_pairing(&p.id, &p.secret);
        let token = first["token"].as_str().unwrap().to_string();
        assert_eq!(app.poll_pairing(&p.id, &p.secret)["status"], "expired");

        let device = app.device_from(&bearer(&token)).unwrap();
        assert_eq!(device.name, "Kitchen");
        assert!(app.device_from(&bearer("nope")).is_none());

        let ticket = app.new_ticket(&device.id);
        assert!(app.redeem_ticket(&ticket).is_some());
        assert!(app.redeem_ticket(&ticket).is_none(), "tickets are single use");
    }

    #[test]
    fn pending_pairings_are_capped_without_locking_anyone_out() {
        let app = test_app();
        let first = app.new_pairing().unwrap();
        for _ in 0..MAX_PENDING_PAIRINGS {
            app.new_pairing().unwrap();
        }
        let list = app.pairings.lock().unwrap();
        assert_eq!(list.len(), MAX_PENDING_PAIRINGS);
        assert!(list.iter().all(|p| p.id != first.id), "the oldest gave way");
    }

    #[test]
    fn replacing_a_screen_keeps_its_layout_and_kills_the_old_token() {
        let app = test_app();
        let p = app.new_pairing().unwrap();
        let old = app.approve_pairing(&p.code, &p.pin, "Kitchen", PairTarget::New).unwrap();
        let old_token = app.poll_pairing(&p.id, &p.secret)["token"].as_str().unwrap().to_string();
        app.store.lock().unwrap().data.devices[0].tiles = vec!["x:lamp".into()];
        let (tx, rx) = std::sync::mpsc::channel();
        app.add_client(&old, tx);
        let ticket = app.new_ticket(&old.id);

        let p = app.new_pairing().unwrap();
        assert!(app.approve_pairing(&p.code, &p.pin, "", PairTarget::Replace("nope")).is_err());
        let new = app.approve_pairing(&p.code, &p.pin, "", PairTarget::Replace(&old.id)).unwrap();
        app.end_device_streams(&new.id);
        let new_token = app.poll_pairing(&p.id, &p.secret)["token"].as_str().unwrap().to_string();

        assert_eq!((new.id.as_str(), new.name.as_str(), new.tiles.len()), (old.id.as_str(), "Kitchen", 1));
        assert_eq!(app.store.lock().unwrap().data.devices.len(), 1);
        assert!(app.device_from(&bearer(&old_token)).is_none());
        assert!(app.device_from(&bearer(&new_token)).is_some());
        assert!(app.redeem_ticket(&ticket).is_none());
        assert!(rx.recv().unwrap().starts_with("event: revoked"));
        assert!(rx.recv().is_err(), "the old stream is closed");
    }

    #[test]
    fn a_link_alone_cannot_approve_a_screen() {
        let app = test_app();
        let p = app.new_pairing().unwrap();
        let wrong = if p.pin == "000000" { "000001" } else { "000000" };
        assert!(app.approve_pairing(&p.code, wrong, "T", PairTarget::New).is_err());
        assert!(app.approve_pairing(&p.code, "", "T", PairTarget::New).is_err());
        assert!(app.approve_pairing(&p.code, wrong, "T", PairTarget::New).is_err());
        assert!(app.approve_pairing(&p.code, &p.pin, "T", PairTarget::New).is_err(), "three misses end the pairing");
        assert!(app.store.lock().unwrap().data.devices.is_empty());
    }

    #[test]
    fn setup_code_fails_closed_and_rotates() {
        let app = test_app();
        let first = app.setup_code.lock().unwrap().clone().unwrap();
        for _ in 0..MAX_SETUP_ATTEMPTS {
            assert!(!app.check_setup_code("WRONG"));
        }
        assert!(!app.check_setup_code(&first), "replaced after too many misses");
        *app.setup_code.lock().unwrap() = None;
        assert!(!app.check_setup_code(""), "no code means no setup");
    }

    #[test]
    fn revoked_screen_loses_its_stream() {
        let app = test_app();
        let p = app.new_pairing().unwrap();
        let d = app.approve_pairing(&p.code, &p.pin, "T", PairTarget::New).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        app.add_client(&d, tx);
        app.store.lock().unwrap().data.devices.clear();
        app.drop_revoked_clients();
        assert!(matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)));
    }

    #[test]
    fn stale_session_must_confirm_before_a_backup() {
        let app = test_app();
        let cookie = app.new_session("a").unwrap();
        let pair = cookie.split(';').next().unwrap().to_string();
        app.store.lock().unwrap().data.admins.push(Admin { id: "a".into(), name: "A".into(), user_handle: "u".into(), credentials: vec![] });
        let mut req = bearer("x");
        req.path = "/api/admin/backup".into();
        req.headers.insert("cookie".into(), pair);
        assert_eq!(crate::api::handle(&app, &req).status, 200);
        app.store.lock().unwrap().data.sessions[0].verified = now() - FRESH_AUTH;
        assert_eq!(crate::api::handle(&app, &req).status, 403);
    }

    #[test]
    fn challenges_are_capped() {
        let app = test_app();
        for _ in 0..MAX_CHALLENGES + 50 {
            app.new_challenge(Challenge::Login);
        }
        assert_eq!(app.challenges.lock().unwrap().len(), MAX_CHALLENGES);
    }

    #[test]
    fn image_token_needs_a_live_screen_and_the_lan_port() {
        let app = test_app();
        let p = app.new_pairing().unwrap();
        let d = app.approve_pairing(&p.code, &p.pin, "T", PairTarget::New).unwrap();
        let mut req = bearer("x");
        assert!(!app.can_fetch_image(&req));
        req.query.insert("t".into(), App::image_token(&d));
        assert!(app.can_fetch_image(&req));
        req.admin_port = true;
        assert!(!app.can_fetch_image(&req));
        req.admin_port = false;
        app.store.lock().unwrap().data.devices.clear();
        assert!(!app.can_fetch_image(&req));
    }

    #[test]
    fn challenges_are_single_use() {
        let app = test_app();
        let c = app.new_challenge(Challenge::Login);
        assert!(app.take_challenge(&c).is_some());
        assert!(app.take_challenge(&c).is_none());
    }
}
