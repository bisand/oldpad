//! A screen for e-ink readers: oldpad draws a grayscale PNG, the reader shows it
//! and sends back where it was tapped. Sized for a Paperwhite 3 held upright.

use crate::app::App;
use crate::http::{Request, Response};
use crate::store::Device;
use crate::util::sha256_hex;
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
    /// `None` for a scene, which has no state to show.
    on: Option<bool>,
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
        Some(Item { id: id.clone(), name: device.names.get(id).unwrap_or(&e.name).clone(), on })
    }).collect()
}

/// One page of buttons, and a fingerprint of what it shows. A tap carries the
/// fingerprint, so a picture gone stale can never press the wrong thing.
fn page_of(all: Vec<Item>, wanted: u64) -> (Vec<Item>, usize, usize, String) {
    let pages = all.len().div_ceil(PER_PAGE).max(1);
    let page = (wanted as usize).min(pages - 1);
    let shown: Vec<Item> = all.into_iter().skip(page * PER_PAGE).take(PER_PAGE).collect();
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
    let (shown, page, pages, layout) = page_of(items(app, device), wanted);
    let nb = app.store.lock().unwrap().data.settings.language == "nb";
    // The server's clock may be in another zone, so the reader says what time it is.
    let time = req.query.get("time").filter(|t| valid_time(t)).map_or("", String::as_str);
    let temp = |id: &Option<String>| {
        let v = id.as_ref().and_then(|i| app.registry.entity(i))?.capabilities.get("measure_temperature")?.value.as_f64()?;
        Some(format!("{v:.1}°"))
    };
    let (indoor, outdoor) = (temp(&device.panels.indoor), temp(&device.panels.outdoor));

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
    let (shown, page, pages, layout) = page_of(items(app, device), body["page"].as_u64().unwrap_or(0));
    if body["layout"].as_str() != Some(layout.as_str()) {
        return Err(Response::error(409, "The screen has changed"));
    }
    let (x, y) = (usize::try_from(x).unwrap_or(usize::MAX), usize::try_from(y).unwrap_or(usize::MAX));
    let next = match hit(x, y, shown.len(), pages) {
        Some(Hit::Prev) => (page + pages - 1) % pages,
        Some(Hit::Next) => (page + 1) % pages,
        Some(Hit::Item(i)) => {
            let item = &shown[i];
            let (capability, value) = item.on.map_or(("button", json!(true)), |on| ("onoff", json!(!on)));
            app.registry.set(&item.id, capability, &value).map_err(|e| Response::error(502, &e))?;
            page
        }
        None => page,
    };
    Ok(Response::ok(json!({ "ok": true, "page": next })))
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
        Item { id: id.into(), name: id.into(), on }
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
        let print = |a: Option<bool>, b: &str| page_of(vec![item("x", a), item(b, None)], 0).3;
        assert_eq!(print(Some(true), "y"), print(Some(true), "y"));
        assert_ne!(print(Some(true), "y"), print(Some(false), "y"));
        assert_ne!(print(Some(true), "y"), print(Some(true), "z"));
    }

    #[test]
    fn pages_are_clamped() {
        let all = (0..9).map(|i| item(&i.to_string(), None)).collect();
        let (shown, page, pages, _) = page_of(all, 99);
        assert_eq!((shown.len(), page, pages), (1, 1, 2));
        assert_eq!(page_of(vec![], 5).1, 0);
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
