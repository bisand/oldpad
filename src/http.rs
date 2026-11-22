//! A deliberately small HTTP/1.1 server: one request per connection,
//! `Content-Length` bodies only, hard limits on everything.

use crate::util::percent_decode;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

pub const MAX_HEAD: usize = 16 * 1024;
pub const MAX_BODY: usize = 64 * 1024;
/// A backup can outgrow every other request. Only a signed-in admin may send this much.
pub const MAX_RESTORE: usize = 4 * 1024 * 1024;
/// A whole request must arrive within this, however slowly it trickles in.
const REQUEST_DEADLINE: Duration = Duration::from_secs(10);
/// For a body past `MAX_BODY`, which a phone on weak Wi-Fi needs longer to send.
const LARGE_BODY_DEADLINE: Duration = Duration::from_secs(60);

/// Reads from the socket until a fixed moment, not per read.
struct Deadline<'a> {
    stream: &'a TcpStream,
    until: Instant,
}

impl Read for Deadline<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left))?;
        let mut stream = self.stream;
        stream.read(buf)
    }
}

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub peer: String,
    /// Arrived on the internet-facing admin port (`OLDPAD_ADMIN_BIND`).
    pub admin_port: bool,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }

    pub fn cookie(&self, name: &str) -> Option<String> {
        self.header("cookie")?.split(';').find_map(|part| {
            let (k, v) = part.trim().split_once('=')?;
            (k == name).then(|| v.to_string())
        })
    }

    pub fn bearer(&self) -> Option<&str> {
        self.header("authorization")?.strip_prefix("Bearer ")
    }

    pub fn json(&self) -> Result<serde_json::Value, Response> {
        if !self.header("content-type").unwrap_or("").starts_with("application/json") {
            return Err(Response::error(415, "Expected application/json"));
        }
        if self.body.is_empty() {
            return Ok(serde_json::Value::Object(Default::default()));
        }
        serde_json::from_slice(&self.body).map_err(|_| Response::error(400, "Invalid JSON"))
    }

    /// Path split into decoded segments: "/api/a%3Ab" -> ["api", "a:b"].
    pub fn segments(&self) -> Vec<String> {
        self.path.split('/').filter(|s| !s.is_empty()).map(percent_decode).collect()
    }
}

pub enum Body {
    Bytes(Vec<u8>),
    /// Server-Sent Events: each received string is written as-is until the channel closes.
    Stream(Receiver<String>),
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Response({})", self.status)
    }
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

impl Response {
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Response {
            status,
            headers: vec![("Content-Type".into(), content_type.into())],
            body: Body::Bytes(body.into()),
        }
    }

    pub fn json(status: u16, value: &serde_json::Value) -> Self {
        Self::new(status, "application/json; charset=utf-8", value.to_string())
            .header("Cache-Control", "no-store")
    }

    pub fn ok(value: serde_json::Value) -> Self {
        Self::json(200, &value)
    }

    pub fn error(status: u16, message: &str) -> Self {
        Self::json(status, &serde_json::json!({ "error": message }))
    }

    pub fn stream(rx: Receiver<String>) -> Self {
        Response {
            status: 200,
            headers: vec![
                ("Content-Type".into(), "text/event-stream".into()),
                ("Cache-Control".into(), "no-store".into()),
                ("X-Accel-Buffering".into(), "no".into()),
            ],
            body: Body::Stream(rx),
        }
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        303 => "See Other",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

pub fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

/// Reads one request. Errors come back as the response to send. `limit` sees the
/// request before its body is read and says how large that body may be.
pub fn read_request(stream: &TcpStream, peer: String, admin_port: bool, limit: &dyn Fn(&Request) -> usize) -> Result<Request, Response> {
    let mut reader = BufReader::new(Deadline { stream, until: Instant::now() + REQUEST_DEADLINE }.take((MAX_HEAD + MAX_BODY) as u64));
    let mut head_len = 0;
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(|_| Response::error(400, "Bad request"))?;
        if n == 0 {
            return Err(Response::error(400, "Connection closed"));
        }
        head_len += n;
        if head_len > MAX_HEAD {
            return Err(Response::error(431, "Headers too large"));
        }
        let line = line.trim_end_matches(['\r', '\n']).to_string();
        if line.is_empty() {
            break;
        }
        lines.push(line);
    }

    let mut first = lines.first().ok_or_else(|| Response::error(400, "Empty request"))?.split(' ');
    let (method, target, version) = (first.next(), first.next(), first.next());
    let (Some(method), Some(target), Some(version)) = (method, target, version) else {
        return Err(Response::error(400, "Malformed request line"));
    };
    if !version.starts_with("HTTP/1.") || !target.starts_with('/') {
        return Err(Response::error(400, "Unsupported request"));
    }

    let mut headers = HashMap::new();
    for line in &lines[1..] {
        let (k, v) = line.split_once(':').ok_or_else(|| Response::error(400, "Malformed header"))?;
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
    }
    if headers.contains_key("transfer-encoding") {
        return Err(Response::error(501, "Chunked request bodies are not supported"));
    }

    let len: usize = match headers.get("content-length") {
        Some(v) => v.parse().map_err(|_| Response::error(400, "Bad Content-Length"))?,
        None => 0,
    };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut req = Request {
        method: method.to_string(),
        path: path.to_string(),
        query: parse_query(query),
        headers,
        body: Vec::new(),
        peer,
        admin_port,
    };
    if len > limit(&req) {
        return Err(Response::error(413, "Body too large"));
    }
    if len > MAX_BODY {
        reader.get_mut().get_mut().until = Instant::now() + LARGE_BODY_DEADLINE;
    }
    reader.get_mut().set_limit(len as u64);
    let mut body = vec![0; len];
    reader.read_exact(&mut body).map_err(|_| Response::error(400, "Body shorter than Content-Length"))?;
    req.body = body;
    Ok(req)
}

const SECURITY_HEADERS: &[(&str, &str)] = &[
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
    ("X-Frame-Options", "DENY"),
];

pub fn write_response(stream: &mut TcpStream, res: Response) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\nConnection: close\r\n", res.status, reason(res.status));
    for (k, v) in SECURITY_HEADERS {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    for (k, v) in &res.headers {
        // Header values come from our own code, but never let a newline split a header.
        if !k.contains(['\r', '\n']) && !v.contains(['\r', '\n']) {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
    }
    match res.body {
        Body::Bytes(body) => {
            head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
            stream.write_all(head.as_bytes())?;
            stream.write_all(&body)?;
            stream.flush()
        }
        Body::Stream(rx) => {
            head.push_str("\r\n");
            stream.write_all(head.as_bytes())?;
            // Tell EventSource to reconnect after 5 s if the stream drops.
            stream.write_all(b"retry: 5000\n\n")?;
            stream.flush()?;
            loop {
                let chunk = match rx.recv_timeout(std::time::Duration::from_secs(25)) {
                    Ok(chunk) => chunk,
                    // A comment line keeps proxies and the iPad from closing an idle stream.
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => ":\n\n".to_string(),
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                };
                stream.write_all(chunk.as_bytes())?;
                stream.flush()?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    fn roundtrip(raw: &[u8]) -> Result<Request, Response> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let raw = raw.to_vec();
        let client = std::thread::spawn(move || {
            let mut s = TcpStream::connect(addr).unwrap();
            s.write_all(&raw).unwrap();
            s
        });
        let (server, _) = listener.accept().unwrap();
        let _keep = client.join().unwrap();
        read_request(&server, "test".into(), false, &|_| MAX_BODY)
    }

    #[test]
    fn parses_a_post() {
        let req = roundtrip(b"POST /api/x%3Ay?a=1&b=two%20words HTTP/1.1\r\nHost: h\r\nContent-Type: application/json\r\nContent-Length: 7\r\nCookie: a=1; oldpad=xyz\r\n\r\n{\"k\":1}").unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.segments(), vec!["api", "x:y"]);
        assert_eq!(req.query["b"], "two words");
        assert_eq!(req.cookie("oldpad").as_deref(), Some("xyz"));
        assert_eq!(req.json().unwrap()["k"], 1);
    }

    #[test]
    fn rejects_chunked_and_oversized() {
        assert_eq!(roundtrip(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap_err().status, 501);
        let big = format!("POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1);
        assert_eq!(roundtrip(big.as_bytes()).unwrap_err().status, 413);
        let long = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAX_HEAD));
        assert_eq!(roundtrip(long.as_bytes()).unwrap_err().status, 431);
        assert_eq!(roundtrip(b"GET nope HTTP/1.1\r\n\r\n").unwrap_err().status, 400);
    }

    #[test]
    fn json_requires_content_type() {
        let req = roundtrip(b"POST / HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}").unwrap();
        assert_eq!(req.json().unwrap_err().status, 415);
    }
}
