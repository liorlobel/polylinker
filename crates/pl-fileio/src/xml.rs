//! A deliberately small XML reader for the payloads inside a `.dna` file.
//!
//! Not a general XML parser and not trying to be one. The payloads are
//! machine-generated, shallow, and attribute-carrying: `<Feature name="..">`
//! with nested `<Segment/>`, `<Q><V/></Q>`, `<Primer><BindingSite/></Primer>`.
//! A hundred lines that are fully exercised against a real corpus beat a
//! dependency whose surface we would use two percent of.
//!
//! It handles what those payloads actually contain — attributes in either
//! quote style, self-closing tags, character and numeric entities, comments,
//! declarations and CDATA — and ignores namespaces, DTDs and validation,
//! none of which appear.

/// A single event from the scanner.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// `<Name a="1">` or `<Name a="1"/>`; `self_closing` distinguishes them.
    Open {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },
    Close {
        name: String,
    },
    Text(String),
}

impl Event {
    pub fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
        attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Expand the entity forms that appear in these payloads.
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'&' {
            // Copy whole UTF-8 characters, not bytes.
            let ch = s[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        match s[i..].find(';') {
            // A bare '&' is not an entity; emit it and move on rather than
            // discarding the rest of the label.
            None => {
                out.push('&');
                i += 1;
            }
            Some(rel) if rel > 12 => {
                out.push('&');
                i += 1;
            }
            Some(rel) => {
                let ent = &s[i + 1..i + rel];
                let replacement = match ent {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ => ent.strip_prefix('#').and_then(|num| {
                        let cp = if let Some(hex) = num.strip_prefix(['x', 'X']) {
                            u32::from_str_radix(hex, 16).ok()
                        } else {
                            num.parse::<u32>().ok()
                        };
                        cp.and_then(char::from_u32)
                    }),
                };
                match replacement {
                    Some(c) => {
                        out.push(c);
                        i += rel + 1;
                    }
                    None => {
                        out.push('&');
                        i += 1;
                    }
                }
            }
        }
    }
    out
}

/// Escape text for inclusion in an attribute value or element body.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Scan a document into events. Malformed markup is skipped, not fatal: these
/// payloads come from someone else's writer and a single odd tag should not
/// cost the user every feature in the file.
pub fn scan(input: &str) -> Vec<Event> {
    let b = input.as_bytes();
    let mut events = Vec::new();
    let mut i = 0;

    while i < b.len() {
        if b[i] != b'<' {
            let start = i;
            while i < b.len() && b[i] != b'<' {
                i += 1;
            }
            let raw = &input[start..i];
            if !raw.trim().is_empty() {
                events.push(Event::Text(unescape(raw)));
            }
            continue;
        }

        // <!-- comment -->, <![CDATA[..]]>, <?decl?>, <!DOCTYPE ..>
        if input[i..].starts_with("<!--") {
            i = input[i..].find("-->").map(|p| i + p + 3).unwrap_or(b.len());
            continue;
        }
        if input[i..].starts_with("<![CDATA[") {
            let end = input[i..].find("]]>").map(|p| i + p).unwrap_or(b.len());
            let text = &input[(i + 9).min(end)..end];
            if !text.trim().is_empty() {
                events.push(Event::Text(text.to_string()));
            }
            i = (end + 3).min(b.len());
            continue;
        }
        if input[i..].starts_with("<?") || input[i..].starts_with("<!") {
            i = input[i..].find('>').map(|p| i + p + 1).unwrap_or(b.len());
            continue;
        }

        // Find the end of the tag, respecting quoted attribute values so a
        // '>' inside an attribute does not truncate it.
        let mut j = i + 1;
        let mut quote: Option<u8> = None;
        while j < b.len() {
            let c = b[j];
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None if c == b'"' || c == b'\'' => quote = Some(c),
                None if c == b'>' => break,
                None => {}
            }
            j += 1;
        }
        if j >= b.len() {
            break; // unterminated tag: stop cleanly
        }

        let inner = &input[i + 1..j];
        i = j + 1;

        if let Some(name) = inner.strip_prefix('/') {
            events.push(Event::Close {
                name: name.trim().to_string(),
            });
            continue;
        }

        let self_closing = inner.ends_with('/');
        let inner = inner.strip_suffix('/').unwrap_or(inner);

        let mut chars = inner.char_indices();
        let mut name_end = inner.len();
        for (idx, c) in chars.by_ref() {
            if c.is_whitespace() {
                name_end = idx;
                break;
            }
        }
        let name = inner[..name_end].to_string();
        if name.is_empty() {
            continue;
        }

        events.push(Event::Open {
            name,
            attrs: parse_attrs(&inner[name_end..]),
            self_closing,
        });
    }

    events
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let key_start = i;
        while i < b.len() && b[i] != b'=' && !(b[i] as char).is_whitespace() {
            i += 1;
        }
        let key = s[key_start..i].to_string();
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            // Valueless attribute; record it as empty rather than dropping it.
            if !key.is_empty() {
                out.push((key, String::new()));
            }
            continue;
        }
        i += 1; // '='
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let value = if b[i] == b'"' || b[i] == b'\'' {
            let q = b[i];
            i += 1;
            let start = i;
            while i < b.len() && b[i] != q {
                i += 1;
            }
            let v = &s[start..i.min(s.len())];
            i = (i + 1).min(b.len());
            v
        } else {
            let start = i;
            while i < b.len() && !(b[i] as char).is_whitespace() {
                i += 1;
            }
            &s[start..i]
        };
        if !key.is_empty() {
            out.push((key, unescape(value)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opens(x: &str) -> Vec<Event> {
        scan(x)
    }

    #[test]
    fn reads_attributes_and_self_closing_tags() {
        let ev = opens(r#"<Feature name="AmpR" type="CDS"><Segment range="1-10"/></Feature>"#);
        match &ev[0] {
            Event::Open {
                name,
                attrs,
                self_closing,
            } => {
                assert_eq!(name, "Feature");
                assert_eq!(Event::attr(attrs, "name"), Some("AmpR"));
                assert_eq!(Event::attr(attrs, "type"), Some("CDS"));
                assert!(!self_closing);
            }
            e => panic!("expected Open, got {e:?}"),
        }
        match &ev[1] {
            Event::Open {
                name, self_closing, ..
            } => {
                assert_eq!(name, "Segment");
                assert!(self_closing);
            }
            e => panic!("expected Open, got {e:?}"),
        }
        assert_eq!(
            ev[2],
            Event::Close {
                name: "Feature".into()
            }
        );
    }

    #[test]
    fn handles_both_quote_styles_and_angle_brackets_inside_values() {
        let ev = opens(r#"<Q name='note' text="a > b"/>"#);
        let Event::Open { attrs, .. } = &ev[0] else {
            panic!()
        };
        assert_eq!(Event::attr(attrs, "name"), Some("note"));
        assert_eq!(Event::attr(attrs, "text"), Some("a > b"));
    }

    #[test]
    fn expands_the_entities_these_payloads_actually_use() {
        assert_eq!(
            unescape("P&amp;S &lt;tag&gt; &quot;q&quot; &apos;a&apos;"),
            r#"P&S <tag> "q" 'a'"#
        );
        assert_eq!(unescape("&#65;&#x42;"), "AB");
        assert_eq!(unescape("&#948;"), "\u{3b4}"); // Greek delta in a label
    }

    #[test]
    fn a_bare_ampersand_does_not_eat_the_label() {
        assert_eq!(unescape("Smith & Jones"), "Smith & Jones");
        assert_eq!(unescape("100% & rising"), "100% & rising");
        assert_eq!(unescape("&notanentity;"), "&notanentity;");
    }

    #[test]
    fn escape_round_trips_through_unescape() {
        for s in [r#"a&b<c>d"e'f"#, "plain", "δ subunit", "5' & 3'"] {
            assert_eq!(unescape(&escape(s)), s);
        }
    }

    #[test]
    fn skips_comments_declarations_and_cdata() {
        let ev = opens(r#"<?xml version="1.0"?><!-- note --><A><![CDATA[raw <text>]]></A>"#);
        assert!(matches!(&ev[0], Event::Open { name, .. } if name == "A"));
        assert_eq!(ev[1], Event::Text("raw <text>".into()));
    }

    #[test]
    fn survives_an_unterminated_tag_without_panicking() {
        let ev = opens("<Features><Feature name=\"x\"");
        assert_eq!(ev.len(), 1);
    }

    #[test]
    fn multibyte_text_is_not_split_mid_character() {
        let ev = opens("<A>δβγ &amp; more</A>");
        assert_eq!(ev[1], Event::Text("δβγ & more".into()));
    }
}
