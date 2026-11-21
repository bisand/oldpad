mod api;
mod app;
mod cbor;
mod client;
mod http;
mod integrations;
mod store;
mod util;
mod webauthn;
#[cfg(test)]
mod e2e;

use std::collections::HashMap;
use std::net::{IpAddr, TcpListener};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Each connection gets a thread. Old hardware and a household of tablets
/// never need more; the cap stops a misbehaving client from exhausting it.
const MAX_CONNECTIONS: usize = 128;
/// Requests one address may be sending at once. Open event streams do not count,
/// so several tablets behind one proxy or NAT address are fine.
const MAX_READING_PER_PEER: usize = 8;

fn main() {
    let cfg = match app::Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("oldpad: {e}");
            std::process::exit(2);
        }
    };
    let store = store::Store::open(&cfg.data_dir).unwrap_or_else(|e| {
        eprintln!("oldpad: cannot open data directory: {e}");
        std::process::exit(2);
    });
    let listener = TcpListener::bind(&cfg.bind).unwrap_or_else(|e| {
        eprintln!("oldpad: cannot listen on {}: {e}", cfg.bind);
        std::process::exit(2);
    });
    let admin_listener = cfg.admin_bind.as_ref().map(|bind| {
        TcpListener::bind(bind).unwrap_or_else(|e| {
            eprintln!("oldpad: cannot listen on {bind}: {e}");
            std::process::exit(2);
        })
    });
    let app = app::App::new(cfg, store);

    eprintln!("oldpad {} listening on {}", env!("CARGO_PKG_VERSION"), app.cfg.bind);
    eprintln!("  tablets: open {}/ on the tablet", app.cfg.public_url);
    eprintln!("  admin:   {}/admin", app.cfg.public_url);
    if let Some(code) = app.setup_code.lock().unwrap().as_deref() {
        eprintln!("  first-time setup code: {code}  (enter it at {}/admin to register your passkey)", app.cfg.public_url);
    }
    if let Some(bind) = &app.cfg.admin_bind {
        eprintln!("  admin port: {bind}");
    }
    app.start_integrations();

    if let Some(admin) = admin_listener {
        let app = app.clone();
        std::thread::spawn(move || serve(app, admin, true));
    }
    serve(app, listener, false);
}

/// Accepts connections until the listener fails. Each one gets a thread.
/// Each listener has its own limits, so a flood on the admin port cannot stop the tablets.
pub fn serve(app: Arc<app::App>, listener: TcpListener, admin_port: bool) {
    let active = Arc::new(AtomicUsize::new(0));
    let reading: Arc<Mutex<HashMap<IpAddr, usize>>> = Default::default();
    for conn in listener.incoming() {
        let Ok(mut stream) = conn else { continue };
        if active.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
            continue; // dropping the stream closes it
        }
        let Ok(ip) = stream.peer_addr().map(|a| a.ip()) else { continue };
        {
            let mut reading = reading.lock().unwrap();
            let n = reading.entry(ip).or_insert(0);
            if *n >= MAX_READING_PER_PEER {
                continue;
            }
            *n += 1;
        }
        active.fetch_add(1, Ordering::Relaxed);
        let (app, active, reading) = (app.clone(), active.clone(), reading.clone());
        std::thread::spawn(move || {
            stream.set_read_timeout(Some(Duration::from_secs(15))).ok();
            stream.set_write_timeout(Some(Duration::from_secs(15))).ok();
            let parsed = http::read_request(&stream, ip.to_string());
            {
                let mut reading = reading.lock().unwrap();
                if let Some(n) = reading.get_mut(&ip) {
                    *n -= 1;
                    if *n == 0 {
                        reading.remove(&ip);
                    }
                }
            }
            let res = match parsed {
                Ok(mut req) => {
                    req.admin_port = admin_port;
                    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| api::handle(&app, &req)))
                        .unwrap_or_else(|_| http::Response::error(500, "Internal error"));
                    if res.status >= 500 || util::verbose() {
                        eprintln!("{} {} {} -> {}", req.peer, req.method, req.path, res.status);
                    }
                    res
                }
                Err(res) => res,
            };
            let _ = http::write_response(&mut stream, res);
            active.fetch_sub(1, Ordering::Relaxed);
        });
    }
}
