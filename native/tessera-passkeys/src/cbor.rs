//! The little of CBOR (RFC 8949) CTAP2 speaks: unsigned and negative
//! integers, byte and text strings, arrays, maps, true/false/null. Encoding
//! is CTAP2's canonical form (definite lengths, shortest heads, map keys
//! sorted by major type, then length, then bytes). Decoding refuses what
//! CTAP never sends (indefinite lengths, tags, floats) and anything deeper
//! than `MAX_DEPTH` or longer than its input.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Bool(bool),
    Null,
}

pub const MAX_DEPTH: usize = 8;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// the input ends early
    Short,
    /// something CTAP never sends, or not CBOR
    Invalid,
    /// bytes left after the value
    Trailing,
}

impl Value {
    pub fn text(s: &str) -> Value {
        Value::Text(s.to_string())
    }
    pub fn as_int(&self) -> Option<i64> {
        if let Value::Int(i) = self { Some(*i) } else { None }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        if let Value::Bytes(b) = self { Some(b) } else { None }
    }
    pub fn as_text(&self) -> Option<&str> {
        if let Value::Text(t) = self { Some(t) } else { None }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Value::Bool(b) = self { Some(*b) } else { None }
    }
    pub fn as_array(&self) -> Option<&[Value]> {
        if let Value::Array(a) = self { Some(a) } else { None }
    }
    pub fn as_map(&self) -> Option<&[(Value, Value)]> {
        if let Value::Map(m) = self { Some(m) } else { None }
    }
    /// A map's value under an integer key
    pub fn get(&self, key: i64) -> Option<&Value> {
        self.as_map()?.iter().find(|(k, _)| *k == Value::Int(key)).map(|(_, v)| v)
    }
    /// A map's value under a text key
    pub fn get_text(&self, key: &str) -> Option<&Value> {
        self.as_map()?.iter().find(|(k, _)| k.as_text() == Some(key)).map(|(_, v)| v)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_into(&mut out);
        out
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Value::Int(i) if *i >= 0 => head(out, 0, *i as u64),
            Value::Int(i) => head(out, 1, (-1 - *i) as u64),
            Value::Bytes(b) => {
                head(out, 2, b.len() as u64);
                out.extend_from_slice(b);
            }
            Value::Text(t) => {
                head(out, 3, t.len() as u64);
                out.extend_from_slice(t.as_bytes());
            }
            Value::Array(a) => {
                head(out, 4, a.len() as u64);
                for v in a {
                    v.encode_into(out);
                }
            }
            Value::Map(m) => {
                head(out, 5, m.len() as u64);
                let mut entries: Vec<(Vec<u8>, &Value)> = m.iter().map(|(k, v)| (k.encode(), v)).collect();
                entries.sort_by(|a, b| canonical_order(&a.0, &b.0));
                for (k, v) in entries {
                    out.extend_from_slice(&k);
                    v.encode_into(out);
                }
            }
            Value::Bool(false) => out.push(0xf4),
            Value::Bool(true) => out.push(0xf5),
            Value::Null => out.push(0xf6),
        }
    }
}

/// CTAP2's key order: major type, then length, then the bytes
pub fn canonical_order(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let major = |x: &[u8]| x.first().map_or(0, |b| b >> 5);
    major(a).cmp(&major(b)).then(a.len().cmp(&b.len())).then(a.cmp(b))
}

fn head(out: &mut Vec<u8>, major: u8, n: u64) {
    let m = major << 5;
    if n < 24 {
        out.push(m | n as u8);
    } else if n <= 0xff {
        out.push(m | 24);
        out.push(n as u8);
    } else if n <= 0xffff {
        out.push(m | 25);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else if n <= 0xffff_ffff {
        out.push(m | 26);
        out.extend_from_slice(&(n as u32).to_be_bytes());
    } else {
        out.push(m | 27);
        out.extend_from_slice(&n.to_be_bytes());
    }
}

/// One whole value, nothing after it
pub fn decode(input: &[u8]) -> Result<Value, Error> {
    let mut at = 0;
    let v = decode_at(input, &mut at, 0)?;
    if at != input.len() {
        return Err(Error::Trailing);
    }
    Ok(v)
}

fn take<'a>(input: &'a [u8], at: &mut usize, n: usize) -> Result<&'a [u8], Error> {
    let end = at.checked_add(n).ok_or(Error::Short)?;
    if end > input.len() {
        return Err(Error::Short);
    }
    let s = &input[*at..end];
    *at = end;
    Ok(s)
}

fn argument(input: &[u8], at: &mut usize, info: u8) -> Result<u64, Error> {
    Ok(match info {
        0..=23 => info as u64,
        24 => take(input, at, 1)?[0] as u64,
        25 => u16::from_be_bytes(take(input, at, 2)?.try_into().unwrap()) as u64,
        26 => u32::from_be_bytes(take(input, at, 4)?.try_into().unwrap()) as u64,
        27 => u64::from_be_bytes(take(input, at, 8)?.try_into().unwrap()),
        _ => return Err(Error::Invalid), // (indefinite lengths, reserved)
    })
}

fn decode_at(input: &[u8], at: &mut usize, depth: usize) -> Result<Value, Error> {
    if depth > MAX_DEPTH {
        return Err(Error::Invalid);
    }
    let first = take(input, at, 1)?[0];
    let (major, info) = (first >> 5, first & 0x1f);
    if major == 7 {
        return match info {
            20 => Ok(Value::Bool(false)),
            21 => Ok(Value::Bool(true)),
            22 => Ok(Value::Null),
            _ => Err(Error::Invalid),
        };
    }
    let n = argument(input, at, info)?;
    match major {
        0 => i64::try_from(n).map(Value::Int).map_err(|_| Error::Invalid),
        1 => i64::try_from(n).map(|n| Value::Int(-1 - n)).map_err(|_| Error::Invalid),
        2 => Ok(Value::Bytes(take(input, at, usize::try_from(n).map_err(|_| Error::Short)?)?.to_vec())),
        3 => {
            let b = take(input, at, usize::try_from(n).map_err(|_| Error::Short)?)?;
            String::from_utf8(b.to_vec()).map(Value::Text).map_err(|_| Error::Invalid)
        }
        4 => {
            // (each item is at least a byte: a count past the input is short)
            if n > (input.len() - *at) as u64 {
                return Err(Error::Short);
            }
            let mut a = Vec::with_capacity(n as usize);
            for _ in 0..n {
                a.push(decode_at(input, at, depth + 1)?);
            }
            Ok(Value::Array(a))
        }
        5 => {
            if n.saturating_mul(2) > (input.len() - *at) as u64 {
                return Err(Error::Short);
            }
            let mut m = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let k = decode_at(input, at, depth + 1)?;
                let v = decode_at(input, at, depth + 1)?;
                m.push((k, v));
            }
            Ok(Value::Map(m))
        }
        _ => Err(Error::Invalid), // (tags)
    }
}

/// A map with integer keys, as CTAP2 responses are
pub fn int_map(entries: Vec<(i64, Value)>) -> Value {
    Value::Map(entries.into_iter().map(|(k, v)| (Value::Int(k), v)).collect())
}

/// A map with text keys
pub fn text_map(entries: Vec<(&str, Value)>) -> Value {
    Value::Map(entries.into_iter().map(|(k, v)| (Value::text(k), v)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_take_their_shortest_heads() {
        assert_eq!(Value::Int(0).encode(), [0x00]);
        assert_eq!(Value::Int(23).encode(), [0x17]);
        assert_eq!(Value::Int(24).encode(), [0x18, 0x18]);
        assert_eq!(Value::Int(1000).encode(), [0x19, 0x03, 0xe8]);
        assert_eq!(Value::Int(-1).encode(), [0x20]);
        assert_eq!(Value::Int(-7).encode(), [0x26]);
        assert_eq!(Value::Int(-8).encode(), [0x27]);
        assert_eq!(Value::Int(-257).encode(), [0x39, 0x01, 0x00]);
        assert_eq!(Value::Int(1 << 40).encode(), [0x1b, 0, 0, 1, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn maps_come_out_in_ctap_order() {
        // (a COSE key given in any order: 1, 3, -1, -2, -3)
        let m = int_map(vec![(-3, Value::Int(0)), (1, Value::Int(2)), (-1, Value::Int(1)), (3, Value::Int(-7)), (-2, Value::Int(0))]);
        assert_eq!(m.encode(), [0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x00, 0x22, 0x00]);
        // ints before texts, shorter texts first
        let m = Value::Map(vec![(Value::text("up"), Value::Bool(true)), (Value::text("rk"), Value::Bool(false)), (Value::text("plat"), Value::Bool(false)), (Value::Int(1000), Value::Null)]);
        let e = m.encode();
        assert_eq!(&e[..4], &[0xa4, 0x19, 0x03, 0xe8]);
        assert_eq!(decode(&e).unwrap().as_map().unwrap()[1].0, Value::text("rk"));
    }

    #[test]
    fn values_come_back_as_they_went() {
        let v = int_map(vec![
            (1, Value::Bytes(vec![1, 2, 3])),
            (2, text_map(vec![("id", Value::text("example.com")), ("name", Value::text("Example"))])),
            (3, Value::Array(vec![Value::Bool(true), Value::Null, Value::Int(-8)])),
            (4, Value::Bytes(vec![0u8; 300])),
        ]);
        assert_eq!(decode(&v.encode()).unwrap(), v);
        assert_eq!(v.get(2).unwrap().get_text("id").unwrap().as_text(), Some("example.com"));
    }

    #[test]
    fn bad_input_is_refused_not_trusted() {
        assert_eq!(decode(&[]), Err(Error::Short));
        assert_eq!(decode(&[0x42, 1]), Err(Error::Short)); // (2 bytes promised, 1 there)
        assert_eq!(decode(&[0x9f]), Err(Error::Invalid)); // (indefinite)
        assert_eq!(decode(&[0xc0, 0x00]), Err(Error::Invalid)); // (a tag)
        assert_eq!(decode(&[0xf9, 0, 0]), Err(Error::Invalid)); // (a float)
        assert_eq!(decode(&[0x00, 0x00]), Err(Error::Trailing));
        assert_eq!(decode(&[0x62, 0xff, 0xfe]), Err(Error::Invalid)); // (not UTF-8)
        // a huge count with nothing after it
        assert_eq!(decode(&[0x9b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]), Err(Error::Short));
        assert_eq!(decode(&[0xbb, 0, 0, 0, 1, 0, 0, 0, 0]), Err(Error::Short));
        // too deep
        let deep: Vec<u8> = std::iter::repeat(0x81).take(20).chain([0x00]).collect();
        assert_eq!(decode(&deep), Err(Error::Invalid));
    }
}
