//! Just enough JSON, with no dependencies.
//!
//! The correctness crates take no dependencies and this binary is their front
//! door, so it does not pull a serialisation framework in to read a dozen
//! request shapes. What is here is a complete RFC 8259 parser and writer for
//! the subset JSON-RPC uses — which is all of it, since JSON has no subsets.
//!
//! # The parts that are easy to get wrong, and are tested
//!
//! **`\u` escapes and surrogate pairs.** A feature name arriving as
//! `"\ud83e\uddec"` is one character, not two, and a parser that copies the
//! code units through produces a string Rust cannot hold. Unpaired surrogates
//! are real in the wild and become U+FFFD rather than an error.
//!
//! **Numbers are `f64` and integers are checked on the way out.** JSON has one
//! number type. Reading `3.7` where a length was expected is a caller error,
//! not something to round silently.
//!
//! **Depth is bounded.** A deeply nested array is a stack overflow — a crash
//! rather than an error — in any recursive-descent parser that does not count.

use std::collections::BTreeMap;

/// A JSON value. Objects keep keys sorted, so output is deterministic.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(m) => m.get(key),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }
    /// The number as an integer, or `None` if it is not one.
    ///
    /// `3.7` is not 3. A caller that meant a length and wrote a fraction has
    /// made a mistake, and rounding it away hides that.
    pub fn as_i64(&self) -> Option<i64> {
        let n = self.as_f64()?;
        (n.fract() == 0.0 && n.is_finite()).then_some(n as i64)
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    /// Part of the value API and used by the tests; kept because a JSON type
    /// missing one of its six accessors is a trap for the next caller.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(v) => Some(v),
            _ => None,
        }
    }
}

/// Build an object from pairs.
pub fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Object(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

pub fn s(v: impl Into<String>) -> Value {
    Value::String(v.into())
}

pub fn arr(v: Vec<Value>) -> Value {
    Value::Array(v)
}

/// Serialise. Deterministic: objects are emitted in key order.
pub fn write(v: &Value) -> String {
    let mut out = String::new();
    put(v, &mut out);
    out
}

fn put(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => {
            // JSON has no NaN or infinity, and emitting one produces a document
            // no parser will read back.
            if n.is_finite() {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    out.push_str(&format!("{}", *n as i64));
                } else {
                    out.push_str(&format!("{n}"));
                }
            } else {
                out.push_str("null");
            }
        }
        Value::String(x) => put_str(x, out),
        Value::Array(items) => {
            out.push('[');
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                put(it, out);
            }
            out.push(']');
        }
        Value::Object(m) => {
            out.push('{');
            for (i, (k, val)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                put_str(k, out);
                out.push(':');
                put(val, out);
            }
            out.push('}');
        }
    }
}

fn put_str(x: &str, out: &mut String) {
    out.push('"');
    for c in x.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Control characters are not legal raw in a JSON string.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Deepest nesting accepted. A recursive parser without this crashes on hostile
/// input rather than refusing it, and a crash is not an error message.
const MAX_DEPTH: usize = 64;

pub fn parse(text: &str) -> Result<Value, String> {
    let b: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    let v = value(&b, &mut i, 0)?;
    skip_ws(&b, &mut i);
    if i != b.len() {
        return Err(format!("trailing input at character {i}"));
    }
    Ok(v)
}

fn skip_ws(b: &[char], i: &mut usize) {
    while *i < b.len() && matches!(b[*i], ' ' | '\t' | '\n' | '\r') {
        *i += 1;
    }
}

fn value(b: &[char], i: &mut usize, depth: usize) -> Result<Value, String> {
    if depth > MAX_DEPTH {
        return Err(format!("nested deeper than {MAX_DEPTH}"));
    }
    skip_ws(b, i);
    let Some(&c) = b.get(*i) else {
        return Err("unexpected end of input".into());
    };
    match c {
        'n' => lit(b, i, "null", Value::Null),
        't' => lit(b, i, "true", Value::Bool(true)),
        'f' => lit(b, i, "false", Value::Bool(false)),
        '"' => Ok(Value::String(string(b, i)?)),
        '[' => {
            *i += 1;
            let mut items = Vec::new();
            skip_ws(b, i);
            if b.get(*i) == Some(&']') {
                *i += 1;
                return Ok(Value::Array(items));
            }
            loop {
                items.push(value(b, i, depth + 1)?);
                skip_ws(b, i);
                match b.get(*i) {
                    Some(',') => *i += 1,
                    Some(']') => {
                        *i += 1;
                        return Ok(Value::Array(items));
                    }
                    _ => return Err(format!("expected ',' or ']' at character {i}")),
                }
            }
        }
        '{' => {
            *i += 1;
            let mut m = BTreeMap::new();
            skip_ws(b, i);
            if b.get(*i) == Some(&'}') {
                *i += 1;
                return Ok(Value::Object(m));
            }
            loop {
                skip_ws(b, i);
                let k = string(b, i)?;
                skip_ws(b, i);
                if b.get(*i) != Some(&':') {
                    return Err(format!("expected ':' at character {i}"));
                }
                *i += 1;
                m.insert(k, value(b, i, depth + 1)?);
                skip_ws(b, i);
                match b.get(*i) {
                    Some(',') => *i += 1,
                    Some('}') => {
                        *i += 1;
                        return Ok(Value::Object(m));
                    }
                    _ => return Err(format!("expected ',' or '}}' at character {i}")),
                }
            }
        }
        '-' | '0'..='9' => number(b, i),
        c => Err(format!("unexpected {c:?} at character {i}")),
    }
}

fn lit(b: &[char], i: &mut usize, word: &str, v: Value) -> Result<Value, String> {
    if b[*i..].starts_with(&word.chars().collect::<Vec<_>>()[..]) {
        *i += word.len();
        Ok(v)
    } else {
        Err(format!("expected {word} at character {i}"))
    }
}

fn number(b: &[char], i: &mut usize) -> Result<Value, String> {
    let start = *i;
    if b.get(*i) == Some(&'-') {
        *i += 1;
    }
    while matches!(b.get(*i), Some('0'..='9' | '.' | 'e' | 'E' | '+' | '-')) {
        *i += 1;
    }
    let text: String = b[start..*i].iter().collect();
    text.parse::<f64>()
        .map(Value::Number)
        .map_err(|_| format!("bad number {text:?}"))
}

fn string(b: &[char], i: &mut usize) -> Result<String, String> {
    if b.get(*i) != Some(&'"') {
        return Err(format!("expected a string at character {i}"));
    }
    *i += 1;
    let mut out = String::new();
    loop {
        let Some(&c) = b.get(*i) else {
            return Err("unterminated string".into());
        };
        *i += 1;
        match c {
            '"' => return Ok(out),
            '\\' => {
                let Some(&e) = b.get(*i) else {
                    return Err("unterminated escape".into());
                };
                *i += 1;
                match e {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'u' => {
                        let hi = hex4(b, i)?;
                        // A surrogate pair is one character. Copying the two
                        // code units through gives a string Rust cannot hold,
                        // and an emoji in a feature name is not exotic.
                        let ch = if (0xD800..0xDC00).contains(&hi) {
                            if b.get(*i) == Some(&'\\') && b.get(*i + 1) == Some(&'u') {
                                *i += 2;
                                let lo = hex4(b, i)?;
                                if (0xDC00..0xE000).contains(&lo) {
                                    let c = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                                    char::from_u32(c).unwrap_or('\u{fffd}')
                                } else {
                                    // A high surrogate followed by something
                                    // else. Unpaired surrogates are real in the
                                    // wild; refusing the whole document over one
                                    // is worse than the replacement character.
                                    //
                                    // Rewind the six characters this branch
                                    // consumed looking for a low surrogate, so
                                    // the escape is read again as itself.
                                    //
                                    // It was being swallowed along with the
                                    // surrogate. A high surrogate then an
                                    // escaped "AB" came back as "\u{fffd}B",
                                    // with the A gone; a high surrogate then an
                                    // escaped 🧬 came back as two replacement
                                    // characters, destroying the very surrogate
                                    // pair the module header promises to
                                    // preserve. A *literal* character after an
                                    // unpaired surrogate was always safe — only
                                    // a second `\u` escape reaches this branch,
                                    // which is why the existing
                                    // unpaired-surrogate test never saw it.
                                    *i -= 6;
                                    '\u{fffd}'
                                }
                            } else {
                                '\u{fffd}'
                            }
                        } else {
                            char::from_u32(hi).unwrap_or('\u{fffd}')
                        };
                        out.push(ch);
                    }
                    other => return Err(format!("unknown escape \\{other}")),
                }
            }
            c => out.push(c),
        }
    }
}

fn hex4(b: &[char], i: &mut usize) -> Result<u32, String> {
    let s: String = b
        .get(*i..*i + 4)
        .ok_or("truncated \\u escape")?
        .iter()
        .collect();
    *i += 4;
    u32::from_str_radix(&s, 16).map_err(|_| format!("bad \\u escape {s:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(text: &str) -> String {
        write(&parse(text).expect(text))
    }

    #[test]
    fn the_shapes_json_rpc_uses_survive_a_round_trip() {
        assert_eq!(
            round(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#),
            r#"{"id":1,"jsonrpc":"2.0","method":"tools/list"}"#
        );
        assert_eq!(round("[1,2,3]"), "[1,2,3]");
        assert_eq!(round("[]"), "[]");
        assert_eq!(round("{}"), "{}");
        assert_eq!(round("null"), "null");
        assert_eq!(round(" [ true , false ] "), "[true,false]");
    }

    #[test]
    fn a_surrogate_pair_is_one_character() {
        // An emoji in a feature name is not exotic, and a parser that copies
        // the two code units through produces a string Rust cannot hold.
        let v = parse(r#""\ud83e\uddec""#).unwrap();
        assert_eq!(v.as_str(), Some("🧬"));
        assert_eq!(v.as_str().unwrap().chars().count(), 1);
        // And it comes back out as the character, which is legal JSON.
        assert_eq!(write(&v), "\"🧬\"");
    }

    #[test]
    fn an_unpaired_surrogate_does_not_lose_the_whole_document() {
        // These are real in the wild. Refusing an entire request over one is
        // worse than the replacement character.
        assert_eq!(parse(r#""\ud83e""#).unwrap().as_str(), Some("\u{fffd}"));
        assert_eq!(parse(r#""a\udc00b""#).unwrap().as_str(), Some("a\u{fffd}b"));
    }

    #[test]
    fn an_unpaired_surrogate_does_not_swallow_the_escape_that_follows_it() {
        // The escape after a high surrogate was consumed while looking for a
        // low surrogate and then never emitted, so one replacement character
        // ate a second character with it.
        assert_eq!(
            parse(r#""\ud83e\u0041\u0042""#).unwrap().as_str(),
            Some("\u{fffd}AB"),
            "the A was being dropped"
        );
        assert_eq!(
            parse(r#""x\ud83e\u0020y""#).unwrap().as_str(),
            Some("x\u{fffd} y")
        );
        // Worst of all, the escape that followed could be a *valid* pair — the
        // emoji the module header promises to preserve, destroyed by an
        // unpaired surrogate in front of it.
        assert_eq!(
            parse(r#""\ud83e\ud83e\uddec""#).unwrap().as_str(),
            Some("\u{fffd}🧬")
        );
        // A literal character after the surrogate always took the other branch
        // and was always safe. It still is.
        assert_eq!(
            parse(r#""\ud83eC:\\x.gb""#).unwrap().as_str(),
            Some("\u{fffd}C:\\x.gb")
        );
    }

    #[test]
    fn control_characters_are_escaped_on_the_way_out() {
        // Raw control characters are not legal in a JSON string, so a feature
        // name holding one would produce a document no parser accepts.
        let v = s("a\u{1}b\nc");
        assert_eq!(write(&v), r#""a\u0001b\nc""#);
        assert_eq!(parse(&write(&v)).unwrap(), v);
    }

    #[test]
    fn quotes_and_backslashes_in_a_name_cannot_break_out_of_the_string() {
        for x in [
            r#"aph(3')-Ia"#,
            r#"say "hi""#,
            r"C:\temp\thing",
            "back\\\\slash",
        ] {
            let v = s(x);
            assert_eq!(parse(&write(&v)).unwrap().as_str(), Some(x), "{x}");
        }
    }

    #[test]
    fn an_integer_is_an_integer_and_a_fraction_is_not() {
        assert_eq!(parse("42").unwrap().as_i64(), Some(42));
        assert_eq!(parse("-7").unwrap().as_i64(), Some(-7));
        assert_eq!(parse("1e3").unwrap().as_i64(), Some(1000));
        // A caller who meant a length and wrote 3.7 has made a mistake, and
        // rounding it away hides that.
        assert_eq!(parse("3.7").unwrap().as_i64(), None);
        assert_eq!(parse("3.7").unwrap().as_f64(), Some(3.7));
    }

    #[test]
    fn deep_nesting_is_refused_rather_than_overflowing_the_stack() {
        // A crash is not an error message, and this is reachable from any
        // client that can send bytes.
        let deep = format!("{}{}", "[".repeat(500), "]".repeat(500));
        let e = parse(&deep).unwrap_err();
        assert!(e.contains("nested deeper"), "{e}");
        // Just inside the limit still works.
        let ok = format!("{}1{}", "[".repeat(60), "]".repeat(60));
        assert!(parse(&ok).is_ok());
    }

    #[test]
    fn malformed_input_is_an_error_and_not_a_panic() {
        for bad in [
            "",
            "{",
            "}",
            "[1,",
            r#"{"a"}"#,
            r#"{"a":}"#,
            "tru",
            r#""unterminated"#,
            r#""\q""#,
            r#""\u12""#,
            "[1] 2",
            "01x2",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn objects_come_out_in_a_stable_order() {
        // Two identical requests must produce identical bytes, so a transcript
        // can be diffed.
        let a = obj(vec![
            ("z", s("1")),
            ("a", s("2")),
            ("m", Value::Number(3.0)),
        ]);
        assert_eq!(write(&a), r#"{"a":"2","m":3,"z":"1"}"#);
        assert_eq!(write(&a), write(&a));
    }

    #[test]
    fn a_number_that_json_cannot_express_becomes_null_not_garbage() {
        // NaN and infinity have no JSON spelling; emitting one produces a
        // document nothing can read back.
        assert_eq!(write(&Value::Number(f64::NAN)), "null");
        assert_eq!(write(&Value::Number(f64::INFINITY)), "null");
        assert_eq!(write(&Value::Number(1.5)), "1.5");
        assert_eq!(write(&Value::Number(-0.0)), "0");
    }
}
