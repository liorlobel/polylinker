//! The library index: what is known about a folder of sequence files.
//!
//! # What this is, and what it is deliberately not
//!
//! It is a **cache**, and every safety argument here rests on that. Crash
//! during a write → an orphaned temporary and an intact index. Corruption →
//! the checksum catches it and the index is rebuilt. Schema change → the
//! version number forces a rebuild. Two writers → last one wins, over two
//! complete files. Every problem a database would solve degrades to "discard
//! and rebuild", and rebuilding costs seconds.
//!
//! That argument fails the moment the index holds something the user typed.
//! Tags, assigned folders, per-file notes, stars, custom ordering: for those,
//! "rebuild" is data loss, and they belong in a separate file that is never
//! rebuilt, keyed to a stable per-file identifier — **not** to a sequence hash,
//! because in an application whose main verb is "edit the plasmid" that would
//! silently detach the label the user typed the moment they changed a base.
//! `docs/PLAN.md` asks for folders and recent files in the same sentence as
//! search, so this is one release away, and it is written down here rather than
//! rediscovered.
//!
//! # Why not SQLite, which `docs/PLAN.md` specifies
//!
//! Not primarily the dependency. **FTS5 answers the wrong question about this
//! data, and answers it silently.** Measured against real plasmid feature
//! names: `MATCH '"uc"*'` returns nothing for `pUC ori`, and `MATCH '"101"*'`
//! returns nothing for `Rep101(Ts)` or `pSC101 ori`, because the default
//! tokenizer matches prefixes of tokens and a plasmid name is not word-shaped.
//! The trigram tokenizer fixes infixes and then returns nothing for any query
//! under three characters. A user searching `uc` would read "not in my library"
//! where the truth is "not asked".
//!
//! Meanwhile the query nobody else offers — degenerate, both-strand,
//! origin-wrapping motif search — is one FTS5 cannot express at all, and it
//! costs a `for` loop. The measured corpus (`docs/FINDINGS.md`) is 23.2 Mbase,
//! packing to 11.1 MiB and scanning in 69 ms; searchable text is about a
//! megabyte, and a substring pass over it is microseconds. There is no
//! performance problem here for a storage engine to solve.
//!
//! **Reversal condition**: if user-authored state arrives, the answer is redb —
//! zero transitive dependencies, pure Rust, builds for wasm32 — not SQLite, and
//! it goes in a separate never-rebuilt file.
//!
//! # No I/O
//!
//! Nothing in this crate touches the filesystem; `pl-scan` does. It also means
//! the browser tool can search an in-memory `Vec<Row>` through exactly this
//! code.
//!
//! **What enforces that, and what does not.** The `wasm32-unknown-unknown`
//! build in `tools/ci.ps1` is a real gate, but on a *weaker* property than the
//! one it was described as enforcing. It goes red for a dependency that cannot
//! build for wasm32 — rusqlite, memmap2, anything libc-bound — and for a
//! platform-specific API such as `std::os::windows::fs::MetadataExt`. It does
//! not go red for hand-written storage code: wasm32-unknown-unknown ships the
//! whole of `std::fs`, `std::env`, `std::process` and `SystemTime` through the
//! `unsupported` platform layer, so `std::fs::read("library.plx")` added to
//! `codec.rs` compiles and links cleanly and fails only at run time, on a
//! target this project never actually runs. "wasm32 has no filesystem" is not
//! true in the sense that matters to a compiler.
//!
//! So the claim is split in two and both halves are enforced: dependencies and
//! platform APIs by the wasm32 build, and the source itself by
//! `tests/purity.rs`, which reads every file under `src/` and fails on the call
//! — and which carries its own test that it would.

pub mod codec;
pub mod nibble;
pub mod query;
pub mod scan;

/// The derivation version.
///
/// Distinct from the file-layout version, and it exists for a failure nobody
/// catches. Every derived field — the searchable text, the feature count, the
/// identity keys, the state — is a function of *the parser*, not only of the
/// file. Ship a GenBank fix that teaches the reader a location form it used to
/// report as unrepresentable, and on the next rescan every file is "unchanged",
/// every row is reused, and the fix never reaches the library. `--verify` will
/// not catch it either, because the file's content hash still matches. The user
/// sees `3,002 unchanged (reused)`, which reads as success.
///
/// **Bump this by hand whenever parsing or derivation changes.** A false
/// positive costs one rebuild; a false negative costs a wrong answer.
pub const ENGINE: u32 = 1;

/// The on-disk layout version. Bumped when the bytes change shape.
pub const FORMAT: u32 = 1;

/// What we know about one record — one file may hold many.
///
/// Records, not files: a 124-record `.gbk` is 124 rows. Indexing per file would
/// reproduce, across a whole shared drive, the truncation that lost 1,879
/// features from a single file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Row {
    /// Path relative to the indexed root, `/`-separated.
    pub path: String,
    /// 0-based index of this record within its file.
    pub record: u32,
    /// Source file size in bytes.
    ///
    /// Filesystem facts live on the row because they are the *invalidation
    /// key*: `(format, engine, path, size, mtime)` decides whether a rescan may
    /// reuse this row without reading the file. Storing them here rather than
    /// in a sidecar keeps one file to write atomically instead of two that
    /// could be observed disagreeing. They are data about a file, not I/O, so
    /// they do not compromise this crate's purity.
    pub size: u64,
    /// Source file mtime, nanoseconds since the Unix epoch; 0 if unknown.
    ///
    /// **Not proof of sameness.** A sync client, a backup restore, or an edit
    /// inside the filesystem's timestamp granularity can all leave `(size,
    /// mtime)` unchanged over changed content, which is why `content` exists
    /// and `--verify` can check it.
    pub mtime_ns: i128,
    /// SHA-1 of the whole source file, lowercase hex. Empty when not computed.
    ///
    /// Two jobs: it makes the mtime shortcut *checkable*, and it lets an
    /// unchanged file whose mtime moved — which is what a sync client does —
    /// skip the parse instead of re-parsing the entire library on every sync.
    pub content: String,
    pub state: State,
    pub name: String,
    pub topology: Topology,
    /// Bases actually present.
    pub length: u64,
    /// Length the file declared while carrying no bases.
    pub declared_len: u64,
    pub n_features: u32,
    /// Feature names, kinds, qualifier values, primer names, notes — one
    /// searchable blob. Never trimmed: a feature name with meaningful leading
    /// whitespace, or a note value that is entirely whitespace, must stay
    /// findable.
    pub text: String,
    /// Bases whose mask is not exactly one of A/C/G/T.
    pub ambiguous: u64,
    /// Offset into the packed store, in bases.
    pub seq_off: u64,
    /// Bases of this record in the packed store. Zero unless `state` is `Ok`.
    pub seq_bases: u64,
    /// What was observed, when something was. An observation, never a
    /// diagnosis we are not in a position to make.
    pub problem: String,
}

/// Why a record does or does not carry searchable sequence.
///
/// These are distinct facts in `pl-core` already — `sequence_absent` and
/// `is_annotation_track` — and flattening them produces a library that lists
/// 2.9 Mb plasmids holding no bases at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    /// Bases present and searchable.
    #[default]
    Ok,
    /// A real record with a declared length and no bases.
    NoBases,
    /// Coordinates meant to be applied to a sequence held elsewhere.
    AnnotationTrack,
    /// Recognised, but a chromatogram or similar.
    NotASequenceFile,
    /// Could not be read at all.
    Unreadable,
    /// A cloud placeholder that was not materialised.
    NotDownloaded,
    /// Past the size cap. The corpus that motivates this feature holds single
    /// FASTA files of 1.39 GB beside a 3 kB median plasmid, and one of them
    /// would cost six times the parse time of everything else.
    TooLarge,
    /// It parsed, and what came out does not look like a molecule.
    SuspectParse,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Ok => "ok",
            State::NoBases => "no-bases",
            State::AnnotationTrack => "annotation-track",
            State::NotASequenceFile => "not-a-sequence-file",
            State::Unreadable => "unreadable",
            State::NotDownloaded => "not-downloaded",
            State::TooLarge => "too-large",
            State::SuspectParse => "suspect-parse",
        }
    }
    pub fn from_name(s: &str) -> Option<State> {
        Some(match s {
            "ok" => State::Ok,
            "no-bases" => State::NoBases,
            "annotation-track" => State::AnnotationTrack,
            "not-a-sequence-file" => State::NotASequenceFile,
            "unreadable" => State::Unreadable,
            "not-downloaded" => State::NotDownloaded,
            "too-large" => State::TooLarge,
            "suspect-parse" => State::SuspectParse,
            _ => return None,
        })
    }
    /// Is there sequence to search?
    pub fn searchable(self) -> bool {
        matches!(self, State::Ok)
    }
}

/// Topology **with its provenance**, which `pl_core::Topology` cannot carry.
///
/// `Undeclared` is not a third kind of molecule; it is the admission that the
/// file did not say. FASTA never says, and a Plasmidsaurus assembly of a
/// plasmid arrives as FASTA at an arbitrary rotation — so reading "no topology
/// field" as "linear" loses exactly the origin-straddling hits that assembly
/// was sequenced to check.
///
/// An `Undeclared` record is **scanned as circular**, which is a strict
/// superset of the linear scan: the extra members are precisely the wrapping
/// hits. Every such hit is flagged, and the record is counted in the coverage
/// footer. Nothing is missed and nothing is silently asserted; the cost is
/// `k - 1` extra scan starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Topology {
    Circular,
    Linear,
    #[default]
    Undeclared,
}

impl Topology {
    pub fn as_str(self) -> &'static str {
        match self {
            Topology::Circular => "circular",
            Topology::Linear => "linear",
            Topology::Undeclared => "undeclared",
        }
    }
    pub fn from_name(s: &str) -> Option<Topology> {
        Some(match s {
            "circular" => Topology::Circular,
            "linear" => Topology::Linear,
            "undeclared" => Topology::Undeclared,
            _ => return None,
        })
    }
    /// How the scan should treat it. Undeclared scans as circular.
    pub fn scan_as_circular(self) -> bool {
        !matches!(self, Topology::Linear)
    }
    /// Did the file say, or are we guessing?
    pub fn declared(self) -> bool {
        !matches!(self, Topology::Undeclared)
    }
    /// From a `pl-core` topology plus the provenance `pl-fileio` reports.
    pub fn of(circular: bool, declared: bool) -> Topology {
        match (declared, circular) {
            (false, _) => Topology::Undeclared,
            (true, true) => Topology::Circular,
            (true, false) => Topology::Linear,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_and_topology_round_trips_through_its_name() {
        // The names are the on-disk spelling. A silent rename would make old
        // indexes parse into the wrong bucket rather than fail.
        for s in [
            State::Ok,
            State::NoBases,
            State::AnnotationTrack,
            State::NotASequenceFile,
            State::Unreadable,
            State::NotDownloaded,
            State::TooLarge,
            State::SuspectParse,
        ] {
            assert_eq!(State::from_name(s.as_str()), Some(s));
        }
        for t in [Topology::Circular, Topology::Linear, Topology::Undeclared] {
            assert_eq!(Topology::from_name(t.as_str()), Some(t));
        }
        assert_eq!(State::from_name("nonsense"), None);
        assert_eq!(Topology::from_name("round"), None);
    }

    #[test]
    fn an_undeclared_topology_is_scanned_as_circular_and_says_so() {
        assert!(Topology::Undeclared.scan_as_circular());
        assert!(!Topology::Undeclared.declared());
        assert!(Topology::Circular.scan_as_circular());
        assert!(!Topology::Linear.scan_as_circular());
        assert!(Topology::Linear.declared());
    }

    #[test]
    fn undeclared_beats_the_default_that_pl_core_had_to_pick() {
        // pl-fileio returns Topology::Linear for a file that said nothing,
        // because pl_core::Topology has no third state. The provenance flag is
        // the only thing that separates the two, and this is where they part.
        assert_eq!(Topology::of(false, false), Topology::Undeclared);
        assert_eq!(Topology::of(true, false), Topology::Undeclared);
        assert_eq!(Topology::of(false, true), Topology::Linear);
        assert_eq!(Topology::of(true, true), Topology::Circular);
    }

    #[test]
    fn only_ok_records_carry_searchable_sequence() {
        assert!(State::Ok.searchable());
        for s in [
            State::NoBases,
            State::AnnotationTrack,
            State::NotASequenceFile,
            State::Unreadable,
            State::NotDownloaded,
            State::TooLarge,
            State::SuspectParse,
        ] {
            assert!(!s.searchable(), "{} must not be searched", s.as_str());
        }
    }
}
