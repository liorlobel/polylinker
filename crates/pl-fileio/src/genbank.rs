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
type Pending = (String, String, Vec<(String, String)>);

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
                .find_map(|k| quals.iter().find(|(qk, _)| qk == k).map(|(_, v)| v.clone()))
                .unwrap_or_else(|| key.clone());
            let color = quals
                .iter()
                .find(|(k, _)| k == "ApEinfo_fwdcolor" || k == "ApEinfo_revcolor")
                .map(|(_, v)| v.clone())
                .or_else(|| {
                    quals
                        .iter()
                        .find(|(k, v)| k == "note" && v.contains('#'))
                        .and_then(|(_, v)| v.split('#').nth(1))
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

            if let Some(rest) = t.strip_prefix('/') {
                loc_open = false;
                let (k, v) = match rest.split_once('=') {
                    Some((k, v)) => (k.to_string(), v.to_string()),
                    None => (rest.to_string(), String::new()),
                };
                let quoted_open = v.starts_with('"') && !(v.len() > 1 && v.ends_with('"'));
                let clean = v.trim_matches('"').to_string();
                quals.push((k, clean));
                open_qual = if quoted_open {
                    Some(quals.len() - 1)
                } else {
                    None
                };
            } else if let Some(idx) = open_qual {
                // Continuation of a quoted qualifier such as /translation.
                let closing = t.ends_with('"');
                let piece = t.trim_end_matches('"');
                let sep = if quals[idx].0 == "translation" {
                    ""
                } else {
                    " "
                };
                quals[idx].1.push_str(sep);
                quals[idx].1.push_str(piece);
                if closing {
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

fn format_location(f: &Feature) -> String {
    let parts: Vec<String> = f
        .segments
        .iter()
        .map(|s| format!("{}..{}", s.start, s.end))
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
    const PAD: &str = "                     "; // 21 spaces
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
        out.push_str(&format!("     {:<15} {}\n", key, format_location(f)));
        qualifier_lines("label", &f.name, &mut out);
        for (k, v) in &f.qualifiers {
            if k == "label" || v.is_empty() || k.starts_with("ApEinfo") {
                continue;
            }
            qualifier_lines(k, v, &mut out);
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
