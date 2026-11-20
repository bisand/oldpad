use crate::http::Request;
use crate::integrations::{Event, Registry, Sink};
use crate::store::{Admin, Device, Store};
use crate::util::{constant_eq, now, random_code, random_token, sha256_hex};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, Weak};

pub struct Config {
    pub bind: String,
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
const TOUCH_INTERVAL: u64 = 3600;

pub struct Pairing {
    pub id: String,
    pub secret: String,
    pub code: String,
    pub expires: u64,
    /// Set once an admin approves; handed to the tablet exactly once.
    pub token: Option<String>,
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

    pub fn add_client(&self, device: &Device, tx: Sender<String>) {
        let mut clients = self.clients.lock().unwrap();
        // shortcut: a fixed cap per device; raise it if one tablet ever needs several streams.
        let open = clients.iter().filter(|c| c.device_id == device.id).count();
        if open >= 3 {
            clients.retain(|c| c.device_id != device.id);
        }
        clients.push(Client { device_id: device.id.clone(), tiles: device.allowed(), tx });
    }

    pub fn update_client_tiles(&self, device: &Device) {
        for c in self.clients.lock().unwrap().iter_mut().filter(|c| c.device_id == device.id) {
            c.tiles = device.allowed();
        }
    }

    // ---------- admin sessions ----------

    pub fn admin_from(&self, req: &Request) -> Option<Admin> {
        let hash = sha256_hex(&req.cookie("oldpad_admin")?);
        let store = self.store.lock().unwrap();
        let session = store.data.sessions.iter().find(|s| constant_eq(&s.hash, &hash) && s.expires > now())?;
        store.data.admins.iter().find(|a| a.id == session.admin_id).cloned()
    }

    /// Creates a session and returns the Set-Cookie value.
    pub fn new_session(&self, admin_id: &str) -> Result<String, &'static str> {
        let token = random_token();
        let mut store = self.store.lock().unwrap();
        let t = now();
        store.data.sessions.retain(|s| s.expires > t);
        store.data.sessions.push(crate::store::Session { hash: sha256_hex(&token), admin_id: admin_id.into(), expires: t + self.cfg.admin_ttl });
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
        if t - device.last_seen > TOUCH_INTERVAL {
            device.last_seen = t;
            device.expires = t + self.cfg.device_ttl;
            let d = device.clone();
            // Bookkeeping, not the tablet's change: a failure is logged and the tablet carries on.
            let _ = store.save();
            return Some(d);
        }
        Some(device.clone())
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
        self.store.lock().unwrap().data.devices.iter().find(|d| d.id == device_id && d.expires > now()).cloned()
    }

    // ---------- pairing ----------

    pub fn new_pairing(&self) -> Result<Pairing, &'static str> {
        let mut list = self.pairings.lock().unwrap();
        let t = now();
        list.retain(|p| p.expires > t);
        if list.iter().filter(|p| p.token.is_none()).count() >= MAX_PENDING_PAIRINGS {
            return Err("Too many pairings in progress. Try again in a few minutes.");
        }
        let p = Pairing { id: random_token(), secret: random_token(), code: random_code(8), expires: t + PAIRING_TTL, token: None };
        list.push(Pairing { id: p.id.clone(), secret: p.secret.clone(), code: p.code.clone(), expires: p.expires, token: None });
        Ok(p)
    }

    /// Admin approval: creates the device and parks its token for the tablet to collect.
    pub fn approve_pairing(&self, code: &str, name: &str) -> Result<Device, &'static str> {
        let mut list = self.pairings.lock().unwrap();
        let p = list
            .iter_mut()
            .find(|p| constant_eq(&p.code, &code.to_ascii_uppercase()) && p.expires > now())
            .ok_or("This code has expired or does not exist. Start pairing again on the tablet.")?;
        if p.token.is_some() {
            return Err("This tablet is already approved.");
        }
        let token = random_token();
        let t = now();
        let device = Device {
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
        };
        let mut store = self.store.lock().unwrap();
        store.data.devices.push(device.clone());
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
        }
    }

    #[test]
    fn pairing_hands_out_the_token_once() {
        let app = test_app();
        let p = app.new_pairing().unwrap();
        assert_eq!(app.poll_pairing(&p.id, &p.secret)["status"], "pending");
        assert_eq!(app.poll_pairing(&p.id, "wrong")["status"], "expired");
        app.approve_pairing(&p.code.to_lowercase(), "Kitchen").unwrap();
        assert!(app.approve_pairing(&p.code, "Again").is_err());

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
    fn pending_pairings_are_capped() {
        let app = test_app();
        for _ in 0..MAX_PENDING_PAIRINGS {
            app.new_pairing().unwrap();
        }
        assert!(app.new_pairing().is_err());
    }

    #[test]
    fn challenges_are_single_use() {
        let app = test_app();
        let c = app.new_challenge(Challenge::Login);
        assert!(app.take_challenge(&c).is_some());
        assert!(app.take_challenge(&c).is_none());
    }
}
