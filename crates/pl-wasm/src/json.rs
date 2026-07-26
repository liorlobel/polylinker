//! A minimal JSON writer.
//!
//! Hand-written for the same reason the rest of this workspace has no
//! dependencies: it is a hundred lines, it is the only thing crossing the wasm
//! boundary, and a reviewer can check it in one sitting.

pub struct Json {
    buf: String,
    /// True when the current container already holds an element, so the next
    /// one needs a comma. One flag per nesting level.
    needs_comma: Vec<bool>,
}

impl Json {
    pub fn new() -> Self {
        Json {
            buf: String::with_capacity(4096),
            needs_comma: vec![false],
        }
    }

    pub fn finish(self) -> String {
        self.buf
    }

    fn sep(&mut self) {
        if let Some(last) = self.needs_comma.last_mut() {
            if *last {
                self.buf.push(',');
            } else {
                *last = true;
            }
        }
    }

    pub fn obj(&mut self) -> &mut Self {
        self.sep();
        self.buf.push('{');
        self.needs_comma.push(false);
        self
    }
    pub fn end_obj(&mut self) -> &mut Self {
        self.buf.push('}');
        self.needs_comma.pop();
        self
    }
    pub fn arr(&mut self) -> &mut Self {
        self.sep();
        self.buf.push('[');
        self.needs_comma.push(false);
        self
    }
    pub fn end_arr(&mut self) -> &mut Self {
        self.buf.push(']');
        self.needs_comma.pop();
        self
    }

    /// A key inside an object. The value that follows is part of this member,
    /// not a new element, so the comma flag is cleared after writing the colon.
    pub fn key(&mut self, k: &str) -> &mut Self {
        self.sep();
        escape_into(k, &mut self.buf);
        self.buf.push(':');
        if let Some(last) = self.needs_comma.last_mut() {
            *last = false;
        }
        self
    }

    pub fn str(&mut self, v: &str) -> &mut Self {
        self.sep();
        escape_into(v, &mut self.buf);
        self
    }
    pub fn num(&mut self, v: u64) -> &mut Self {
        self.sep();
        self.buf.push_str(&v.to_string());
        self
    }
    pub fn float(&mut self, v: f64) -> &mut Self {
        self.sep();
        // JSON has no NaN or Infinity; emit null rather than invalid output.
        if v.is_finite() {
            self.buf.push_str(&format!("{v}"));
        } else {
            self.buf.push_str("null");
        }
        self
    }
    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.sep();
        self.buf.push_str(if v { "true" } else { "false" });
        self
    }
    pub fn null(&mut self) -> &mut Self {
        self.sep();
        self.buf.push_str("null");
        self
    }

    pub fn kv_str(&mut self, k: &str, v: &str) -> &mut Self {
        self.key(k).str(v)
    }
    pub fn kv_num(&mut self, k: &str, v: u64) -> &mut Self {
        self.key(k).num(v)
    }
    pub fn kv_bool(&mut self, k: &str, v: bool) -> &mut Self {
        self.key(k).bool(v)
    }
    pub fn kv_opt_str(&mut self, k: &str, v: Option<&str>) -> &mut Self {
        match v {
            Some(s) => self.key(k).str(s),
            None => self.key(k).null(),
        }
    }
    pub fn kv_opt_float(&mut self, k: &str, v: Option<f64>) -> &mut Self {
        match v {
            Some(x) => self.key(k).float(x),
            None => self.key(k).null(),
        }
    }
}

impl Default for Json {
    fn default() -> Self {
        Self::new()
    }
}

fn escape_into(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_nested_structures() {
        let mut j = Json::new();
        j.obj()
            .kv_str("name", "pUC19")
            .kv_num("bp", 2686)
            .kv_bool("circular", true)
            .key("features")
            .arr()
            .obj()
            .kv_str("label", "AmpR")
            .kv_num("start", 1)
            .end_obj()
            .obj()
            .kv_str("label", "ori")
            .kv_num("start", 9)
            .end_obj()
            .end_arr()
            .end_obj();
        assert_eq!(
            j.finish(),
            r#"{"name":"pUC19","bp":2686,"circular":true,"features":[{"label":"AmpR","start":1},{"label":"ori","start":9}]}"#
        );
    }

    #[test]
    fn escapes_what_json_requires() {
        let mut j = Json::new();
        j.obj().kv_str("k", "a\"b\\c\nd\te\u{01}f").end_obj();
        assert_eq!(j.finish(), r#"{"k":"a\"b\\c\nd\te\u0001f"}"#);
    }

    #[test]
    fn keeps_multibyte_text_intact() {
        let mut j = Json::new();
        j.obj().kv_str("label", "δ subunit").end_obj();
        assert_eq!(j.finish(), r#"{"label":"δ subunit"}"#);
    }

    #[test]
    fn nulls_and_non_finite_floats() {
        let mut j = Json::new();
        j.obj()
            .kv_opt_str("color", None)
            .kv_opt_float("tm", Some(55.5))
            .kv_opt_float("bad", Some(f64::NAN))
            .kv_opt_float("missing", None)
            .end_obj();
        assert_eq!(
            j.finish(),
            r#"{"color":null,"tm":55.5,"bad":null,"missing":null}"#
        );
    }

    #[test]
    fn empty_containers_are_valid() {
        let mut j = Json::new();
        j.obj()
            .key("a")
            .arr()
            .end_arr()
            .key("b")
            .obj()
            .end_obj()
            .end_obj();
        assert_eq!(j.finish(), r#"{"a":[],"b":{}}"#);
    }
}
