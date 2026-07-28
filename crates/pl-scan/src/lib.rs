//! Walking a folder, and keeping its index on disk.
//!
//! **This is the only crate under `crates/` that performs I/O.** That is an
//! invariant stronger than the written rule (`pl-core` has no I/O) and it was
//! emergent rather than stated, so it is stated here: the search engine in
//! `pl-index` is pure and provable, and the exception has its own crate whose
//! name announces it.
//!
//! It lives here rather than in `bins/pl` because two binaries need
//! byte-identical crash-safety semantics, and the CLI must be able to exercise
//! everything. Duplicating temp-write, fsync, rename and the fallback paths in
//! the CLI and the GUI guarantees the two copies drift apart.
//!
//! # Things a directory walker gets wrong on a real lab drive
//!
//! All measured on the corpus that motivates this feature; see
//! `docs/FINDINGS.md`.
//!
//! **Reparse points are not symlinks here.** The usual defence against
//! traversal cycles is to skip reparse points. On the measured corpus,
//! **68,811 of 68,813 files are reparse points** — tag `IO_REPARSE_TAG_CLOUD_6`,
//! with empty link target — because OneDrive marks every file it manages. A
//! walker that skipped them would skip the entire library and report zero
//! sequence files with no error at all. Depth is bounded instead, and symbolic
//! links are not followed unless asked.
//!
//! **Enumerate once and filter in memory.** `Get-ChildItem -Include` costs
//! 7.2× a plain walk of the same tree. The equivalent mistake here is calling
//! `metadata()` per candidate; on Windows the directory iterator already
//! carries size and mtime, so `DirEntry::metadata` is free.
//!
//! **Size-gate.** The corpus is bimodal: 1,075 plasmid files with a 3,184-byte
//! median beside NGS FASTA up to 1.39 GB. Parsing the largest single file costs
//! about six times everything else combined, so it is skipped by default and
//! *reported* as skipped rather than silently omitted.
//!
//! **Parse in-process.** Process startup is 7.94 ms against 0.53 ms to parse a
//! mean corpus file. 3,000 files take 1.6 s in-process and 25.4 s as
//! subprocesses. No threads: the 1.6 s is already acceptable and 24 cores would
//! only buy 1.5 s at the price of thread-safety everywhere.

use std::path::{Path, PathBuf};

pub mod scan;
pub mod store;
pub mod walk;

pub use scan::scan;
pub use store::{cache_dir, index_path, load, save, SaveError};
pub use walk::{walk, Found, WalkOptions, WalkReport};

use pl_core::{iupac, Molecule};
#[cfg(test)]
use pl_index::nibble;
use pl_index::{codec::Library, Row, State, Topology};

/// Files holding more bases than this are recorded, not searched.
///
/// **2 Mbase, chosen against the biology rather than against a disk budget.**
/// The largest thing this tool is for is a BAC at roughly 300 kb; a plasmid is
/// a few kb. Anything past 2 Mbase is a genome, an assembly or a reference set,
/// and indexing it means a lab drive's search cost is dominated by files nobody
/// is looking for a cloning site in.
///
/// The number was set by running on a real drive, not by guessing. At the first
/// attempt — a 64 MB byte cap and nothing else — the measured corpus produced
/// **11,562,363 records and a 5.9 GB index**. Capping records per file brought
/// that to 13,233 records and 1.1 Gbase, still 34x the size at which a search
/// stops feeling instant, because 2,338 individually-reasonable files add up.
/// Only a cap that reflects what a plasmid *is* fixes it.
///
/// Over-cap files get a row, a `TooLarge` state naming the count, and a line in
/// every coverage footer — never silence. `pl library --problems` lists them.
pub const MAX_BASES: u64 = 2_000_000;

/// The same cap in bytes, since we cannot count bases in a file we have not
/// opened.
///
/// Applied in **two** places, and the first one is the one that matters:
/// `scan` compares the walk's `size` against it *before* `std::fs::read`, so an
/// over-cap file is never allocated and never hashed. `rows_for_file` checks it
/// again because that function is pure and only ever sees bytes — a caller
/// handing it a 1.39 GB buffer has already paid, and the row it gets back must
/// still say `TooLarge` rather than attempt the parse.
pub const MAX_BYTES: u64 = 64_000_000;

/// Records one file may contribute before it is refused as a reference set.
///
/// **The byte cap does not catch this, and running on a real corpus is how it
/// was found.** A 14 MB FASTA holding 9,712 sequences passes a 64 MB gate
/// comfortably; the measured lab drive holds enough of them that a first index
/// came out at **11,562,363 records and 5.9 GB**, against ~1,100 actual plasmid
/// files. Every synthetic test passed.
///
/// A construct file holds one record. A legitimate multi-construct file holds a
/// few dozen — the largest in this project's own corpus is 124. A file holding
/// thousands is a reference database (SILVA, RefSeq, an assembly's contigs),
/// which is not what a plasmid library is for. Such files get one row, a
/// `TooLarge` state naming the count, and a line in every coverage footer.
pub const MAX_RECORDS_PER_FILE: usize = 256;

/// Total bases beyond which searches stop feeling instant.
///
/// Not a cap — a threshold to *report*. Measured throughput is ~335 Mbase/s
/// single-threaded, so the 100 ms budget that makes a search box feel
/// responsive is spent at roughly this size. Past it the library still works
/// and the caller says how long a search will take, rather than the user
/// discovering it.
pub const INTERACTIVE_BASES: u64 = 33_000_000;

/// Options for a scan.
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    pub walk: WalkOptions,
    /// Reuse rows from this index where the file has not changed.
    pub previous: Option<Library>,
}

/// What a scan did, for the user to read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub files_seen: usize,
    pub parsed: usize,
    /// Rows reused because the file was unchanged.
    pub reused: usize,
    /// Files whose content hash matched despite a new mtime — a sync client
    /// rewriting timestamps, typically.
    pub touched_only: usize,
    pub removed: usize,
    pub records: usize,
    /// Paths that could not be read, with the reason.
    pub unreadable: Vec<(String, String)>,
    /// The walk did not finish; **nothing was removed**.
    pub incomplete: Option<String>,
}

/// Content identity, used to decide whether a file must be re-parsed.
///
/// mtime and size alone are not proof of sameness: a sync client, a backup
/// restore, or an edit inside the filesystem's timestamp granularity can all
/// leave them unchanged over changed content. Storing the hash makes that
/// limitation *checkable* — `verify` re-reads every file and reports any row
/// whose bytes no longer agree — while keeping the everyday rescan at a `stat`.
pub fn content_id(bytes: &[u8]) -> String {
    hex(&pl_core::sha1::sha1(bytes))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Derive every row a file contributes, and its bases.
///
/// Pure apart from being handed the bytes, so it can be tested without a
/// filesystem.
pub fn rows_for_file(rel_path: &str, data: &[u8], size: u64) -> (Vec<Row>, Vec<u8>) {
    let mut rows = Vec::new();
    let mut bases = Vec::new();

    if size > MAX_BYTES {
        rows.push(Row {
            path: rel_path.to_string(),
            state: State::TooLarge,
            problem: format!("{size} bytes; the cap is {MAX_BYTES}"),
            ..Default::default()
        });
        return (rows, bases);
    }

    let (mols, _format, report) = match pl_fileio::load_all(data) {
        Ok(v) => v,
        Err(e) => {
            let state = match &e {
                pl_fileio::LoadError::NotASequenceFile(_) => State::NotASequenceFile,
                _ => State::Unreadable,
            };
            rows.push(Row {
                path: rel_path.to_string(),
                state,
                problem: e.to_string(),
                ..Default::default()
            });
            return (rows, bases);
        }
    };

    if mols.len() > MAX_RECORDS_PER_FILE {
        rows.push(Row {
            path: rel_path.to_string(),
            state: State::TooLarge,
            name: mols[0].name.clone(),
            n_features: 0,
            problem: format!(
                "{} records; the cap is {MAX_RECORDS_PER_FILE}. a file with this many                  sequences is a reference set, not a construct",
                mols.len()
            ),
            ..Default::default()
        });
        return (rows, bases);
    }

    if mols.is_empty() || report.suspect {
        rows.push(Row {
            path: rel_path.to_string(),
            state: State::SuspectParse,
            // An observation, not a diagnosis we cannot make: we can say what
            // was seen, not whether the file is corrupt or merely exotic.
            problem: format!("recognised as a sequence file; yielded no molecule ({size} bytes)"),
            ..Default::default()
        });
        return (rows, bases);
    }

    // The cap is on the *file*, not on each record: a hundred 1 Mbase contigs
    // cost as much to search as one 100 Mbase genome, and a caller reading a
    // per-record cap would think it was protected.
    let total: u64 = mols.iter().map(|m| m.len()).sum();
    if total > MAX_BASES {
        rows.push(Row {
            path: rel_path.to_string(),
            state: State::TooLarge,
            name: mols[0].name.clone(),
            problem: format!(
                "{total} bases across {} record(s); the cap is {MAX_BASES}",
                mols.len()
            ),
            ..Default::default()
        });
        return (rows, bases);
    }

    for (i, mol) in mols.iter().enumerate() {
        rows.push(derive_row(
            rel_path,
            i as u32,
            mol,
            report.topology_declared,
            &mut bases,
        ));
    }
    (rows, bases)
}

fn derive_row(
    rel_path: &str,
    record: u32,
    mol: &Molecule,
    topology_declared: bool,
    bases: &mut Vec<u8>,
) -> Row {
    let state = if !mol.seq.is_empty() {
        if mol.len() > MAX_BASES {
            State::TooLarge
        } else {
            State::Ok
        }
    } else if mol.is_annotation_track() {
        State::AnnotationTrack
    } else {
        State::NoBases
    };

    let mut row = Row {
        path: rel_path.to_string(),
        record,
        state,
        name: mol.name.clone(),
        topology: Topology::of(mol.topology.is_circular(), topology_declared),
        length: mol.len(),
        declared_len: mol.declared_len.unwrap_or(0),
        n_features: mol.features.len() as u32,
        text: searchable_text(mol),
        ..Default::default()
    };

    if state == State::Ok {
        // Offsets are in bases and assigned in walk order, so the store is
        // built once and never reshuffled.
        row.seq_off = 0; // fixed up by the caller, which owns the running total
        row.seq_bases = mol.len();
        row.ambiguous = mol
            .seq
            .iter()
            .filter(|&&b| !matches!(iupac::code_mask(b), 0b0001 | 0b0010 | 0b0100 | 0b1000))
            .count() as u64;
        bases.extend_from_slice(&mol.seq);
    } else if state == State::TooLarge {
        row.problem = format!("{} bases; the cap is {MAX_BASES}", mol.len());
    }
    row
}

/// Everything about a record that a text search should reach.
///
/// **Not trimmed, and joined with `\n`.** A feature name with meaningful
/// leading whitespace has to stay findable, and a separator that could appear
/// in a value would let two fields match as one.
fn searchable_text(mol: &Molecule) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if !mol.description.is_empty() {
        parts.push(&mol.description);
    }
    for f in &mol.features {
        parts.push(&f.name);
        parts.push(&f.kind);
        for (k, v) in &f.qualifiers {
            parts.push(k);
            if let Some(v) = v {
                parts.push(v);
            }
        }
    }
    for p in &mol.primers {
        parts.push(&p.name);
    }
    for (k, v) in &mol.notes {
        parts.push(k);
        parts.push(v);
    }
    parts.join("\n")
}

/// Normalise a path for storage: relative to the root, `/`-separated.
pub fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Join a stored relative path back onto a root.
pub fn abs(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in rel.split('/') {
        p.push(part);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_multi_record_file_becomes_one_row_per_record() {
        let text = "\
LOCUS       one           10 bp    DNA     circular SYN 26-JUL-2026
ORIGIN
        1 ACGTACGTAC
//
LOCUS       two            8 bp    DNA     linear SYN 26-JUL-2026
ORIGIN
        1 TTTTGGGG
//
";
        let (rows, bases) = rows_for_file("multi.gb", text.as_bytes(), text.len() as u64);
        assert_eq!(rows.len(), 2, "a folder walk must not keep only record 1");
        assert_eq!(rows[0].name, "one");
        assert_eq!(rows[1].name, "two");
        assert_eq!(rows[0].topology, Topology::Circular);
        assert_eq!(rows[1].topology, Topology::Linear);
        assert_eq!(bases, b"ACGTACGTACTTTTGGGG".to_vec());
    }

    #[test]
    fn a_fasta_record_is_undeclared_not_linear() {
        let (rows, _) = rows_for_file("x.fa", b">p\nACGT\n", 7);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].topology,
            Topology::Undeclared,
            "FASTA has no topology field; calling it linear loses wrapping hits"
        );
    }

    #[test]
    fn no_bases_and_annotation_track_and_suspect_are_three_different_states() {
        // "the file declares 3,000 bases and shipped none" is a different fact
        // from "these are coordinates for a sequence held elsewhere", and both
        // differ from "this parsed to nothing at all". Flattening any pair
        // gives a library that lists 3 kb plasmids holding no sequence.
        let declared = "\
LOCUS       track    3000 bp    DNA     circular SYN 26-JUL-2026
FEATURES             Location/Qualifiers
     misc_feature    1..3000
                     /label=\"everything\"
ORIGIN
//
";
        let (rows, bases) = rows_for_file("t.gb", declared.as_bytes(), declared.len() as u64);
        assert_eq!(
            rows[0].state,
            State::NoBases,
            "a declared length with no bases is NoBases, not a track"
        );
        assert_eq!(rows[0].declared_len, 3000);
        assert_eq!(rows[0].n_features, 1);
        assert!(bases.is_empty());

        // No declared length either: coordinates meant for a sequence held
        // somewhere else. UGENE and SnapGene both export these.
        let track = "\
LOCUS       track
FEATURES             Location/Qualifiers
     misc_feature    1..300
                     /label=\"elsewhere\"
//
";
        let (rows, _) = rows_for_file("u.gb", track.as_bytes(), track.len() as u64);
        assert_eq!(rows[0].state, State::AnnotationTrack);
        assert_eq!(rows[0].declared_len, 0);
        assert_eq!(rows[0].n_features, 1);

        let noise = "LOCUS\n\u{1}\u{2} not a genbank file\n";
        let (rows, _) = rows_for_file("n.gb", noise.as_bytes(), noise.len() as u64);
        assert_eq!(rows[0].state, State::SuspectParse);
        assert!(
            rows[0].problem.contains("yielded no molecule"),
            "{:?}",
            rows[0].problem
        );
    }

    #[test]
    fn a_file_that_is_not_a_sequence_file_gets_a_row_and_a_reason() {
        // A lab drive holds 394 `.ab1` chromatograms. Silence about them would
        // be indistinguishable from having missed them.
        let mut abif = b"ABIF".to_vec();
        abif.extend_from_slice(&[0u8; 64]);
        let (rows, _) = rows_for_file("trace.ab1", &abif, abif.len() as u64);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, State::NotASequenceFile);
        assert!(rows[0].problem.contains("ABIF"), "{:?}", rows[0].problem);
    }

    #[test]
    fn an_oversized_file_is_recorded_rather_than_skipped_in_silence() {
        let (rows, bases) = rows_for_file("huge.fa", b">x\nACGT\n", MAX_BYTES + 1);
        assert_eq!(rows[0].state, State::TooLarge);
        assert!(rows[0].problem.contains("cap"), "{:?}", rows[0].problem);
        assert!(bases.is_empty(), "an over-cap file contributes no sequence");
    }

    #[test]
    fn searchable_text_reaches_qualifiers_and_is_not_trimmed() {
        let text = "\
LOCUS       x           10 bp    DNA     circular SYN 26-JUL-2026
DEFINITION  a description
FEATURES             Location/Qualifiers
     CDS             1..9
                     /label=\"  AmpR  \"
                     /note=\"confers ampicillin resistance\"
ORIGIN
        1 ACGTACGTAC
//
";
        let (rows, _) = rows_for_file("x.gb", text.as_bytes(), text.len() as u64);
        let t = &rows[0].text;
        assert!(
            t.contains("  AmpR  "),
            "leading whitespace must survive: {t:?}"
        );
        assert!(t.contains("confers ampicillin resistance"), "{t:?}");
        assert!(
            t.contains("CDS"),
            "the feature key is searchable too: {t:?}"
        );
    }

    #[test]
    fn ambiguous_bases_are_counted_per_record() {
        let text = "\
LOCUS       x           10 bp    DNA     circular SYN 26-JUL-2026
ORIGIN
        1 ACGTNNRYAC
//
";
        let (rows, _) = rows_for_file("x.gb", text.as_bytes(), text.len() as u64);
        assert_eq!(rows[0].ambiguous, 4, "N N R Y");
    }

    #[test]
    fn paths_are_stored_relative_and_slash_separated() {
        let root = Path::new("C:/lab/plasmids");
        let p = Path::new("C:/lab/plasmids/sub dir/x.gb");
        assert_eq!(rel(root, p), "sub dir/x.gb");
        assert_eq!(
            abs(root, "sub dir/x.gb"),
            PathBuf::from("C:/lab/plasmids/sub dir/x.gb")
        );
        // A path outside the root is kept whole rather than mangled.
        assert!(rel(root, Path::new("D:/elsewhere/y.gb")).contains("elsewhere"));
    }

    #[test]
    fn content_id_is_stable_and_distinguishes_a_one_bit_change() {
        assert_eq!(content_id(b"abc"), content_id(b"abc"));
        assert_ne!(content_id(b"abc"), content_id(b"abd"));
        assert_eq!(content_id(b"abc").len(), 40);
    }

    #[test]
    fn packing_a_derived_record_agrees_with_its_own_row() {
        // The row says how many bases it contributed; the store must hold
        // exactly that many, or every later record reads from the wrong place.
        let text = "\
LOCUS       one           10 bp    DNA     circular SYN 26-JUL-2026
ORIGIN
        1 ACGTACGTAC
//
LOCUS       two            8 bp    DNA     linear SYN 26-JUL-2026
ORIGIN
        1 TTTTGGGG
//
";
        let (rows, bases) = rows_for_file("m.gb", text.as_bytes(), text.len() as u64);
        let total: u64 = rows.iter().map(|r| r.seq_bases).sum();
        assert_eq!(total, bases.len() as u64);
        let packed = nibble::pack(&bases);
        assert_eq!(packed.len(), bases.len().div_ceil(2));
    }
}
