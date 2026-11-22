//! A screen for e-ink readers: oldpad draws a grayscale PNG, the reader shows it
//! and sends back where it was tapped. Sized for a Paperwhite 3 held upright.

use crate::app::App;
use crate::integrations::Entity;
use crate::http::{Request, Response};
use crate::store::{Device, Page};
use crate::http::parse_query;
use crate::util::{constant_eq, escape_xml, now, sha256_hex};
use serde_json::{json, Value};

const W: usize = 1072;
const H: usize = 1448;
const MARGIN: usize = 32;
const GAP: usize = 24;
const GRID_TOP: usize = 340;
const ROWS: usize = 4;
const CELL_W: usize = (W - 2 * MARGIN - GAP) / 2;
const CELL_H: usize = 216;
const PER_PAGE: usize = ROWS * 2;
const NAV_Y: usize = 1320;
const NAV_H: usize = 96;
const NAV_W: usize = 240;
const WHITE: u8 = 255;
const BLACK: u8 = 0;

/// Locks, alarms and anything unclassified stay off a reader on the wall.
const CONTROLLABLE: [&str; 4] = ["light", "socket", "thermostat", "button"];

struct Item {
    id: String,
    name: String,
    class: String,
    /// `None` for a scene, which has no state to show.
    on: Option<bool>,
    /// A live value beside the state. Left out of the fingerprint, or a drifting reading would void every tap.
    detail: Option<String>,
}

fn number(e: &Entity, capability: &str) -> Option<f64> {
    e.capabilities.get(capability)?.value.as_f64()
}

fn detail(e: &Entity, on: Option<bool>) -> Option<String> {
    if let Some(target) = number(e, "target_temperature") {
        return Some(number(e, "measure_temperature").map_or(format!("{target:.1}°"), |now| format!("{now:.1}° → {target:.1}°")));
    }
    if let Some(watts) = number(e, "measure_power").filter(|_| on == Some(true)) {
        return Some(format!("{watts:.0} W"));
    }
    number(e, "dim").filter(|_| on == Some(true)).map(|d| format!("{:.0} %", d * 100.0))
}

fn items(app: &App, device: &Device) -> Vec<Item> {
    let pages = device.pages.iter().flat_map(|p| p.items.iter()).filter(|i| !i.starts_with("card:"));
    let mut seen = std::collections::HashSet::new();
    device.tiles.iter().chain(device.more.iter()).chain(pages).filter(|id| seen.insert(id.as_str())).filter_map(|id| {
        let e = app.registry.entity(id)?;
        if !CONTROLLABLE.contains(&e.class.as_str()) {
            return None;
        }
        let on = match (e.capabilities.get("onoff"), e.capabilities.get("button")) {
            (Some(c), _) if c.settable => Some(c.value == json!(true)),
            (_, Some(c)) if c.settable => None,
            _ => return None,
        };
        Some(Item { id: id.clone(), name: device.names.get(id).unwrap_or(&e.name).clone(), class: e.class.clone(), on, detail: detail(&e, on) })
    }).collect()
}

/// One page of buttons, and a fingerprint of what it shows. A tap carries the
/// fingerprint, so a picture gone stale can never press the wrong thing.
fn page_of(all: Vec<Item>, wanted: u64, per_page: usize) -> (Vec<Item>, usize, usize, String) {
    let pages = all.len().div_ceil(per_page).max(1);
    let page = (wanted as usize).min(pages - 1);
    let shown: Vec<Item> = all.into_iter().skip(page * per_page).take(per_page).collect();
    let print: String = shown.iter().map(|i| format!("{}={:?};", i.id, i.on)).collect();
    let layout = sha256_hex(&format!("{page}/{pages}:{print}"))[..16].to_string();
    (shown, page, pages, layout)
}

fn cell(i: usize) -> (usize, usize) {
    (MARGIN + (i % 2) * (CELL_W + GAP), GRID_TOP + (i / 2) * (CELL_H + GAP))
}

#[derive(Debug, PartialEq)]
enum Hit {
    Item(usize),
    Prev,
    Next,
}

fn hit(x: usize, y: usize, count: usize, pages: usize) -> Option<Hit> {
    if pages > 1 && (NAV_Y..NAV_Y + NAV_H).contains(&y) {
        if (MARGIN..MARGIN + NAV_W).contains(&x) {
            return Some(Hit::Prev);
        }
        if (W - MARGIN - NAV_W..W - MARGIN).contains(&x) {
            return Some(Hit::Next);
        }
    }
    (0..count).find(|&i| {
        let (cx, cy) = cell(i);
        (cx..cx + CELL_W).contains(&x) && (cy..cy + CELL_H).contains(&y)
    }).map(Hit::Item)
}

pub fn screen(app: &App, device: &Device, req: &Request) -> Response {
    let wanted = req.query.get("page").and_then(|p| p.parse().ok()).unwrap_or(0);
    let (shown, page, pages, layout) = page_of(items(app, device), wanted, PER_PAGE);
    let nb = app.store.lock().unwrap().data.settings.language == "nb";
    // The server's clock may be in another zone, so the reader says what time it is.
    let time = req.query.get("time").filter(|t| valid_time(t)).map_or("", String::as_str);
    let (indoor, outdoor) = temperatures(app, device);

    let mut c = Canvas { px: vec![WHITE; W * H] };
    c.text(MARGIN, 40, 7, &fit(&device.name, W - 2 * MARGIN - 260, 7), BLACK);
    c.text(W - MARGIN - width(time, 8), 32, 8, time, BLACK);
    c.rect(MARGIN, 124, W - 2 * MARGIN, 4, BLACK);
    for (i, (label, value)) in [(if nb { "Inne" } else { "Indoor" }, indoor), (if nb { "Ute" } else { "Outdoor" }, outdoor)].into_iter().enumerate() {
        let x = MARGIN + i * (CELL_W + GAP);
        c.text(x, 156, 5, label, BLACK);
        c.text(x, 212, 11, value.as_deref().unwrap_or("--"), BLACK);
    }
    for (i, item) in shown.iter().enumerate() {
        let (x, y) = cell(i);
        let (bg, fg) = if item.on == Some(true) { (BLACK, WHITE) } else { (WHITE, BLACK) };
        c.rect(x, y, CELL_W, CELL_H, BLACK);
        c.rect(x + 5, y + 5, CELL_W - 10, CELL_H - 10, bg);
        for (l, line) in wrap(&item.name, CELL_W - 48, 6).iter().enumerate() {
            c.text(x + 24, y + 24 + l * 60, 6, line, fg);
        }
        // Words as well as fill: the state must not rest on shade alone.
        let state = match (item.on, nb) {
            (Some(true), true) => "På",
            (Some(true), false) => "On",
            (Some(false), _) if nb => "Av",
            (Some(false), _) => "Off",
            (None, true) => "Kjør",
            (None, false) => "Run",
        };
        c.text(x + 24, y + CELL_H - 60, 5, state, fg);
    }
    if pages > 1 {
        for (x, label) in [(MARGIN, "<"), (W - MARGIN - NAV_W, ">")] {
            c.rect(x, NAV_Y, NAV_W, NAV_H, BLACK);
            c.rect(x + 5, NAV_Y + 5, NAV_W - 10, NAV_H - 10, WHITE);
            c.text(x + (NAV_W - width(label, 7)) / 2, NAV_Y + 24, 7, label, BLACK);
        }
        let n = format!("{} / {pages}", page + 1);
        c.text((W - width(&n, 6)) / 2, NAV_Y + 28, 6, &n, BLACK);
    }
    Response::new(200, "image/png", png(W, H, &c.px)).header("Cache-Control", "no-store").header("X-Oldpad-Layout", &layout).header("X-Oldpad-Page", &page.to_string())
}

pub fn tap(app: &App, device: &Device, body: &Value) -> Result<Response, Response> {
    let (Some(x), Some(y)) = (body["x"].as_u64(), body["y"].as_u64()) else {
        return Err(Response::error(400, "Send x and y"));
    };
    let (shown, page, pages, layout) = page_of(items(app, device), body["page"].as_u64().unwrap_or(0), PER_PAGE);
    if body["layout"].as_str() != Some(layout.as_str()) {
        return Err(Response::error(409, "The screen has changed"));
    }
    let (x, y) = (usize::try_from(x).unwrap_or(usize::MAX), usize::try_from(y).unwrap_or(usize::MAX));
    let next = match hit(x, y, shown.len(), pages) {
        Some(Hit::Prev) => (page + pages - 1) % pages,
        Some(Hit::Next) => (page + 1) % pages,
        Some(Hit::Item(i)) => {
            press_item(app, &shown[i]).map_err(|e| Response::error(502, &e))?;
            page
        }
        None => page,
    };
    Ok(Response::ok(json!({ "ok": true, "page": next })))
}

fn temperatures(app: &App, device: &Device) -> (Option<String>, Option<String>) {
    let temp = |id: &Option<String>| {
        let v = id.as_ref().and_then(|i| app.registry.entity(i))?.capabilities.get("measure_temperature")?.value.as_f64()?;
        Some(format!("{v:.1}°"))
    };
    (temp(&device.panels.indoor), temp(&device.panels.outdoor))
}

fn press_item(app: &App, item: &Item) -> Result<(), String> {
    let (capability, value) = item.on.map_or(("button", json!(true)), |on| ("onoff", json!(!on)));
    app.registry.set(&item.id, capability, &value)
}

// ---------------------------------------------------------------- the browser page

const SCREEN_COOKIE: &str = "oldpad_kindle";
const PAIR_COOKIE: &str = "oldpad_kindle_pair";
const PAGE_CSP: &str = "default-src 'none'; style-src 'self'; img-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";
/// Long enough to see a change, rare enough to spare a reader's battery.
const PAGE_REFRESH: u32 = 300;
const PAIR_REFRESH: u32 = 5;
const COLUMNS: usize = 4;
const PAGE_BUTTONS: usize = COLUMNS * 4;
/// Two rows at most, so the buttons stay on the first screen.
const MAX_BOXES: usize = COLUMNS * 2;
// Tables, not flex or grid: the Kindle's browser is an old WebKit.
const MENU_COLUMNS: usize = 6;
/// One picture, so a page costs the reader's Wi-Fi one request: a black row, then a white one for what is on.
const ICON_SHEET: &[u8] = include_bytes!("../web/kindle-icons.png");
/// In the order they stand in the sheet. Drawn at twice the size they are shown, for the sharper panel.
const ICONS: [&str; 20] = ["home", "music", "bulb", "thermo", "bolt", "cloud", "sun", "moon", "chart", "radio", "flame", "plug", "partly", "moonpartly", "rain", "sleet", "snow", "fog", "thunder", "wind"];
const ICON_SIZE: usize = 36;
const CSS: &str = "html{height:100%}body{height:100%;margin:0;padding:10px;-webkit-box-sizing:border-box;box-sizing:border-box;background:#fff;color:#000;font-family:sans-serif;font-size:20px}\
.shell{position:relative;min-height:100%;-webkit-box-sizing:border-box;box-sizing:border-box}.r1{padding-bottom:82px}.r2{padding-bottom:160px}.r3{padding-bottom:238px}\
.menu{position:absolute;left:0;right:0;bottom:0;display:table;table-layout:fixed;width:100%;border-spacing:6px}.menu div{display:table-row}\
.menu a{display:table-cell;height:66px;vertical-align:middle;padding:0 4px;border:2px solid #000;color:#000;background:#fff;font-size:15px;font-weight:bold;text-align:center;text-decoration:none;overflow:hidden;word-wrap:break-word}.menu a.on{background:#000;color:#fff}.menu .ic{display:block;margin:0 auto 1px}\
@media (orientation:landscape){.shell{padding-bottom:0}.menu{position:static;float:left;display:block;width:128px}.menu div{display:block}.menu a{display:block;height:auto;padding:6px 4px;margin-bottom:6px}.main{margin-left:138px}}\
h1{font-size:26px;margin:0 4px 2px}p{margin:8px 4px}\
table{width:100%;border-collapse:separate;border-spacing:6px;table-layout:fixed}td{padding:0;vertical-align:top}form{margin:0}\
.box{height:112px;padding:6px 8px;border:2px solid #000;overflow:hidden;word-wrap:break-word}\
.box small{display:block;font-size:15px;white-space:nowrap;overflow:hidden}.box b{display:block;font-size:30px;line-height:38px;white-space:nowrap;overflow:hidden}\
.box b.words{font-size:18px;line-height:20px;height:40px;white-space:normal}.box span{display:block;font-size:15px;line-height:18px}\
.box .ic{float:right}button .ic{display:block;margin-bottom:1px}\
button{display:block;width:100%;height:116px;padding:3px 6px;border:3px solid #000;border-radius:0;background:#fff;color:#000;font-size:18px;line-height:21px;font-weight:bold;text-align:left;overflow:hidden;word-wrap:break-word}\
button.on{background:#000;color:#fff}button span{display:block;font-size:15px;line-height:18px;font-weight:normal}\
.nav{text-align:center;font-size:28px;margin-top:12px}.nav a{display:inline-block;color:#000;border:3px solid #000;padding:12px 36px;margin:0 12px;text-decoration:none}\
.notice{border:3px solid #000;padding:8px}.code{font-size:40px;font-weight:bold;word-wrap:break-word}.start{height:90px;text-align:center}";

pub fn route(app: &App, req: &Request, s: &[&str]) -> Response {
    if req.admin_port {
        return Response::error(404, "Not found");
    }
    // Forms from another site are refused here, and the press form also carries a token.
    if req.method == "POST" && req.header("origin").is_some_and(|o| o != app.cfg.public_url) {
        return Response::error(403, "Wrong origin");
    }
    match (req.method.as_str(), s) {
        ("GET", ["kindle"]) => show(app, req),
        ("GET", ["kindle", "style.css"]) => Response::new(200, "text/css; charset=utf-8", stylesheet()).header("Cache-Control", "no-cache"),
        ("GET", ["kindle", "icons.png"]) => Response::new(200, "image/png", ICON_SHEET).header("Cache-Control", "public, max-age=86400"),
        ("POST", ["kindle", "pair"]) => start_pairing(app),
        ("POST", ["kindle", "press"]) => press(app, req),
        _ => Response::error(404, "Not found"),
    }
}

fn stylesheet() -> String {
    let (size, sheet) = (ICON_SIZE, ICONS.len() * ICON_SIZE);
    let mut css = format!("{CSS}.ic{{display:inline-block;width:{size}px;height:{size}px;background:url(/kindle/icons.png) no-repeat;-webkit-background-size:{sheet}px {}px;background-size:{sheet}px {}px}}", size * 2, size * 2);
    for (i, name) in ICONS.iter().enumerate() {
        let x = i * size;
        css += &format!(".i-{name}{{background-position:-{x}px 0}}.on .i-{name}{{background-position:-{x}px -{size}px}}");
    }
    css
}

/// Decoration beside a word, so it is hidden from screen readers. An unknown name gets the cloud.
fn icon(name: &str) -> String {
    let known = ICONS.iter().find(|i| **i == name).unwrap_or(&"cloud");
    format!("<i class=\"ic i-{known}\" aria-hidden=\"true\"></i>")
}

fn class_icon(class: &str) -> &'static str {
    match class {
        "light" => "bulb",
        "socket" => "plug",
        "thermostat" => "thermo",
        _ => "bolt",
    }
}

fn cookie(app: &App, name: &str, value: &str, max_age: u64) -> String {
    let secure = if app.cfg.secure_cookies() { "; Secure" } else { "" };
    format!("{name}={value}; Path=/kindle; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}")
}

fn redirect(to: &str) -> Response {
    Response::new(303, "text/plain; charset=utf-8", "").header("Location", to).header("Cache-Control", "no-store")
}

fn html(title: &str, refresh: u32, refresh_to: &str, body: &str) -> Response {
    let page = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta http-equiv=\"refresh\" content=\"{refresh};url={refresh_to}\"><title>{}</title><link rel=\"stylesheet\" href=\"/kindle/style.css\"></head><body>{body}</body></html>",
        escape_xml(title)
    );
    Response::new(200, "text/html; charset=utf-8", page).header("Cache-Control", "no-store").header("Content-Security-Policy", PAGE_CSP)
}

fn form_token(device: &Device) -> String {
    sha256_hex(&format!("kindle-form:{}", device.token_hash))
}

fn screen_device(app: &App, req: &Request) -> Option<Device> {
    app.device_by_token(&req.cookie(SCREEN_COOKIE)?)
}

fn show(app: &App, req: &Request) -> Response {
    let nb = app.store.lock().unwrap().data.settings.language == "nb";
    if let Some(device) = screen_device(app, req) {
        return dashboard(app, req, &device, nb);
    }
    let pending = req.cookie(PAIR_COOKIE);
    let parts: Vec<&str> = pending.as_deref().unwrap_or("").split('.').collect();
    if let [id, secret, code, pin] = parts[..] {
        let poll = app.poll_pairing(id, secret);
        match poll["status"].as_str() {
            Some("approved") => {
                let token = poll["token"].as_str().unwrap_or("");
                return redirect("/kindle").header("Set-Cookie", &cookie(app, PAIR_COOKIE, "", 0)).header("Set-Cookie", &cookie(app, SCREEN_COOKIE, token, app.cfg.device_ttl));
            }
            Some("pending") => {
                let url = format!("{}/admin#/pair/{code}", app.cfg.public_url);
                let (title, open, pin_label) = if nb { ("Koble til Kindle", "Åpne denne adressen på telefonen:", "PIN") } else { ("Pair this Kindle", "Open this address on your phone:", "PIN") };
                let body = format!("<h1>{title}</h1><p>{open}</p><p class=\"code\">{}</p><p>{pin_label}:</p><p class=\"code\">{}</p>", escape_xml(&url), escape_xml(pin));
                return html(title, PAIR_REFRESH, "/kindle", &body);
            }
            _ => {}
        }
    }
    let (title, label) = if nb { ("Koble til Kindle", "Start") } else { ("Pair this Kindle", "Start") };
    let body = format!("<h1>oldpad</h1><p>{title}</p><form method=\"post\" action=\"/kindle/pair\"><button class=\"start\" type=\"submit\">{label}</button></form>");
    html(title, PAGE_REFRESH, "/kindle", &body).header("Set-Cookie", &cookie(app, PAIR_COOKIE, "", 0))
}

fn start_pairing(app: &App) -> Response {
    match app.new_pairing() {
        Ok(p) => redirect("/kindle").header("Set-Cookie", &cookie(app, PAIR_COOKIE, &format!("{}.{}.{}.{}", p.id, p.secret, p.code, p.pin), crate::app::PAIRING_TTL)),
        Err(e) => Response::error(429, e),
    }
}

/// The address of a view, which only ever holds an id from `menu`.
fn link(view: &str, page: usize) -> String {
    if view == "home" { format!("/kindle?page={page}") } else { format!("/kindle?view={view}&page={page}") }
}

fn own_page<'a>(device: &'a Device, view: &str) -> Option<&'a Page> {
    device.pages.get(view.strip_prefix('p')?.parse::<usize>().ok()?)
}

fn view_items(app: &App, device: &Device, view: &str) -> Vec<Item> {
    let mut all = items(app, device);
    if let Some(p) = own_page(device, view) {
        all.retain(|i| p.items.contains(&i.id));
        all.sort_by_key(|i| p.items.iter().position(|x| *x == i.id));
        return all;
    }
    let classes: &[&str] = match view {
        "home" => return all,
        "lights" => &["light", "socket"],
        "climate" => &["thermostat"],
        "flows" => &["button"],
        _ => &[],
    };
    all.retain(|i| classes.contains(&i.class.as_str()));
    all
}

/// The tablet's pages that work without a live connection, then the admin's own. Empty ones stay out.
fn menu(app: &App, device: &Device, nb: bool) -> Vec<(String, String, String)> {
    let built_in = [("home", "Hjem", "Home", "home"), ("lights", "Lys", "Lights", "bulb"), ("climate", "Klima", "Climate", "thermo"), ("flows", "Scener", "Scenes", "bolt"), ("weather", "Vær", "Weather", "cloud")];
    let fixed = built_in.iter().map(|(id, no, en, ic)| (id.to_string(), if nb { no } else { en }.to_string(), ic.to_string()));
    let own = device.pages.iter().enumerate().map(|(i, p)| (format!("p{i}"), p.name.clone(), p.icon.clone()));
    fixed.chain(own).filter(|(id, ..)| id == "home" || !view_items(app, device, id).is_empty() || !info_boxes(app, device, id, nb).is_empty()).collect()
}

fn menu_html(entries: &[(String, String, String)], view: &str) -> String {
    // Never more than three rows, the most the stylesheet leaves room for.
    let columns = entries.len().div_ceil(3).max(MENU_COLUMNS);
    let rows: String = entries.chunks(columns).map(|row| {
        let links: String = row.iter().map(|(id, label, ic)| {
            let current = if id == view { " class=\"on\" aria-current=\"page\"" } else { "" };
            format!("<a{current} href=\"{}\">{}{}</a>", escape_xml(&link(id, 0)), icon(ic), escape_xml(label))
        }).collect();
        format!("<div>{links}</div>")
    }).collect();
    format!("<nav class=\"menu\">{rows}</nav>")
}

fn dashboard(app: &App, req: &Request, device: &Device, nb: bool) -> Response {
    let wanted = req.query.get("page").and_then(|p| p.parse().ok()).unwrap_or(0);
    let entries = menu(app, device, nb);
    let view = entries.iter().find(|(id, ..)| Some(id) == req.query.get("view")).map_or("home", |(id, ..)| id.as_str());
    let (shown, page, pages, layout) = page_of(view_items(app, device, view), wanted, PAGE_BUTTONS);
    let token = form_token(device);
    let mut body = format!("<h1>{}</h1>", escape_xml(&device.name));
    body += &grid(info_boxes(app, device, view, nb).iter().map(Info::html).collect());
    let notice = match req.query.get("notice").map(String::as_str) {
        Some("stale") => Some(if nb { "Siden var utdatert. Trykk på nytt." } else { "The page was out of date. Tap again." }),
        Some("failed") => Some(if nb { "Det virket ikke." } else { "That did not work." }),
        _ => None,
    };
    if let Some(n) = notice {
        body += &format!("<p class=\"notice\">{n}</p>");
    }
    if shown.is_empty() && view != "weather" {
        body += if nb { "<p>Ingen knapper er valgt for denne skjermen.</p>" } else { "<p>No controls are set up for this screen.</p>" };
    }
    body += &grid(shown.iter().enumerate().map(|(i, item)| {
        let state = match (item.on, nb) {
            (Some(true), true) => "På",
            (Some(true), false) => "On",
            (Some(false), true) => "Av",
            (Some(false), false) => "Off",
            (None, true) => "Kjør",
            (None, false) => "Run",
        };
        let state = item.detail.as_ref().map_or(state.to_string(), |d| format!("{state} · {}", escape_xml(d)));
        let class = if item.on == Some(true) { " class=\"on\"" } else { "" };
        format!(
            "<form method=\"post\" action=\"/kindle/press\"><input type=\"hidden\" name=\"token\" value=\"{token}\"><input type=\"hidden\" name=\"layout\" value=\"{layout}\"><input type=\"hidden\" name=\"view\" value=\"{view}\"><input type=\"hidden\" name=\"page\" value=\"{page}\"><input type=\"hidden\" name=\"item\" value=\"{i}\"><button{class} type=\"submit\">{}{}<span>{state}</span></button></form>",
            icon(class_icon(&item.class)),
            escape_xml(&item.name)
        )
    }).collect());
    if pages > 1 {
        body += &format!("<p class=\"nav\"><a href=\"{}\">&lt;</a> {} / {pages} <a href=\"{}\">&gt;</a></p>", escape_xml(&link(view, (page + pages - 1) % pages)), page + 1, escape_xml(&link(view, (page + 1) % pages)));
    }
    let rows = entries.len().div_ceil(entries.len().div_ceil(3).max(MENU_COLUMNS));
    let body = if entries.len() > 1 { format!("<div class=\"shell r{rows}\">{}<div class=\"main\">{body}</div></div>", menu_html(&entries, view)) } else { body };
    html(&device.name, PAGE_REFRESH, &escape_xml(&link(view, page)), &body)
}

/// Rows of equal columns; a short last row keeps its cells the same width.
fn grid(cells: Vec<String>) -> String {
    if cells.is_empty() {
        return String::new();
    }
    let rows: String = cells.chunks(COLUMNS).map(|row| {
        let blanks = "<td></td>".repeat(COLUMNS - row.len());
        format!("<tr>{}{blanks}</tr>", row.iter().map(|c| format!("<td>{c}</td>")).collect::<String>())
    }).collect();
    format!("<table>{rows}</table>")
}

struct Info {
    icon: &'static str,
    label: String,
    value: String,
    /// The value is a phrase, not a number, and needs a smaller size to fit.
    words: bool,
    lines: Vec<String>,
}

impl Info {
    fn html(&self) -> String {
        let lines: String = self.lines.iter().map(|l| format!("<span>{}</span>", escape_xml(l))).collect();
        let class = if self.words { " class=\"words\"" } else { "" };
        format!("<div class=\"box\">{}<small>{}</small><b{class}>{}</b>{lines}</div>", icon(self.icon), escape_xml(&self.label), escape_xml(&self.value))
    }
}

fn readings(e: &Entity) -> Vec<String> {
    let mut out = Vec::new();
    if let (Some(lo), Some(hi)) = (number(e, "temperature_min"), number(e, "temperature_max")) {
        out.push(format!("{lo:.0}° / {hi:.0}°"));
    }
    if let Some(h) = number(e, "measure_humidity") {
        out.push(format!("{h:.0} %"));
    }
    if let Some(c) = number(e, "measure_co2") {
        out.push(format!("CO₂ {c:.0}"));
    }
    out
}

fn climate(e: &Entity, label: &str, icon: &'static str) -> Info {
    let value = number(e, "measure_temperature").map_or("--".to_string(), |t| format!("{t:.1}°"));
    Info { icon, label: label.to_string(), value, words: false, lines: readings(e) }
}

/// Wind is stored in km/h and shown in m/s, as on the tablet.
fn wind(e: &Entity, nb: bool) -> Option<Info> {
    let strength = number(e, "measure_wind_strength")?;
    let lines = number(e, "measure_gust_strength").map(|g| format!("{} {:.0} m/s", if nb { "Kast" } else { "Gusts" }, g / 3.6)).into_iter().collect();
    Some(Info { icon: "wind", label: if nb { "Vind" } else { "Wind" }.to_string(), value: format!("{:.0} m/s", strength / 3.6), words: false, lines })
}

fn forecast(e: &Entity, nb: bool) -> Option<Info> {
    let symbol = e.capabilities.get("weather_symbol")?.value.as_str()?;
    let hours = e.capabilities.get("forecast").and_then(|c| c.value.as_array());
    let t = now();
    // Hours from now, because the server's clock may be in another zone.
    let lines = [3u64, 6].iter().filter_map(|h| {
        let row = hours?.iter().find(|r| r[0].as_u64().is_some_and(|at| at >= t + h * 3600))?;
        Some(format!("+{h} {} {:.0}°", if nb { "t" } else { "h" }, row[1].as_f64()?))
    }).collect();
    Some(Info { icon: weather_icon(symbol), label: if nb { "Vær" } else { "Weather" }.to_string(), value: weather_text(symbol, nb).to_string(), words: true, lines })
}

fn weather_icon(symbol: &str) -> &'static str {
    let base = symbol.split('_').next().unwrap_or("");
    let night = symbol.contains("_night");
    match base {
        _ if base.contains("thunder") => "thunder",
        "clearsky" => if night { "moon" } else { "sun" },
        "fair" | "partlycloudy" => if night { "moonpartly" } else { "partly" },
        "fog" => "fog",
        _ if base.contains("sleet") => "sleet",
        _ if base.contains("snow") => "snow",
        _ if base.contains("rain") => "rain",
        _ => "cloud",
    }
}

fn weather_text(symbol: &str, nb: bool) -> &'static str {
    let base = symbol.split('_').next().unwrap_or("");
    let (no, en) = match base {
        _ if base.contains("thunder") => ("Torden", "Thunder"),
        "clearsky" => ("Klart", "Clear"),
        "fair" => ("Lettskyet", "Fair"),
        "partlycloudy" => ("Delvis skyet", "Partly cloudy"),
        "fog" => ("Tåke", "Fog"),
        "lightrain" => ("Lett regn", "Light rain"),
        "rain" => ("Regn", "Rain"),
        "heavyrain" => ("Kraftig regn", "Heavy rain"),
        "lightrainshowers" => ("Lette regnbyger", "Light showers"),
        "rainshowers" => ("Regnbyger", "Showers"),
        "heavyrainshowers" => ("Kraftige regnbyger", "Heavy showers"),
        "lightsleet" => ("Lett sludd", "Light sleet"),
        "sleet" => ("Sludd", "Sleet"),
        "heavysleet" => ("Kraftig sludd", "Heavy sleet"),
        "lightsleetshowers" | "sleetshowers" => ("Sluddbyger", "Sleet showers"),
        "heavysleetshowers" => ("Kraftige sluddbyger", "Heavy sleet showers"),
        "lightsnow" => ("Lett snø", "Light snow"),
        "snow" => ("Snø", "Snow"),
        "heavysnow" => ("Kraftig snø", "Heavy snow"),
        "lightsnowshowers" | "snowshowers" => ("Snøbyger", "Snow showers"),
        "heavysnowshowers" => ("Kraftige snøbyger", "Heavy snow showers"),
        _ => ("Skyet", "Cloudy"),
    };
    if nb { no } else { en }
}

/// Every three hours from now, in hours rather than clock times for the same reason as `forecast`.
fn forecast_boxes(e: &Entity, nb: bool) -> Vec<Info> {
    let Some(hours) = e.capabilities.get("forecast").and_then(|c| c.value.as_array()) else { return Vec::new() };
    let t = now();
    (0..12u64).filter_map(|n| {
        let h = n * 3;
        let row = hours.iter().find(|r| r[0].as_u64().is_some_and(|at| at + 3600 > t + h * 3600))?;
        let label = if h == 0 { if nb { "Nå".to_string() } else { "Now".to_string() } } else { format!("+{h} {}", if nb { "t" } else { "h" }) };
        let mut lines = vec![weather_text(row[2].as_str().unwrap_or(""), nb).to_string()];
        lines.extend(row[3].as_f64().map(|w| format!("{w:.0} m/s")));
        Some(Info { icon: weather_icon(row[2].as_str().unwrap_or("")), label, value: format!("{:.0}°", row[1].as_f64()?), words: false, lines })
    }).collect()
}

fn info_boxes(app: &App, device: &Device, view: &str, nb: bool) -> Vec<Info> {
    let entity = |id: &Option<String>| id.as_ref().and_then(|i| app.registry.entity(i));
    if view == "weather" {
        return entity(&device.panels.weather).map_or(Vec::new(), |e| forecast_boxes(&e, nb));
    }
    let own = own_page(device, view);
    let card = |c: &str| own.map_or(view == "home" || (view == "climate" && c == "card:climate"), |p| p.items.iter().any(|i| i == c));
    let mut boxes = Vec::new();
    if card("card:climate") {
        let (indoor, outdoor) = (entity(&device.panels.indoor), entity(&device.panels.outdoor));
        boxes.extend(indoor.as_ref().map(|e| climate(e, if nb { "Inne" } else { "Indoor" }, "home")));
        boxes.extend(outdoor.as_ref().map(|e| climate(e, if nb { "Ute" } else { "Outdoor" }, "cloud")));
        boxes.extend(outdoor.as_ref().and_then(|e| wind(e, nb)).or_else(|| entity(&device.panels.wind).and_then(|e| wind(&e, nb))));
    }
    if card("card:forecast") {
        boxes.extend(entity(&device.panels.weather).and_then(|e| forecast(&e, nb)));
    }
    let panels: Vec<&String> = if card("card:climate") { device.panels.ids().collect() } else { Vec::new() };
    let ids: Vec<&String> = match own {
        Some(p) => p.items.iter().collect(),
        None if view == "home" || view == "climate" => device.tiles.iter().chain(device.more.iter()).chain(device.pages.iter().flat_map(|p| p.items.iter())).collect(),
        None => Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    for id in ids.into_iter().filter(|id| seen.insert(id.as_str()) && !panels.contains(id)) {
        let Some(e) = app.registry.entity(id).filter(|e| e.class == "sensor") else { continue };
        let label = device.names.get(id).unwrap_or(&e.name);
        if number(&e, "measure_temperature").is_some() {
            boxes.push(climate(&e, label, "thermo"));
        } else if let Some((value, lines)) = readings(&e).split_first() {
            boxes.push(Info { icon: "chart", label: label.clone(), value: value.clone(), words: false, lines: lines.to_vec() });
        }
    }
    // Home keeps its buttons on the first screen; a page of its own can spend more.
    boxes.truncate(if view == "home" { MAX_BOXES } else { MAX_BOXES * 2 });
    boxes
}

fn press(app: &App, req: &Request) -> Response {
    let Some(device) = screen_device(app, req) else { return redirect("/kindle") };
    let form = parse_query(std::str::from_utf8(&req.body).unwrap_or(""));
    let field = |k: &str| form.get(k).map_or("", String::as_str);
    if !constant_eq(field("token"), &form_token(&device)) {
        return Response::error(403, "Bad form token");
    }
    let entries = menu(app, &device, false);
    let view = entries.iter().find(|(id, ..)| id == field("view")).map_or("home", |(id, ..)| id.as_str());
    let (shown, page, _, layout) = page_of(view_items(app, &device, view), field("page").parse().unwrap_or(0), PAGE_BUTTONS);
    let item = field("item").parse::<usize>().ok().and_then(|i| shown.get(i));
    let Some(item) = item.filter(|_| field("layout") == layout) else {
        return redirect(&format!("{}&notice=stale", link(view, page)));
    };
    match press_item(app, item) {
        Ok(()) => redirect(&link(view, page)),
        Err(_) => redirect(&format!("{}&notice=failed", link(view, page))),
    }
}

fn valid_time(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() == 5 && b[2] == b':' && b[0].is_ascii_digit() && b[1].is_ascii_digit() && b[3].is_ascii_digit() && b[4].is_ascii_digit() && t[..2] < *"24" && t[3..] < *"60"
}

// ---------------------------------------------------------------- drawing

struct Canvas {
    px: Vec<u8>,
}

impl Canvas {
    fn rect(&mut self, x: usize, y: usize, w: usize, h: usize, v: u8) {
        for row in y..(y + h).min(H) {
            let end = (x + w).min(W);
            if x < end {
                self.px[row * W + x..row * W + end].fill(v);
            }
        }
    }

    fn text(&mut self, x: usize, y: usize, scale: usize, s: &str, v: u8) {
        for (n, ch) in s.chars().enumerate() {
            for (col, bits) in glyph(ch).iter().enumerate() {
                for row in 0..8 {
                    if bits >> row & 1 == 1 {
                        self.rect(x + (n * 6 + col) * scale, y + row * scale, scale, scale, v);
                    }
                }
            }
        }
    }
}

fn width(s: &str, scale: usize) -> usize {
    s.chars().count() * 6 * scale
}

fn fit(s: &str, max: usize, scale: usize) -> String {
    let room = max / (6 * scale);
    if s.chars().count() <= room {
        return s.to_string();
    }
    s.chars().take(room.saturating_sub(2)).chain("..".chars()).collect()
}

/// At most two lines, broken between words where it can be.
fn wrap(s: &str, max: usize, scale: usize) -> Vec<String> {
    let room = max / (6 * scale);
    let mut first = String::new();
    let mut words = s.split_whitespace().peekable();
    while let Some(w) = words.peek() {
        let len = first.chars().count() + usize::from(!first.is_empty()) + w.chars().count();
        if len > room {
            break;
        }
        if !first.is_empty() {
            first.push(' ');
        }
        first.push_str(w);
        words.next();
    }
    if first.is_empty() {
        return vec![fit(s, max, scale)];
    }
    let rest = words.collect::<Vec<_>>().join(" ");
    if rest.is_empty() { vec![first] } else { vec![first, fit(&rest, max, scale)] }
}

fn glyph(c: char) -> [u8; 5] {
    match c {
        // The font squeezes these above the baseline, where g reads as 9.
        'g' => [0x18, 0xA4, 0xA4, 0xA4, 0x7C],
        'j' => [0x40, 0x80, 0x84, 0x7D, 0x00],
        'p' => [0xFC, 0x24, 0x24, 0x24, 0x18],
        'q' => [0x18, 0x24, 0x24, 0x24, 0xFC],
        'y' => [0x1C, 0xA0, 0xA0, 0xA0, 0x7C],
        ' '..='~' => {
            let i = (c as usize - 32) * 5;
            [FONT[i], FONT[i + 1], FONT[i + 2], FONT[i + 3], FONT[i + 4]]
        }
        'Æ' => [0x7E, 0x09, 0x7F, 0x49, 0x49],
        'Ø' => [0x3E, 0x51, 0x49, 0x45, 0x3E],
        'Å' => [0x78, 0x14, 0x15, 0x14, 0x78],
        'æ' => [0x74, 0x54, 0x7C, 0x54, 0x58],
        'ø' => [0x38, 0x64, 0x54, 0x4C, 0x38],
        'å' => [0x20, 0x55, 0x55, 0x55, 0x78],
        '°' => [0x06, 0x09, 0x09, 0x06, 0x00],
        _ => [0x02, 0x01, 0x51, 0x09, 0x06],
    }
}

/// The classic 5x7 font, ASCII 32 to 126, one byte per column.
#[rustfmt::skip]
const FONT: [u8; 475] = [
    0x00,0x00,0x00,0x00,0x00, 0x00,0x00,0x5F,0x00,0x00, 0x00,0x07,0x00,0x07,0x00, 0x14,0x7F,0x14,0x7F,0x14,
    0x24,0x2A,0x7F,0x2A,0x12, 0x23,0x13,0x08,0x64,0x62, 0x36,0x49,0x55,0x22,0x50, 0x00,0x05,0x03,0x00,0x00,
    0x00,0x1C,0x22,0x41,0x00, 0x00,0x41,0x22,0x1C,0x00, 0x14,0x08,0x3E,0x08,0x14, 0x08,0x08,0x3E,0x08,0x08,
    0x00,0x50,0x30,0x00,0x00, 0x08,0x08,0x08,0x08,0x08, 0x00,0x60,0x60,0x00,0x00, 0x20,0x10,0x08,0x04,0x02,
    0x3E,0x51,0x49,0x45,0x3E, 0x00,0x42,0x7F,0x40,0x00, 0x42,0x61,0x51,0x49,0x46, 0x21,0x41,0x45,0x4B,0x31,
    0x18,0x14,0x12,0x7F,0x10, 0x27,0x45,0x45,0x45,0x39, 0x3C,0x4A,0x49,0x49,0x30, 0x01,0x71,0x09,0x05,0x03,
    0x36,0x49,0x49,0x49,0x36, 0x06,0x49,0x49,0x29,0x1E, 0x00,0x36,0x36,0x00,0x00, 0x00,0x56,0x36,0x00,0x00,
    0x08,0x14,0x22,0x41,0x00, 0x14,0x14,0x14,0x14,0x14, 0x00,0x41,0x22,0x14,0x08, 0x02,0x01,0x51,0x09,0x06,
    0x32,0x49,0x79,0x41,0x3E, 0x7E,0x11,0x11,0x11,0x7E, 0x7F,0x49,0x49,0x49,0x36, 0x3E,0x41,0x41,0x41,0x22,
    0x7F,0x41,0x41,0x22,0x1C, 0x7F,0x49,0x49,0x49,0x41, 0x7F,0x09,0x09,0x09,0x01, 0x3E,0x41,0x49,0x49,0x7A,
    0x7F,0x08,0x08,0x08,0x7F, 0x00,0x41,0x7F,0x41,0x00, 0x20,0x40,0x41,0x3F,0x01, 0x7F,0x08,0x14,0x22,0x41,
    0x7F,0x40,0x40,0x40,0x40, 0x7F,0x02,0x0C,0x02,0x7F, 0x7F,0x04,0x08,0x10,0x7F, 0x3E,0x41,0x41,0x41,0x3E,
    0x7F,0x09,0x09,0x09,0x06, 0x3E,0x41,0x51,0x21,0x5E, 0x7F,0x09,0x19,0x29,0x46, 0x46,0x49,0x49,0x49,0x31,
    0x01,0x01,0x7F,0x01,0x01, 0x3F,0x40,0x40,0x40,0x3F, 0x1F,0x20,0x40,0x20,0x1F, 0x3F,0x40,0x38,0x40,0x3F,
    0x63,0x14,0x08,0x14,0x63, 0x07,0x08,0x70,0x08,0x07, 0x61,0x51,0x49,0x45,0x43, 0x00,0x7F,0x41,0x41,0x00,
    0x02,0x04,0x08,0x10,0x20, 0x00,0x41,0x41,0x7F,0x00, 0x04,0x02,0x01,0x02,0x04, 0x40,0x40,0x40,0x40,0x40,
    0x00,0x01,0x02,0x04,0x00, 0x20,0x54,0x54,0x54,0x78, 0x7F,0x48,0x44,0x44,0x38, 0x38,0x44,0x44,0x44,0x20,
    0x38,0x44,0x44,0x48,0x7F, 0x38,0x54,0x54,0x54,0x18, 0x08,0x7E,0x09,0x01,0x02, 0x0C,0x52,0x52,0x52,0x3E,
    0x7F,0x08,0x04,0x04,0x78, 0x00,0x44,0x7D,0x40,0x00, 0x20,0x40,0x44,0x3D,0x00, 0x7F,0x10,0x28,0x44,0x00,
    0x00,0x41,0x7F,0x40,0x00, 0x7C,0x04,0x18,0x04,0x78, 0x7C,0x08,0x04,0x04,0x78, 0x38,0x44,0x44,0x44,0x38,
    0x7C,0x14,0x14,0x14,0x08, 0x08,0x14,0x14,0x18,0x7C, 0x7C,0x08,0x04,0x04,0x08, 0x48,0x54,0x54,0x54,0x20,
    0x04,0x3F,0x44,0x40,0x20, 0x3C,0x40,0x40,0x20,0x7C, 0x1C,0x20,0x40,0x20,0x1C, 0x3C,0x40,0x30,0x40,0x3C,
    0x44,0x28,0x10,0x28,0x44, 0x0C,0x50,0x50,0x50,0x3C, 0x44,0x64,0x54,0x4C,0x44, 0x00,0x08,0x36,0x41,0x00,
    0x00,0x00,0x7F,0x00,0x00, 0x00,0x41,0x36,0x08,0x00, 0x08,0x04,0x08,0x10,0x08,
];

// ---------------------------------------------------------------- PNG

/// 8-bit grayscale. Each row is stored as its difference from the one above, so
/// the long zero runs of a mostly blank screen compress to almost nothing.
fn png(w: usize, h: usize, px: &[u8]) -> Vec<u8> {
    debug_assert!((1024..1536).contains(&w), "zlib's row distance code");
    let mut raw = Vec::with_capacity((w + 1) * h);
    for y in 0..h {
        raw.push(2); // the PNG "Up" filter
        for x in 0..w {
            raw.push(px[y * w + x].wrapping_sub(if y == 0 { 0 } else { px[(y - 1) * w + x] }));
        }
    }
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend((w as u32).to_be_bytes());
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([8, 0, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib(&raw, w + 1));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for b in data {
        crc ^= *b as u32;
        for _ in 0..8 {
            crc = if crc & 1 == 1 { crc >> 1 ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn put(&mut self, value: u32, count: u32) {
        self.acc |= value << self.n;
        self.n += count;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// Deflate packs Huffman codes from their top bit.
    fn code(&mut self, code: u32, count: u32) {
        self.put(code.reverse_bits() >> (32 - count), count);
    }
}

/// Deflate with the fixed Huffman table. It only looks for long repeats: of one
/// byte, or of the row above, which is what a screen of flat shapes is made of.
fn zlib(data: &[u8], stride: usize) -> Vec<u8> {
    const LONGEST: usize = 258;
    let mut b = Bits { out: vec![0x78, 0x01], acc: 0, n: 0 };
    b.put(1, 1); // the last block
    b.put(1, 2); // fixed Huffman
    let mut i = 0;
    while i < data.len() {
        if i >= stride && i + LONGEST <= data.len() && data[i..i + LONGEST] == data[i - stride..i - stride + LONGEST] {
            b.code(0xC5, 8); // length 258
            b.code(20, 5); // distances from 1025, plus nine extra bits
            b.put((stride - 1025) as u32, 9);
            i += LONGEST;
            continue;
        }
        let v = data[i];
        if v < 144 { b.code(0x30 + v as u32, 8) } else { b.code(0x190 + v as u32 - 144, 9) }
        i += 1;
        while i + LONGEST <= data.len() && data[i..i + LONGEST].iter().all(|x| *x == v) {
            b.code(0xC5, 8); // length 258
            b.code(0, 5); // distance 1
            i += LONGEST;
        }
    }
    b.code(0, 7); // end of block
    if b.n > 0 {
        b.put(0, 8 - b.n);
    }
    let (mut s1, mut s2) = (1u32, 0u32);
    for x in data {
        s1 = (s1 + *x as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    b.out.extend((s2 << 16 | s1).to_be_bytes());
    b.out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, on: Option<bool>) -> Item {
        Item { id: id.into(), name: id.into(), class: "light".into(), on, detail: None }
    }

    #[test]
    fn taps_land_on_the_right_control() {
        let (x, y) = cell(3);
        assert_eq!(hit(x + 1, y + 1, 8, 1), Some(Hit::Item(3)));
        assert_eq!(hit(x + 1, y + 1, 3, 1), None, "an empty cell");
        assert_eq!(hit(MARGIN + CELL_W + 1, GRID_TOP + 1, 8, 1), None, "the gap");
        assert_eq!(hit(MARGIN + 1, NAV_Y + 1, 8, 2), Some(Hit::Prev));
        assert_eq!(hit(W - MARGIN - 1, NAV_Y + 1, 8, 2), Some(Hit::Next));
        assert_eq!(hit(MARGIN + 1, NAV_Y + 1, 8, 1), None, "one page has no arrows");
        assert!(cell(PER_PAGE - 1).1 + CELL_H <= NAV_Y, "the grid clears the arrows");
    }

    #[test]
    fn a_changed_state_or_layout_changes_the_fingerprint() {
        let print = |a: Option<bool>, b: &str| page_of(vec![item("x", a), item(b, None)], 0, PER_PAGE).3;
        assert_eq!(print(Some(true), "y"), print(Some(true), "y"));
        assert_ne!(print(Some(true), "y"), print(Some(false), "y"));
        assert_ne!(print(Some(true), "y"), print(Some(true), "z"));
    }

    #[test]
    fn pages_are_clamped() {
        let all = (0..9).map(|i| item(&i.to_string(), None)).collect();
        let (shown, page, pages, _) = page_of(all, 99, PER_PAGE);
        assert_eq!((shown.len(), page, pages), (1, 1, 2));
        assert_eq!(page_of(vec![], 5, PER_PAGE).1, 0);
    }

    #[test]
    fn grid_rows_are_full_width() {
        let html = grid(vec!["a".into(); COLUMNS + 1]);
        assert_eq!(html.matches("<tr>").count(), 2);
        assert_eq!(html.matches("<td").count(), COLUMNS * 2, "the short row is padded");
        assert_eq!(grid(vec![]), "");
    }

    #[test]
    fn views_have_safe_links() {
        assert_eq!(link("home", 2), "/kindle?page=2");
        assert_eq!(link("p1", 0), "/kindle?view=p1&page=0");
        let html = menu_html(&(0..7).map(|i| (format!("p{i}"), "<x>".to_string(), "moon".to_string())).collect::<Vec<_>>(), "p6");
        assert_eq!(html.matches("<div>").count(), 2);
        assert!(html.contains("&lt;x&gt;") && html.contains("class=\"on\" aria-current=\"page\" href=\"/kindle?view=p6&amp;page=0\"><i class=\"ic i-moon\""));
    }

    #[test]
    fn icons_match_their_sheet() {
        let size = |at: usize| u32::from_be_bytes(ICON_SHEET[at..at + 4].try_into().unwrap()) as usize;
        assert_eq!((size(16), size(20)), (ICONS.len() * ICON_SIZE * 2, ICON_SIZE * 4), "two rows, one column per icon");
        for name in crate::store::PAGE_ICONS {
            assert!(ICONS.contains(&name), "{name}");
        }
        assert!(icon("<script>").contains("i-cloud"), "only known names reach the page");
        assert_eq!(weather_icon("clearsky_night"), "moon");
        assert_eq!(weather_icon("lightsleetshowers_day"), "sleet");
        assert_eq!(weather_icon("heavyrainandthunder"), "thunder");
    }

    #[test]
    fn weather_symbols_have_words() {
        assert_eq!(weather_text("partlycloudy_night", false), "Partly cloudy");
        assert_eq!(weather_text("heavyrainandthunder", true), "Torden");
        assert_eq!(weather_text("unknown", true), "Skyet");
    }

    #[test]
    fn times_are_checked() {
        assert!(valid_time("23:59"));
        for bad in ["24:00", "12:60", "1:00", "ab:cd", "12345"] {
            assert!(!valid_time(bad), "{bad}");
        }
    }

    #[test]
    fn text_is_fitted() {
        assert_eq!(wrap("Reading lamp", 36 * 20, 6), vec!["Reading lamp"]);
        assert_eq!(wrap("Stue taklampe nord", 36 * 8, 6), vec!["Stue", "taklam.."]);
        assert_eq!(wrap("Kjøkkenbenkbelysning", 36 * 6, 6), vec!["Kjøk.."]);
    }

    #[test]
    fn png_has_known_checksums() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let image = png(W, H, &vec![WHITE; W * H]);
        assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");
        assert!(image.len() < 20_000, "a blank screen compresses: {}", image.len());
    }
}
