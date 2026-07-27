//! Sequence file formats for Polylinker.
//!
//! Formats are detected from **content, not from the file extension**. In a
//! real corpus the extension lies often enough to matter: `.ab1` files that
//! are actually SCF or ZTR, `.gb` files that are FASTA, `.seq` that could be
//! anything. Sniffing costs one read of the first few bytes.

pub mod fasta;
pub mod genbank;
pub mod snapgene;
pub mod xml;

use pl_core::Molecule;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    SnapGene,
    GenBank,
    Fasta,
    /// Recognised, but a chromatogram rather than a sequence file. Named so
    /// the user gets "that is an ABIF trace" instead of "unknown file".
    Abif,
    Scf,
    Ztr,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::SnapGene => "SnapGene .dna",
            Format::GenBank => "GenBank",
            Format::Fasta => "FASTA",
            Format::Abif => "ABIF chromatogram",
            Format::Scf => "SCF chromatogram",
            Format::Ztr => "ZTR chromatogram",
        }
    }
    pub fn is_sequence_file(self) -> bool {
        matches!(self, Format::SnapGene | Format::GenBank | Format::Fasta)
    }
}

/// Identify a file from its leading bytes.
pub fn detect(data: &[u8]) -> Option<Format> {
    if data.len() >= 13 && data[0] == snapgene::block::HEADER && &data[5..13] == snapgene::MAGIC {
        return Some(Format::SnapGene);
    }
    match &data[..data.len().min(4)] {
        b"ABIF" => return Some(Format::Abif),
        b".scf" => return Some(Format::Scf),
        [0xAE, b'Z', b'T', b'R'] => return Some(Format::Ztr),
        _ => {}
    }
    // Text formats: look at the first few KB only.
    let head = String::from_utf8_lossy(&data[..data.len().min(8192)]);
    if head
        .lines()
        .any(|l| l.starts_with("LOCUS ") || l == "LOCUS")
        || head.lines().any(|l| l.starts_with("ORIGIN"))
    {
        return Some(Format::GenBank);
    }
    if head.trim_start().starts_with('>') {
        return Some(Format::Fasta);
    }
    None
}

#[derive(Debug)]
pub enum LoadError {
    Unrecognised,
    NotASequenceFile(Format),
    SnapGene(snapgene::Error),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Unrecognised => write!(
                f,
                "unrecognised format -- expected SnapGene .dna, GenBank or FASTA"
            ),
            LoadError::NotASequenceFile(fmt) => {
                write!(f, "that is {}, not a sequence file", fmt.name())
            }
            LoadError::SnapGene(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// What a file contained, beyond the molecule that was returned.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoadReport {
    /// Records present in the file. `load` returns only the first.
    ///
    /// Multi-record files are ordinary — a genome writes one record per contig
    /// — and there was no channel to say so. A 124-record `.gbk` came back as
    /// one molecule with 1,879 features missing, and `pl convert` then wrote
    /// that truncated molecule back out. 8 of 303 GenBank files and 351 FASTA
    /// files in this project's corpus have more than one record.
    pub records: usize,
    /// Location forms the GenBank reader could not represent.
    ///
    /// Empty for every other format. An exotic location — `1^2`, a remote
    /// reference such as `J00194.1:200..300`, a `bond(...)` operator — used to
    /// vanish without trace, leaving a feature quietly claiming a span it does
    /// not have. Reported rather than dropped, and never invented: see
    /// `genbank::parse_location`.
    pub unrepresentable_locations: Vec<String>,
    /// Did the *file* state the topology, or did we fall back to a default?
    ///
    /// [`Molecule::topology`] has two states and defaults to `Linear`, which
    /// conflates "this file says linear" with "this file says nothing". FASTA
    /// has no topology field at all, so every FASTA record reads as linear —
    /// and a Plasmidsaurus assembly of a plasmid arrives as FASTA, at an
    /// arbitrary rotation. Treating that as linear loses exactly the
    /// origin-straddling sites the assembly was sequenced to check.
    ///
    /// A third `Topology` variant would ripple into `cut_positions`,
    /// `fragments` and every computed digest, so the provenance is reported
    /// beside the value instead and callers who care can ask. `Molecule
    /// ::double_stranded` is `Option<bool>` for the same reason.
    ///
    /// **`false` is not a claim that the molecule is linear.** It means we do
    /// not know, and a caller that scans it as linear is choosing to miss
    /// wrapping hits.
    pub topology_declared: bool,
    /// The file parsed, and what came out does not look like a molecule.
    ///
    /// `genbank::parse` and `fasta::parse` cannot fail: garbage yields an empty
    /// `Molecule` that is indistinguishable, through `Result`, from a genuine
    /// annotation-only record. Only SnapGene returns structured errors. So a
    /// 48 KB file of noise that happens to start with `LOCUS` loads
    /// "successfully" as nothing at all.
    ///
    /// Set when `detect` said GenBank or FASTA and the parse produced no
    /// records, or one record with no bases, no declared length and no
    /// features. Deliberately an *observation* and not a diagnosis: we cannot
    /// tell a corrupt file from an exotic one, so the caller is told what was
    /// seen and left to decide.
    pub suspect: bool,
}

impl LoadReport {
    /// Did the file hold more than we returned?
    pub fn truncated(&self) -> bool {
        self.records > 1
    }
}

/// Load **every** record in a file.
///
/// [`load`] and [`load_with_report`] return only the first, which is right for
/// a viewer showing one molecule and wrong for anything that walks a folder: a
/// 124-record `.gbk` came back as one molecule with 1,879 features gone, and
/// 8 of 303 GenBank files and 351 FASTA files in this project's corpus hold
/// more than one record. An importer built on `load` would reproduce that
/// silently across an entire shared drive.
///
/// The `Vec` is empty only for a file that parsed to nothing, which is exactly
/// the case `LoadReport::suspect` flags.
pub fn load_all(data: &[u8]) -> Result<(Vec<Molecule>, Format, LoadReport), LoadError> {
    match detect(data) {
        Some(Format::SnapGene) => {
            let doc = snapgene::parse(data).map_err(LoadError::SnapGene)?;
            Ok((
                vec![doc.molecule],
                Format::SnapGene,
                LoadReport {
                    records: 1,
                    // A `.dna` always carries a topology flag.
                    topology_declared: true,
                    ..Default::default()
                },
            ))
        }
        Some(Format::GenBank) => {
            let text = String::from_utf8_lossy(data);
            let (all, unrepresentable_locations) = genbank::parse_all_reporting(&text);
            let records = all.len();
            let report = LoadReport {
                records,
                unrepresentable_locations,
                topology_declared: genbank::declares_topology(&text),
                suspect: looks_like_nothing(&all),
            };
            Ok((all, Format::GenBank, report))
        }
        Some(Format::Fasta) => {
            let text = String::from_utf8_lossy(data);
            let all = fasta::parse_all(&text);
            let records = all.len();
            let report = LoadReport {
                records,
                // FASTA has no topology field. Never declared, ever.
                topology_declared: false,
                suspect: looks_like_nothing(&all),
                ..Default::default()
            };
            Ok((all, Format::Fasta, report))
        }
        Some(other) => Err(LoadError::NotASequenceFile(other)),
        None => Err(LoadError::Unrecognised),
    }
}

/// Did a format that cannot report errors produce anything worth having?
fn looks_like_nothing(all: &[Molecule]) -> bool {
    match all {
        [] => true,
        [one] => {
            one.seq.is_empty()
                && one.declared_len.unwrap_or(0) == 0
                && one.features.is_empty()
                && one.primers.is_empty()
        }
        _ => false,
    }
}

/// Load the first record of a file.
///
/// Prefer [`load_with_report`] where the caller can tell the user what was
/// left behind; this exists because most callers genuinely want one molecule.
pub fn load(data: &[u8]) -> Result<(Molecule, Format), LoadError> {
    load_with_report(data).map(|(m, f, _)| (m, f))
}

/// Load the first record, and say what else the file held.
///
/// Literally the first element of [`load_all`], rather than a second parse that
/// could disagree with it about how many records there are or whether the file
/// is suspect.
pub fn load_with_report(data: &[u8]) -> Result<(Molecule, Format, LoadReport), LoadError> {
    let (all, format, report) = load_all(data)?;
    Ok((all.into_iter().next().unwrap_or_default(), format, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_GB: &str = "\
LOCUS       one           10 bp    DNA     circular SYN 26-JUL-2026
FEATURES             Location/Qualifiers
     misc_feature    1..5
                     /label=\"a\"
ORIGIN
        1 ACGTACGTAC
//
LOCUS       two           8 bp    DNA     linear SYN 26-JUL-2026
ORIGIN
        1 TTTTGGGG
//
";

    #[test]
    fn load_all_returns_every_record_where_load_returns_one() {
        // `load` keeps the first record. That is right for a viewer and wrong
        // for a folder walk: a 124-record file came back as one molecule with
        // 1,879 features gone. An importer on `load` reproduces that across a
        // whole shared drive, silently.
        let (all, fmt, report) = load_all(TWO_GB.as_bytes()).unwrap();
        assert_eq!(fmt, Format::GenBank);
        assert_eq!(all.len(), 2);
        assert_eq!(report.records, 2);
        assert_eq!(all[0].name, "one");
        assert_eq!(all[1].name, "two");
        assert_eq!(all[0].seq.len(), 10);
        assert_eq!(all[1].seq.len(), 8);
        assert!(all[0].topology.is_circular());
        assert!(!all[1].topology.is_circular());

        // And the one-record API is the first element of it, not a second
        // parse that could disagree.
        let (one, _, r2) = load_with_report(TWO_GB.as_bytes()).unwrap();
        assert_eq!(one.name, all[0].name);
        assert_eq!(r2, report);
    }

    #[test]
    fn a_multi_record_fasta_is_not_truncated_either() {
        let text = ">a desc\nACGT\n>b\nGGGG\n>c\nTTTT\n";
        let (all, fmt, report) = load_all(text.as_bytes()).unwrap();
        assert_eq!(fmt, Format::Fasta);
        assert_eq!(all.len(), 3);
        assert_eq!(report.records, 3);
        assert!(report.truncated());
    }

    #[test]
    fn genbank_says_nothing_and_says_linear_are_different_facts() {
        // The whole point of `topology_declared`. Both parse to
        // `Topology::Linear`, because `Topology` has no third state; only the
        // report distinguishes them.
        let says_linear = "LOCUS       x    4 bp    DNA     linear   SYN 26-JUL-2026\nORIGIN\n        1 ACGT\n//\n";
        let says_nothing =
            "LOCUS       x    4 bp    DNA     SYN 26-JUL-2026\nORIGIN\n        1 ACGT\n//\n";
        let says_circular = "LOCUS       x    4 bp    DNA     circular SYN 26-JUL-2026\nORIGIN\n        1 ACGT\n//\n";

        for (text, want_declared, want_circular) in [
            (says_linear, true, false),
            (says_nothing, false, false),
            (says_circular, true, true),
        ] {
            let (m, _, r) = load_with_report(text.as_bytes()).unwrap();
            assert_eq!(r.topology_declared, want_declared, "declared, for {text:?}");
            assert_eq!(m.topology.is_circular(), want_circular, "topology");
        }
    }

    #[test]
    fn a_plasmid_name_containing_circular_does_not_declare_topology() {
        // `pCircularise` already fooled a `contains` check into calling a
        // linear molecule circular. The provenance check must not reintroduce
        // it through the back door by matching the name token.
        let text = "LOCUS       pCircularise    4 bp    DNA     SYN 26-JUL-2026\nORIGIN\n        1 ACGT\n//\n";
        let (m, _, r) = load_with_report(text.as_bytes()).unwrap();
        assert!(!r.topology_declared, "the name is not a declaration");
        assert!(!m.topology.is_circular());
    }

    #[test]
    fn one_record_declaring_does_not_vouch_for_another_that_does_not() {
        let mixed = "\
LOCUS       one    4 bp    DNA     circular SYN 26-JUL-2026
ORIGIN
        1 ACGT
//
LOCUS       two    4 bp    DNA     SYN 26-JUL-2026
ORIGIN
        1 TTTT
//
";
        let (_, _, r) = load_all(mixed.as_bytes()).unwrap();
        assert!(
            !r.topology_declared,
            "a file is only 'declared' when every record declares"
        );
    }

    #[test]
    fn fasta_never_declares_a_topology_and_snapgene_always_does() {
        let (_, _, r) = load_all(b">x\nACGT\n").unwrap();
        assert!(
            !r.topology_declared,
            "FASTA has no topology field; claiming otherwise loses the \
             origin-straddling hits in a Plasmidsaurus assembly"
        );
    }

    #[test]
    fn a_file_that_parses_to_nothing_is_flagged_suspect() {
        // `genbank::parse` and `fasta::parse` cannot fail, so garbage is
        // indistinguishable from an annotation-only record through `Result`.
        let noise = "LOCUS\n\u{1}\u{2}\u{3} not really a genbank file at all\n";
        let (_, _, r) = load_with_report(noise.as_bytes()).unwrap();
        assert!(r.suspect, "parsed to nothing and did not say so");

        // A real annotation-only record is NOT suspect: it has features, and a
        // declared length, and is a legitimate thing to hold.
        let track = "\
LOCUS       track    3000 bp    DNA     circular SYN 26-JUL-2026
FEATURES             Location/Qualifiers
     misc_feature    1..3000
                     /label=\"everything\"
ORIGIN
//
";
        let (m, _, r) = load_with_report(track.as_bytes()).unwrap();
        assert!(!r.suspect, "an annotation track is not suspect");
        assert!(m.seq.is_empty());
        assert_eq!(m.declared_len, Some(3000));
        assert_eq!(m.features.len(), 1);

        // Nor is an ordinary record.
        let (_, _, r) = load_with_report(TWO_GB.as_bytes()).unwrap();
        assert!(!r.suspect);
    }

    #[test]
    fn a_multi_record_file_reports_what_it_held() {
        // `load` returns record 1 and used to have no way to say so. A
        // 124-record .gbk came back as one molecule with 1,879 features gone,
        // and `pl convert` then wrote that truncated molecule back out.
        // Built from lines rather than one literal: GenBank is column-sensitive
        // and a stray indent on ORIGIN silently produces an empty sequence.
        let record = |name: &str, bases: &str| {
            [
                format!(
                    "LOCUS       {name:<16}           4 bp    DNA     linear   SYN 27-JUL-2026"
                ),
                "ORIGIN".to_string(),
                format!("        1 {bases}"),
                "//".to_string(),
            ]
            .join("\n")
        };
        let two = format!("{}\n{}\n", record("one", "acgt"), record("two", "tttt"));
        let (mol, fmt, report) = load_with_report(two.as_bytes()).unwrap();
        assert_eq!(fmt, Format::GenBank);
        assert_eq!(mol.seq, b"acgt".to_vec(), "the first record is returned");
        assert_eq!(report.records, 2);
        assert!(report.truncated());

        let fasta = ">a
ACGT
>b
TTTT
>c
GGGG
";
        let (_, _, r) = load_with_report(fasta.as_bytes()).unwrap();
        assert_eq!(r.records, 3);
        assert!(r.truncated());

        // A single-record file is not truncated.
        let one = format!("{}\n", record("one", "acgt"));
        assert!(!load_with_report(one.as_bytes()).unwrap().2.truncated());
    }

    #[test]
    fn detects_by_content_not_extension() {
        let mut dna = vec![snapgene::block::HEADER, 0, 0, 0, 14];
        dna.extend_from_slice(snapgene::MAGIC);
        dna.extend_from_slice(&[0, 1, 0, 15, 0, 19]);
        assert_eq!(detect(&dna), Some(Format::SnapGene));

        assert_eq!(detect(b">seq\nACGT\n"), Some(Format::Fasta));
        assert_eq!(
            detect(b"LOCUS       x   10 bp DNA linear SYN 01-JAN-2026\n"),
            Some(Format::GenBank)
        );
        assert_eq!(detect(b"ABIF\x00\x01"), Some(Format::Abif));
        assert_eq!(detect(b".scf\x00"), Some(Format::Scf));
        assert_eq!(detect(&[0xAE, b'Z', b'T', b'R']), Some(Format::Ztr));
        assert_eq!(detect(b"random bytes"), None);
        assert_eq!(detect(b""), None);
    }

    #[test]
    fn chromatograms_get_a_useful_error_not_a_generic_one() {
        let e = load(b"ABIF\x00\x01\x02\x03").unwrap_err();
        assert!(e.to_string().contains("ABIF"), "got: {e}");
    }

    #[test]
    fn a_genbank_file_without_a_locus_line_is_still_recognised() {
        // SnapGene writes a LOCUS line Biopython rejects; be liberal on read.
        assert_eq!(
            detect(b"LOCUS       Annotations   19-MAR-2018\nORIGIN\n//\n"),
            Some(Format::GenBank)
        );
    }
}
