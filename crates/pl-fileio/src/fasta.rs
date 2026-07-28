//! FASTA. The simplest format, and the one most often malformed.

use pl_core::Molecule;

/// Parse the first record.
pub fn parse(text: &str) -> Molecule {
    parse_all(text).into_iter().next().unwrap_or_default()
}

pub fn parse_all(text: &str) -> Vec<Molecule> {
    let mut out: Vec<Molecule> = Vec::new();
    let mut cur: Option<Molecule> = None;

    for line in text.lines() {
        if let Some(header) = line.strip_prefix('>') {
            if let Some(m) = cur.take() {
                out.push(m);
            }
            let header = header.trim();
            let (name, desc) = match header.split_once(char::is_whitespace) {
                Some((n, d)) => (n.to_string(), d.trim().to_string()),
                None => (header.to_string(), String::new()),
            };
            cur = Some(Molecule {
                name: if name.is_empty() {
                    "sequence".into()
                } else {
                    name
                },
                description: desc,
                ..Default::default()
            });
            continue;
        }
        // A file that starts with bases and no header is still readable.
        let m = cur.get_or_insert_with(|| Molecule {
            name: "sequence".into(),
            ..Default::default()
        });
        // Case preserved; whitespace and the '*' terminator dropped.
        m.seq.extend(
            line.bytes()
                .filter(|b| !b.is_ascii_whitespace() && *b != b'*'),
        );
    }
    if let Some(m) = cur.take() {
        out.push(m);
    }
    out
}

pub fn write(mol: &Molecule, title: &str, line_width: usize) -> String {
    let width = line_width.max(1);
    let name = title.rsplit_once('.').map(|(a, _)| a).unwrap_or(title);
    let mut out = format!(
        ">{} {} bp {}\n",
        if name.is_empty() { "sequence" } else { name },
        mol.span(),
        mol.topology.as_str()
    );
    // Decoded once, then wrapped — the same rule as the GenBank ORIGIN writer,
    // and for the same reason. `seq.chunks(width)` cut the raw bytes first, so
    // a multi-byte character landing on a line boundary was decoded as a lone
    // lead byte at the end of one line and a lone continuation byte at the
    // start of the next, and `from_utf8_lossy` turned each half into its own
    // U+FFFD: one character in, two replacement characters out, and a sequence
    // three bytes longer than it started. Wrapping decoded characters cannot
    // split one.
    let decoded = String::from_utf8_lossy(&mol.seq);
    let mut chars = decoded.chars().peekable();
    while chars.peek().is_some() {
        let line: String = chars.by_ref().take(width).collect();
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_name_from_description() {
        let m = parse(">pUC19 cloning vector\nACGT\nACGT\n");
        assert_eq!(m.name, "pUC19");
        assert_eq!(m.description, "cloning vector");
        assert_eq!(m.seq, b"ACGTACGT".to_vec());
    }

    #[test]
    fn preserves_case_and_drops_the_stop_marker() {
        let m = parse(">x\nAcGt*\n");
        assert_eq!(m.seq, b"AcGt".to_vec());
    }

    #[test]
    fn reads_multiple_records() {
        let all = parse_all(">a\nAAAA\n>b\nCCCC\n");
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].name, "b");
        assert_eq!(all[1].seq, b"CCCC".to_vec());
    }

    #[test]
    fn headerless_input_is_still_read() {
        let m = parse("ACGT\nACGT\n");
        assert_eq!(m.seq, b"ACGTACGT".to_vec());
    }

    #[test]
    fn write_wraps_at_the_requested_width() {
        let m = Molecule {
            seq: b"A".repeat(25),
            ..Default::default()
        };
        let f = write(&m, "t.dna", 10);
        let body: Vec<&str> = f.lines().skip(1).collect();
        assert_eq!(body, vec!["AAAAAAAAAA", "AAAAAAAAAA", "AAAAA"]);
    }

    #[test]
    fn wrapping_never_splits_a_multibyte_character() {
        // `seq.chunks(width)` cut the raw bytes, so a character landing on a
        // line boundary was decoded as a lone lead byte at the end of one line
        // and a lone continuation byte at the start of the next, and each half
        // became its own U+FFFD: one character in, two out, and a file three
        // bytes longer than the sequence it claims to hold.
        let mut seq = b"a".repeat(9);
        seq.extend_from_slice("µ".as_bytes());
        seq.extend_from_slice(&b"c".repeat(15));
        let m = Molecule {
            seq: seq.clone(),
            ..Default::default()
        };
        let text = write(&m, "x", 10);
        assert!(
            !text.contains('\u{FFFD}'),
            "the character was split across two lossy decodes:\n{text}"
        );
        assert_eq!(parse(&text).seq, seq, "the exported sequence changed");
    }

    #[test]
    fn round_trips() {
        let m = Molecule {
            seq: b"ACGTacgtNN".repeat(9),
            ..Default::default()
        };
        assert_eq!(parse(&write(&m, "x", 70)).seq, m.seq);
    }
}
