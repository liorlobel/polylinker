//! Just enough JSON to read a fixture.
//!
//! Test-only, and deliberately not in `pl-core`: the workspace ships no JSON
//! reader because nothing in the product needs one, and adding a public parser
//! to satisfy a test would enlarge the audited surface for no user's benefit.
//!
//! Panics on malformed input, which for a file this crate generates itself is
//! the correct response.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

impl Json {
    pub fn parse(s: &str) -> Result<Json, String> {
        let b = s.as_bytes();
        let mut i = 0;
        let v = value(b, &mut i)?;
        skip_ws(b, &mut i);
        if i != b.len() {
            return Err(format!("trailing input at byte {i}"));
        }
        Ok(v)
    }

    /// Field lookup that says which key is missing rather than `None`.
    pub fn get(&self, key: &str) -> &Json {
        match self {
            Json::Obj(m) => m.get(key).unwrap_or_else(|| {
                panic!("no key {key:?}; have {:?}", m.keys().collect::<Vec<_>>())
            }),
            other => panic!("expected an object for key {key:?}, found {other:?}"),
        }
    }

    pub fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(v) => v,
            other => panic!("expected an array, found {other:?}"),
        }
    }

    pub fn num(&self) -> f64 {
        match self {
            Json::Num(v) => *v,
            other => panic!("expected a number, found {other:?}"),
        }
    }

    /// `null` reads as absent — the fixture spells a dropped label that way,
    /// because JSON cannot carry `NaN`.
    pub fn opt_num(&self) -> Option<f64> {
        match self {
            Json::Null => None,
            other => Some(other.num()),
        }
    }

    pub fn nums(&self) -> Vec<f64> {
        self.arr().iter().map(Json::num).collect()
    }

    pub fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            other => panic!("expected a string, found {other:?}"),
        }
    }

    pub fn opt_str(&self) -> Option<String> {
        match self {
            Json::Null => None,
            other => Some(other.str().to_string()),
        }
    }

    pub fn boolean(&self) -> bool {
        match self {
            Json::Bool(b) => *b,
            other => panic!("expected a bool, found {other:?}"),
        }
    }
}

fn skip_ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && matches!(b[*i], b' ' | b'\t' | b'\n' | b'\r') {
        *i += 1;
    }
}

fn value(b: &[u8], i: &mut usize) -> Result<Json, String> {
    skip_ws(b, i);
    match b.get(*i) {
        None => Err("unexpected end of input".into()),
        Some(b'{') => object(b, i),
        Some(b'[') => array(b, i),
        Some(b'"') => string(b, i).map(Json::Str),
        Some(b't') => lit(b, i, "true", Json::Bool(true)),
        Some(b'f') => lit(b, i, "false", Json::Bool(false)),
        Some(b'n') => lit(b, i, "null", Json::Null),
        Some(_) => number(b, i),
    }
}

fn lit(b: &[u8], i: &mut usize, word: &str, out: Json) -> Result<Json, String> {
    if b[*i..].starts_with(word.as_bytes()) {
        *i += word.len();
        Ok(out)
    } else {
        Err(format!("expected {word} at byte {i}"))
    }
}

fn object(b: &[u8], i: &mut usize) -> Result<Json, String> {
    *i += 1;
    let mut m = BTreeMap::new();
    skip_ws(b, i);
    if b.get(*i) == Some(&b'}') {
        *i += 1;
        return Ok(Json::Obj(m));
    }
    loop {
        skip_ws(b, i);
        let k = string(b, i)?;
        skip_ws(b, i);
        if b.get(*i) != Some(&b':') {
            return Err(format!("expected ':' at byte {i}"));
        }
        *i += 1;
        m.insert(k, value(b, i)?);
        skip_ws(b, i);
        match b.get(*i) {
            Some(b',') => *i += 1,
            Some(b'}') => {
                *i += 1;
                return Ok(Json::Obj(m));
            }
            _ => return Err(format!("expected ',' or '}}' at byte {i}")),
        }
    }
}

fn array(b: &[u8], i: &mut usize) -> Result<Json, String> {
    *i += 1;
    let mut v = Vec::new();
    skip_ws(b, i);
    if b.get(*i) == Some(&b']') {
        *i += 1;
        return Ok(Json::Arr(v));
    }
    loop {
        v.push(value(b, i)?);
        skip_ws(b, i);
        match b.get(*i) {
            Some(b',') => *i += 1,
            Some(b']') => {
                *i += 1;
                return Ok(Json::Arr(v));
            }
            _ => return Err(format!("expected ',' or ']' at byte {i}")),
        }
    }
}

fn string(b: &[u8], i: &mut usize) -> Result<String, String> {
    if b.get(*i) != Some(&b'"') {
        return Err(format!("expected a string at byte {i}"));
    }
    *i += 1;
    let mut out = String::new();
    loop {
        let c = *b.get(*i).ok_or("unterminated string")?;
        *i += 1;
        match c {
            b'"' => return Ok(out),
            b'\\' => {
                let e = *b.get(*i).ok_or("unterminated escape")?;
                *i += 1;
                match e {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let hex = std::str::from_utf8(&b[*i..*i + 4]).map_err(|e| e.to_string())?;
                        let cp = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
                        *i += 4;
                        // Surrogate pairs: the generator escapes non-ASCII, and
                        // an emoji in a feature name would otherwise arrive as
                        // two unpaired halves.
                        let ch = if (0xD800..0xDC00).contains(&cp) {
                            if b.get(*i) != Some(&b'\\') || b.get(*i + 1) != Some(&b'u') {
                                return Err("lone high surrogate".into());
                            }
                            let lo = std::str::from_utf8(&b[*i + 2..*i + 6])
                                .map_err(|e| e.to_string())?;
                            let lo = u32::from_str_radix(lo, 16).map_err(|e| e.to_string())?;
                            *i += 6;
                            char::from_u32(0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00))
                        } else {
                            char::from_u32(cp)
                        };
                        out.push(ch.ok_or("invalid code point")?);
                    }
                    other => return Err(format!("bad escape \\{}", other as char)),
                }
            }
            _ => {
                // Copy the whole UTF-8 sequence, not the lead byte.
                let extra = match c {
                    0x00..=0x7F => 0,
                    0xC0..=0xDF => 1,
                    0xE0..=0xEF => 2,
                    _ => 3,
                };
                let bytes = &b[*i - 1..*i + extra];
                out.push_str(std::str::from_utf8(bytes).map_err(|e| e.to_string())?);
                *i += extra;
            }
        }
    }
}

fn number(b: &[u8], i: &mut usize) -> Result<Json, String> {
    let start = *i;
    while *i < b.len() && matches!(b[*i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E') {
        *i += 1;
    }
    std::str::from_utf8(&b[start..*i])
        .map_err(|e| e.to_string())?
        .parse::<f64>()
        .map(Json::Num)
        .map_err(|e| format!("bad number at byte {start}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_the_generator_writes() {
        let v = Json::parse(r#"{"a": [1, -2.5, 1e3, null, true], "b": "x\tyé🧬"}"#).unwrap();
        assert_eq!(v.get("a").arr().len(), 5);
        assert_eq!(v.get("a").arr()[1].num(), -2.5);
        assert_eq!(v.get("a").arr()[2].num(), 1000.0);
        assert_eq!(v.get("a").arr()[3].opt_num(), None);
        assert!(v.get("a").arr()[4].boolean());
        assert_eq!(v.get("b").str(), "x\ty\u{e9}\u{1f9ec}");
    }

    #[test]
    fn rejects_rather_than_guesses() {
        for bad in [r#"{"a": }"#, "[1, 2", r#""abc"#, "{,}", "[1] junk", ""] {
            assert!(Json::parse(bad).is_err(), "accepted {bad:?}");
        }
    }
}
