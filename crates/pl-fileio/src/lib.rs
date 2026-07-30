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

/// A UTF-8 byte-order mark, if the file opens with one.
///
/// U+FEFF is not whitespace to `str::trim`, not a digit, and not an ASCII
/// letter, so every `starts_with("LOCUS")` and `strip_prefix('>')` in this crate
/// treats it as ordinary content — and nothing else in the crate removed it.
/// PowerShell 5.1's `Out-File -Encoding utf8` and older Notepad "Save As UTF-8"
/// both emit one, so this arrives on real files that a user edited by hand.
///
/// What it cost, measured on a BOM'd copy of pUC19 (L09137.2, `circular`):
/// `genbank::parse_record` missed the LOCUS line entirely, so the name, the
/// declared length, the strandedness and the topology were all silently lost and
/// the plasmid loaded as **linear** — `pl digest --enzyme EcoRI` then printed two
/// fragments of 2290 and 396 bp where the same bytes without the BOM give one of
/// 2686, exit 0 and nothing on stderr. Worse, the same `starts_with("LOCUS")`
/// guards the trailing record in `parse_all_reporting`, so a BOM'd file with no
/// closing `//` produced **zero** records: "length 0 bp", exit 0. And a BOM'd
/// FASTA was not recognised at all, because `detect` sniffs `>` after
/// `trim_start`, which leaves U+FEFF in place.
///
/// Stripped once here, at the format boundary, rather than teaching each token
/// rule about it: the rules are right, the bytes reaching them were not. Only
/// one mark is removed — a second U+FEFF is a zero-width no-break space in the
/// document, not a mark, and inventing a rule for it would be guessing.
fn strip_bom(data: &[u8]) -> &[u8] {
    data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data)
}

/// Identify a file from its leading bytes.
pub fn detect(data: &[u8]) -> Option<Format> {
    // Before the magic checks as well as the text sniff: no binary format here
    // begins `EF BB BF`, so nothing is masked, and `detect` and the parsers then
    // agree about where the file starts.
    let data = strip_bom(data);
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
    /// Paths in a `.dna`'s notes block that the model has no shape for.
    ///
    /// Three forms, spelled apart so one report can carry all three:
    /// `Notes/References/Reference` (a subtree under a note),
    /// `Notes/Comments/text()` (text following one) and `Notes@version` (an
    /// attribute on the root).
    ///
    /// A sibling of `unrepresentable_locations` and deliberately not the same
    /// field: `pl info` prints that one as "location(s) this reader cannot
    /// represent", and folding `Notes/References/Reference` into it would make
    /// the CLI state something false about coordinates — the same
    /// encode-it-into-the-neighbouring-field mistake this whole channel exists
    /// to avoid. Empty for every format that is not SnapGene, and for every
    /// `.dna` whose block 6 is flat, which is most of them.
    ///
    /// A note's own key, text and attributes are all kept; see
    /// `snapgene::Document::unrepresentable_notes`.
    pub unrepresentable_notes: Vec<String>,
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
            let report = LoadReport {
                records: 1,
                // A `.dna` always carries a topology flag.
                topology_declared: true,
                unrepresentable_notes: doc.unrepresentable_notes,
                ..Default::default()
            };
            Ok((vec![doc.molecule], Format::SnapGene, report))
        }
        Some(Format::GenBank) => {
            // `strip_bom`: a leading U+FEFF survives `from_utf8_lossy` and makes
            // the first line's `starts_with("LOCUS")` false, which loses the
            // name, the declared length, the strandedness and the topology.
            let text = String::from_utf8_lossy(strip_bom(data));
            let (all, unrepresentable_locations) = genbank::parse_all_reporting(&text);
            let records = all.len();
            let report = LoadReport {
                records,
                unrepresentable_locations,
                topology_declared: genbank::declares_topology(&text),
                suspect: looks_like_nothing(&all),
                // GenBank has no notes block; only a `.dna` fills this.
                unrepresentable_notes: Vec::new(),
            };
            Ok((all, Format::GenBank, report))
        }
        Some(Format::Fasta) => {
            // `strip_bom`: U+FEFF is not ASCII whitespace, so the reader would
            // otherwise keep its three bytes as bases of the first record.
            let text = String::from_utf8_lossy(strip_bom(data));
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

/// A file name reduced to what a map should print in the middle of the ring.
///
/// The fallback to the filename is **correct and stays**: the `.dna` container
/// carries no molecule name at all — [`snapgene`]'s reader lifts only a
/// `Description` note, and `pl info` confirms there is no name field — so for a
/// SnapGene file there is nothing else to print. What was wrong was printing
/// the container's extension as though it were part of the plasmid's name.
/// "pKoV with His decR.dna" is a filename; "pKoV with His decR" is what the
/// user calls the plasmid.
///
/// Only the **final** extension goes, and nothing else is touched:
///
/// ```text
/// "pKoV with His decR.dna"  ->  "pKoV with His decR"
/// "pBR322.v2.gb"            ->  "pBR322.v2"     (rsplit, not split)
/// "pUC19"                   ->  "pUC19"         (no extension)
/// ".hidden"                 ->  ".hidden"       (empty stem: kept whole)
/// ```
///
/// **Neither existing helper does this**, which is why there is a third.
/// [`genbank::locus_name`] sanitises every non-alphanumeric to `_` and truncates
/// to 16, so it would caption this file `pKoV_with_His_de` — mangled *and* still
/// not the plasmid's name; it is the right function for an output *filename* and
/// the wrong one for a caption. `pl-gui`'s `design::stem_of` splits on the
/// *first* dot, so `pBR322.v2.gb` becomes `pBR322`.
///
/// A real molecule name always wins over this — see the caller in
/// `bins/pl-gui/src/main.rs` and `pl_draw::Options::title`. This is only what
/// to say when the file did not say anything.
pub fn caption_of(file_name: &str) -> &str {
    let base = file_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(file_name)
        .trim();
    match base.rsplit_once('.') {
        // An empty stem is a dotfile, not an extension: `.hidden` names the
        // whole thing. Captioning it with the empty string would leave a map
        // with no title at all.
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => base,
    }
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

    /// COMPILE-ONLY at e087e27: `caption_of` does not exist there, and the
    /// caption was `d.title` with its extension on. The behaviour it fixes is
    /// proven at e087e27 by the frame test in `bins/pl-gui`, which paints the
    /// caption and reads "pKoV with His decR.dna" off it.
    #[test]
    fn a_caption_loses_the_container_and_nothing_else() {
        // The user's own file.
        assert_eq!(caption_of("pKoV with His decR.dna"), "pKoV with His decR");
        // rsplit, not split: `stem_of` in the GUI's design panel splits on the
        // FIRST dot and would caption this `pBR322`.
        assert_eq!(caption_of("pBR322.v2.gb"), "pBR322.v2");
        // No extension is not an empty name.
        assert_eq!(caption_of("pUC19"), "pUC19");
        // A dotfile is all stem. Captioning it with the empty string would
        // leave a ring with no title in the middle of it.
        assert_eq!(caption_of(".hidden"), ".hidden");
        assert_eq!(caption_of(""), "");
        // A path, from a drop or a recent-files entry, is not a name.
        assert_eq!(
            caption_of(r"C:\plasmids\pKoV with His decR.dna"),
            "pKoV with His decR"
        );
        assert_eq!(caption_of("/home/lior/pKoV.gb"), "pKoV");
        // And nothing is sanitised. `genbank::locus_name` would answer
        // `pKoV_with_His_de` here -- mangled, truncated to 16, and still not the
        // plasmid's name. It is the right function for an output filename and
        // the wrong one for a caption.
        assert_ne!(
            caption_of("pKoV with His decR.dna"),
            genbank::locus_name("pKoV with His decR")
        );
        assert!(caption_of("a b (c) 2.dna").contains(' '));
    }

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
    fn a_utf8_bom_does_not_hide_the_first_line() {
        // PowerShell 5.1's `Out-File -Encoding utf8` and older Notepad both
        // write one. U+FEFF is not whitespace, not a digit and not a letter, so
        // it made `starts_with("LOCUS")` false and the whole LOCUS block was
        // skipped: a circular plasmid loaded linear and `pl digest` printed two
        // fragments where there is one, at exit 0 with an empty stderr.
        const BOM: &str = "\u{feff}";
        let gb =
            "LOCUS       pUC19                   2686 bp ds-DNA     circular SYN 26-JUL-2026\n\
                  ORIGIN\n        1 acgtacgtac\n//\n";
        let (m, fmt, r) = load_with_report(format!("{BOM}{gb}").as_bytes()).unwrap();
        assert_eq!(fmt, Format::GenBank);
        assert_eq!(m.name, "pUC19");
        assert!(
            m.topology.is_circular(),
            "a circular plasmid read as linear"
        );
        assert_eq!(m.declared_len, Some(2686));
        assert_eq!(m.double_stranded, Some(true));
        assert!(r.topology_declared);
        // Same bytes, same answer.
        let (plain, _, _) = load_with_report(gb.as_bytes()).unwrap();
        assert_eq!(plain.topology.is_circular(), m.topology.is_circular());

        // The trailing-record guard uses the same token test, so a file with no
        // closing `//` — which the parser otherwise accepts — lost its record
        // entirely rather than just its header.
        let noterm = format!(
            "{BOM}LOCUS       x                         10 bp    DNA     circular SYN 26-JUL-2026\n\
             ORIGIN\n        1 acgtacgtac\n"
        );
        let (m, _, _) = load_with_report(noterm.as_bytes()).unwrap();
        assert_eq!(m.seq.len(), 10, "the whole sequence was dropped");
        assert!(m.topology.is_circular());

        // And a BOM'd FASTA was not recognised at all: `detect` sniffs `>` after
        // `trim_start`, which does not remove U+FEFF.
        let fa = format!("{BOM}>pUC19 cloning vector\nACGTACGTAC\n");
        assert_eq!(detect(fa.as_bytes()), Some(Format::Fasta));
        let (m, _, r) = load_with_report(fa.as_bytes()).unwrap();
        assert_eq!(m.name, "pUC19");
        assert_eq!(m.description, "cloning vector");
        assert_eq!(m.seq, b"ACGTACGTAC".to_vec(), "the mark was read as bases");
        assert_eq!(r.records, 1);
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
