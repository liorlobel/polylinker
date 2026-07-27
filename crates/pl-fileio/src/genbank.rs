//! GenBank flat file: the way out of any proprietary container.
//!
//! Plain text, read by ApE, UGENE, Benchling, Biopython and SnapGene itself.
//!
//! Feature colours are written in two conventions at once, because the tools
//! disagree and the cost of writing both is three lines:
//! `/ApEinfo_fwdcolor` + `/ApEinfo_revcolor` (ApE, UGENE, SnapGene) and
//! `/note="color: #rrggbb"` (Benchling and several web viewers).

use pl_core::{Feature, Molecule, Segment, Strand, Topology};

const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// Parse the first record of a GenBank file.
///
/// Multi-record files are common (a genome per contig); this returns the first
/// and callers that care should use [`parse_all`].
pub fn parse(text: &str) -> Molecule {
    parse_all(text).into_iter().next().unwrap_or_default()
}

pub fn parse_all(text: &str) -> Vec<Molecule> {
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.starts_with("//") {
            if !current.is_empty() {
                out.push(parse_record(&current));
                current.clear();
            }
            continue;
        }
        current.push(line);
    }
    if current.iter().any(|l| l.starts_with("LOCUS")) {
        out.push(parse_record(&current));
    }
    out
}

/// A feature being accumulated across lines: (key, location, qualifiers).
///
/// GenBank spreads one feature over many lines, and both the location and any
/// quoted qualifier can wrap, so parsing has to hold a partial feature open.
type Pending = (String, String, Vec<(String, Option<String>)>);

fn parse_record(lines: &[&str]) -> Molecule {
    let mut mol = Molecule::default();
    let mut declared: Option<u64> = None;

    if let Some(locus) = lines.iter().find(|l| l.starts_with("LOCUS")) {
        let mut it = locus.split_whitespace();
        it.next();
        mol.name = it.next().unwrap_or_default().to_string();
        mol.topology = if locus.to_ascii_lowercase().contains("circular") {
            Topology::Circular
        } else {
            Topology::Linear
        };
        // "<n> bp" or "<n> aa"
        let toks: Vec<&str> = locus.split_whitespace().collect();
        for w in toks.windows(2) {
            if (w[1] == "bp" || w[1] == "aa") && w[0].chars().all(|c| c.is_ascii_digit()) {
                declared = w[0].parse().ok();
                break;
            }
        }
        // The molecule-type field may carry an ss-/ds-/ms- prefix, but usually
        // does not. Absent means unknown, not single-stranded.
        let lower = locus.to_ascii_lowercase();
        mol.double_stranded = if lower.contains("ds-") {
            Some(true)
        } else if lower.contains("ss-") {
            Some(false)
        } else {
            None
        };
    }

    if let Some(d) = lines.iter().find(|l| l.starts_with("DEFINITION")) {
        mol.description = d[10..].trim().trim_end_matches('.').to_string();
    }

    // --- ORIGIN ---
    if let Some(oi) = lines.iter().position(|l| l.starts_with("ORIGIN")) {
        let mut seq = Vec::new();
        for line in &lines[oi + 1..] {
            if line.starts_with("//") {
                break;
            }
            // Case preserved: lowercase marks soft-masked / low-coverage bases.
            seq.extend(
                line.bytes()
                    .filter(|b| !b.is_ascii_whitespace() && !b.is_ascii_digit()),
            );
        }
        mol.seq = seq;
    }
    mol.declared_len = declared;

    // --- FEATURES ---
    if let Some(fi) = lines.iter().position(|l| l.starts_with("FEATURES")) {
        let mut pending: Option<Pending> = None;
        let mut open_qual: Option<usize> = None;
        let mut loc_open = false;

        let flush = |p: Option<Pending>, out: &mut Vec<Feature>| {
            let Some((key, loc, quals)) = p else { return };
            // `source` is whole-molecule metadata every file carries; showing
            // it would draw a full-length bar across the map.
            if key == "source" {
                return;
            }
            let (segments, strand) = parse_location(&loc);
            if segments.is_empty() {
                return;
            }
            let name = ["label", "gene", "product", "locus_tag", "note"]
                .iter()
                .find_map(|k| {
                    quals
                        .iter()
                        .find(|(qk, _)| qk == k)
                        // A valueless qualifier cannot name anything.
                        .and_then(|(_, v)| v.clone())
                })
                .unwrap_or_else(|| key.clone());
            let color = quals
                .iter()
                .find(|(k, _)| k == "ApEinfo_fwdcolor" || k == "ApEinfo_revcolor")
                .and_then(|(_, v)| v.clone())
                .or_else(|| {
                    quals
                        .iter()
                        .find(|(k, v)| k == "note" && v.as_deref().is_some_and(|s| s.contains('#')))
                        .and_then(|(_, v)| v.as_deref())
                        .and_then(|v| v.split('#').nth(1))
                        .and_then(|h| {
                            // Take hex digits by character, never by byte: a
                            // multibyte char straddling byte 6 would panic, and
                            // this runs inside wasm where a panic kills the
                            // module rather than one call.
                            let hex: String = h
                                .chars()
                                .take(6)
                                .take_while(|c| c.is_ascii_hexdigit())
                                .collect();
                            (hex.len() == 6).then(|| format!("#{hex}"))
                        })
                });
            let segments = segments
                .into_iter()
                .map(|mut s| {
                    s.color = color.clone();
                    s
                })
                .collect();
            out.push(Feature {
                name,
                kind: key,
                strand,
                segments,
                qualifiers: quals,
            });
        };

        for line in &lines[fi + 1..] {
            if line.starts_with("ORIGIN") || line.starts_with("CONTIG") || line.starts_with("BASE")
            {
                break;
            }
            if line.len() < 6 || !line.is_char_boundary(5) {
                continue;
            }
            let indent_5 = &line[..5];
            if !indent_5.trim().is_empty() {
                continue; // a new top-level keyword
            }
            let body = line[5..].trim_end();
            let is_new_feature = !body.starts_with(' ') && !body.trim().is_empty();

            if is_new_feature {
                flush(pending.take(), &mut mol.features);
                let mut parts = body.trim().splitn(2, char::is_whitespace);
                let key = parts.next().unwrap_or_default().to_string();
                let loc = parts.next().unwrap_or_default().trim().to_string();
                loc_open = unbalanced(&loc);
                open_qual = None;
                pending = Some((key, loc, Vec::new()));
                continue;
            }

            let Some((_, loc, quals)) = pending.as_mut() else {
                continue;
            };
            let t = body.trim();

            if loc_open && !t.starts_with('/') {
                loc.push_str(t);
                loc_open = unbalanced(loc);
                continue;
            }

            // A line only starts a new qualifier when no quoted value is open.
            //
            // Testing `starts_with('/')` first meant a continuation line that
            // happened to begin with `/` — extremely common in COG and product
            // descriptions, 168 times across 66 of this project's corpus files
            // — was read as a brand new qualifier. The real value truncated,
            // a junk key was fabricated from the prose, and every later
            // continuation line was dropped. It is also self-inflicted:
            // `qualifier_lines` wraps on spaces, so any name containing " / "
            // failed to survive our own write-then-read.
            if open_qual.is_none() && t.starts_with('/') {
                let rest = &t[1..];
                loc_open = false;
                let (k, raw) = match rest.split_once('=') {
                    Some((k, v)) => (k.to_string(), Some(v.to_string())),
                    // No '=' at all: a valueless qualifier such as /pseudo.
                    None => (rest.to_string(), None),
                };
                match raw {
                    None => {
                        quals.push((k, None));
                        open_qual = None;
                    }
                    Some(v) => {
                        let (text, closed) = open_value(&v);
                        quals.push((k, Some(text)));
                        open_qual = if closed { None } else { Some(quals.len() - 1) };
                    }
                }
            } else if let Some(idx) = open_qual {
                // Continuation of a quoted qualifier such as /translation.
                let (piece, closed) = continue_value(t);
                let sep = if quals[idx].0 == "translation" {
                    ""
                } else {
                    " "
                };
                let slot = quals[idx].1.get_or_insert_with(String::new);
                slot.push_str(sep);
                slot.push_str(&piece);
                if closed {
                    open_qual = None;
                }
            }
        }
        flush(pending.take(), &mut mol.features);
    }

    // Features are left in file order on purpose. A reader reports what the
    // file says; sorting is a presentation choice and belongs to whatever is
    // drawing the map. Sorting here also silently breaks round-trip fidelity,
    // because the writer emits in model order.
    mol
}

fn unbalanced(s: &str) -> bool {
    s.matches('(').count() > s.matches(')').count()
}

/// Read the text of a quoted qualifier value, collapsing `""` to `"`.
///
/// Returns the decoded text and whether the closing quote was reached on this
/// line. An unquoted value (`/codon_start=1`) is taken whole and is always
/// closed.
///
/// The old code used `trim_matches('"')`, which never collapsed the escape, so
/// a value containing a literal quote doubled its quote count on **every**
/// save/load cycle: 2 → 4 → 8 → 16. Worse, when an escaped `""` fell on a line
/// wrap, `ends_with('"')` read it as the close, the qualifier was marked
/// finished, and every remaining continuation line was silently discarded.
fn open_value(v: &str) -> (String, bool) {
    match v.strip_prefix('"') {
        None => (v.to_string(), true),
        Some(body) => decode_quoted(body),
    }
}

/// The same, for a continuation line of an already-open value.
fn continue_value(t: &str) -> (String, bool) {
    decode_quoted(t)
}

/// Walk a quoted run, treating `""` as one literal `"` and a lone `"` as the
/// terminator.
fn decode_quoted(s: &str) -> (String, bool) {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' {
            if i + 1 < b.len() && b[i + 1] == b'"' {
                out.push('"');
                i += 2;
                continue;
            }
            // A single quote closes the value; GenBank puts nothing after it.
            return (out, true);
        }
        // Copy whole characters so multi-byte text is not split.
        let ch = s[i..].chars().next().expect("index is on a char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    (out, false)
}

/// Parse a GenBank location into 1-based inclusive segments plus a strand.
fn parse_location(loc: &str) -> (Vec<Segment>, Strand) {
    let mut s = loc.trim();
    let mut strand = Strand::Forward;
    if let Some(inner) = s.strip_prefix("complement(") {
        strand = Strand::Reverse;
        s = inner.strip_suffix(')').unwrap_or(inner);
    }
    for p in ["join(", "order("] {
        if let Some(inner) = s.strip_prefix(p) {
            s = inner.strip_suffix(')').unwrap_or(inner);
            break;
        }
    }
    // A complement() nested inside join() flips the whole feature for our model.
    if s.contains("complement(") {
        strand = Strand::Reverse;
    }

    let mut segs = Vec::new();
    for part in s.split(',') {
        let part = part
            .trim()
            .trim_start_matches("complement(")
            .trim_end_matches(')')
            .replace(['<', '>'], "");
        let (a, b) = match part.split_once("..") {
            Some((a, b)) => (a, b),
            None if !part.is_empty() => (part.as_str(), part.as_str()),
            None => continue,
        };
        if let (Ok(start), Ok(end)) = (a.trim().parse::<u64>(), b.trim().parse::<u64>()) {
            if end >= start && start > 0 {
                segs.push(Segment::new(start, end));
            }
        }
    }
    (segs, strand)
}

// ---------------------------------------------------------------------------
// writing
// ---------------------------------------------------------------------------

/// Sanitise a name for the LOCUS line: no spaces, conventionally short.
pub fn locus_name(title: &str) -> String {
    let stem = title.rsplit_once('.').map(|(a, _)| a).unwrap_or(title);
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(['_', '.', '-']);
    // A name of "..." or "___" is not a name. Fall back rather than emit a
    // LOCUS line no parser will like.
    let usable = trimmed.chars().any(|c| c.is_ascii_alphanumeric());
    let name = if usable { trimmed } else { "sequence" };
    // The LOCUS name occupies columns 13-28: sixteen characters. Overrunning it
    // shifts every field after it, and the LOCUS line is the one line in the
    // format that strict parsers read positionally.
    name.chars().take(16).collect()
}

/// Render a feature's location.
///
/// `span` is the molecule length, needed only to split a segment that crosses
/// the origin; pass 0 when there is nothing to split against.
///
/// # The origin split belongs here
///
/// The model writes an origin-spanning segment as `end < start`, which
/// `Molecule::subseq`, the annotator and the SVG renderer all understand.
/// GenBank has no such form: `12..3` is not a location, and our own reader
/// silently dropped it — one feature in, zero out, and the molecule still
/// reported valid. So the wrap is expanded into `join(12..16,1..3)` at the
/// format boundary, which is exactly where `docs/PLAN.md` §5.3.1 says
/// coordinate conversions belong.
fn format_location(f: &Feature, span: u64) -> String {
    let parts: Vec<String> = f
        .segments
        .iter()
        .flat_map(|s| {
            if s.end < s.start && span >= s.start {
                // Crosses the origin: two ranges, in reading order.
                vec![format!("{}..{}", s.start, span), format!("1..{}", s.end)]
            } else {
                vec![format!("{}..{}", s.start, s.end)]
            }
        })
        .collect();
    let joined = if parts.len() > 1 {
        format!("join({})", parts.join(","))
    } else {
        parts.concat()
    };
    if f.strand.is_reverse() {
        format!("complement({joined})")
    } else {
        joined
    }
}

fn qualifier_lines(key: &str, value: &str, out: &mut String) {
    qualifier_lines_opt(key, Some(value), out)
}

/// Write one qualifier. `None` emits the bare `/key` form.
fn qualifier_lines_opt(key: &str, value: Option<&str>, out: &mut String) {
    const PAD: &str = "                     "; // 21 spaces
    let Some(value) = value else {
        // `/pseudo` and friends. These used to be skipped entirely by a
        // `v.is_empty()` test at the call site, which silently turned every
        // pseudogene in an exported file into an ordinary protein-coding gene.
        out.push_str(PAD);
        out.push('/');
        out.push_str(key);
        out.push('\n');
        return;
    };
    let raw = format!("/{}=\"{}\"", key, value.replace('"', "\"\""));
    let mut line = String::from(PAD);
    for word in raw.split(' ') {
        if line.len() + word.len() + 1 > 79 && line.trim() != "" {
            out.push_str(&line);
            out.push('\n');
            line = String::from(PAD);
        }
        if line != PAD {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.trim().is_empty() {
        out.push_str(&line);
        out.push('\n');
    }
}

/// Render a molecule as GenBank. `date` is `(day, month_index_0_based, year)`;
/// passing it in keeps this function pure and its output reproducible.
pub fn write(mol: &Molecule, title: &str, date: (u32, usize, i32)) -> String {
    let name = locus_name(title);
    let n = mol.span();
    let (d, m, y) = date;
    let date_str = format!("{:02}-{}-{}", d, MONTHS[m.min(11)], y);

    let mut out = String::new();
    out.push_str(&format!(
        "LOCUS       {:<16} {:>7} bp    DNA     {} SYN {}\n",
        name,
        n,
        if mol.topology.is_circular() {
            "circular"
        } else {
            "linear  "
        },
        date_str
    ));
    let def = if mol.description.is_empty() {
        name.as_str()
    } else {
        mol.description.as_str()
    };
    out.push_str(&format!("DEFINITION  {def}.\n"));
    out.push_str("ACCESSION   .\nVERSION     .\nKEYWORDS    .\n");
    out.push_str("SOURCE      synthetic DNA construct\n  ORGANISM  synthetic DNA construct\n");
    out.push_str("COMMENT     Converted by Polylinker.\n");
    if let Some((_, uuid)) = mol.notes.iter().find(|(k, _)| k == "UUID") {
        out.push_str(&format!("            Source document UUID: {uuid}\n"));
    }
    out.push_str("FEATURES             Location/Qualifiers\n");
    out.push_str(&format!("     source          1..{n}\n"));
    qualifier_lines("organism", "synthetic DNA construct", &mut out);
    qualifier_lines("mol_type", "other DNA", &mut out);

    for f in &mol.features {
        let kind = if f.kind.is_empty() {
            "misc_feature"
        } else {
            &f.kind
        };
        // Truncate by character. A feature key is normally ASCII, but this must
        // not panic on one that is not.
        let key: String = kind.chars().take(15).collect();
        out.push_str(&format!("     {:<15} {}\n", key, format_location(f, n)));
        qualifier_lines("label", &f.name, &mut out);
        for (k, v) in &f.qualifiers {
            // A key must be a legal GenBank qualifier name. The reader used to
            // manufacture keys out of prose when it mistook a continuation line
            // for a new qualifier; that is fixed, but validating here keeps a
            // malformed input from becoming malformed output.
            if k == "label"
                || k.starts_with("ApEinfo")
                || k.is_empty()
                || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                continue;
            }
            qualifier_lines_opt(k, v.as_deref(), &mut out);
        }
        if let Some(c) = f.color() {
            qualifier_lines("ApEinfo_fwdcolor", c, &mut out);
            qualifier_lines("ApEinfo_revcolor", c, &mut out);
            qualifier_lines("note", &format!("color: {c}"), &mut out);
        }
    }

    for p in &mol.primers {
        for s in &p.sites {
            if s.start < 1 || s.end > n || s.end < s.start {
                continue;
            }
            let loc = if s.strand.is_reverse() {
                format!("complement({}..{})", s.start, s.end)
            } else {
                format!("{}..{}", s.start, s.end)
            };
            out.push_str(&format!("     {:<15} {}\n", "primer_bind", loc));
            qualifier_lines("label", &p.name, &mut out);
            let note = match s.tm {
                Some(tm) => format!("primer {}; Tm: {} C", p.seq, tm),
                None => format!("primer {}", p.seq),
            };
            qualifier_lines("note", &note, &mut out);
        }
    }

    out.push_str("ORIGIN\n");
    let seq = &mol.seq;
    let mut i = 0usize;
    while i < seq.len() {
        let end = (i + 60).min(seq.len());
        let mut line = format!("{:>9}", i + 1);
        let mut j = i;
        while j < end {
            let k = (j + 10).min(end);
            line.push(' ');
            line.push_str(&String::from_utf8_lossy(&seq[j..k]));
            j = k;
        }
        out.push_str(&line);
        out.push('\n');
        i = end;
    }
    out.push_str("//\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal record wrapping one feature's qualifier block.
    fn with_quals(quals: &str) -> Molecule {
        let src = format!(
            "LOCUS       test                      12 bp    DNA     linear   SYN 27-JUL-2026\n\
             FEATURES             Location/Qualifiers\n\
             \x20    CDS             1..12\n{quals}\
             ORIGIN\n        1 acgtacgtacgt\n//\n"
        );
        parse(&src)
    }

    #[test]
    fn a_continuation_line_starting_with_a_slash_is_not_a_new_qualifier() {
        // 168 occurrences across 66 of this project's corpus files. The value
        // truncated, the continuation became a fabricated qualifier key, and
        // every later continuation line was dropped.
        let m = with_quals(
            "                     /product=\"Energy production\n\
             \x20                    /conversion; Region: UbiH\n\
             \x20                    /and more text\"\n\
             \x20                    /codon_start=1\n",
        );
        assert_eq!(m.features.len(), 1);
        let f = &m.features[0];
        assert_eq!(
            f.qualifier("product"),
            Some("Energy production /conversion; Region: UbiH /and more text")
        );
        // No key was invented out of the prose...
        assert!(!f.has_qualifier("conversion; Region: UbiH"));
        assert!(!f.has_qualifier("and"));
        // ...and the qualifier that genuinely followed still arrived.
        assert_eq!(f.qualifier("codon_start"), Some("1"));
        assert_eq!(f.qualifiers.len(), 2, "{:?}", f.qualifiers);
    }

    #[test]
    fn an_escaped_quote_survives_repeated_round_trips() {
        // `trim_matches('"')` never collapsed `""`, so the quote count doubled
        // on every save/load: 2 -> 4 -> 8 -> 16.
        let m = with_quals("                     /note=\"a \"\"quoted\"\" word\"\n");
        assert_eq!(m.features[0].qualifier("note"), Some(r#"a "quoted" word"#));

        let mut cur = m;
        for cycle in 0..4 {
            let text = write(&cur, "test", (27, 6, 2026));
            cur = parse(&text);
            assert_eq!(
                cur.features[0].qualifier("note"),
                Some(r#"a "quoted" word"#),
                "quote handling drifted on cycle {cycle}"
            );
        }
    }

    #[test]
    fn an_escaped_quote_at_a_line_wrap_does_not_end_the_value() {
        // The nastier half: when `""` landed on a wrap boundary the old
        // `ends_with('"')` read it as the close, and every remaining
        // continuation line was silently discarded.
        let m = with_quals(
            "                     /note=\"first part ends with an escaped \"\"\n\
             \x20                    second line must not be lost\"\n",
        );
        let note = m.features[0].qualifier("note").unwrap();
        assert!(note.contains("second line must not be lost"), "{note:?}");
        assert!(
            note.contains('"'),
            "the escaped quote should be literal: {note:?}"
        );
    }

    #[test]
    fn a_valueless_qualifier_survives_a_round_trip() {
        // 11,561 `/pseudo` in this project's corpus were being deleted on
        // write, silently turning every pseudogene into a protein-coding gene.
        let m = with_quals(
            "                     /pseudo\n\
             \x20                    /ribosomal_slippage\n\
             \x20                    /gene=\"thrA\"\n",
        );
        let f = &m.features[0];
        assert!(f.has_qualifier("pseudo"));
        assert_eq!(f.qualifier("pseudo"), None, "it has no value, and says so");
        assert!(f.has_qualifier("ribosomal_slippage"));
        assert_eq!(f.qualifier("gene"), Some("thrA"));

        let text = write(&m, "test", (27, 6, 2026));
        assert!(
            text.lines().any(|l| l.trim() == "/pseudo"),
            "/pseudo must be written bare, not dropped:\n{text}"
        );
        let again = parse(&text);
        assert!(again.features[0].has_qualifier("pseudo"));
        assert!(again.features[0].has_qualifier("ribosomal_slippage"));
        assert_eq!(again.features[0].qualifier("gene"), Some("thrA"));
    }

    #[test]
    fn an_empty_value_is_not_the_same_as_no_value() {
        let m = with_quals("                     /pseudo\n                     /replace=\"\"\n");
        let f = &m.features[0];
        assert_eq!(f.qualifier("pseudo"), None);
        assert_eq!(f.qualifier("replace"), Some(""));
        let text = write(&m, "test", (27, 6, 2026));
        assert!(text.lines().any(|l| l.trim() == "/pseudo"));
        assert!(text.lines().any(|l| l.trim() == r#"/replace="""#), "{text}");
    }

    #[test]
    fn a_feature_crossing_the_origin_survives_a_round_trip() {
        // The model writes a wrap as `end < start`. GenBank has no such form,
        // and our own reader silently dropped `12..3` — one feature in, zero
        // out, molecule still reported valid. It must be split into a join at
        // the format boundary.
        let mut m = Molecule {
            seq: b"AAAACCCCGGGGTTTT".to_vec(),
            topology: Topology::Circular,
            ..Default::default()
        };
        let mut f = Feature::new("wraps", "misc_feature");
        f.segments.push(Segment::new(13, 3));
        m.features.push(f);

        let text = write(&m, "test", (27, 6, 2026));
        assert!(
            text.contains("join(13..16,1..3)"),
            "the wrap must become a join:
{text}"
        );

        let again = parse(&text);
        assert_eq!(again.features.len(), 1, "the feature was dropped");
        let segs = &again.features[0].segments;
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[0].start, segs[0].end), (13, 16));
        assert_eq!((segs[1].start, segs[1].end), (1, 3));
        // The same seven bases, either way round.
        assert_eq!(segs.iter().map(|s| s.len()).sum::<u64>(), 7);
    }

    #[test]
    fn location_forms_all_parse() {
        assert_eq!(parse_location("1..10").0, vec![Segment::new(1, 10)]);
        assert_eq!(parse_location("complement(5..8)").1, Strand::Reverse);
        let (segs, strand) = parse_location("join(1..3,7..9)");
        assert_eq!(segs.len(), 2);
        assert_eq!(strand, Strand::Forward);
        // fuzzy boundaries
        assert_eq!(parse_location("<1..>10").0, vec![Segment::new(1, 10)]);
        // single base
        assert_eq!(parse_location("42").0, vec![Segment::new(42, 42)]);
        // complement inside a join still flips the feature
        assert_eq!(
            parse_location("join(complement(1..3),complement(7..9))").1,
            Strand::Reverse
        );
    }

    #[test]
    fn round_trip_preserves_the_things_that_matter() {
        let mut mol = Molecule {
            name: "test".into(),
            seq: b"ACGTacgtNN".repeat(10),
            topology: Topology::Circular,
            ..Default::default()
        };
        let mut f = Feature::new("AmpR", "CDS");
        f.strand = Strand::Reverse;
        let mut s = Segment::new(5, 20);
        s.color = Some("#9a5b8c".into());
        f.segments.push(s);
        f.set_qualifier("gene", "bla");
        mol.features.push(f);

        let gb = write(&mol, "test.dna", (26, 6, 2026));
        let back = parse(&gb);

        assert_eq!(back.seq, mol.seq, "sequence and its case survive");
        assert_eq!(back.topology, Topology::Circular);
        assert_eq!(
            back.features.len(),
            1,
            "the source feature must not be counted"
        );
        let g = &back.features[0];
        assert_eq!(g.name, "AmpR");
        assert_eq!(g.strand, Strand::Reverse);
        assert_eq!((g.start(), g.end()), (5, 20));
        assert_eq!(g.color(), Some("#9a5b8c"));
        assert_eq!(g.qualifier("gene"), Some("bla"));
    }

    #[test]
    fn annotation_only_file_reports_no_bases_but_keeps_the_declared_length() {
        // ORIGIN immediately followed by // -- real, and common for genomes.
        let gb =
            "LOCUS       NC_003210            2944528 bp    DNA     circular BCT 30-MAR-2010\n\
                  FEATURES             Location/Qualifiers\n\
                  \x20    gene            1..100\n\
                  \x20                    /gene=\"dnaA\"\n\
                  ORIGIN      \n//\n";
        let m = parse(gb);
        assert!(
            m.seq.is_empty(),
            "there are genuinely no bases in this file"
        );
        assert_eq!(m.declared_len, Some(2_944_528));
        assert_eq!(m.span(), 2_944_528);
        assert!(m.sequence_absent());
        assert_eq!(m.features.len(), 1);
    }

    #[test]
    fn multi_line_locations_and_translations_are_reassembled() {
        let gb = "LOCUS       x                        30 bp    DNA     linear   SYN 01-JAN-2026\n\
                  FEATURES             Location/Qualifiers\n\
                  \x20    CDS             join(1..6,\n\
                  \x20                    13..18)\n\
                  \x20                    /translation=\"MAD\n\
                  \x20                    EIT\"\n\
                  ORIGIN\n\
                  \x20       1 acgtacgtac gtacgtacgt acgtacgtac\n//\n";
        let m = parse(gb);
        assert_eq!(m.features.len(), 1);
        assert_eq!(
            m.features[0].segments.len(),
            2,
            "wrapped location must rejoin"
        );
        assert_eq!(m.features[0].qualifier("translation"), Some("MADEIT"));
    }

    #[test]
    fn multibyte_text_never_panics_on_truncation() {
        // Both of these used to slice a str by byte offset. A feature key or a
        // colour note containing a multibyte character would panic, which in
        // wasm takes down the whole module.
        let mut mol = Molecule {
            seq: b"acgt".to_vec(),
            ..Default::default()
        };
        let mut f = Feature::new("δ subunit", "δδδδδδδδδδδδδδδδδδδδ");
        f.segments.push(Segment::new(1, 4));
        f.set_qualifier("note", "color: #δβγ");
        mol.features.push(f);

        let gb = write(&mol, "t.dna", (1, 0, 2026)); // must not panic
        let back = parse(&gb);
        assert_eq!(back.features.len(), 1);
        assert_eq!(back.features[0].name, "δ subunit");
        // "#δβγ" is not six hex digits, so no colour is claimed.
        assert_eq!(back.features[0].color(), None);
    }

    #[test]
    fn colour_notes_are_only_believed_when_they_are_real_hex() {
        let gb = "LOCUS       x                         4 bp    DNA     linear   SYN 01-JAN-2026\n\
                  FEATURES             Location/Qualifiers\n\
                  \x20    gene            1..4\n\
                  \x20                    /note=\"color: #1a2b3c\"\n\
                  ORIGIN\n\
                  \x20       1 acgt\n//\n";
        assert_eq!(parse(gb).features[0].color(), Some("#1a2b3c"));

        let short = gb.replace("#1a2b3c", "#12");
        assert_eq!(parse(&short).features[0].color(), None);
    }

    #[test]
    fn standalone_annotation_tracks_are_read() {
        // UGENE and SnapGene both export these: features only, no ORIGIN block,
        // and no bp field on the LOCUS line. Real files, and Biopython refuses
        // them outright.
        let gb =
            "LOCUS       Annotations                                             19-MAR-2018\n\
                  UNIMARK     Annotations\n\
                  FEATURES             Location/Qualifiers\n\
                  \x20    CDS             242..1015\n\
                  \x20                    /ugene_name=\"hypothetical protein\"\n\
                  \x20    CDS             complement(1118..1951)\n\
                  \x20                    /ugene_name=\"PknD\"\n//\n";
        let m = parse(gb);
        assert_eq!(m.features.len(), 2);
        assert!(m.seq.is_empty());
        assert_eq!(m.declared_len, None);
        assert!(m.is_annotation_track());
        assert!(
            !m.sequence_absent(),
            "nothing was declared, so nothing is missing"
        );
        // No length anywhere, so the span has to be inferred for display.
        assert_eq!(m.span(), 0);
        assert_eq!(m.annotation_span(), 1951);
        assert_eq!(m.features[1].strand, Strand::Reverse);
    }

    #[test]
    fn strandedness_is_unknown_unless_the_file_says() {
        let base = "LOCUS       x                        4 bp    {} linear   SYN 01-JAN-2026\nORIGIN\n        1 acgt\n//\n";
        assert_eq!(
            parse(&base.replace("{}", "DNA    ")).double_stranded,
            None,
            "a plain DNA record records nothing, so we must not claim single-stranded"
        );
        assert_eq!(
            parse(&base.replace("{}", "ds-DNA ")).double_stranded,
            Some(true)
        );
        assert_eq!(
            parse(&base.replace("{}", "ss-DNA ")).double_stranded,
            Some(false)
        );
    }

    #[test]
    fn features_are_left_in_file_order() {
        let gb = "LOCUS       x                        99 bp    DNA     linear   SYN 01-JAN-2026\n\
                  FEATURES             Location/Qualifiers\n\
                  \x20    gene            50..60\n\
                  \x20                    /label=\"later\"\n\
                  \x20    gene            10..20\n\
                  \x20                    /label=\"earlier\"\n\
                  ORIGIN\n//\n";
        let m = parse(gb);
        assert_eq!(m.features[0].name, "later", "a reader reports file order");
        assert_eq!(m.features[1].name, "earlier");
    }

    #[test]
    fn multi_record_files_yield_every_record() {
        let one = "LOCUS       a                         4 bp    DNA     linear   SYN 01-JAN-2026\nORIGIN\n        1 acgt\n//\n";
        let two = "LOCUS       b                         8 bp    DNA     circular SYN 01-JAN-2026\nORIGIN\n        1 acgtacgt\n//\n";
        let all = parse_all(&format!("{one}{two}"));
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].seq, b"acgt".to_vec());
        assert_eq!(all[1].topology, Topology::Circular);
    }

    #[test]
    fn locus_names_are_made_safe() {
        assert_eq!(locus_name("my plasmid v2.dna"), "my_plasmid_v2");
        assert_eq!(locus_name("....dna"), "sequence");
        assert_eq!(locus_name("___.dna"), "sequence");
        // Sixteen characters, so the LOCUS line's fixed columns still line up.
        assert_eq!(locus_name(&"x".repeat(40)).len(), 16);
        assert_eq!(
            locus_name("pACYC184-Ppho-fab2-6his.dna"),
            "pACYC184-Ppho-fa"
        );
    }

    #[test]
    fn locus_line_keeps_its_columns_even_with_a_long_name() {
        let mol = Molecule {
            seq: b"acgt".to_vec(),
            ..Default::default()
        };
        let line = write(&mol, &format!("{}.dna", "x".repeat(40)), (1, 0, 2026));
        let first = line.lines().next().unwrap();
        // "LOCUS" + 7 spaces = 12 chars, then 16 for the name.
        assert!(first.starts_with("LOCUS       "));
        assert_eq!(&first[12..28], &"x".repeat(16));
        assert!(
            first[28..].starts_with(" "),
            "bp field must follow the name column"
        );
    }

    #[test]
    fn origin_block_is_sixty_bases_in_ten_base_groups() {
        let mol = Molecule {
            seq: b"a".repeat(65),
            ..Default::default()
        };
        let gb = write(&mol, "t", (1, 0, 2026));
        let lines: Vec<&str> = gb
            .lines()
            .skip_while(|l| !l.starts_with("ORIGIN"))
            .collect();
        assert!(lines[1].starts_with("        1 "));
        assert_eq!(lines[1].split_whitespace().count(), 7); // index + 6 groups
        assert!(lines[2].starts_with("       61 "));
    }
}
