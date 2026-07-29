//! FASTA. The simplest format, and the one most often malformed.

use pl_core::Molecule;

/// Parse the first record.
pub fn parse(text: &str) -> Molecule {
    parse_all(text).into_iter().next().unwrap_or_default()
}

pub fn parse_all(text: &str) -> Vec<Molecule> {
    // See `lib::strip_bom`. U+FEFF is not ASCII whitespace, so without this the
    // mark's three bytes are kept as bases of the first record — and the `>`
    // that follows it never matches `strip_prefix`, so the header is lost too.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
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
        // Case preserved; whitespace and the '*' terminator dropped. Filtered
        // *before* the record is opened, because "contributes no bases" and
        // "opens a record" are different questions and answering them in the
        // wrong order fabricated a record.
        let bases: Vec<u8> = line
            .bytes()
            .filter(|b| !b.is_ascii_whitespace() && *b != b'*')
            .collect();
        // A file that starts with bases and no header is still readable — but a
        // leading blank line is not bases. `"\n>pUC19 cloning vector\nACGT..."`
        // opened a nameless empty molecule on the blank first line, which the
        // `>` on the next line then pushed into `out`: an ordinary one-record
        // 20 bp file came back as *two* records, `pl info` printed "length
        // 0 bp / GC n/a" and never showed a base, `pl convert` refused it with
        // "holds 2 records and this would write only the first", and `pl index`
        // wrote the phantom into the library. `detect` (lib.rs) already sniffs
        // `>` after `trim_start`, so detection and parsing disagreed about
        // whether the leading whitespace was content; they agree now. `"\r\n"`,
        // `"   \n"` and `"\n\n"` all took the same path.
        if cur.is_none() && bases.is_empty() {
            continue;
        }
        cur.get_or_insert_with(|| Molecule {
            name: "sequence".into(),
            ..Default::default()
        })
        .seq
        .extend(bases);
    }
    if let Some(m) = cur.take() {
        out.push(m);
    }
    out
}

/// Render a molecule as FASTA. `title` names the destination file and is used
/// only when the molecule has no name of its own.
///
/// # The header carries the record's own two fields, not derived facts
///
/// A FASTA header is `>identifier description`, and this wrote
/// `>{file stem} {n} bp {topology}` — so **both** fields were synthesised and
/// the molecule's own name and description were discarded. Converting the NCBI
/// pUC19 record (`DEFINITION Cloning vector pUC19c, complete sequence.`) gave
/// `>L09137 2686 bp circular`, exit 0, nothing on stderr, and `Out::Fasta` in
/// `pl convert` has no report channel to say so. Everything after the first
/// space in a header *is* the description, so re-reading that file yielded
/// `description == "2686 bp circular"`, which then propagated: into `DEFINITION`
/// on the way back to GenBank, into `<Description>` in a `.dna`, and into the
/// text `pl-scan` indexes, so `pl find --text "cloning vector"` stopped matching
/// the converted copy. FASTA->FASTA lost the accession too:
/// `>NC_000913.3 Escherichia coli ...` became `>ecoli`.
///
/// The substituted metadata was not even recoverable — `lib.rs` states that
/// FASTA has no topology field and the reader never parses `circular` back out —
/// so a real description was traded for a claim nothing reads, and a round trip
/// ended up asserting `DEFINITION 2686 bp circular` under a `LOCUS ... linear`
/// line. `genbank::write_reporting` in this same crate preserves
/// `mol.description`; this is now consistent with it.
///
/// The length and topology are **not** appended in the NCBI `[length=..]`
/// convention either: nothing in this crate reads that convention back, so each
/// export would append another copy to the description it just parsed, which is
/// the grow-for-ever failure `genbank::is_generated_colour_note` exists to stop.
pub fn write(mol: &Molecule, title: &str, line_width: usize) -> String {
    let width = line_width.max(1);
    let stem = title.rsplit_once('.').map(|(a, _)| a).unwrap_or(title);
    // The identifier may not contain whitespace — everything past the first
    // space is the description — so a name that does is joined with `_` rather
    // than truncated at the space, which would have made the tail read back as
    // prose. Control characters are dropped for the same reason a newline is:
    // one would end the header line and the rest would be read as bases.
    let ident: String = if mol.name.trim().is_empty() {
        stem.chars().filter(|c| !c.is_control()).collect()
    } else {
        mol.name
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("_")
            .chars()
            .filter(|c| !c.is_control())
            .collect()
    };
    let desc: String = mol
        .description
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let desc = desc.trim();
    let mut out = String::from(">");
    out.push_str(if ident.is_empty() { "sequence" } else { &ident });
    if !desc.is_empty() {
        out.push(' ');
        out.push_str(desc);
    }
    out.push('\n');
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
    fn a_leading_blank_line_is_not_a_record() {
        // A one-record file that happens to begin with a newline opened a
        // nameless empty molecule on that blank line, which the `>` then pushed
        // into `out`. `pl info` reported "records 2 ... length 0 bp / GC n/a"
        // and never showed a base of the 20 that are there; `pl convert`
        // refused the file as multi-record; `pl index` wrote the phantom into
        // the library. Biopython's strict parser rejects such a file outright
        // and its `fasta-pearson` parser reads one 20 bp record — "two records,
        // the first empty" is the one answer nobody else gives.
        for text in [
            "\n>pUC19 cloning vector\nACGTACGTACGTACGTACGT\n",
            "\r\n>pUC19 cloning vector\r\nACGTACGTACGTACGTACGT\r\n",
            "   \n>pUC19 cloning vector\nACGTACGTACGTACGTACGT\n",
            "\n\n>pUC19 cloning vector\nACGTACGTACGTACGTACGT\n",
        ] {
            let all = parse_all(text);
            assert_eq!(all.len(), 1, "fabricated a record from {text:?}: {all:?}");
            assert_eq!(all[0].name, "pUC19");
            assert_eq!(all[0].seq.len(), 20, "the bases went missing");
        }

        // A file that is nothing but blank lines holds no record at all, which
        // is what `LoadReport::suspect` is there to notice.
        assert!(parse_all("\n\n   \n").is_empty());
    }

    #[test]
    fn the_header_carries_the_record_own_name_and_description() {
        // Both header fields used to be synthesised — `>{file stem} {n} bp
        // {topology}` — so the accession and the description were discarded and
        // the fabricated string became the description on re-read, propagating
        // into GenBank's DEFINITION, into a `.dna`'s <Description> and into the
        // text pl-scan indexes.
        let m = Molecule {
            name: "L09137.2".into(),
            description: "Cloning vector pUC19c, complete sequence.".into(),
            seq: b"ACGTACGTAC".to_vec(),
            topology: pl_core::Topology::Circular,
            ..Default::default()
        };
        let text = write(&m, "out.fa", 70);
        let header = text.lines().next().unwrap();
        assert_eq!(
            header, ">L09137.2 Cloning vector pUC19c, complete sequence.",
            "the record's own two fields, not derived facts"
        );

        // And the trip closes: re-reading gives back what went in, and a second
        // export is byte-identical rather than accumulating another copy of the
        // derived facts.
        let back = parse(&text);
        assert_eq!(back.name, "L09137.2");
        assert_eq!(
            back.description,
            "Cloning vector pUC19c, complete sequence."
        );
        assert_eq!(write(&back, "out.fa", 70), text, "export is not idempotent");

        // No name of its own: the file stem is the fallback, as before.
        let bare = Molecule {
            seq: b"ACGT".to_vec(),
            ..Default::default()
        };
        assert_eq!(
            write(&bare, "plasmid.fa", 70).lines().next(),
            Some(">plasmid")
        );

        // A name with a space would otherwise put its tail in the description
        // field, and a newline anywhere in either field would end the header
        // line and turn the rest into bases.
        let awkward = Molecule {
            name: "my plasmid".into(),
            description: "line one\nline two".into(),
            seq: b"ACGT".to_vec(),
            ..Default::default()
        };
        let text = write(&awkward, "x.fa", 70);
        assert_eq!(text.lines().count(), 2, "the header spilled: {text:?}");
        assert_eq!(text.lines().next(), Some(">my_plasmid line one line two"));
        assert_eq!(parse(&text).seq, b"ACGT".to_vec());
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
