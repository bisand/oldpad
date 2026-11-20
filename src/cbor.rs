//! Minimal CBOR decoder: just enough for WebAuthn attestation objects and
//! COSE keys. Definite lengths only, depth- and size-limited.

#[derive(Debug, Clone, PartialEq)]
pub enum Cbor {
    Int(i64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Cbor>),
    Map(Vec<(Cbor, Cbor)>),
    Bool(bool),
    Null,
}

impl Cbor {
    pub fn get(&self, key: &Cbor) -> Option<&Cbor> {
        match self {
            Cbor::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn get_text(&self, key: &str) -> Option<&Cbor> {
        self.get(&Cbor::Text(key.into()))
    }
    pub fn get_int(&self, key: i64) -> Option<&Cbor> {
        self.get(&Cbor::Int(key))
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        if let Cbor::Bytes(b) = self { Some(b) } else { None }
    }
    pub fn as_int(&self) -> Option<i64> {
        if let Cbor::Int(i) = self { Some(*i) } else { None }
    }
}

const MAX_DEPTH: usize = 16;

/// Decodes one item and returns it with the number of bytes it used.
pub fn decode(data: &[u8]) -> Result<(Cbor, usize), &'static str> {
    let mut pos = 0;
    let value = item(data, &mut pos, 0)?;
    Ok((value, pos))
}

fn take<'a>(data: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], &'static str> {
    let end = pos.checked_add(n).ok_or("length overflow")?;
    let slice = data.get(*pos..end).ok_or("truncated")?;
    *pos = end;
    Ok(slice)
}

fn argument(data: &[u8], pos: &mut usize, info: u8) -> Result<u64, &'static str> {
    Ok(match info {
        0..=23 => info as u64,
        24 => take(data, pos, 1)?[0] as u64,
        25 => u16::from_be_bytes(take(data, pos, 2)?.try_into().unwrap()) as u64,
        26 => u32::from_be_bytes(take(data, pos, 4)?.try_into().unwrap()) as u64,
        27 => u64::from_be_bytes(take(data, pos, 8)?.try_into().unwrap()),
        _ => return Err("indefinite lengths are not supported"),
    })
}

fn item(data: &[u8], pos: &mut usize, depth: usize) -> Result<Cbor, &'static str> {
    if depth > MAX_DEPTH {
        return Err("nesting too deep");
    }
    let head = take(data, pos, 1)?[0];
    let (major, info) = (head >> 5, head & 0x1f);
    if major == 7 {
        return match info {
            20 => Ok(Cbor::Bool(false)),
            21 => Ok(Cbor::Bool(true)),
            22 | 23 => Ok(Cbor::Null),
            _ => Err("unsupported simple value"),
        };
    }
    let arg = argument(data, pos, info)?;
    // Every element takes at least one byte, so a count beyond the input is a lie.
    let count = |n: u64| -> Result<usize, &'static str> {
        if n > (data.len() - *pos) as u64 { Err("length exceeds input") } else { Ok(n as usize) }
    };
    Ok(match major {
        0 => Cbor::Int(i64::try_from(arg).map_err(|_| "integer too large")?),
        1 => Cbor::Int(-1 - i64::try_from(arg).map_err(|_| "integer too large")?),
        2 => Cbor::Bytes(take(data, pos, count(arg)?)?.to_vec()),
        3 => Cbor::Text(String::from_utf8(take(data, pos, count(arg)?)?.to_vec()).map_err(|_| "invalid utf-8")?),
        4 => {
            let n = count(arg)?;
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(item(data, pos, depth + 1)?);
            }
            Cbor::Array(items)
        }
        5 => {
            let n = count(arg)?;
            let mut entries = Vec::with_capacity(n);
            for _ in 0..n {
                let k = item(data, pos, depth + 1)?;
                let v = item(data, pos, depth + 1)?;
                entries.push((k, v));
            }
            Cbor::Map(entries)
        }
        6 => item(data, pos, depth + 1)?, // tag: ignore it, keep the value
        _ => return Err("unsupported major type"),
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Tiny encoder, used by tests to build WebAuthn fixtures.
    pub fn encode(v: &Cbor) -> Vec<u8> {
        fn head(out: &mut Vec<u8>, major: u8, n: u64) {
            let m = major << 5;
            match n {
                0..=23 => out.push(m | n as u8),
                24..=0xff => out.extend([m | 24, n as u8]),
                0x100..=0xffff => {
                    out.push(m | 25);
                    out.extend((n as u16).to_be_bytes())
                }
                _ => {
                    out.push(m | 26);
                    out.extend((n as u32).to_be_bytes())
                }
            }
        }
        let mut out = Vec::new();
        match v {
            Cbor::Int(i) if *i >= 0 => head(&mut out, 0, *i as u64),
            Cbor::Int(i) => head(&mut out, 1, (-1 - *i) as u64),
            Cbor::Bytes(b) => {
                head(&mut out, 2, b.len() as u64);
                out.extend(b)
            }
            Cbor::Text(t) => {
                head(&mut out, 3, t.len() as u64);
                out.extend(t.as_bytes())
            }
            Cbor::Array(a) => {
                head(&mut out, 4, a.len() as u64);
                a.iter().for_each(|x| out.extend(encode(x)))
            }
            Cbor::Map(m) => {
                head(&mut out, 5, m.len() as u64);
                m.iter().for_each(|(k, x)| {
                    out.extend(encode(k));
                    out.extend(encode(x))
                })
            }
            Cbor::Bool(b) => out.push(if *b { 0xf5 } else { 0xf4 }),
            Cbor::Null => out.push(0xf6),
        }
        out
    }

    #[test]
    fn round_trip() {
        let v = Cbor::Map(vec![
            (Cbor::Int(1), Cbor::Int(2)),
            (Cbor::Int(-7), Cbor::Bytes(vec![1; 300])),
            (Cbor::Text("fmt".into()), Cbor::Array(vec![Cbor::Bool(true), Cbor::Null, Cbor::Int(-300)])),
        ]);
        let bytes = encode(&v);
        assert_eq!(decode(&bytes).unwrap(), (v, bytes.len()));
    }

    #[test]
    fn rejects_hostile_input() {
        assert!(decode(&[0x5b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]).is_err()); // huge byte string
        assert!(decode(&[0x9a, 0xff, 0xff, 0xff, 0xff]).is_err()); // huge array
        assert!(decode(&[0x5f]).is_err()); // indefinite length
        assert!(decode(&[0x81; 40]).is_err()); // too deep
        assert!(decode(&[]).is_err());
    }
}
