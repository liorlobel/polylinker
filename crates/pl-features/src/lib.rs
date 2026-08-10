//! `polylinker-features` — the annotation database, and the matcher that uses it.
//!
//! # Why this crate exists
//!
//! Every open annotator in this field ultimately depends on one CSV that was
//! scraped from SnapGene's proprietary Common Features list in 2021 and
//! redistributed with no licence at all. It is simultaneously the most-used
//! dataset in the ecosystem and its clearest legal exposure. `docs/PLAN.md` §8.3
//! calls replacing it "the highest-leverage contribution available to anyone in
//! this field", and this crate is the consuming half of that replacement.
//!
//! The data lives in `features/` under **CC BY 4.0**, versioned and released
//! separately from this code. `features/SOURCING.md` records how each source was
//! cleared and by what evidence.
//!
//! # The two things this schema does differently, and why
//!
//! **Provenance is per *field*, not per row.** One feature record legitimately
//! mixes licences: the name and family come from UniProt (CC BY 4.0), the
//! nucleotides from an INSDC record (free-and-unrestricted, but with a credit
//! expectation to the original submitter), and the boundary rule and description
//! are our own work. A single `source_licence` column would have to pick one and
//! be wrong about the rest — which is the exact failure this project exists to
//! avoid. See [`FieldSource`].
//!
//! **A boundary is a claim, so it says how it was reached.** "Where does AmpR
//! start?" has three different kinds of answer: computed from a reading frame,
//! stipulated by a paper, or a convention read off several depositors. The first
//! is a fact we derived and can show the arithmetic for; the last is the actual
//! intellectual content of a feature database, and the part that must be derived
//! independently rather than inherited. [`BoundaryRule`] makes the distinction a
//! field instead of a footnote, so a challenge can be answered per row.
//!
//! # Layout
//!
//! - [`align`] — infix alignment, the verification step.
//! - [`index`] — k-mer seeding, the "which features are worth checking" step.
//! - [`annotate`] — the §7.7 pipeline that turns a molecule into annotations.

pub mod align;
pub mod annotate;
pub mod index;

use std::collections::{BTreeMap, BTreeSet};

/// How far an entry has got through curation.
///
/// Ordered so a release gate is one comparison: `Proposed < Reviewed < Verified`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReviewStatus {
    /// Machine-extracted from a source, or drafted by a tool. **Not shippable.**
    /// §8.3 rule 6: "AI may propose, never assert."
    Proposed,
    /// A named human checked the sequence against the cited accession and wrote
    /// the description from the primary source.
    Reviewed,
    /// Reviewed, and additionally confirmed to match in a real construct.
    Verified,
}

impl ReviewStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewStatus::Proposed => "proposed",
            ReviewStatus::Reviewed => "reviewed",
            ReviewStatus::Verified => "verified",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "proposed" => Some(ReviewStatus::Proposed),
            "reviewed" => Some(ReviewStatus::Reviewed),
            "verified" => Some(ReviewStatus::Verified),
            _ => None,
        }
    }
}

/// What kind of thing a feature is — which decides how its boundary can be known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Protein-coding. The boundary is the open reading frame, so it is
    /// computed rather than chosen, and both nucleotide and protein references
    /// exist.
    ///
    /// This doc used to end "This is the only class translated matching can
    /// find", and that sentence was the whole reason twenty curated tags sat
    /// declared and unissued in `features/build/stage_curated.py`. It stopped
    /// being true on 2026-07-28: [`Class::SyntheticPart`] may now carry
    /// `reference_aa` too. What is still true, and is the part worth keeping, is
    /// that `cds` is the only class whose boundary is *derived* from a reading
    /// frame — see [`BoundaryRule::is_derived`].
    Cds,
    /// Promoters, terminators, operators, RBS. No automatable source gives a
    /// defensible boundary; depositors disagree with each other.
    Regulatory,
    Origin,
    Repeat,
    /// Tags, linkers, protease sites, 2A peptides, MCSs — designed, so the
    /// boundary is whatever the designing paper stipulated.
    ///
    /// The second class permitted to carry `reference_aa`, and for the opposite
    /// reason to [`Class::Cds`]: a tag *is* a peptide. FLAG is `DYKDDDDK` and
    /// has dozens of synonymous encodings, so a nucleotide reference for it can
    /// only ever be one arbitrary encoding and will miss every re-coded copy —
    /// which is `features/SOURCING.md` §3's argument, made in the schema at
    /// last. A row of this class may carry nucleotides, a peptide, or both.
    ///
    /// A peptide-only row of this class is matched under two extra rules the
    /// annotator applies and the loader does not: it must match **exactly and
    /// wholly**, and the hit must lie in frame inside an open reading frame of
    /// the query. See `annotate::Annotator` and `features/README.md`.
    SyntheticPart,
    Misc,
}

impl Class {
    pub fn as_str(&self) -> &'static str {
        match self {
            Class::Cds => "cds",
            Class::Regulatory => "regulatory",
            Class::Origin => "origin",
            Class::Repeat => "repeat",
            Class::SyntheticPart => "synthetic_part",
            Class::Misc => "misc",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cds" => Some(Class::Cds),
            "regulatory" => Some(Class::Regulatory),
            "origin" => Some(Class::Origin),
            "repeat" => Some(Class::Repeat),
            "synthetic_part" => Some(Class::SyntheticPart),
            "misc" => Some(Class::Misc),
            _ => None,
        }
    }
}

/// How this record's start and end were arrived at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryRule {
    /// Start codon through stop codon of the frame translating to the verified
    /// reference protein. Nobody chose it; it is a property of the sequence.
    ///
    /// READ THE DEFINITION, NOT THE NAME. "Atg" in the identifier is narrower
    /// than the rule: an initiation codon is whatever initiates, and `GTG` and
    /// `TTG` are common real starts in transposon-borne markers — tet(A) begins
    /// `GTG` — read as formyl-Met rather than as Val or Leu. Seven shipped rows
    /// carry this rule over a sequence that does not begin `ATG`, all of them
    /// correctly, and each states its initiator codon in `notes`.
    ///
    /// The string form is published, so it is not being renamed; the mismatch
    /// is stated here and in `features/README.md` instead. It matters because
    /// [`BoundaryRule::is_derived`] treats this as the strongest derivation
    /// claim in the schema, and an auditor reading the label literally would
    /// think those seven rows misclaim it.
    OrfAtgToStop,
    /// The mature peptide, i.e. the ORF minus a cleaved signal sequence.
    OrfMaturePeptide,
    /// A publication states it. `boundary_evidence` carries the DOI.
    LiteratureDefined,
    /// Read off several independent INSDC depositors who agree.
    ConsensusOfInsdc,
    /// A designed sequence: the boundary is the design.
    DesignedSequence,
}

impl BoundaryRule {
    pub fn as_str(&self) -> &'static str {
        match self {
            BoundaryRule::OrfAtgToStop => "orf_atg_to_stop",
            BoundaryRule::OrfMaturePeptide => "orf_mature_peptide",
            BoundaryRule::LiteratureDefined => "literature_defined",
            BoundaryRule::ConsensusOfInsdc => "consensus_of_insdc",
            BoundaryRule::DesignedSequence => "designed_sequence",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "orf_atg_to_stop" => Some(BoundaryRule::OrfAtgToStop),
            "orf_mature_peptide" => Some(BoundaryRule::OrfMaturePeptide),
            "literature_defined" => Some(BoundaryRule::LiteratureDefined),
            "consensus_of_insdc" => Some(BoundaryRule::ConsensusOfInsdc),
            "designed_sequence" => Some(BoundaryRule::DesignedSequence),
            _ => None,
        }
    }

    /// Is this boundary a computation we can show the arithmetic for, rather
    /// than a judgement someone made?
    ///
    /// The distinction is the project's strongest legal position: a derived
    /// boundary was not copied from anyone, and the derivation is publishable.
    pub fn is_derived(&self) -> bool {
        matches!(
            self,
            BoundaryRule::OrfAtgToStop | BoundaryRule::OrfMaturePeptide
        )
    }
}

/// Where one *field* of one record came from.
///
/// Rows live in a separate table keyed by `(record_id, field)`, so a licence
/// challenge is answered field by field and a single tainted field can be
/// dropped without rebuilding anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSource {
    pub record_id: String,
    /// The [`Record`] field this describes: `reference_nt`, `name`, …
    pub field: String,
    /// One of the values [`Db::audit`] clears: `polylinker` for our own work,
    /// `amrfinderplus`, `ena`, `genbank`, `uniprot`, `rfam`, `wwpdb`, or
    /// `insdc-ft`.
    ///
    /// This list used to read `uniprot, ena, ncbi-nuccore, rfam,
    /// amrfinderplus, literature, or polylinker` and was **already false**:
    /// `audit()` has no arm for `ncbi-nuccore` or `literature`, so a row naming
    /// either was rejected as not cleared while this line said it was expected.
    /// The authority is `audit()` and `features/SOURCING.md` §1, not this
    /// sentence; it is now written from them rather than beside them.
    pub source_db: String,
    /// Precise enough to re-fetch, including version and coordinates.
    pub source_accession: String,
    /// SPDX where one applies, else a short term: `CC-BY-4.0`, `CC0-1.0`,
    /// `INSDC-free`, `own-work`.
    pub licence: String,
    pub url: String,
    /// ISO 8601 date retrieved.
    pub retrieved: String,
    /// Of the archived copy in `legal/`, so the evidence is itself evidenced.
    pub sha256: String,
}

/// One row of the database.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Ours: `PLF:0001`. Deliberately **not** SnapGene's `CmR_(2)` / `KanR_(3)`
    /// convention — §8.3 rule 2 identifies that suffix pattern as a copying
    /// fingerprint, and 21.5% of their rows carry it.
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub class: Class,
    /// INSDC feature key to emit: `CDS`, `promoter`, `rep_origin`.
    ///
    /// Not a Sequence Ontology term: SO's own `LICENSE` (CC BY 4.0) and its
    /// `README` (CC BY-SA 4.0) contradict each other, and share-alike would be
    /// incompatible with a CC BY 4.0 release. Held until upstream resolves it.
    pub genbank_key: String,
    /// Nucleotide reference. May be empty **only** on a [`Class::SyntheticPart`]
    /// row that carries `reference_aa` instead.
    ///
    /// This line used to read "Present for every class", which is what made a
    /// peptide part inexpressible: a designed tag has no single nucleotide
    /// sequence to be present, only an arbitrary choice among its synonymous
    /// encodings. The invariant is now *at least one reference*, not *a
    /// nucleotide reference* — see the pair-rule in [`Db::parse`].
    pub reference_nt: Vec<u8>,
    /// Protein reference — [`Class::Cds`] and [`Class::SyntheticPart`] only.
    ///
    /// On a CDS this is what makes a codon-optimised marker findable at all: a
    /// humanised GFP and EGFP are identical proteins and can be ~70% identical
    /// in nucleotides, far below any threshold a nucleotide matcher would
    /// accept.
    ///
    /// On a synthetic part it is usually the *whole* record, because a tag is a
    /// peptide and its nucleotides are whichever codons the vector's designer
    /// happened to pick (`features/SOURCING.md` §3). The four remaining classes
    /// still may not carry one: a promoter has no protein, and putting one in
    /// the translated index would match noise.
    pub reference_aa: Option<Vec<u8>>,
    pub boundary_rule: BoundaryRule,
    /// `accession.version:start-end:strand`, or a DOI plus table reference.
    pub boundary_evidence: String,
    /// Written by us, from the primary source. §8.3 rule 1: never SnapGene's.
    pub description: String,
    pub review_status: ReviewStatus,
    pub curator: String,
    pub date_added: String,
    /// Set when a reference sequence may be encumbered by a patent independent
    /// of copyright — engineered fluorescent proteins especially. CC BY 4.0
    /// grants no patent rights and says so, so this cannot be waved away.
    pub patent_flag: bool,
    pub notes: String,
}

impl Record {
    /// Length of the **nucleotide** reference, in bases.
    ///
    /// Named `len` for the usual Rust reasons and kept that way because it is
    /// published, but read the unit: a peptide-only row reports 0 here while
    /// carrying 38 residues. Callers measuring "how big is this feature" on a
    /// synthetic part want [`Record::units`].
    pub fn len(&self) -> usize {
        self.reference_nt.len()
    }
    /// True when there are no **nucleotides**, which since 2026-07-28 is not
    /// the same as "this record is empty" — see [`Record::is_peptide_only`].
    pub fn is_empty(&self) -> bool {
        self.reference_nt.is_empty()
    }
    /// Symbols of the record's own reference, in whichever alphabet it has.
    ///
    /// Bases for anything carrying nucleotides, residues for a peptide-only
    /// row. The one number that is never zero for a record the loader accepted.
    pub fn units(&self) -> usize {
        if self.reference_nt.is_empty() {
            self.reference_aa.as_ref().map_or(0, |p| p.len())
        } else {
            self.reference_nt.len()
        }
    }
    /// Can translated matching find this?
    pub fn has_protein(&self) -> bool {
        self.reference_aa.as_ref().is_some_and(|p| !p.is_empty())
    }
    /// A residue string and nothing else: the shape every designed peptide part
    /// has today.
    ///
    /// Defined once because several rules key on it and they must not drift
    /// apart — the loader's refusal to let such a row claim an ORF-derived
    /// boundary, [`Db::duplicates`]'s alphabet discriminant, and the builder's
    /// peptide length floor.
    ///
    /// **Not** what the annotator's two extra rules key on. That is
    /// [`Record::is_designed_peptide`], and the difference is the point.
    pub fn is_peptide_only(&self) -> bool {
        self.reference_nt.is_empty() && self.has_protein()
    }
    /// A [`Class::SyntheticPart`] carrying residues, whether or not it also
    /// carries bases.
    ///
    /// The predicate the annotator's two extra rules key on: a **translated**
    /// hit on such a row must match exactly and wholly, and must lie in frame
    /// inside an open reading frame of the query. Those two are the difference
    /// between "there is a FLAG tag here" being a claim and being noise.
    ///
    /// Deliberately wider than [`Record::is_peptide_only`]. The relaxed schema
    /// permits a synthetic part to carry a nucleotide reference *and* a
    /// peptide; no shipped row does, and keying the rules on the absence of
    /// nucleotides would mean that adding a peptide to one of the eight
    /// parented tags — HA, Myc, V5, Protein C, P2A, T2A, E2A, F2A — silently
    /// opened an ungated six-frame route for an eight-residue epitope. A hole
    /// that opens because of the shape of a row rather than because anyone
    /// decided anything is exactly what [`Db::audit`]'s own comment means by
    /// "discipline is not a control".
    ///
    /// It says nothing about the nucleotide route, which is untouched: a
    /// synthetic part carrying real codons keeps finding them by tier 1, and
    /// 44 suites of existing behaviour depend on that.
    pub fn is_designed_peptide(&self) -> bool {
        self.class == Class::SyntheticPart && self.has_protein()
    }
}

/// A loaded database: the records, and the provenance of their fields.
#[derive(Debug, Clone, Default)]
pub struct Db {
    pub records: Vec<Record>,
    pub provenance: Vec<FieldSource>,
    /// Release this came from, e.g. `2026.10`. Every annotation Polylinker
    /// emits stamps it, so a map can be traced to the library that produced it.
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    pub file: &'static str,
    pub line: usize,
    pub problem: String,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.problem)
    }
}

/// Columns of `features.tsv`, in order.
///
/// TSV, and normalised across two files, on purpose. This is a *curated*
/// database whose main interaction is community pull requests, and one line per
/// feature makes a diff reviewable by a biologist. Reviewability is what keeps a
/// curated database alive — pLannotate's rotted partly because nobody could see
/// what changed between releases.
pub const FEATURE_COLUMNS: &[&str] = &[
    "id",
    "name",
    "aliases",
    "class",
    "genbank_key",
    "reference_nt",
    "reference_aa",
    "boundary_rule",
    "boundary_evidence",
    "description",
    "review_status",
    "curator",
    "date_added",
    "patent_flag",
    "notes",
];

/// Columns of `provenance.tsv`, in order.
pub const PROVENANCE_COLUMNS: &[&str] = &[
    "record_id",
    "field",
    "source_db",
    "source_accession",
    "licence",
    "url",
    "retrieved",
    "sha256",
];

/// Columns of `features/SIGNOFF.tsv`, in order.
///
/// A separate committed file rather than more columns on `features.tsv`, for
/// the reason [`FEATURE_COLUMNS`] gives about diffs: a sign-off is the single
/// most important thing a reviewer must be able to read in a pull request, and
/// `signed_date` has no column in `features.tsv` to live in — `date_added` is
/// the build clock and must stay that.
pub const SIGNOFF_COLUMNS: &[&str] = &[
    "record_id",
    "review_status",
    "curator",
    "signed_date",
    "content_sha256",
    "note",
];

/// The columns a curator's signature covers, in digest order.
///
/// Written out rather than computed as `FEATURE_COLUMNS` minus the bookkeeping
/// set, even though it happens to equal that today. Coupling the digest to a
/// mutable set means a future edit to that set either silently invalidates
/// every signature in the repository or silently stops covering a column, and
/// neither failure announces itself.
///
/// The four that are absent, each for its own reason:
///
/// - `id` — the key the signature is *on*, not content it covers.
/// - `review_status` and `curator` — what the signature *sets*. Including them
///   would make the digest depend on its own outcome.
/// - `date_added` — the build clock. `build.py` stamps it on every row on every
///   run, so a whole-row digest would invalidate every sign-off in the
///   repository on every build. This exclusion is what makes the scheme
///   possible at all.
///
/// `description` and `notes` are deliberately **in**. What a curator signs is
/// the claim made to a user, and [`ReviewStatus::Reviewed`] is defined as
/// having "wrote the description from the primary source"; a signature that
/// survived arbitrary rewriting of the prose would look like an approval of
/// text nobody has read.
pub const SIGNED_COLUMNS: &[&str] = &[
    "name",
    "aliases",
    "class",
    "genbank_key",
    "reference_nt",
    "reference_aa",
    "boundary_rule",
    "boundary_evidence",
    "description",
    "patent_flag",
    "notes",
];

/// One line of `features/SIGNOFF.tsv`: a human's approval of one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signoff {
    pub record_id: String,
    pub review_status: ReviewStatus,
    pub curator: String,
    /// The date the human asserted, which is deliberately not the build clock.
    pub signed_date: String,
    /// [`Db::content_digest`] of the record as it stood when it was signed.
    pub content_sha256: String,
    /// What was actually checked, in the curator's own words.
    pub note: String,
}

/// Reverse [`escape`], in one left-to-right pass.
///
/// The chained-`replace` version this replaces did not round-trip. Escaping
/// `C:\temp\thing` gives `C:\\temp\\thing`; unescaping then ran
/// `replace("\\t", "\t")` **first**, which saw the `\t` formed by the second
/// escape backslash and the following `t`, and produced `C:\<TAB>emp\<TAB>hing`.
/// Three of eight probe strings came back wrong, and a `/note` quoting a
/// Windows path is all it takes to hit it. Consuming the escape character as
/// you go cannot make that mistake.
///
/// `\r` is handled too. `str::lines()` strips a trailing `\r`, so a cell ending
/// in one silently lost it — and GenBank written on Windows is full of them.
///
/// Deliberately identical to `pl_index::codec::{escape, unescape}`: the two
/// tables are read by the same eyes, and a second dialect would be a second
/// bug.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            // An unknown escape is kept verbatim: losing a byte is worse than
            // keeping one we did not write.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// Parse a boolean cell, refusing anything not on the list.
///
/// `patent_flag` used to be `matches!(cell, "1" | "true" | "yes")`, which is a
/// case-*sensitive* three-way match with no rejection path: a hand-authored row
/// saying `TRUE`, `Yes`, `Y` or `T` loaded clean with the flag cleared, and
/// `to_tsv` then wrote `0`, so one round trip through the tool erased it
/// permanently. That is the one field where a silent clear is least
/// affordable — `Record::patent_flag`'s own doc says CC BY 4.0 grants no patent
/// rights and says so, so this cannot be waved away — and it was the only
/// enumerated column in the row with no `LoadError` path, unlike `class`,
/// `boundary_rule` and `review_status`, which all lowercase and all reject.
///
/// Empty is `false` and not an error: an unset optional column is a stated
/// default, not a misspelling. Every other spelling is returned as `None` so
/// the caller can refuse the row by name rather than guess at it.
fn parse_flag(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "" | "0" | "false" | "no" | "n" | "f" => Some(false),
        "1" | "true" | "yes" | "y" | "t" => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod escaping_tests {
    use super::{escape, unescape};

    #[test]
    fn a_windows_path_in_a_note_survives_the_round_trip() {
        // The exact case the previous codec destroyed.
        for s in [
            "C:\\temp\\thing",
            "a\\tb",
            "\\n",
            "\\r",
            "\\\\",
            "\\",
            "ends with a backslash\\",
            "a\tb",
            "a\nb",
            "a\rb",
            "a\r\nb",
            "   leading and trailing   ",
            "",
            "plain",
        ] {
            assert_eq!(unescape(&escape(s)), s, "{s:?}");
        }
    }

    #[test]
    fn an_escaped_cell_never_contains_a_raw_separator() {
        // The reason escaping exists at all: a tab or newline inside a cell
        // would silently become a new column or a new row.
        for s in ["a\tb", "a\nb", "a\rb", "\t\n\r"] {
            let e = escape(s);
            assert!(!e.contains('\t'), "{e:?}");
            assert!(!e.contains('\n'), "{e:?}");
            assert!(!e.contains('\r'), "{e:?}");
        }
    }
}

/// Split a TSV body into data rows, checking the header.
fn rows<'a>(
    text: &'a str,
    columns: &[&str],
    file: &'static str,
    errors: &mut Vec<LoadError>,
    version: &mut String,
) -> Vec<(usize, Vec<&'a str>)> {
    let mut out = Vec::new();
    let mut header_seen = false;
    for (n, raw) in text.lines().enumerate() {
        let line = n + 1;
        if let Some(rest) = raw.strip_prefix("#!version") {
            *version = rest.trim().to_string();
            continue;
        }
        if raw.trim().is_empty() || raw.starts_with('#') {
            continue;
        }
        let cells: Vec<&str> = raw.split('\t').collect();
        if !header_seen {
            header_seen = true;
            if cells != columns {
                errors.push(LoadError {
                    file,
                    line,
                    problem: format!(
                        "header does not match the schema ({} columns expected)",
                        columns.len()
                    ),
                });
                // Discard the whole file, not just this line: with the header
                // wrong the column positions are unknown, so every data row below
                // would be read against the wrong columns. `parse_signoff`'s
                // documented "a bad header discards the file" downgrade — nothing
                // signed rather than something mis-parsed — depends on this.
                return Vec::new();
            }
            continue;
        }
        if cells.len() != columns.len() {
            errors.push(LoadError {
                file,
                line,
                problem: format!("{} columns, expected {}", cells.len(), columns.len()),
            });
            continue;
        }
        out.push((line, cells));
    }
    if !header_seen {
        errors.push(LoadError {
            file,
            line: 0,
            problem: "no header row".into(),
        });
    }
    out
}

/// Read `features/SIGNOFF.tsv` into `record_id -> Signoff`.
///
/// A blank file is not an error and must not be: "nothing is signed" is a
/// legitimate state, and the one every failure path below resolves to. It is
/// **not** this repository's state — `features/SIGNOFF.tsv` carries a signature
/// for every row `features.tsv` ships — and this sentence used to claim it was,
/// which invited a reader to treat the whole mechanism as inert. A file that is
/// present but malformed *is* reported, and yields nothing for the lines it
/// cannot read — a bad header discards the file, any other unreadable line
/// discards that line, and a record signed twice loses **both** lines. Every
/// path is a downgrade; none is a half-applied grant.
fn parse_signoff(text: &str, errors: &mut Vec<LoadError>) -> BTreeMap<String, Signoff> {
    let mut out = BTreeMap::new();
    // Ids seen twice. A duplicate must not be re-admitted by a third line
    // either, so the id is remembered rather than merely removed.
    let mut duplicated: BTreeSet<String> = BTreeSet::new();
    if text.trim().is_empty() {
        return out;
    }
    let mut version = String::new();
    for (line, c) in rows(text, SIGNOFF_COLUMNS, "SIGNOFF.tsv", errors, &mut version) {
        let get = |i: usize| c[i].trim().to_string();
        let mut bad = |problem: String| {
            errors.push(LoadError {
                file: "SIGNOFF.tsv",
                line,
                problem,
            })
        };
        let Some(status) = ReviewStatus::parse(&get(1)) else {
            bad(format!("unknown review_status {:?}", get(1)));
            continue;
        };
        // `proposed` is what a row is when nobody has signed it, so writing it
        // here is a contradiction rather than a no-op: it would name a curator
        // for an unreviewed row.
        if status == ReviewStatus::Proposed {
            bad("review_status 'proposed' is the absence of a sign-off; delete the line instead of signing it 'proposed'".into());
            continue;
        }
        if get(0).is_empty() || get(2).is_empty() {
            bad("record_id and curator are required: a signature with no name is not one".into());
            continue;
        }
        let digest = get(4).to_ascii_lowercase();
        if digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
            bad(format!(
                "content_sha256 {digest:?} is not 64 hex characters; run the build and paste \
                 the digest it prints"
            ));
            continue;
        }
        let s = Signoff {
            record_id: get(0),
            review_status: status,
            curator: get(2),
            signed_date: get(3),
            content_sha256: digest,
            note: unescape(&get(5)),
        };
        // Dropped, not last-wins. This used to keep whichever line came last
        // while reporting the problem, so a file that says two things still
        // granted trust — the one place where the code contradicted the
        // invariant three doc comments state, with the error text saying so
        // openly. "It says two things" resolves to "no signature", like every
        // other unreadable state.
        if duplicated.contains(&s.record_id) || out.remove(&s.record_id).is_some() {
            let id = s.record_id.clone();
            duplicated.insert(id.clone());
            bad(format!(
                "{id} is signed twice, so NEITHER line is applied and the row stays \
                 'proposed'; this file holds one current signature per record"
            ));
            continue;
        }
        out.insert(s.record_id.clone(), s);
    }
    out
}

/// The shipped tables, compiled in.
///
/// `include_str!` rather than a file read: this crate does no I/O, and an
/// offline desktop tool should not depend on finding a data directory next to
/// its own executable.
const BUILTIN_FEATURES: &str = include_str!("../../../features/features.tsv");
const BUILTIN_PROVENANCE: &str = include_str!("../../../features/provenance.tsv");
const BUILTIN_SIGNOFF: &str = include_str!("../../../features/SIGNOFF.tsv");

impl Db {
    /// The database compiled into this binary.
    ///
    /// A row ships at [`ReviewStatus::Reviewed`] or above only if
    /// `features/SIGNOFF.tsv` names it, with a curator that matches and a
    /// [`Db::content_digest`] that still matches; everything else is
    /// [`ReviewStatus::Proposed`], and [`Db::reviewed`] ships only the
    /// remainder.
    ///
    /// **How many rows that is, is a property of `features/SIGNOFF.tsv` and not
    /// of this function.** Ask [`Db::review_counts`]; do not believe a count
    /// written into a doc comment. This one used to carry one — "that file is
    /// empty of signatures as this is written, so `reviewed()` returns an empty
    /// database" — and it was false the moment it was written, having landed in
    /// the same commit that added the first 84 signatures. The cost of leaving
    /// it was not cosmetic: a reader auditing this crate's central trust claim
    /// would have concluded the tool asserts nothing, while by default it
    /// asserts every signed name onto a user's map.
    ///
    /// Writing `AmpR` onto somebody's plasmid map is an assertion, and the rule
    /// here is that the tool may propose and never assert. A caller that wants
    /// the proposed rows too has to ask for them by name, and owes the user
    /// that sentence.
    pub fn builtin() -> (Db, Vec<LoadError>) {
        Db::parse(BUILTIN_FEATURES, BUILTIN_PROVENANCE, BUILTIN_SIGNOFF)
    }

    /// How many records sit at each review status.
    ///
    /// What a caller needs in order to explain an empty result honestly rather
    /// than printing "no features found" over a database nobody has approved.
    pub fn review_counts(&self) -> BTreeMap<ReviewStatus, usize> {
        let mut m = BTreeMap::new();
        for r in &self.records {
            *m.entry(r.review_status).or_insert(0) += 1;
        }
        m
    }

    /// Parse the three tables.
    ///
    /// Every problem is reported rather than failing on the first, because a
    /// curator fixing a contributed file wants the whole list.
    ///
    /// `signoff` is `features/SIGNOFF.tsv`. It can only ever *remove* trust: a
    /// missing, blank, malformed or stale sign-off table leaves every row
    /// `proposed`, which is the behaviour this project already ships and
    /// already documents everywhere. Pass `""` for "nothing is signed".
    pub fn parse(features: &str, provenance: &str, signoff: &str) -> (Db, Vec<LoadError>) {
        let mut db = Db::default();
        let mut errors = Vec::new();

        for (line, c) in rows(
            features,
            FEATURE_COLUMNS,
            "features.tsv",
            &mut errors,
            &mut db.version,
        ) {
            let get = |i: usize| c[i].trim().to_string();
            let mut bad = |problem: String| {
                errors.push(LoadError {
                    file: "features.tsv",
                    line,
                    problem,
                })
            };

            let (Some(class), Some(rule), Some(review)) = (
                Class::parse(&get(3)),
                BoundaryRule::parse(&get(7)),
                ReviewStatus::parse(&get(10)),
            ) else {
                bad(format!(
                    "unknown class/boundary_rule/review_status: {:?} {:?} {:?}",
                    get(3),
                    get(7),
                    get(10)
                ));
                continue;
            };

            // Both references are read before either is judged. The order used
            // to be nt-then-refuse-then-aa, so the emptiness test could not see
            // whether a peptide was present and a peptide-only row was rejected
            // before its peptide was ever parsed.
            let nt = get(5).to_ascii_uppercase().into_bytes();
            let aa = {
                let s = get(6).to_ascii_uppercase();
                if s.is_empty() {
                    None
                } else {
                    Some(s.into_bytes())
                }
            };

            // The invariant is *at least one reference*, not *a nucleotide
            // reference*. This is the clause that keeps "reference_nt may be
            // empty" from becoming "anything goes".
            if nt.is_empty() && aa.is_none() {
                bad("row carries neither a nucleotide nor a protein reference; nothing in either index could ever match it".into());
                continue;
            }
            if let Some(b) = nt.iter().find(|c| !b"ACGTRYSWKMBDHVN".contains(c)) {
                bad(format!("{:?} is not a nucleotide code", *b as char));
                continue;
            }
            // `reference_aa` had no alphabet check at all while it was
            // decoration on a row that also carried nucleotides. On a
            // peptide-only row it is the entire record, so an unchecked cell is
            // the whole sequence unchecked. `*` is refused by name: a stop codon
            // is meaningless in a tag, and unlike `X` — which `index::seedable`
            // excludes, so a window carrying one is simply not indexed and the
            // record is routed to the annotator's exact scan instead, matching
            // an `X` the query's own translation produced — a `*` would be
            // indexed and could only ever match a query frame at a position the
            // frame renders as a terminator.
            //
            // That sentence used to end "so it degrades honestly into
            // 'unseedable'", which stopped being true on 2026-07-28: an
            // all-`X` peptide is now scanned rather than written off, and
            // `Annotator::unseedable` no longer consults `protein.short()` at
            // all. See `a_peptide_that_indexes_no_word_is_scanned_rather_than_
            // written_off`.
            if let Some(p) = aa.as_ref() {
                if let Some(b) = p.iter().find(|c| !b"ACDEFGHIKLMNPQRSTVWYX".contains(c)) {
                    bad(format!("{:?} is not an amino-acid code", *b as char));
                    continue;
                }
            }
            // A protein reference on a promoter, an origin, a repeat or a misc
            // feature is a category error, and would put a promoter into the
            // translated index. That reasoning is unchanged; what changed on
            // 2026-07-28 is that it never applied to a tag, which is nothing but
            // protein. Hence a list of two rather than of one — and still not of
            // six.
            if aa.is_some() && !matches!(class, Class::Cds | Class::SyntheticPart) {
                bad(format!(
                    "class {} carries a protein reference; only cds and synthetic_part may",
                    class.as_str()
                ));
                continue;
            }
            // The mirror refusal, and not belt-and-braces: without it the
            // pair-rule above admits a protein-only CDS, whose `orf_atg_to_stop`
            // boundary would be a derived-boundary claim over no bases at all
            // and `BoundaryRule::is_derived` would return true for it.
            if class == Class::Cds && nt.is_empty() {
                bad("class cds requires reference_nt: its boundary is a claim about a reading frame, and there is no frame without bases".into());
                continue;
            }
            // ...and the same rule stated over the boundary instead of the
            // class, which is the form that closes the remaining route: a
            // synthetic part with no nucleotides may not claim its boundary was
            // computed from a frame it does not carry. Every peptide row shipped
            // today is `designed_sequence` or `literature_defined`, so this
            // rejects nothing that exists; it stops `is_derived()` becoming a lie
            // later.
            if nt.is_empty() && rule.is_derived() {
                bad(format!(
                    "boundary_rule {} is a claim about a reading frame; this row carries no nucleotides to read",
                    rule.as_str()
                ));
                continue;
            }
            if get(0).is_empty() || get(1).is_empty() {
                bad("id and name are required".into());
                continue;
            }
            // The mechanical half of "AI may propose, never assert".
            if review > ReviewStatus::Proposed && get(11).is_empty() {
                bad(format!(
                    "review_status is {} but no curator is named; only 'proposed' rows may be uncurated",
                    review.as_str()
                ));
                continue;
            }
            if get(8).is_empty() {
                bad("boundary_evidence is required: a boundary with no evidence is the thing this database exists to replace".into());
                continue;
            }
            // Refused rather than defaulted. A cell this parser does not
            // recognise is a curator saying something it failed to hear, and
            // reading it as `false` clears a patent warning without a word.
            let Some(patent_flag) = parse_flag(&get(13)) else {
                bad(format!(
                    "patent_flag {:?} is not a boolean; write 0 or 1",
                    get(13)
                ));
                continue;
            };

            let genbank_key = {
                let g = get(4);
                if g.is_empty() {
                    "misc_feature".to_string()
                } else {
                    g
                }
            };

            // THE TWO IMPLEMENTATIONS OF `content_digest` MUST HASH THE SAME
            // BYTES, and `get` is where they can quietly stop doing so. `get`
            // trims every cell; `Class::parse` and `BoundaryRule::parse`
            // lower-case theirs; an empty `genbank_key` becomes `misc_feature`;
            // the two reference columns are upper-cased. `build.py`'s
            // `content_digest` hashes the cell as it sits on disk, upper-casing
            // the references and canonicalising `aliases` and `patent_flag` and
            // nothing else. So `description = "A description "` — one trailing
            // space — hashes one way there and another here, `check_signoff.py`
            // certifies the row, and the shipped binary then reports "the row
            // has changed since it was signed" and clears the curator's name:
            // an error that blames a curator's data for a disagreement between
            // two hashers, on a row nobody edited. Measured on the real tables:
            // one trailing space on PLF:0001's description gives
            // `check_signoff.py` zero violations and `Db::parse` a lapse.
            //
            // Named at load, because once `get` has run the digest cannot tell
            // the two apart and every downstream message is about the wrong
            // thing. `build.py`'s `coerce_row` already states this as the
            // design — "the bytes written to features.tsv are already what
            // `Db::parse` will store" — and implemented it for `aliases` alone.
            //
            // REPORTED, NOT DROPPED, unlike every other refusal in this loop: a
            // stray space is a bookkeeping problem and the row is still a real
            // feature, so discarding it would lose data over one. Same direction
            // of failure `apply_signoff` picks when it downgrades a row instead
            // of deleting it. `Db::builtin`'s own test asserts no errors, so the
            // shipped tables cannot reach this state regardless.
            //
            // `aliases` and `patent_flag` are absent on purpose: build.py
            // canonicalises both itself, which is why the pin fixture in
            // tests/schema_pin.rs can write ` a | b |` and `TRUE`.
            //
            // WHICH IS ALSO WHY THE TWO REFERENCE COLUMNS ARE COMPARED AGAINST
            // THE TRIMMED CELL AND NOT THE STORED, UPPER-CASED ONE. build.py's
            // `content_digest` writes `r.reference_nt.upper()` and
            // `r.reference_aa.upper()`, so case is canonicalised on BOTH sides
            // and a lower-case cell is not a divergence at all. Demanding the
            // stored spelling here reported one anyway — a curator with a
            // soft-masked sequence would have been sent looking for a signature
            // mismatch that does not exist, on the one column where lower case
            // is ordinary (`align::same` matches case-insensitively for exactly
            // that reason). Whitespace is the only thing that actually parts the
            // two hashers for these two, so whitespace is all this asks about.
            for (i, canonical) in [
                (1usize, get(1)),
                (3, class.as_str().to_string()),
                (4, genbank_key.clone()),
                (5, get(5)),
                (6, get(6)),
                (7, rule.as_str().to_string()),
                (8, get(8)),
                (9, get(9)),
                (14, get(14)),
            ] {
                if c[i] != canonical {
                    bad(format!(
                        "{} reads {:?} on disk but is stored as {:?}; \
                         features/build/build.py hashes the cell and this loader hashes \
                         what it stored, so the two content digests differ and a \
                         signature taken over one lapses against the other. Write the \
                         stored spelling into the cell.",
                        FEATURE_COLUMNS[i], c[i], canonical
                    ));
                }
            }

            db.records.push(Record {
                id: get(0),
                name: get(1),
                aliases: get(2)
                    .split('|')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                class,
                genbank_key,
                reference_nt: nt,
                reference_aa: aa,
                boundary_rule: rule,
                boundary_evidence: get(8),
                description: unescape(&get(9)),
                review_status: review,
                curator: get(11),
                date_added: get(12),
                patent_flag,
                notes: unescape(&get(14)),
            });
        }

        let mut v2 = String::new();
        for (line, c) in rows(
            provenance,
            PROVENANCE_COLUMNS,
            "provenance.tsv",
            &mut errors,
            &mut v2,
        ) {
            let get = |i: usize| c[i].trim().to_string();
            if get(0).is_empty() || get(1).is_empty() || get(4).is_empty() {
                errors.push(LoadError {
                    file: "provenance.tsv",
                    line,
                    problem: "record_id, field and licence are required".into(),
                });
                continue;
            }
            db.provenance.push(FieldSource {
                record_id: get(0),
                field: get(1),
                source_db: get(2),
                source_accession: get(3),
                licence: get(4),
                url: get(5),
                retrieved: get(6),
                sha256: get(7),
            });
        }

        db.apply_signoff(signoff, &mut errors);
        errors.extend(db.audit());
        (db, errors)
    }

    /// The digest a curator's signature is taken over: the record's semantic
    /// content plus the provenance that says where it came from.
    ///
    /// Length-framed rather than concatenated, and the column name is inside
    /// the hash. The framing means no cell's content can impersonate a
    /// delimiter, and — the reason it is there rather than plain joining — it
    /// removes any need for Rust and Python to agree about escaping. Both sides
    /// hash the **unescaped, canonical** value, which Python holds natively and
    /// Rust holds after parsing. Hashing the on-disk bytes would have been the
    /// fragile choice: `escape(unescape(x)) == x` does not hold for an
    /// unrecognised escape such as `\q`, which [`unescape`] keeps verbatim and
    /// [`escape`] then doubles.
    ///
    /// The column name is inside the hash so that adding or reordering a
    /// [`SIGNED_COLUMNS`] entry changes every digest rather than silently
    /// producing the same bytes under a different schema.
    ///
    /// Provenance enters as `(field, source_db, source_accession, licence)`,
    /// sorted. [`ReviewStatus::Reviewed`] means "checked the sequence *against
    /// the cited accession*", so the accession is part of what was signed.
    /// `url` is excluded because it is derived from the accession, `retrieved`
    /// because it churns on every `--refresh`, and `sha256` because upstream
    /// reissuing identical content under a stable accession must not invalidate
    /// a human's reading. That last exclusion is a real gap and is stated
    /// rather than hidden — it is closed from the other side, because bytes
    /// that changed the *sequence* change `reference_nt`, which is in the
    /// digest.
    pub fn content_digest(&self, r: &Record) -> String {
        let mut quads: Vec<(&str, &str, &str, &str)> = self
            .provenance
            .iter()
            .filter(|p| p.record_id == r.id)
            .map(|p| {
                (
                    p.field.as_str(),
                    p.source_db.as_str(),
                    p.source_accession.as_str(),
                    p.licence.as_str(),
                )
            })
            .collect();
        quads.sort_unstable();

        let mut buf = String::from("polylinker-signoff-v1\n");
        let mut frame = |name: &str, value: &str| {
            buf.push_str(name);
            buf.push('\u{1f}');
            buf.push_str(&value.len().to_string());
            buf.push('\u{1f}');
            buf.push_str(value);
            buf.push('\u{1e}');
        };
        let nt = String::from_utf8_lossy(&r.reference_nt);
        let aa = r
            .reference_aa
            .as_ref()
            .map(|p| String::from_utf8_lossy(p).to_string())
            .unwrap_or_default();
        let aliases = r.aliases.join("|");
        for col in SIGNED_COLUMNS {
            // Every spelling that means the same claim must produce the same
            // digest, or re-writing `1` as `TRUE` breaks a signature without
            // changing anything a curator read.
            let v: &str = match *col {
                "name" => &r.name,
                "aliases" => &aliases,
                "class" => r.class.as_str(),
                "genbank_key" => &r.genbank_key,
                "reference_nt" => &nt,
                "reference_aa" => &aa,
                "boundary_rule" => r.boundary_rule.as_str(),
                "boundary_evidence" => &r.boundary_evidence,
                "description" => &r.description,
                "patent_flag" => {
                    if r.patent_flag {
                        "1"
                    } else {
                        "0"
                    }
                }
                "notes" => &r.notes,
                other => unreachable!(
                    "SIGNED_COLUMNS names {other}, which content_digest cannot canonicalise"
                ),
            };
            frame(col, v);
        }
        for (f, db, acc, lic) in quads {
            frame("provenance", &[f, db, acc, lic].join("\u{1f}"));
        }
        pl_core::sha256::sha256_hex(buf.as_bytes())
    }

    /// Apply `features/SIGNOFF.tsv` to the parsed records.
    ///
    /// **The governing invariant: a missing, stale, malformed or unreadable
    /// sign-off can only ever REMOVE trust, never add it.** Every failure path
    /// resolves to [`ReviewStatus::Proposed`], which is why the degenerate case
    /// is safe without a judgement call — it is the behaviour this project
    /// already ships.
    ///
    /// This is what makes the compiled-in binary refuse a hand-edited
    /// `features.tsv`, which no build-time checker can do for an executable
    /// somebody already downloaded. A downgraded row keeps its sequence and its
    /// provenance and loses only its signature, which is the correct direction
    /// of failure: dropping the row would lose real data over a bookkeeping
    /// problem, and accepting it is the forgery.
    ///
    /// The digest cannot *authenticate* anything — the builder computes it from
    /// the same content and could write a valid-looking line. Its job is
    /// stale-approval detection. Authentication is the git history plus the CI
    /// step that proves the build never writes this file; see
    /// `features/SIGNOFF.tsv`'s own preamble, which says so out loud rather
    /// than implying more.
    fn apply_signoff(&mut self, text: &str, errors: &mut Vec<LoadError>) {
        let signed = parse_signoff(text, errors);

        let ids: std::collections::BTreeSet<&str> =
            self.records.iter().map(|r| r.id.as_str()).collect();
        for (id, s) in &signed {
            if !ids.contains(id.as_str()) {
                // The mirror of `audit()`'s orphan-provenance rule, and for the
                // same reason: a signature pointing at nothing is a silent lie
                // about how much of the table a human has read.
                errors.push(LoadError {
                    file: "SIGNOFF.tsv",
                    line: 0,
                    problem: format!(
                        "signs unknown record {id} (curator {:?}); the row it approves is not in features.tsv",
                        s.curator
                    ),
                });
            }
        }

        // Digests are computed against the whole database, so they are taken
        // before any record is mutated.
        let digests: Vec<String> = self
            .records
            .iter()
            .map(|r| self.content_digest(r))
            .collect();

        for (i, r) in self.records.iter_mut().enumerate() {
            let claimed = r.review_status;
            let Some(s) = signed.get(&r.id) else {
                if claimed > ReviewStatus::Proposed {
                    errors.push(LoadError {
                        file: "features.tsv",
                        line: 0,
                        problem: format!(
                            "{} claims review_status {} but SIGNOFF.tsv does not name it; \
                             downgraded to proposed",
                            r.id,
                            claimed.as_str()
                        ),
                    });
                    r.review_status = ReviewStatus::Proposed;
                    r.curator.clear();
                }
                continue;
            };
            // A signed row that the table still carries as `proposed` means the
            // features.tsv in hand was written by a build that did not see this
            // signature — or saw it and refused it. Either way the two files
            // disagree and somebody must be told which one is stale.
            if claimed == ReviewStatus::Proposed {
                errors.push(LoadError {
                    file: "SIGNOFF.tsv",
                    line: 0,
                    problem: format!(
                        "{} is signed {} by {:?} but features.tsv carries it as proposed; \
                         rebuild, or the signature has lapsed",
                        r.id,
                        s.review_status.as_str(),
                        s.curator
                    ),
                });
                continue;
            }
            let mut why: Option<String> = None;
            if s.review_status != claimed {
                why = Some(format!(
                    "features.tsv says {} but SIGNOFF.tsv says {}",
                    claimed.as_str(),
                    s.review_status.as_str()
                ));
            } else if s.curator != r.curator {
                why = Some(format!(
                    "features.tsv names curator {:?} but SIGNOFF.tsv names {:?}",
                    r.curator, s.curator
                ));
            } else if s.content_sha256 != digests[i] {
                // The make-or-break case. `reference_nt` is inside the digest,
                // so a sequence that moved under a published id lands here even
                // when the build was run with --allow-id-drift.
                why = Some(format!(
                    "the row has changed since it was signed on {}: recorded digest {}, \
                     recomputed {}",
                    s.signed_date, s.content_sha256, digests[i]
                ));
            }
            if let Some(why) = why {
                errors.push(LoadError {
                    file: "SIGNOFF.tsv",
                    line: 0,
                    problem: format!("{}: {why}; downgraded to proposed", r.id),
                });
                r.review_status = ReviewStatus::Proposed;
                // The name goes with the status. Leaving it would attribute a
                // row to a curator who is no longer standing behind it.
                r.curator.clear();
            }
        }
    }

    /// Cross-table checks: the ones a single-file schema could not express.
    pub fn audit(&self) -> Vec<LoadError> {
        let mut out = Vec::new();
        let ids: std::collections::BTreeSet<&str> =
            self.records.iter().map(|r| r.id.as_str()).collect();

        for p in &self.provenance {
            if !ids.contains(p.record_id.as_str()) {
                out.push(LoadError {
                    file: "provenance.tsv",
                    line: 0,
                    problem: format!("provenance for unknown record {}", p.record_id),
                });
            }
        }
        // A provenance row keyed on something that is not a column attributes
        // nothing, and does so silently. Forty shipped rows once keyed on
        // `citation` and `peptide_anchor`, neither of which is in the schema,
        // while the column the sourced text actually landed in was labelled
        // own-work. A misspelling would have behaved identically.
        for p in &self.provenance {
            if !FEATURE_COLUMNS.contains(&p.field.as_str()) {
                out.push(LoadError {
                    file: "provenance.tsv",
                    line: 0,
                    problem: format!(
                        "{}: provenance names field {:?}, which is not a column of features.tsv",
                        p.record_id, p.field
                    ),
                });
            }
        }

        // Source and licence, against features/SOURCING.md section 1. Enforced
        // here as well as in the builder because this table can be hand-edited,
        // and it was: two provenance rows citing Addgene and PlasMapper were
        // appended to a copy of the shipped file, and every check in the project
        // passed them, counted them, and reported the result green. The NO_GO
        // list was author discipline, and discipline is not a control.
        for p in &self.provenance {
            let ok = Self::provenance_cleared(&p.source_db, &p.licence);
            if !ok {
                out.push(LoadError {
                    file: "provenance.tsv",
                    line: 0,
                    problem: format!(
                        "{} field {}: source {:?} under licence {:?} is not cleared for use \
                         as data by features/SOURCING.md",
                        p.record_id, p.field, p.source_db, p.licence
                    ),
                });
            }
        }

        // The rule the whole schema exists for: a sequence with no stated
        // origin must never ship.
        //
        // Conditional on the row having nucleotides at all. A peptide-only
        // synthetic part has none, so demanding provenance for them would
        // demand a source for a field that is deliberately empty; its residues
        // are covered by the per-field loop below, under `reference_aa`.
        for r in &self.records {
            if r.reference_nt.is_empty() {
                continue;
            }
            let has = self
                .provenance
                .iter()
                .any(|p| p.record_id == r.id && p.field == "reference_nt");
            if !has {
                out.push(LoadError {
                    file: "features.tsv",
                    line: 0,
                    problem: format!("{}: no provenance for reference_nt", r.id),
                });
            }
        }

        // ...and the same rule for every other populated field, because
        // features/NOTICE promises "which source each individual field came
        // from and under what licence" for every field of every row, and
        // measured against that promise four populated columns had none at all
        // -- including `genbank_key`, the one column SOURCING.md Risk 4 flags as
        // legally unresolved.
        //
        // `reference_nt` is handled above so it is reported once, in the wording
        // it has always had. `id`, `review_status`, `curator` and `date_added`
        // are exempt by name: they are the build's own bookkeeping and the
        // sign-off protocol, not sourced content.
        for r in &self.records {
            let covered: std::collections::BTreeSet<&str> = self
                .provenance
                .iter()
                .filter(|p| p.record_id == r.id)
                .map(|p| p.field.as_str())
                .collect();
            let populated: [(&str, bool); 10] = [
                ("name", !r.name.is_empty()),
                ("aliases", !r.aliases.is_empty()),
                ("class", true),
                ("genbank_key", !r.genbank_key.is_empty()),
                ("reference_aa", r.reference_aa.is_some()),
                ("boundary_rule", true),
                ("boundary_evidence", !r.boundary_evidence.is_empty()),
                ("description", !r.description.is_empty()),
                ("patent_flag", true),
                ("notes", !r.notes.is_empty()),
            ];
            for (field, is_populated) in populated {
                if is_populated && !covered.contains(field) {
                    out.push(LoadError {
                        file: "features.tsv",
                        line: 0,
                        problem: format!("{}: no provenance for populated field {}", r.id, field),
                    });
                }
            }
        }
        out
    }

    /// Provenance rows for one record.
    pub fn provenance_of(&self, id: &str) -> Vec<&FieldSource> {
        self.provenance
            .iter()
            .filter(|p| p.record_id == id)
            .collect()
    }

    /// Every distinct licence in play, for the NOTICE file.
    pub fn licences(&self) -> BTreeMap<String, usize> {
        let mut m = BTreeMap::new();
        for p in &self.provenance {
            *m.entry(p.licence.clone()).or_insert(0) += 1;
        }
        m
    }

    /// Whether a provenance row's (source, licence) pair is cleared for use as
    /// data. The single source of truth for the taint gate — `audit` reports a
    /// violation of it and `reviewed` refuses to ship one — so the rule cannot
    /// drift between the two.
    ///
    /// The clearing document is `features/SOURCING.md` §1 for every arm **but
    /// one**. §1's table covers fourteen named sources and the INSDC feature
    /// table is not among them, so the `insdc-ft` arm below is governed by §5
    /// Risk 4 instead, which marks the question **[UNVERIFIED]** — whether the
    /// INSDC feature-table *specification* itself carries a licence. This doc
    /// used to cite §1 for the whole function, which sent anyone auditing the
    /// one arm with an unresolved licence to a table that says nothing about
    /// it. The decision is disclosed in `NOTICE`; `PROVENANCE.md`'s rule that
    /// "an unretrievable licence is a hold, never a permission" is why the
    /// licence string spells the hold out rather than reading as cleared.
    fn provenance_cleared(source_db: &str, licence: &str) -> bool {
        match source_db {
            "polylinker" => licence == "own-work",
            "amrfinderplus" | "ena" | "genbank" => licence == "INSDC-free",
            "uniprot" => licence == "CC-BY-4.0",
            "rfam" => licence == "CC0-1.0",
            // Deposited PDB archive data, served through the RCSB API. The wwPDB
            // usage policy puts "data files contained in the PDB archive" under
            // CC0 1.0, and `legal/wwpdb-usage-policies.html` holds that page under
            // a sha256. RCSB's *own* website content is separately CC BY 4.0,
            // which is why only the deposited one-letter sequence is read out of
            // it and never the annotation layer.
            "wwpdb" => licence == "CC0-1.0",
            "insdc-ft" => licence == "unresolved-see-SOURCING-Risk-4",
            _ => false,
        }
    }

    /// The subset a release may ship: every record a human has signed off *and*
    /// whose provenance clears the licence/taint gate. `review_status` alone is
    /// not enough — the taint audit only *reports* an uncleared source, it never
    /// downgrades the row — so without the second filter a signed-but-tainted
    /// record would ship, the exact "discipline is not a control" gap `audit`
    /// exists to close.
    pub fn reviewed(&self) -> Db {
        let tainted: std::collections::BTreeSet<&str> = self
            .provenance
            .iter()
            .filter(|p| !Self::provenance_cleared(&p.source_db, &p.licence))
            .map(|p| p.record_id.as_str())
            .collect();
        let records: Vec<Record> = self
            .records
            .iter()
            .filter(|r| {
                r.review_status >= ReviewStatus::Reviewed && !tainted.contains(r.id.as_str())
            })
            .cloned()
            .collect();
        let keep: std::collections::BTreeSet<&str> =
            records.iter().map(|r| r.id.as_str()).collect();
        Db {
            provenance: self
                .provenance
                .iter()
                .filter(|p| keep.contains(p.record_id.as_str()))
                .cloned()
                .collect(),
            records,
            version: self.version.clone(),
        }
    }

    /// The everyday plasmid parts this database has **no row of any kind** for,
    /// in the words a user would use.
    ///
    /// Today that is all three: `["promoter", "terminator", "origin of
    /// replication"]`. `features/README.md` says why — "Promoters, origins and
    /// terminators have no automatable source that gives a defensible boundary;
    /// depositors disagree with each other, with 21 distinct spellings for
    /// 'origin of replication'" — and that file is the right place for the
    /// reason. It is the wrong place for the FACT, because nobody reads a
    /// repository before opening a plasmid, and the inference this gap invites
    /// is specific and wrong: a user watching `AmpR` and `lacI` light up and
    /// no `ori` concludes their plasmid has no `ori`. The tool caused that by
    /// having just demonstrated that it knows what features are, so the tool
    /// owes them the sentence — in the application, and in the methods
    /// paragraph they would publish. Both are built from this.
    ///
    /// **Computed, never written down**, which is the whole point: the day a
    /// `promoter` row lands, every sentence built on this shortens by itself.
    /// Prose still apologising for a gap somebody filled two releases ago is a
    /// failure this repository has documented more than once.
    ///
    /// The probe is [`Record::genbank_key`] rather than the name or the class,
    /// because the GenBank feature key is a controlled vocabulary: `promoter`,
    /// `terminator` and `rep_origin` are the INSDC keys for exactly these three
    /// things, whatever a curator called the row. Matching on names would miss
    /// `pUC ori` and would count a CDS called "terminator protein".
    ///
    /// Empty when nothing common is missing, so a caller can say nothing rather
    /// than say something empty.
    pub fn absent_common_kinds(&self) -> Vec<&'static str> {
        // (INSDC feature key, what a user calls it)
        const PROBES: [(&str, &str); 3] = [
            ("promoter", "promoter"),
            ("terminator", "terminator"),
            ("rep_origin", "origin of replication"),
        ];
        PROBES
            .iter()
            .filter(|(key, _)| !self.records.iter().any(|r| r.genbank_key == *key))
            .map(|(_, word)| *word)
            .collect()
    }

    pub fn census(&self) -> BTreeMap<&'static str, usize> {
        let mut m = BTreeMap::new();
        for r in &self.records {
            *m.entry(r.review_status.as_str()).or_insert(0) += 1;
        }
        m
    }

    /// Colliding ids, and distinct records holding identical sequences.
    ///
    /// The sequence key carries an alphabet discriminant, and it is required
    /// rather than cosmetic. Keyed on `reference_nt` alone, every peptide-only
    /// row keys on `b""` and they all collide — the first build after the
    /// schema relaxation reported all fourteen new rows as "identical
    /// sequences" and failed `tests/corpus.rs`. Keyed on the sequence alone with
    /// no discriminant, `ATG` the codon and `ATG` the tripeptide Ala-Thr-Gly are
    /// the same key, so a collision could be reported across two alphabets.
    pub fn duplicates(&self) -> Vec<String> {
        let mut by_id: BTreeMap<&str, usize> = BTreeMap::new();
        let mut by_seq: BTreeMap<(bool, &[u8]), Vec<&str>> = BTreeMap::new();
        for r in &self.records {
            *by_id.entry(&r.id).or_insert(0) += 1;
            let key: (bool, &[u8]) = if r.is_peptide_only() {
                (true, r.reference_aa.as_deref().unwrap_or(b""))
            } else {
                (false, &r.reference_nt)
            };
            by_seq.entry(key).or_default().push(&r.id);
        }
        let mut out = Vec::new();
        for (id, n) in by_id {
            if n > 1 {
                out.push(format!("duplicate id {id} ({n}×)"));
            }
        }
        for (_, ids) in by_seq {
            if ids.len() > 1 {
                out.push(format!("identical sequences: {}", ids.join(", ")));
            }
        }
        out
    }

    pub fn to_tsv(&self) -> (String, String) {
        let mut f = String::new();
        if !self.version.is_empty() {
            f.push_str(&format!("#!version {}\n", self.version));
        }
        f.push_str(&FEATURE_COLUMNS.join("\t"));
        f.push('\n');
        for r in &self.records {
            f.push_str(
                &[
                    r.id.clone(),
                    r.name.clone(),
                    r.aliases.join("|"),
                    r.class.as_str().into(),
                    r.genbank_key.clone(),
                    String::from_utf8_lossy(&r.reference_nt).to_string(),
                    r.reference_aa
                        .as_ref()
                        .map(|p| String::from_utf8_lossy(p).to_string())
                        .unwrap_or_default(),
                    r.boundary_rule.as_str().into(),
                    r.boundary_evidence.clone(),
                    escape(&r.description),
                    r.review_status.as_str().into(),
                    r.curator.clone(),
                    r.date_added.clone(),
                    if r.patent_flag { "1" } else { "0" }.into(),
                    escape(&r.notes),
                ]
                .join("\t"),
            );
            f.push('\n');
        }

        let mut p = String::new();
        p.push_str(&PROVENANCE_COLUMNS.join("\t"));
        p.push('\n');
        for s in &self.provenance {
            p.push_str(
                &[
                    s.record_id.clone(),
                    s.field.clone(),
                    s.source_db.clone(),
                    s.source_accession.clone(),
                    s.licence.clone(),
                    s.url.clone(),
                    s.retrieved.clone(),
                    s.sha256.clone(),
                ]
                .join("\t"),
            );
            p.push('\n');
        }
        (f, p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FH: &str = "id\tname\taliases\tclass\tgenbank_key\treference_nt\treference_aa\tboundary_rule\tboundary_evidence\tdescription\treview_status\tcurator\tdate_added\tpatent_flag\tnotes";
    const PH: &str =
        "record_id\tfield\tsource_db\tsource_accession\tlicence\turl\tretrieved\tsha256";
    const SH: &str = "record_id\treview_status\tcurator\tsigned_date\tcontent_sha256\tnote";

    /// [`Db::parse`] with no sign-off table.
    ///
    /// A helper rather than a default argument on `parse` itself: a caller that
    /// forgets the third file must say "nothing is signed" out loud, because
    /// that is a claim about trust and not a formality.
    ///
    /// It is not the shipped tables' state, which this used to say it was:
    /// `features/SIGNOFF.tsv` signs every row, and
    /// `the_shipped_database_parses_and_ships_only_what_is_signed` is what
    /// checks that. The fixtures that need a signature build one with
    /// [`signoff`] below.
    fn parse(features: &str, provenance: &str) -> (Db, Vec<LoadError>) {
        Db::parse(features, provenance, "")
    }

    /// A sign-off table naming one record, with its digest recomputed from the
    /// tables it is signing.
    ///
    /// The digest cannot be written into a fixture by hand — that is the whole
    /// point of it — so it is computed the same way a curator computes it: load
    /// the row, ask the database. Loading with no sign-off table downgrades the
    /// row to `proposed` first, which does not disturb the digest because
    /// neither `review_status` nor `curator` is in [`SIGNED_COLUMNS`].
    fn signoff(features: &str, provenance: &str, id: &str, status: &str, curator: &str) -> String {
        let (db, _) = Db::parse(features, provenance, "");
        let r = db
            .records
            .iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("{id} is not in the fixture, so it cannot be signed"));
        format!(
            "{SH}\n{id}\t{status}\t{curator}\t2026-07-28\t{}\tchecked against the cited accession\n",
            db.content_digest(r)
        )
    }

    fn feat(id: &str, nt: &str, aa: &str, class: &str, status: &str, curator: &str) -> String {
        format!("{id}\tTest\t\t{class}\tCDS\t{nt}\t{aa}\torf_atg_to_stop\tJ01749.1:3293-4153:-\tA description\t{status}\t{curator}\t2026-07-27\t0\t")
    }

    /// `feat` with the patent_flag column under the caller's control.
    fn feat_flagged(id: &str, flag: &str) -> String {
        format!("{id}\tTest\t\tcds\tCDS\tATGACGT\tMT\torf_atg_to_stop\tJ01749.1:3293-4153:-\tA description\tproposed\t\t2026-07-27\t{flag}\t")
    }

    /// Per-field provenance covering everything `feat` populates.
    ///
    /// It used to be the single `reference_nt` row, which was all `audit()`
    /// asked for. That is no longer enough and should never have been: NOTICE
    /// promises a source and a licence for every field of every row, and a
    /// fixture that satisfies only the one rule under test lets the other ten
    /// columns ship unattributed.
    fn prov(id: &str) -> String {
        let ena = |field: &str| {
            format!("{id}\t{field}\tena\tAAB59737.1\tINSDC-free\thttps://www.ebi.ac.uk/ena/browser/api/fasta/AAB59737.1\t2026-07-27\tabc123")
        };
        let ours = |field: &str| format!("{id}\t{field}\tpolylinker\t-\town-work\t-\t2026-07-27\t");
        [
            ena("reference_nt"),
            ena("reference_aa"),
            ena("boundary_evidence"),
            ours("name"),
            ours("class"),
            ours("genbank_key"),
            ours("boundary_rule"),
            ours("description"),
            ours("patent_flag"),
        ]
        .join("\n")
    }

    /// How many rows `prov` emits, so a count assertion names the reason.
    const PROV_ROWS: usize = 9;

    #[test]
    fn a_well_formed_database_round_trips() {
        let f = format!(
            "#!version 2026.10\n{FH}\n{}\n",
            feat("PLF:0001", "ATGACGT", "MT", "cds", "reviewed", "L. Lobel")
        );
        let p = format!("{PH}\n{}\n", prov("PLF:0001"));
        // A `reviewed` row now needs a sign-off line to stay reviewed, so the
        // round trip is exercised over all three tables rather than two.
        let s = signoff(&f, &p, "PLF:0001", "reviewed", "L. Lobel");
        let (db, errs) = Db::parse(&f, &p, &s);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(db.version, "2026.10");
        assert_eq!(db.records.len(), 1);
        assert_eq!(db.records[0].review_status, ReviewStatus::Reviewed);
        assert_eq!(db.provenance.len(), PROV_ROWS);

        let (f2, p2) = db.to_tsv();
        let (again, errs2) = Db::parse(&f2, &p2, &s);
        assert!(errs2.is_empty(), "{errs2:?}");
        assert_eq!(again.records, db.records);
        assert_eq!(again.provenance, db.provenance);
    }

    #[test]
    fn a_patent_flag_is_read_however_a_curator_spells_it() {
        // It was a case-sensitive `matches!` against exactly "1", "true" and
        // "yes". A row saying `TRUE` loaded clean with the flag *cleared*, and
        // `to_tsv` then wrote "0", so one round trip erased the warning.
        for spelling in [
            "1", "true", "TRUE", "True", "yes", "Yes", "Y", "y", "T", "t",
        ] {
            let f = format!("{FH}\n{}\n", feat_flagged("PLF:0001", spelling));
            let (db, errs) = parse(&f, &format!("{PH}\n{}\n", prov("PLF:0001")));
            assert!(errs.is_empty(), "{spelling:?}: {errs:?}");
            assert!(
                db.records[0].patent_flag,
                "{spelling:?} was read as not patented"
            );

            // ...and it survives the round trip that used to erase it.
            let (f2, p2) = db.to_tsv();
            let (again, errs2) = parse(&f2, &p2);
            assert!(errs2.is_empty(), "{spelling:?}: {errs2:?}");
            assert!(
                again.records[0].patent_flag,
                "{spelling:?} lost in a round trip"
            );
        }

        // The control: the false spellings, including the empty cell that
        // `features.tsv` may legitimately carry, must stay false and must not
        // start raising errors.
        for spelling in ["", "0", "false", "FALSE", "No", "n", "F", " 0 "] {
            let f = format!("{FH}\n{}\n", feat_flagged("PLF:0001", spelling));
            let (db, errs) = parse(&f, &format!("{PH}\n{}\n", prov("PLF:0001")));
            assert!(errs.is_empty(), "{spelling:?}: {errs:?}");
            assert!(!db.records[0].patent_flag, "{spelling:?} read as patented");
        }
    }

    #[test]
    fn an_unreadable_patent_flag_is_refused_not_read_as_unpatented() {
        // Nothing may fail silently, and this field least of all: guessing
        // `false` is the answer that loses the warning.
        for spelling in ["probably", "maybe", "2", "yes?"] {
            let f = format!("{FH}\n{}\n", feat_flagged("PLF:0001", spelling));
            let (db, errs) = parse(&f, &format!("{PH}\n{}\n", prov("PLF:0001")));
            assert_eq!(db.records.len(), 0, "{spelling:?} was accepted");
            assert!(
                errs.iter().any(|e| e.problem.contains("patent_flag")),
                "{spelling:?}: no error names the column: {errs:?}"
            );
        }
    }

    #[test]
    fn a_sequence_with_no_stated_origin_is_refused() {
        // The single rule the whole schema exists to enforce.
        let f = format!(
            "{FH}\n{}\n",
            feat("PLF:0001", "ATGACGT", "", "cds", "proposed", "")
        );
        let (_, errs) = parse(&f, &format!("{PH}\n"));
        assert!(
            errs.iter()
                .any(|e| e.problem.contains("no provenance for reference_nt")),
            "{errs:?}"
        );
    }

    #[test]
    fn an_unreviewed_row_may_not_claim_a_review_status() {
        let f = format!(
            "{FH}\n{}\n",
            feat("PLF:0001", "ATGACGT", "", "cds", "reviewed", "")
        );
        let (db, errs) = parse(&f, &format!("{PH}\n{}\n", prov("PLF:0001")));
        assert_eq!(db.records.len(), 0);
        assert!(
            errs.iter().any(|e| e.problem.contains("no curator")),
            "{errs:?}"
        );

        // Proposing without a curator is exactly what a proposal is.
        let f = format!(
            "{FH}\n{}\n",
            feat("PLF:0001", "ATGACGT", "", "cds", "proposed", "")
        );
        let (db, errs) = parse(&f, &format!("{PH}\n{}\n", prov("PLF:0001")));
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(db.records.len(), 1);
    }

    #[test]
    fn only_reviewed_rows_and_their_provenance_are_shippable() {
        let f = format!(
            "{FH}\n{}\n{}\n",
            feat("PLF:0001", "ATGACGT", "", "cds", "proposed", ""),
            feat("PLF:0002", "TTTTGGG", "", "cds", "reviewed", "L. Lobel"),
        );
        let p = format!("{PH}\n{}\n{}\n", prov("PLF:0001"), prov("PLF:0002"));
        let s = signoff(&f, &p, "PLF:0002", "reviewed", "L. Lobel");
        let (db, errs) = Db::parse(&f, &p, &s);
        assert!(errs.is_empty(), "{errs:?}");
        let ship = db.reviewed();
        assert_eq!(ship.records.len(), 1);
        // Provenance for the dropped record must go with it, or the release
        // carries attribution obligations for data it does not contain.
        assert_eq!(ship.provenance.len(), PROV_ROWS);
        assert_eq!(ship.provenance[0].record_id, "PLF:0002");
    }

    #[test]
    fn provenance_is_per_field_so_one_row_can_mix_licences() {
        let f = format!(
            "{FH}\n{}\n",
            feat("PLF:0001", "ATGACGT", "MT", "cds", "proposed", "")
        );
        let p = format!(
            "{PH}\n{}\n{}\n{}\n",
            prov("PLF:0001"),
            "PLF:0001\tname\tuniprot\tP62593\tCC-BY-4.0\thttps://rest.uniprot.org/uniprotkb/P62593\t2026-07-27\tdef456",
            "PLF:0001\tdescription\tpolylinker\t-\town-work\t-\t2026-07-27\t-",
        );
        let (db, errs) = parse(&f, &p);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(db.provenance_of("PLF:0001").len(), PROV_ROWS + 2);
        let l = db.licences();
        assert_eq!(l.len(), 3, "three distinct licences in one row: {l:?}");
        assert!(
            l.contains_key("CC-BY-4.0")
                && l.contains_key("INSDC-free")
                && l.contains_key("own-work")
        );
    }

    #[test]
    fn a_non_coding_feature_may_not_carry_a_protein() {
        // Otherwise a promoter lands in the translated index and matches noise.
        // The control for the relaxation below: the list of classes that may
        // carry a peptide grew from one to two, not from one to six.
        for class in ["regulatory", "origin", "repeat", "misc"] {
            let f = format!(
                "{FH}\n{}\n",
                feat("PLF:0001", "ATGACGT", "MT", class, "proposed", "")
            );
            let (db, errs) = parse(&f, &format!("{PH}\n{}\n", prov("PLF:0001")));
            assert!(db.records.is_empty(), "{class} was accepted");
            assert!(
                errs.iter()
                    .any(|e| e.problem.contains("only cds and synthetic_part may")),
                "{class}: {errs:?}"
            );
        }
    }

    #[test]
    fn a_synthetic_part_may_be_a_peptide_and_nothing_else() {
        // Decision 1, 2026-07-28. A tag is a peptide: FLAG is DYKDDDDK and its
        // nucleotides are whichever codons a vector's designer picked, so a
        // nucleotide-only row for it is one arbitrary encoding that misses
        // every re-coded copy. Before this, the loader refused the row twice
        // over -- once for the empty reference_nt and once for carrying a
        // protein on a class that was not `cds`.
        let f = format!(
            "{FH}\n{}\n",
            "PLF:3000\tFLAG tag\tFLAG\tsynthetic_part\tmisc_feature\t\tDYKDDDDK\t\
             designed_sequence\tDOI:10.1038/nbt1088-1204\tAn epitope tag\tproposed\t\t\
             2026-07-28\t0\t"
        );
        let p = format!(
            "{PH}\n{}\n",
            [
                "PLF:3000\treference_aa\twwpdb\t8RMO_1\tCC0-1.0\thttps://data.rcsb.org/rest/v1/core/polymer_entity/8RMO/1\t2026-07-28\tabc123",
                "PLF:3000\tname\tpolylinker\t-\town-work\t-\t2026-07-28\t",
                "PLF:3000\taliases\tpolylinker\t-\town-work\t-\t2026-07-28\t",
                "PLF:3000\tclass\tpolylinker\t-\town-work\t-\t2026-07-28\t",
                "PLF:3000\tgenbank_key\tpolylinker\t-\town-work\t-\t2026-07-28\t",
                "PLF:3000\tboundary_rule\tpolylinker\t-\town-work\t-\t2026-07-28\t",
                "PLF:3000\tboundary_evidence\tpolylinker\t-\town-work\t-\t2026-07-28\t",
                "PLF:3000\tdescription\tpolylinker\t-\town-work\t-\t2026-07-28\t",
                "PLF:3000\tpatent_flag\tpolylinker\t-\town-work\t-\t2026-07-28\t",
            ]
            .join("\n")
        );
        let (db, errs) = parse(&f, &p);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(db.records.len(), 1);
        let r = &db.records[0];
        assert!(r.is_peptide_only(), "{r:?}");
        assert_eq!(r.reference_aa.as_deref(), Some(&b"DYKDDDDK"[..]));
        assert_eq!(r.units(), 8, "eight residues, not zero bases");
        // `audit()` must not ask a peptide-only row for nucleotide provenance
        // it has no nucleotides to source.
        assert!(
            !errs
                .iter()
                .any(|e| e.problem.contains("provenance for reference_nt")),
            "{errs:?}"
        );
    }

    #[test]
    fn a_row_with_neither_reference_is_refused() {
        // The clause that keeps "reference_nt may be empty" from becoming
        // "anything goes". The invariant is *at least one reference*.
        let f = format!(
            "{FH}\n{}\n",
            "PLF:3000\tNothing\t\tsynthetic_part\tmisc_feature\t\t\tdesigned_sequence\t\
             DOI:10/x\tA description\tproposed\t\t2026-07-28\t0\t"
        );
        let (db, errs) = parse(&f, &format!("{PH}\n{}\n", prov("PLF:3000")));
        assert!(db.records.is_empty());
        assert!(
            errs.iter().any(|e| e
                .problem
                .contains("neither a nucleotide nor a protein reference")),
            "{errs:?}"
        );
    }

    #[test]
    fn a_cds_with_no_nucleotides_is_refused() {
        // Without this, the pair-rule admits a protein-only CDS whose
        // `orf_atg_to_stop` boundary is a derived-boundary claim over no bases
        // at all -- the strongest assertion in the schema, made about a
        // sequence the row does not carry.
        let f = format!(
            "{FH}\n{}\n",
            feat("PLF:0001", "", "MTMTMTMT", "cds", "proposed", "")
        );
        let (db, errs) = parse(&f, &format!("{PH}\n{}\n", prov("PLF:0001")));
        assert!(db.records.is_empty());
        assert!(
            errs.iter()
                .any(|e| e.problem.contains("class cds requires reference_nt")),
            "{errs:?}"
        );
    }

    #[test]
    fn a_peptide_only_row_may_not_claim_an_orf_derived_boundary() {
        // `is_derived()` is described as the project's strongest legal
        // position. A row with no bases cannot have computed a boundary from a
        // reading frame, and this is the clause that stops that claim being
        // makeable at all rather than merely being absent today.
        let f = format!(
            "{FH}\n{}\n",
            "PLF:3000\tTag\t\tsynthetic_part\tmisc_feature\t\tDYKDDDDK\torf_atg_to_stop\t\
             DOI:10/x\tA description\tproposed\t\t2026-07-28\t0\t"
        );
        let (db, errs) = parse(&f, &format!("{PH}\n{}\n", prov("PLF:3000")));
        assert!(db.records.is_empty());
        assert!(
            errs.iter()
                .any(|e| e.problem.contains("no nucleotides to read")),
            "{errs:?}"
        );
    }

    #[test]
    fn a_protein_reference_is_checked_against_the_amino_acid_alphabet() {
        // There was no check at all while `reference_aa` was decoration on a
        // row that also carried nucleotides. On a peptide-only row it is the
        // whole record, so an unchecked cell is the whole sequence unchecked.
        for (aa, why) in [
            ("DYKDDDDZ", "Z is not a residue"),
            ("DYKDDDD*", "a stop codon is meaningless in a tag"),
            ("DYK DDDD", "a space is not a residue"),
        ] {
            let f = format!(
                "{FH}\n{}\n",
                format_args!(
                    "PLF:3000\tTag\t\tsynthetic_part\tmisc_feature\t\t{aa}\tdesigned_sequence\t\
                     DOI:10/x\tA description\tproposed\t\t2026-07-28\t0\t"
                )
            );
            let (db, errs) = parse(&f, &format!("{PH}\n{}\n", prov("PLF:3000")));
            assert!(db.records.is_empty(), "{aa:?} accepted: {why}");
            assert!(
                errs.iter()
                    .any(|e| e.problem.contains("is not an amino-acid code")),
                "{aa:?}: {errs:?}"
            );
        }
        // ...and `X` is accepted, because `index::seedable` already excludes it
        // from every seed, so an unknown residue degrades into "harder to find"
        // rather than into a symbol that could match a terminator.
        let f = format!(
            "{FH}\n{}\n",
            "PLF:3000\tTag\t\tsynthetic_part\tmisc_feature\t\tDYKXDDDD\tdesigned_sequence\t\
             DOI:10/x\tA description\tproposed\t\t2026-07-28\t0\t"
        );
        let (db, _) = parse(&f, &format!("{PH}\n{}\n", prov("PLF:3000")));
        assert_eq!(db.records.len(), 1);
    }

    #[test]
    fn a_peptide_only_row_does_not_collide_with_every_other_one() {
        // `duplicates()` keyed on `reference_nt`, so every peptide-only row
        // keyed on the empty string and they all reported as "identical
        // sequences" -- which broke tests/corpus.rs on the first build after
        // the relaxation. The alphabet discriminant is required rather than
        // cosmetic: `ATG` is a valid codon triple and a valid tripeptide.
        let pep = |id: &str, aa: &str| {
            format!(
                "{id}\tTag\t\tsynthetic_part\tmisc_feature\t\t{aa}\tdesigned_sequence\t\
                 DOI:10/x\tA description\tproposed\t\t2026-07-28\t0\t"
            )
        };
        let f = format!(
            "{FH}\n{}\n{}\n{}\n",
            pep("PLF:3000", "DYKDDDDK"),
            pep("PLF:3001", "WSHPQFEK"),
            feat("PLF:0001", "ATG", "", "cds", "proposed", ""),
        );
        let (db, _) = parse(&f, &format!("{PH}\n"));
        assert_eq!(db.records.len(), 3);
        let d = db.duplicates();
        assert!(d.is_empty(), "two different peptides collided: {d:?}");

        // The control: two rows really holding the same peptide are still
        // reported, so the discriminant did not disarm the check.
        let f = format!(
            "{FH}\n{}\n{}\n",
            pep("PLF:3000", "DYKDDDDK"),
            pep("PLF:3001", "DYKDDDDK")
        );
        let (db, _) = parse(&f, &format!("{PH}\n"));
        assert!(
            db.duplicates()
                .iter()
                .any(|s| s.contains("identical sequences")),
            "{:?}",
            db.duplicates()
        );
    }

    #[test]
    fn a_boundary_without_evidence_is_refused() {
        let mut cells: Vec<String> = feat("PLF:0001", "ATGACGT", "", "cds", "proposed", "")
            .split('\t')
            .map(String::from)
            .collect();
        cells[8] = String::new();
        let (_, errs) = parse(&format!("{FH}\n{}\n", cells.join("\t")), &format!("{PH}\n"));
        assert!(
            errs.iter().any(|e| e.problem.contains("boundary_evidence")),
            "{errs:?}"
        );
    }

    #[test]
    fn derived_boundaries_are_distinguishable_from_chosen_ones() {
        assert!(BoundaryRule::OrfAtgToStop.is_derived());
        assert!(BoundaryRule::OrfMaturePeptide.is_derived());
        // These are judgements, and the schema must not let them pass as facts.
        assert!(!BoundaryRule::LiteratureDefined.is_derived());
        assert!(!BoundaryRule::ConsensusOfInsdc.is_derived());
        assert!(!BoundaryRule::DesignedSequence.is_derived());
    }

    #[test]
    fn orphan_provenance_is_reported() {
        let f = format!(
            "{FH}\n{}\n",
            feat("PLF:0001", "ATGACGT", "", "cds", "proposed", "")
        );
        let p = format!("{PH}\n{}\n{}\n", prov("PLF:0001"), prov("PLF:9999"));
        let (_, errs) = parse(&f, &p);
        assert!(
            errs.iter()
                .any(|e| e.problem.contains("unknown record PLF:9999")),
            "{errs:?}"
        );
    }

    #[test]
    fn a_malformed_row_is_reported_not_silently_dropped() {
        let f = format!(
            "{FH}\n{}\nnot\tenough\n{}\n",
            feat("PLF:0001", "ATGACGT", "", "cds", "proposed", ""),
            feat("PLF:0002", "GGGGTTT", "", "cds", "proposed", ""),
        );
        let p = format!("{PH}\n{}\n{}\n", prov("PLF:0001"), prov("PLF:0002"));
        let (db, errs) = parse(&f, &p);
        assert_eq!(db.records.len(), 2, "the good rows still load");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].line, 3);
    }

    #[test]
    fn duplicates_are_found() {
        let f = format!(
            "{FH}\n{}\n{}\n{}\n",
            feat("PLF:0001", "ATGACGT", "", "cds", "proposed", ""),
            feat("PLF:0001", "TTTTGGG", "", "cds", "proposed", ""),
            feat("PLF:0003", "ATGACGT", "", "cds", "proposed", ""),
        );
        let (db, _) = parse(&f, &format!("{PH}\n"));
        let d = db.duplicates();
        assert!(
            d.iter().any(|s| s.contains("duplicate id PLF:0001")),
            "{d:?}"
        );
        assert!(d.iter().any(|s| s.contains("identical sequences")), "{d:?}");
    }

    #[test]
    fn a_description_may_contain_tabs_and_newlines() {
        let mut cells: Vec<String> = feat("PLF:0001", "ATGACGT", "", "cds", "proposed", "")
            .split('\t')
            .map(String::from)
            .collect();
        cells[9] = "line one\\nline two\\tindented".into();
        let (db, errs) = parse(
            &format!("{FH}\n{}\n", cells.join("\t")),
            &format!("{PH}\n{}\n", prov("PLF:0001")),
        );
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(db.records[0].description, "line one\nline two\tindented");
        let (f2, p2) = db.to_tsv();
        let (again, _) = parse(&f2, &p2);
        assert_eq!(again.records[0].description, db.records[0].description);
    }

    #[test]
    fn a_file_without_a_header_is_an_error_not_an_empty_database() {
        let (db, errs) = parse("# just a comment\n", "");
        assert!(db.records.is_empty());
        assert!(
            errs.iter().any(|e| e.problem.contains("no header")),
            "{errs:?}"
        );
    }

    #[test]
    fn the_shipped_database_parses_and_ships_only_what_is_signed() {
        // Two claims, and the second is a promise to Lior rather than a
        // property of the code: the compiled-in table is well formed, and
        // *nothing ships that a human has not signed*.
        //
        // This test used to be named `..._and_ships_nothing`, and asserted a
        // stronger thing: that every row is `proposed` and `reviewed()` is
        // empty. That was true of the data, not of the rule, and it would have
        // had to be deleted the first time a curator signed anything — which is
        // exactly when a test of this kind is most needed. So it now asserts
        // the rule: `reviewed()` contains precisely those rows
        // `features/SIGNOFF.tsv` names with a digest that still matches, and
        // the count is printed rather than pinned.
        //
        // Putting `AmpR` on somebody's plasmid map is an assertion, and the
        // rule for this project is that it may propose and never assert.
        let (db, errors) = Db::builtin();
        assert!(
            errors.is_empty(),
            "the shipped tables must parse: {errors:?}"
        );
        assert!(!db.records.is_empty(), "and it must not be empty");
        assert!(
            !db.version.is_empty(),
            "every annotation is stamped with it"
        );

        let ship = db.reviewed();
        // NOT a pinned count, for the reason the comment above gives. But not
        // nothing either: the rustdoc on `Db::builtin` used to state that
        // SIGNOFF.tsv is empty of signatures and that `reviewed()` therefore
        // returns an empty database, and prose alone could not stop that from
        // going stale a second time. Asserting non-emptiness puts the claim
        // where a change to the data has to face it.
        assert!(
            !ship.records.is_empty(),
            "the shipped tables carry signatures; if that is deliberately no \
             longer true, `Db::builtin`'s doc and `pl annotate --db`'s empty-set \
             message both describe the new state and must be re-read"
        );
        for r in &ship.records {
            assert!(
                !r.curator.is_empty(),
                "{} ships at {} with no curator named",
                r.id,
                r.review_status.as_str()
            );
        }
        // Every row that is not signed is `proposed`, with nobody's name on it.
        for r in &db.records {
            if r.review_status == ReviewStatus::Proposed {
                assert!(
                    r.curator.is_empty(),
                    "{} is proposed but carries curator {:?}",
                    r.id,
                    r.curator
                );
            }
            // The relaxed pair-rule, asserted over the real table: every row
            // carries at least one reference, and the ones with no nucleotides
            // are synthetic parts carrying a peptide.
            assert!(
                !r.reference_nt.is_empty() || r.is_peptide_only(),
                "{} has neither a sequence nor a peptide",
                r.id
            );
            if r.reference_nt.is_empty() {
                assert_eq!(r.class, Class::SyntheticPart, "{}", r.id);
            }
            assert!(r.id.starts_with("PLF:"), "{} is not one of ours", r.id);
        }
        eprintln!(
            "shipped database: {} record(s), {} signed off, census {:?}",
            db.records.len(),
            ship.records.len(),
            db.census()
        );
    }

    /// The two README files must state the sign-off count the database
    /// actually has.
    ///
    /// PROVEN TO FAIL at 713bd3b, on both files at once: `README.md` said the
    /// database "ships **0 reviewed records**" and "**84 records as of release
    /// 2026.07.28, 0 reviewed**", and `features/README.md` said "89 records. 84
    /// carry a curator sign-off ... the other 5 are `proposed`". Measured from
    /// the shipped tables at that commit: 89 rows, `review_status=reviewed` on
    /// all 89, and 89 `PLF:` lines in `SIGNOFF.tsv`. Every one of those numbers
    /// was wrong, and they were wrong in the direction that reads *safer* than
    /// the truth — the front page said the tool asserts nothing while by
    /// default it writes 89 signed names onto somebody's map.
    ///
    /// [`Db::builtin`]'s own rustdoc records this same sentence being found
    /// false and corrected *there* in an earlier pass; the READMEs were not
    /// touched, because no check existed that could have noticed. This is that
    /// check: the counts are generated from the live tables and searched for in
    /// the prose, so signing a 90th row breaks the build rather than quietly
    /// making the front page a lie.
    ///
    /// Deliberately `include_str!` and not `fs::read`: the paths resolve at
    /// compile time relative to this file, so the test cannot pass by failing
    /// to find the files.
    ///
    /// **Rewritten 2026-08-10, when the table stopped being all-signed.** The
    /// version above asserted `total == signed` outright and built both
    /// sentences around "all N reviewed", so the first `proposed` row anyone
    /// contributed broke it — not because the prose was wrong but because the
    /// test could only express one state of the world. That is the wrong shape
    /// for a check on a database whose entire premise is that a machine may add
    /// rows a human has not yet approved: the interesting number is the gap.
    /// Both sentences now carry all three counts, and the inverted control at
    /// the bottom is what stops the READMEs quietly reverting to the reassuring
    /// wording — "all N reviewed" must be ABSENT while anything is proposed.
    #[test]
    fn the_readmes_state_the_signoff_count_the_database_has() {
        const ROOT_README: &str = include_str!("../../../README.md");
        const FEATURES_README: &str = include_str!("../../../features/README.md");

        let (db, errors) = Db::builtin();
        assert!(errors.is_empty(), "{errors:?}");
        let total = db.records.len();
        let signed = db.reviewed().records.len();
        let proposed = total - signed;

        // The whole phrase, built from the tables rather than written out.
        // Matching a phrase and not a bare integer on purpose: "89" turns up in
        // a README for many reasons, and a check that some digit occurs
        // somewhere is a check that cannot fail.
        let root_claim = format!(
            "**{total} records as of release {}, {signed} reviewed and {proposed} \
             proposed.**",
            db.version
        );
        assert!(
            ROOT_README.contains(&root_claim),
            "README.md does not carry {root_claim:?}"
        );
        let features_claim = format!(
            "v0.1 pre-release, {total} records. {signed} carry a curator sign-off, \
             and {proposed} are `proposed`"
        );
        assert!(
            FEATURES_README.contains(&features_claim),
            "features/README.md does not carry {features_claim:?}"
        );

        for (name, text) in [
            ("README.md", ROOT_README),
            ("features/README.md", FEATURES_README),
        ] {
            // The claim that started this: nothing may say the shipped set is
            // empty while it is not. Asserted as the absence of the exact stale
            // wording, because that wording is what actually shipped.
            assert!(
                !text.contains("0 reviewed"),
                "{name} still says the database ships 0 reviewed records; it ships {signed}"
            );
            // And its mirror image, which is the failure mode this file now has
            // and did not before: prose that says everything is approved while
            // `proposed` rows sit in the table. Both spellings the old wording
            // used are refused, so restoring either one fails here.
            if proposed > 0 {
                for stale in [
                    format!("all {total} reviewed"),
                    format!("All {total} carry"),
                ] {
                    assert!(
                        !text.contains(&stale),
                        "{name} says {stale:?} while {proposed} row(s) are still 'proposed'"
                    );
                }
            }
        }
    }

    /// The "no promoter is in it yet" disclosure follows the SIGNED table.
    ///
    /// [`Db::absent_common_kinds`] is what the desktop app's proposals panel
    /// and `pl methods annotate` both use to tell a user which whole classes of
    /// feature the database has never held, and it exists because somebody who
    /// watches `AmpR` light up and sees no `ori` will otherwise conclude their
    /// plasmid has none. It probes for three literal `genbank_key` values.
    ///
    /// That coupling is invisible from either end, and on 2026-08-10 it broke
    /// for the length of one build. Twelve promoter, enhancer, terminator and
    /// poly(A) rows landed carrying `genbank_key = regulatory` -- the current
    /// INSDC spelling, and the correct one, since `promoter` and `terminator`
    /// are retired keys. The probe looks for `promoter`, so those rows were
    /// invisible to it, and the app would have gone on saying "no promoter is
    /// in it yet" after promoters were signed off. Nothing in the schema, the
    /// loader or the build would have noticed: the rows are well formed, the
    /// key is more correct than what replaced it, and the only symptom is a
    /// sentence on a panel that quietly stops being true.
    ///
    /// So the property is pinned from both sides at once. PROVEN TO FAIL by
    /// reverting `stage_classb.py` to `genbank_key="regulatory"` and rebuilding:
    ///
    /// ```text
    /// the whole table still reports 'promoter' absent, but it holds 12 Class B
    /// rows; Db::absent_common_kinds probes literal genbank_key values and
    /// something has stopped matching
    /// ```
    ///
    /// and it fails the other way if the twelve are ever signed without this
    /// test being reconsidered, because the first assertion then stops holding.
    #[test]
    fn the_absent_kinds_disclosure_tracks_the_reviewed_set_and_not_the_whole_table() {
        let (all, errors) = Db::builtin();
        assert!(errors.is_empty(), "{errors:?}");
        let reviewed = all.reviewed();
        assert!(
            all.records.len() > reviewed.records.len(),
            "this test is about the difference between the two tables and there is none"
        );

        // What a user actually searches by default. Still has all three gaps,
        // because none of the Class B rows is signed.
        let searched = reviewed.absent_common_kinds();
        for kind in ["promoter", "terminator", "origin of replication"] {
            assert!(
                searched.contains(&kind),
                "the signed table reports {kind:?} present; the disclosure the app \
                 shows by default would be wrong"
            );
        }

        // And the whole table, which `--include-proposed` searches, must NOT
        // claim the two gaps it no longer has.
        let whole = all.absent_common_kinds();
        for kind in ["promoter", "terminator"] {
            assert!(
                !whole.contains(&kind),
                "the whole table still reports {kind:?} absent, but it holds {} Class B \
                 rows; Db::absent_common_kinds probes literal genbank_key values and \
                 something has stopped matching",
                all.records
                    .iter()
                    .filter(|r| r.id.starts_with("PLF:4"))
                    .count()
            );
        }
        // Origins are genuinely absent from both, and saying so here is what
        // keeps the two assertions above from being a test that anything
        // non-empty passes.
        assert!(
            whole.contains(&"origin of replication"),
            "an origin row appeared; this test's third probe needs rewriting rather \
             than deleting"
        );
    }

    /// `tools/release.ps1` must copy `features/NOTICE` into `dist/`, because
    /// this crate puts the database it belongs to inside every shipped binary.
    ///
    /// PROVEN TO FAIL at HEAD on 2026-08-04, with `$notices` holding ten
    /// entries and none of them from `features/`:
    ///
    /// ```text
    /// tools/release.ps1 copies 10 notice(s) and none of them is
    /// 'features/NOTICE'. NOTICE says it "must be packaged with any
    /// distribution that includes the database", and this crate include_str!s
    /// the database into every binary. Copied: ["NOTICE", "LICENSE",
    /// "TRADEMARKS.md", "bins/pl-gui/fonts/IBMPlex-OFL.txt", ...]
    /// ```
    ///
    /// Ten, and every one of them a *code* notice — the three top-level files
    /// and seven font licence texts. Nothing the script copied had anything to
    /// do with the data.
    ///
    /// [`Db::builtin`] `include_str!`s `features/features.tsv` and
    /// `features/provenance.tsv`, and all four artifacts `release.ps1` ships —
    /// `pl.exe`, `polylinker.exe`, `pl-mcp.exe` and the Python extension —
    /// depend on this crate. So all four carry the CC BY 4.0 dataset and all
    /// four carry its attribution obligations, whether or not the recipient
    /// ever sees a source tree. `dist/` is exactly the case where they will
    /// not: `NOTICE` and `pl licences` both end by pointing at `features/NOTICE`
    /// "in the source distribution", and a user handed a zip of binaries has no
    /// such thing.
    ///
    /// **THE GAP WAS PARTLY COVERED, WHICH IS WHY IT SURVIVED ELEVEN ENTRIES.**
    /// `dist/NOTICE.txt` does ship, and it carries the UniProt CC BY 4.0
    /// statement of changes, the NLM courtesy line and the Rfam CC0 note; `pl
    /// licences` prints the same subset out of the compiled-in table. What
    /// neither carries is the part `features/NOTICE` exists for: the per-family
    /// Rfam primary-source credit table — 24 rows, `PLF:2000` to `PLF:2023`,
    /// each naming the PMID taken from that family's own `#=GF RM` line — and
    /// the "sources not used" list that records FPbase as HOLD and UniVec as
    /// NO-GO. Rfam asks for per-family credit; the pointer is not the credit.
    ///
    /// PARSED OUT OF THE `$notices` ARRAY, NOT SEARCHED FOR IN THE FILE. The
    /// script's comments name the paths they discuss, at length, so a
    /// `contains("features/NOTICE")` over the whole script would go green on
    /// the prose describing the defect — the same "a check that cannot fail"
    /// shape [`Db::builtin`]'s neighbours keep running into. Only `From = '...'`
    /// is read, which appears nowhere but in the array.
    ///
    /// Deliberately `include_str!` and not `fs::read`: the path resolves at
    /// compile time relative to this file, so the test cannot pass by failing
    /// to find the script, and editing the script rebuilds this crate.
    #[test]
    fn the_release_script_packages_the_database_notice() {
        const RELEASE_PS1: &str = include_str!("../../../tools/release.ps1");
        const ROOT_NOTICE: &str = include_str!("../../../NOTICE");

        // The obligation, read from NOTICE rather than asserted from memory. If
        // somebody deletes this sentence the test has to fail loudly rather
        // than quietly stop meaning anything.
        //
        // Newlines normalised first because the sentence wraps and
        // `include_str!` hands back whatever the checkout put on disk. Left as
        // a bare `\n` this would assert the repository's line-ending policy as
        // a side effect of asserting a licence obligation, and would fail on a
        // CRLF checkout with a message about attribution.
        let notice = ROOT_NOTICE.replace("\r\n", "\n");
        assert!(
            notice.contains(
                "is `features/NOTICE`,\nand it must be packaged with any distribution that \
                 includes the database."
            ),
            "NOTICE no longer says features/NOTICE must be packaged; if that obligation \
             really has gone, this test goes with it"
        );

        // Every source path the script copies.
        let copied: Vec<&str> = RELEASE_PS1
            .match_indices("From = '")
            .map(|(i, m)| {
                let rest = &RELEASE_PS1[i + m.len()..];
                let end = rest
                    .find('\'')
                    .expect("unterminated From = '...' in release.ps1");
                &rest[..end]
            })
            .collect();

        // The parser itself, against entries that were in the array before this
        // test existed. A scan that silently matched nothing -- the array
        // reformatted, the quoting changed -- would leave `copied` empty and
        // make the assertion below vacuous while it went on reading as a check.
        for anchor in ["NOTICE", "LICENSE", "bins/pl-gui/fonts/IBMPlex-OFL.txt"] {
            assert!(
                copied.contains(&anchor),
                "the $notices parser found {} entry(ies) and not {anchor:?}; it is broken, \
                 not the script",
                copied.len()
            );
        }

        assert!(
            copied.contains(&"features/NOTICE"),
            "tools/release.ps1 copies {} notice(s) and none of them is 'features/NOTICE'. \
             NOTICE says it \"must be packaged with any distribution that includes the \
             database\", and this crate include_str!s the database into every binary. \
             Copied: {copied:?}",
            copied.len()
        );
    }

    #[test]
    fn a_signed_record_whose_provenance_is_not_cleared_does_not_ship() {
        // The licence gate arrived in e0109f8 with NO test, gating what a
        // release may ship — the one constraint here with legal weight rather
        // than merely correctness weight — so this is it.
        //
        // THE HOLE IS NARROWER THAN IT LOOKS, and this test aims at the part
        // that is real. `content_digest` hashes each record's provenance quads,
        // so APPENDING an uncleared row to an already-signed record changes the
        // digest, `apply_signoff` reports it and forces the row back to
        // Proposed — `a_signed_row_that_changes_loses_its_sign_off` covers that
        // and it was already covered before the gate existed. What was NOT
        // covered is a record signed WHILE the uncleared row was already there:
        // the digest matches, the signature stands, and `review_status` alone
        // says ship it. A curator can reach that state by honest mistake —
        // `build.py --show` prints the quads but does not refuse an uncleared
        // one — which is the whole reason "discipline is not a control".
        //
        // Addgene's terms do not clear it for use as data; see
        // features/SOURCING.md. `check_signoff.py` plants this exact row.
        let tainted = "PLF:0001\tdescription\taddgene\tAddgene-52961\t\
             noncommercial-informational-only\thttps://www.addgene.org/52961/\t2026-07-28\tdeadbeef";
        let f = format!(
            "{FH}\n{}\n{}\n",
            feat("PLF:0001", "ATGACGT", "MT", "cds", "reviewed", "L. Lobel"),
            feat("PLF:0002", "ATGACGT", "MT", "cds", "reviewed", "L. Lobel"),
        );
        let p = format!(
            "{PH}\n{}\n{}\n{tainted}\n",
            prov("PLF:0001"),
            prov("PLF:0002")
        );

        // Both signed over the tables AS THEY STAND, tainted row included, so
        // both digests are honest and both signatures hold.
        let (staged, _) = Db::parse(&f, &p, "");
        let dig = |id: &str| {
            let r = staged
                .records
                .iter()
                .find(|r| r.id == id)
                .unwrap_or_else(|| panic!("{id} missing from the fixture"));
            staged.content_digest(r)
        };
        let s = format!(
            "{SH}\nPLF:0001\treviewed\tL. Lobel\t2026-07-28\t{}\tchecked\n\
             PLF:0002\treviewed\tL. Lobel\t2026-07-28\t{}\tchecked\n",
            dig("PLF:0001"),
            dig("PLF:0002")
        );

        let (db, errs) = Db::parse(&f, &p, &s);

        // THE CONTROL, and without it this test proves nothing: both signatures
        // must survive. If PLF:0001 had been demoted to Proposed by the digest
        // check, it would drop out of `reviewed()` for a reason that has nothing
        // to do with the licence gate, and the assertion below would pass on a
        // build where that gate had been deleted.
        for id in ["PLF:0001", "PLF:0002"] {
            let r = db.records.iter().find(|r| r.id == id).expect(id);
            assert_eq!(
                r.review_status,
                ReviewStatus::Reviewed,
                "{id}'s signature must hold, or this tests the digest and not the licence"
            );
        }
        // The audit reports it — but only reports it, which is the gap.
        assert!(
            errs.iter().any(|e| e.problem.contains("addgene")),
            "the taint must be reported: {errs:?}"
        );

        // And the shipped subset leaves it behind, record and provenance alike.
        let shipped = db.reviewed();
        let ids: Vec<&str> = shipped.records.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["PLF:0002"],
            "a signed record with uncleared provenance reached a release"
        );
        assert!(
            !shipped.provenance.iter().any(|x| x.source_db == "addgene"),
            "the uncleared provenance row shipped even though its record did not"
        );
        // Scoped: the clean sibling is untouched. A gate that drops the whole
        // table on one bad row would be safe and useless.
        assert_eq!(
            shipped
                .provenance
                .iter()
                .filter(|x| x.record_id == "PLF:0002")
                .count(),
            staged
                .provenance
                .iter()
                .filter(|x| x.record_id == "PLF:0002")
                .count(),
            "the clean record lost provenance rows it was entitled to keep"
        );

        // The same tables with the taint removed ship BOTH, so the exclusion is
        // caused by the licence and by nothing else in the fixture.
        //
        // RE-SIGNED, and it has to be: `content_digest` covers the provenance
        // quads, so deleting the row moves PLF:0001's digest and the old
        // signature lapses. Reusing `s` here fails with "the row has changed
        // since it was signed" — which is the append-after-signing defence
        // doing its job, and precisely why it is NOT the hole this test is
        // about. Two mechanisms, and the fixture has to keep them apart.
        let clean = p.replace(&format!("{tainted}\n"), "");
        assert_ne!(
            clean, p,
            "the fixture moved; the tainted row was not removed"
        );
        let (staged2, _) = Db::parse(&f, &clean, "");
        let dig2 = |id: &str| {
            let r = staged2.records.iter().find(|r| r.id == id).expect(id);
            staged2.content_digest(r)
        };
        let s2 = format!(
            "{SH}\nPLF:0001\treviewed\tL. Lobel\t2026-07-28\t{}\tchecked\n\
             PLF:0002\treviewed\tL. Lobel\t2026-07-28\t{}\tchecked\n",
            dig2("PLF:0001"),
            dig2("PLF:0002")
        );
        let (db2, errs2) = Db::parse(&f, &clean, &s2);
        assert!(errs2.is_empty(), "the control must load clean: {errs2:?}");
        assert_eq!(
            db2.reviewed().records.len(),
            2,
            "with the licence cleared, the same two records ship"
        );
    }

    #[test]
    fn a_signed_row_that_changes_loses_its_sign_off() {
        // The case the whole content-hash design exists for, and the one the
        // id-stability audit in build.py cannot catch on a fresh clone: it
        // compares against the file it is about to overwrite, so on a clone
        // there is no baseline at all. SIGNOFF.tsv is committed, so it is the
        // only baseline a clone has.
        let f = format!(
            "{FH}\n{}\n",
            feat("PLF:0001", "ATGACGT", "MT", "cds", "reviewed", "L. Lobel")
        );
        let p = format!("{PH}\n{}\n", prov("PLF:0001"));
        let s = signoff(&f, &p, "PLF:0001", "reviewed", "L. Lobel");

        let (db, errs) = Db::parse(&f, &p, &s);
        assert!(errs.is_empty(), "the control must load clean: {errs:?}");
        assert_eq!(db.records[0].review_status, ReviewStatus::Reviewed);
        assert_eq!(db.reviewed().records.len(), 1);

        // One base changes. Nothing else moves -- same id, same name, same
        // curator, same signature line.
        let moved = f.replace("ATGACGT", "ATGACGA");
        assert_ne!(moved, f, "the mutation did not apply; the fixture moved");
        let (db, errs) = Db::parse(&moved, &p, &s);
        assert_eq!(
            db.records[0].review_status,
            ReviewStatus::Proposed,
            "a row whose sequence changed still shipped as reviewed"
        );
        assert!(
            db.records[0].curator.is_empty(),
            "the curator's name outlived their approval"
        );
        assert!(db.reviewed().records.is_empty());
        assert!(
            errs.iter()
                .any(|e| e.problem.contains("has changed since it was signed")),
            "the lapse was silent: {errs:?}"
        );

        // ...and so does a prose change with the sequence untouched. That is
        // not a nicety: ReviewStatus::Reviewed is defined as having written the
        // description from the primary source, so a row whose prose a machine
        // rewrote since a human read it is a row nobody has read.
        let reworded = f.replace("A description", "A different description");
        assert_ne!(reworded, f, "the mutation did not apply");
        let (db, _) = Db::parse(&reworded, &p, &s);
        assert_eq!(db.records[0].review_status, ReviewStatus::Proposed);

        // The inverted control, and the one most likely to be skipped: change
        // ONLY `date_added` and the signature must survive. Without this the
        // digest could quietly be defined over the whole row, which would break
        // on every single build -- build.py stamps today's date on every row
        // every run.
        let restamped = f.replace("2026-07-27", "2099-01-01");
        assert_ne!(restamped, f, "the mutation did not apply");
        let (db, errs) = Db::parse(&restamped, &p, &s);
        assert_eq!(
            db.records[0].review_status,
            ReviewStatus::Reviewed,
            "re-stamping the build clock lapsed a signature: {errs:?}"
        );
    }

    #[test]
    fn a_cell_this_loader_canonicalises_is_named_before_it_can_lapse_a_signature() {
        // The cross-language half of the digest, and the failure it has is the
        // one that looks like data corruption. `Db::parse` canonicalises before
        // it hashes — trim everywhere, lower-case for `class` and
        // `boundary_rule`, `misc_feature` for an empty `genbank_key` — while
        // `features/build/build.py` hashes the cell as written. So a curator who
        // leaves one trailing space gets a digest out of `build.py --show`,
        // `check_signoff.py` reports zero violations, and the shipped binary
        // says "the row has changed since it was signed" and clears their name.
        //
        // Every case below is a cell whose bytes differ and whose digest THIS
        // SIDE cannot tell apart — asserted, because that identity is exactly
        // what makes the row undiagnosable later, and a fixture that lost it
        // would be testing nothing. `aliases` and `patent_flag` are absent
        // because build.py canonicalises those two itself; the pin fixture in
        // tests/schema_pin.rs covers them.
        let mut base: Vec<String> = feat("PLF:0001", "ATGACGT", "MT", "cds", "proposed", "")
            .split('\t')
            .map(String::from)
            .collect();
        // Written as the value the loader stores, so that emptying it below is
        // a pure on-disk change and the digest stays put.
        base[4] = "misc_feature".into();
        let p = format!("{PH}\n{}\n", prov("PLF:0001"));
        let table = |cells: &[String]| format!("{FH}\n{}\n", cells.join("\t"));

        let (control, errs) = parse(&table(&base), &p);
        assert!(errs.is_empty(), "the control must load clean: {errs:?}");
        let want = control.content_digest(&control.records[0]);

        for (i, col, cell) in [
            (1usize, "name", "Test "),
            (3, "class", "CDS"),
            (4, "genbank_key", ""),
            (5, "reference_nt", "ATGACGT "),
            (6, "reference_aa", " MT"),
            (7, "boundary_rule", "ORF_ATG_TO_STOP"),
            (8, "boundary_evidence", "J01749.1:3293-4153:- "),
            (9, "description", "A description "),
            (14, "notes", " "),
        ] {
            let mut cells = base.clone();
            cells[i] = cell.into();
            assert_ne!(cells, base, "{col}: the perturbation did not apply");

            let (db, errs) = parse(&table(&cells), &p);
            assert_eq!(
                db.records.len(),
                1,
                "{col}: the row is still a real feature and must not be dropped"
            );
            assert_eq!(
                db.content_digest(&db.records[0]),
                want,
                "{col}: this fixture no longer exercises a divergence — the digest \
                 moved, so the two implementations would still agree"
            );
            assert!(
                errs.iter().any(|e| e.problem.starts_with(col)),
                "{col}: a cell build.py and this loader hash differently was accepted \
                 without a word: {errs:?}"
            );
        }

        // The control, and the reason the two reference columns are compared on
        // whitespace alone rather than on the stored spelling:
        // `build.py::content_digest` upper-cases `reference_nt` and
        // `reference_aa` itself, so a lower-case cell is NOT a divergence.
        // Reporting one would send a curator looking for a signature mismatch
        // that does not exist — and lower case is a cell a real table can
        // carry, since soft-masked sequence is ordinary sequence everywhere
        // else in this workspace (`align::same` matches case-insensitively for
        // exactly that reason).
        for (i, col, cell) in [
            (5usize, "reference_nt", "atgacgt"),
            (6, "reference_aa", "mt"),
        ] {
            let mut cells = base.clone();
            cells[i] = cell.into();
            let (db, errs) = parse(&table(&cells), &p);
            assert_eq!(db.records.len(), 1, "{col}");
            assert_eq!(
                db.content_digest(&db.records[0]),
                want,
                "{col}: case does not move this side's digest"
            );
            assert!(
                errs.is_empty(),
                "{col}: both implementations upper-case this column, so a lower-case \
                 cell is not a divergence and must not be reported as one: {errs:?}"
            );
        }
    }

    #[test]
    fn a_signature_can_only_ever_remove_trust() {
        // The governing invariant, exercised on every way the sign-off table
        // can be wrong. Each must resolve to `proposed`, never to accepted.
        let f = format!(
            "{FH}\n{}\n",
            feat("PLF:0001", "ATGACGT", "MT", "cds", "reviewed", "L. Lobel")
        );
        let p = format!("{PH}\n{}\n", prov("PLF:0001"));
        let good = signoff(&f, &p, "PLF:0001", "reviewed", "L. Lobel");

        let cases: Vec<(&str, String)> = vec![
            ("absent", String::new()),
            ("header only", format!("{SH}\n")),
            ("malformed header", "record_id\tstatus\n".into()),
            (
                "wrong curator",
                signoff(&f, &p, "PLF:0001", "reviewed", "Somebody Else"),
            ),
            (
                "claims a stronger status than the row does",
                signoff(&f, &p, "PLF:0001", "verified", "L. Lobel"),
            ),
            (
                "digest is not a digest",
                format!("{SH}\nPLF:0001\treviewed\tL. Lobel\t2026-07-28\tnope\tx\n"),
            ),
            (
                "digest is 64 hex characters of the wrong thing",
                format!(
                    "{SH}\nPLF:0001\treviewed\tL. Lobel\t2026-07-28\t{}\tx\n",
                    "0".repeat(64)
                ),
            ),
            // A file that says two things about one record says nothing about
            // it. This used to be last-wins — the problem was reported and the
            // line applied anyway — so a duplicated signature still GRANTED
            // trust: the one case where the code contradicted the invariant
            // this test is named for, in both languages, with the Rust error
            // text admitting it ("the second line replaces the first") three
            // lines below the docstring that denied it. BOTH lines below are
            // individually valid, so nothing but the duplication can reject it.
            ("signed twice, both lines valid", {
                let one = signoff(&f, &p, "PLF:0001", "reviewed", "L. Lobel");
                let line = one.lines().nth(1).expect("the signature line").to_string();
                format!("{one}{line}\n")
            }),
        ];
        for (label, s) in cases {
            let (db, _) = Db::parse(&f, &p, &s);
            assert_eq!(
                db.records[0].review_status,
                ReviewStatus::Proposed,
                "{label}: a bad sign-off table added trust"
            );
            assert!(db.reviewed().records.is_empty(), "{label}");
        }

        // The control: the good table really does grant it, so the seven
        // failures above are the mechanism working rather than the mechanism
        // being inert.
        let (db, errs) = Db::parse(&f, &p, &good);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(db.records[0].review_status, ReviewStatus::Reviewed);
    }

    #[test]
    fn a_signature_pointing_at_nothing_is_reported() {
        // The mirror of audit()'s orphan-provenance rule: a signature naming a
        // record that is not in the table is a silent lie about how much of it
        // a human has read.
        let f = format!(
            "{FH}\n{}\n",
            feat("PLF:0001", "ATGACGT", "MT", "cds", "proposed", "")
        );
        let p = format!("{PH}\n{}\n", prov("PLF:0001"));
        let s = format!(
            "{SH}\nPLF:9999\treviewed\tL. Lobel\t2026-07-28\t{}\tx\n",
            "a".repeat(64)
        );
        let (db, errs) = Db::parse(&f, &p, &s);
        assert_eq!(db.records.len(), 1);
        assert!(
            errs.iter()
                .any(|e| e.problem.contains("signs unknown record PLF:9999")),
            "{errs:?}"
        );
    }

    #[test]
    fn the_digest_covers_the_provenance_the_curator_read() {
        // "Checked the sequence against the cited accession" is what
        // ReviewStatus::Reviewed means, so the accession is part of what was
        // signed and re-pointing it must lapse the signature.
        let f = format!(
            "{FH}\n{}\n",
            feat("PLF:0001", "ATGACGT", "MT", "cds", "reviewed", "L. Lobel")
        );
        let p = format!("{PH}\n{}\n", prov("PLF:0001"));
        let s = signoff(&f, &p, "PLF:0001", "reviewed", "L. Lobel");

        let repointed = p.replace("AAB59737.1", "AAB59738.1");
        assert_ne!(repointed, p, "the mutation did not apply");
        let (db, _) = Db::parse(&f, &repointed, &s);
        assert_eq!(db.records[0].review_status, ReviewStatus::Proposed);

        // ...and the exclusions hold: a re-fetch that yields identical content
        // under the same accession must not invalidate a human's reading, so
        // `retrieved` and the fetch `sha256` are outside the digest.
        let refetched = p
            .replace("2026-07-27", "2027-01-01")
            .replace("abc123", "def456");
        assert_ne!(refetched, p, "the mutation did not apply");
        let (db, errs) = Db::parse(&f, &refetched, &s);
        assert_eq!(
            db.records[0].review_status,
            ReviewStatus::Reviewed,
            "a re-fetch lapsed a signature: {errs:?}"
        );
    }
}
