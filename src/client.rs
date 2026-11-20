//! Outgoing connections: a small HTTP/1.1 client (plain TCP for LAN devices,
//! TLS via rustls for internet services) and a plain-TCP WebSocket client.

use crate::util::{b64_encode, random_bytes};
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

const MAX_RESPONSE: usize = 16 * 1024 * 1024;
const MAX_FRAME: usize = 4 * 1024 * 1024;
/// MET Norway's terms ask clients to identify the application and a contact; the project page is that contact.
const USER_AGENT: &str = concat!("oldpad/", env!("CARGO_PKG_VERSION"), " (+https://github.com/bisand/oldpad)");

pub struct Target {
    pub https: bool,
    pub host: String,
    pub port: u16,
    pub prefix: String,
}

pub fn parse_url(url: &str) -> Result<Target, String> {
    let (https, rest) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
        (Some(r), _) => (true, r),
        (_, Some(r)) => (false, r),
        _ => return Err("address must start with http:// or https://".into()),
    };
    let (authority, prefix) = rest.split_once('/').map(|(a, p)| (a, format!("/{p}"))).unwrap_or((rest, String::new()));
    // shortcut: no IPv6 literals ("[::1]:80"); add when someone needs them.
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().map_err(|_| "bad port")?),
        None => (authority.to_string(), if https { 443 } else { 80 }),
    };
    if host.is_empty() || host.contains(['/', '@', ' ']) {
        return Err("bad host".into());
    }
    Ok(Target { https, host, port, prefix: prefix.trim_end_matches('/').to_string() })
}

fn connect(t: &Target, timeout: Duration) -> Result<TcpStream, String> {
    let addr = (t.host.as_str(), t.port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve {}: {e}", t.host))?
        .next()
        .ok_or_else(|| format!("no address for {}", t.host))?;
    let s = TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("cannot reach {}:{}: {e}", t.host, t.port))?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(timeout)).ok();
    s.set_nodelay(true).ok();
    Ok(s)
}

pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn json(&self) -> Result<serde_json::Value, String> {
        serde_json::from_slice(&self.body).map_err(|e| format!("invalid JSON from device: {e}"))
    }
}

/// Splits raw response bytes into status, headers and the body bytes after the head.
fn parse_head(raw: &[u8]) -> Result<(u16, HashMap<String, String>, usize), String> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("incomplete response")?;
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| "response head is not UTF-8")?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or("bad status line")?;
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    Ok((status, headers, end + 4))
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = data.windows(2).position(|w| w == b"\r\n").ok_or("truncated chunk")?;
        let size_str = std::str::from_utf8(&data[..line_end]).map_err(|_| "bad chunk")?;
        let size = usize::from_str_radix(size_str.split(';').next().unwrap_or("").trim(), 16).map_err(|_| "bad chunk size")?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size + 2 || out.len() + size > MAX_RESPONSE {
            return Err("truncated or oversized chunk".into());
        }
        out.extend(&data[..size]);
        data = &data[size + 2..];
    }
}

trait Stream: Read + Write {}
impl<T: Read + Write> Stream for T {}

fn tls_config() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.into() };
            let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .expect("ring supports the default TLS versions")
                .with_root_certificates(roots)
                .with_no_client_auth();
            Arc::new(config)
        })
        .clone()
}

fn open(t: &Target) -> Result<Box<dyn Stream>, String> {
    let tcp = connect(t, Duration::from_secs(10))?;
    if !t.https {
        return Ok(Box::new(tcp));
    }
    let name = rustls::pki_types::ServerName::try_from(t.host.clone()).map_err(|_| format!("bad TLS host name {}", t.host))?;
    let conn = rustls::ClientConnection::new(tls_config(), name).map_err(|e| format!("TLS: {e}"))?;
    Ok(Box::new(rustls::StreamOwned::new(conn, tcp)))
}

pub fn request(base: &str, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Result<HttpResponse, String> {
    let t = parse_url(base)?;
    let mut s = open(&t)?;
    let mut req = format!("{method} {}{path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: {USER_AGENT}\r\n", t.prefix, t.host);
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    let has = |name: &str| headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name));
    if !has("accept") {
        req.push_str("Accept: application/json\r\n");
    }
    if let Some(b) = body {
        if !has("content-type") {
            req.push_str("Content-Type: application/json\r\n");
        }
        req.push_str(&format!("Content-Length: {}\r\n", b.len()));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    if let Some(b) = body {
        s.write_all(b).map_err(|e| e.to_string())?;
    }

    let mut raw = Vec::new();
    match s.take(MAX_RESPONSE as u64 + 1).read_to_end(&mut raw) {
        Ok(_) => {}
        // Many servers close TLS without close_notify after Connection: close; the body is complete.
        Err(e) if e.kind() == ErrorKind::UnexpectedEof && !raw.is_empty() => {}
        Err(e) => return Err(format!("reading response: {e}")),
    }
    if raw.len() > MAX_RESPONSE {
        return Err("response too large".into());
    }
    let (status, headers, start) = parse_head(&raw)?;
    let mut body = raw[start..].to_vec();
    if headers.get("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        body = dechunk(&body)?;
    } else if let Some(len) = headers.get("content-length").and_then(|v| v.parse::<usize>().ok()) {
        body.truncate(len);
    }
    Ok(HttpResponse { status, headers, body })
}

/// A WebSocket client connection (RFC 6455), text frames only.
pub struct WebSocket {
    stream: TcpStream,
    buf: Vec<u8>,
    fragments: Vec<u8>,
}

impl WebSocket {
    pub fn connect(base: &str, path: &str) -> Result<WebSocket, String> {
        let t = parse_url(base)?;
        if t.https {
            return Err("wss:// is not supported; use the device's http:// LAN address".into());
        }
        let mut stream = connect(&t, Duration::from_secs(10))?;
        let key = b64_encode(&random_bytes::<16>());
        let req = format!(
            "GET {}{path} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nUser-Agent: {USER_AGENT}\r\n\r\n",
            t.prefix, t.host
        );
        stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;

        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut chunk).map_err(|e| format!("websocket handshake: {e}"))?;
            if n == 0 || buf.len() > 16 * 1024 {
                return Err("websocket handshake failed".into());
            }
            buf.extend(&chunk[..n]);
        }
        let (status, _, start) = parse_head(&buf)?;
        if status != 101 {
            return Err(format!("websocket upgrade refused with HTTP {status}"));
        }
        // Sec-WebSocket-Accept is not checked: it needs SHA-1, guards only against
        // misbehaving proxies, and this connection is plain-text LAN anyway.
        Ok(WebSocket { stream, buf: buf[start..].to_vec(), fragments: Vec::new() })
    }

    /// Short read timeout so the caller's loop can send pings and notice shutdown.
    pub fn set_poll_interval(&self, d: Duration) {
        self.stream.set_read_timeout(Some(d)).ok();
    }

    fn send_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<(), String> {
        let mut frame = vec![0x80 | opcode];
        match payload.len() {
            n @ 0..=125 => frame.push(0x80 | n as u8),
            n @ 126..=0xffff => {
                frame.push(0x80 | 126);
                frame.extend((n as u16).to_be_bytes());
            }
            n => {
                frame.push(0x80 | 127);
                frame.extend((n as u64).to_be_bytes());
            }
        }
        let mask = random_bytes::<4>();
        frame.extend(mask);
        frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.stream.write_all(&frame).map_err(|e| format!("websocket write: {e}"))
    }

    pub fn send(&mut self, text: &str) -> Result<(), String> {
        self.send_frame(0x1, text.as_bytes())
    }

    /// Takes one complete frame out of the buffer, if there is one.
    fn parse_frame(&mut self) -> Result<Option<(bool, u8, Vec<u8>)>, String> {
        let b = &self.buf;
        if b.len() < 2 {
            return Ok(None);
        }
        let (fin, opcode, masked) = (b[0] & 0x80 != 0, b[0] & 0x0f, b[1] & 0x80 != 0);
        let (len, mut pos) = match b[1] & 0x7f {
            126 if b.len() >= 4 => (u16::from_be_bytes([b[2], b[3]]) as usize, 4),
            127 if b.len() >= 10 => (u64::from_be_bytes(b[2..10].try_into().unwrap()) as usize, 10),
            126 | 127 => return Ok(None),
            n => (n as usize, 2),
        };
        if len > MAX_FRAME {
            return Err("websocket frame too large".into());
        }
        let mask = if masked {
            if b.len() < pos + 4 {
                return Ok(None);
            }
            pos += 4;
            Some([b[pos - 4], b[pos - 3], b[pos - 2], b[pos - 1]])
        } else {
            None
        };
        if b.len() < pos + len {
            return Ok(None);
        }
        let mut payload = b[pos..pos + len].to_vec();
        if let Some(m) = mask {
            payload.iter_mut().enumerate().for_each(|(i, x)| *x ^= m[i % 4]);
        }
        self.buf.drain(..pos + len);
        Ok(Some((fin, opcode, payload)))
    }

    /// Returns the next text message, `Ok(None)` if nothing arrived within the
    /// poll interval, or an error when the connection is gone.
    pub fn recv(&mut self) -> Result<Option<String>, String> {
        loop {
            while let Some((fin, opcode, payload)) = self.parse_frame()? {
                match opcode {
                    0x0 | 0x1 => {
                        self.fragments.extend(payload);
                        if self.fragments.len() > MAX_FRAME {
                            return Err("websocket message too large".into());
                        }
                        if fin {
                            let msg = std::mem::take(&mut self.fragments);
                            return String::from_utf8(msg).map(Some).map_err(|_| "non-UTF-8 text frame".into());
                        }
                    }
                    0x8 => return Err("websocket closed by peer".into()),
                    0x9 => self.send_frame(0xA, &payload)?,
                    _ => {} // pong, binary: ignored
                }
            }
            let mut chunk = [0u8; 16 * 1024];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err("websocket connection closed".into()),
                Ok(n) => self.buf.extend(&chunk[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return Ok(None),
                Err(e) => return Err(format!("websocket read: {e}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn parses_urls() {
        let t = parse_url("http://192.168.1.5").unwrap();
        assert_eq!((t.host.as_str(), t.port, t.prefix.as_str()), ("192.168.1.5", 80, ""));
        let t = parse_url("http://homey.local:8080/base/").unwrap();
        assert_eq!((t.host.as_str(), t.port, t.prefix.as_str()), ("homey.local", 8080, "/base"));
        let t = parse_url("https://api.met.no").unwrap();
        assert!(t.https && t.port == 443);
        assert!(parse_url("ftp://x").is_err());
        assert!(parse_url("http://user@x").is_err());
    }

    #[test]
    fn decodes_chunked_bodies() {
        assert_eq!(dechunk(b"4\r\nWiki\r\n5;x=1\r\npedia\r\n0\r\n\r\n").unwrap(), b"Wikipedia");
        assert!(dechunk(b"ff\r\nshort\r\n").is_err());
    }

    #[test]
    fn http_and_websocket_against_a_local_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            // 1: plain request with a chunked reply
            let (mut s, _) = listener.accept().unwrap();
            let mut b = [0u8; 1024];
            let _ = s.read(&mut b).unwrap();
            s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nX-Homey-ID: abc\r\n\r\n2\r\n{}\r\n0\r\n\r\n").unwrap();
            drop(s);
            // 2: websocket: upgrade, echo one masked client frame back unmasked, in two fragments
            let (mut s, _) = listener.accept().unwrap();
            let _ = s.read(&mut b).unwrap();
            s.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").unwrap();
            let n = s.read(&mut b).unwrap();
            let len = (b[1] & 0x7f) as usize;
            let mask = [b[2], b[3], b[4], b[5]];
            let text: Vec<u8> = (0..len).map(|i| b[6 + i] ^ mask[i % 4]).collect();
            assert_eq!(n, 6 + len);
            let (a, c) = text.split_at(2);
            let mut out = vec![0x01, a.len() as u8];
            out.extend(a);
            out.extend([0x89, 0x00]); // a ping in between
            out.extend([0x80, c.len() as u8]);
            out.extend(c);
            s.write_all(&out).unwrap();
            std::thread::sleep(Duration::from_millis(200));
        });

        let res = request(&base, "GET", "/api/manager/system/ping", &[], None).unwrap();
        assert_eq!((res.status, res.headers["x-homey-id"].as_str(), res.body.as_slice()), (200, "abc", &b"{}"[..]));

        let mut ws = WebSocket::connect(&base, "/socket.io/?EIO=3&transport=websocket").unwrap();
        ws.set_poll_interval(Duration::from_millis(500));
        ws.send("42[\"hello\"]").unwrap();
        let mut got = None;
        for _ in 0..10 {
            if let Some(m) = ws.recv().unwrap() {
                got = Some(m);
                break;
            }
        }
        assert_eq!(got.as_deref(), Some("42[\"hello\"]"));
    }
}
