use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).expect("OS random source unavailable");
    buf
}

/// A random URL-safe token with 256 bits of entropy.
pub fn random_token() -> String {
    b64url_encode(&random_bytes::<32>())
}

/// Human-friendly code without look-alike characters (no 0/O, 1/I/L).
pub fn random_code(len: usize) -> String {
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
    let mut out = String::with_capacity(len);
    while out.len() < len {
        for b in random_bytes::<16>() {
            // Rejection sampling keeps the distribution uniform.
            if (b as usize) < 256 - 256 % ALPHABET.len() && out.len() < len {
                out.push(ALPHABET[b as usize % ALPHABET.len()] as char);
            }
        }
    }
    out
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn sha256_hex(data: &str) -> String {
    sha256(data.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares two strings without leaking where they differ through timing.
pub fn constant_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode_with(data: &[u8], url: bool) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..=chunk.len() {
            let c = B64[(n >> (18 - 6 * i) & 63) as usize];
            out.push(match (url, c) {
                (true, b'+') => '-',
                (true, b'/') => '_',
                _ => c as char,
            });
        }
        if !url {
            for _ in chunk.len()..3 {
                out.push('=');
            }
        }
    }
    out
}

pub fn b64url_encode(data: &[u8]) -> String {
    b64_encode_with(data, true)
}

pub fn b64_encode(data: &[u8]) -> String {
    b64_encode_with(data, false)
}

/// Decodes base64url, with or without padding. Also accepts standard base64.
pub fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    // A single leftover base64 character cannot encode a whole byte.
    if s.len() % 4 == 1 {
        return None;
    }
    Some(out)
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip() {
        for len in 0..40 {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(b64url_decode(&b64url_encode(&data)).unwrap(), data);
            assert_eq!(b64url_decode(&b64_encode(&data)).unwrap(), data);
        }
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64_encode(b"fo"), "Zm8=");
        assert_eq!(b64url_encode(&[0xfb, 0xff]), "-_8");
        assert!(b64url_decode("a").is_none());
        assert!(b64url_decode("ab$c").is_none());
    }

    #[test]
    fn codes_use_the_alphabet() {
        let c = random_code(8);
        assert_eq!(c.len(), 8);
        assert!(c.chars().all(|ch| "23456789ABCDEFGHJKMNPQRSTUVWXYZ".contains(ch)));
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%3Ab%20c"), "a:b c");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn constant_time_compare() {
        assert!(constant_eq("abc", "abc"));
        assert!(!constant_eq("abc", "abd"));
        assert!(!constant_eq("abc", "ab"));
    }
}
