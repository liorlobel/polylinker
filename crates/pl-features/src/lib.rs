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

use std::collections::BTreeMap;

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
    /// exist. This is the only class translated matching can find.
    Cds,
    /// Promoters, terminators, operators, RBS. No automatable source gives a
    /// defensible boundary; depositors disagree with each other.
    Regulatory,
    Origin,
    Repeat,
    /// Tags, linkers, protease sites, 2A peptides, MCSs — designed, so the
    /// boundary is whatever the designing paper stipulated.
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
    /// `uniprot`, `ena`, `ncbi-nuccore`, `rfam`, `amrfinderplus`, `literature`,
    /// or `polylinker` for our own work.
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
    /// Nucleotide reference. Present for every class.
    pub reference_nt: Vec<u8>,
    /// Protein reference — [`Class::Cds`] only.
    ///
    /// This is what makes a codon-optimised marker findable at all: a humanised
    /// GFP and EGFP are identical proteins and can be ~70% identical in
    /// nucleotides, far below any threshold a nucleotide matcher would accept.
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
    pub fn len(&self) -> usize {
        self.reference_nt.len()
    }
    pub fn is_empty(&self) -> bool {
        self.reference_nt.is_empty()
    }
    /// Can translated matching find this?
    pub fn has_protein(&self) -> bool {
        self.reference_aa.as_ref().is_some_and(|p| !p.is_empty())
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

/// The shipped tables, compiled in.
///
/// `include_str!` rather than a file read: this crate does no I/O, and an
/// offline desktop tool should not depend on finding a data directory next to
/// its own executable.
const BUILTIN_FEATURES: &str = include_str!("../../../features/features.tsv");
const BUILTIN_PROVENANCE: &str = include_str!("../../../features/provenance.tsv");

impl Db {
    /// The database compiled into this binary.
    ///
    /// **Every row in it is [`ReviewStatus::Proposed`]**, so [`Db::reviewed`]
    /// returns an empty database and an annotator built on that finds nothing.
    /// This is the intended state and not a defect: the rows were assembled by
    /// machine from public sources and no human has checked one. Writing
    /// `AmpR` onto somebody's plasmid map is an assertion, and the rule here is
    /// that the tool may propose and never assert.
    ///
    /// A caller that wants the proposed rows has to ask for them by name, and
    /// owes the user that same sentence.
    pub fn builtin() -> (Db, Vec<LoadError>) {
        Db::parse(BUILTIN_FEATURES, BUILTIN_PROVENANCE)
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

    /// Parse both tables.
    ///
    /// Every problem is reported rather than failing on the first, because a
    /// curator fixing a contributed file wants the whole list.
    pub fn parse(features: &str, provenance: &str) -> (Db, Vec<LoadError>) {
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

            let nt = get(5).to_ascii_uppercase().into_bytes();
            if nt.is_empty() {
                bad("reference_nt is empty".into());
                continue;
            }
            if let Some(b) = nt.iter().find(|c| !b"ACGTRYSWKMBDHVN".contains(c)) {
                bad(format!("{:?} is not a nucleotide code", *b as char));
                continue;
            }
            let aa = {
                let s = get(6).to_ascii_uppercase();
                if s.is_empty() {
                    None
                } else {
                    Some(s.into_bytes())
                }
            };
            // A protein reference on a non-coding feature is a category error,
            // and would put a promoter into the translated index.
            if aa.is_some() && class != Class::Cds {
                bad(format!(
                    "class {} carries a protein reference; only cds may",
                    class.as_str()
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

            db.records.push(Record {
                id: get(0),
                name: get(1),
                aliases: get(2)
                    .split('|')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                class,
                genbank_key: {
                    let g = get(4);
                    if g.is_empty() {
                        "misc_feature".into()
                    } else {
                        g
                    }
                },
                reference_nt: nt,
                reference_aa: aa,
                boundary_rule: rule,
                boundary_evidence: get(8),
                description: unescape(&get(9)),
                review_status: review,
                curator: get(11),
                date_added: get(12),
                patent_flag: matches!(get(13).as_str(), "1" | "true" | "yes"),
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

        errors.extend(db.audit());
        (db, errors)
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
        // The rule the whole schema exists for: a sequence with no stated
        // origin must never ship.
        for r in &self.records {
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

    /// The subset a release may ship: everything a human has signed off.
    pub fn reviewed(&self) -> Db {
        let records: Vec<Record> = self
            .records
            .iter()
            .filter(|r| r.review_status >= ReviewStatus::Reviewed)
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

    pub fn census(&self) -> BTreeMap<&'static str, usize> {
        let mut m = BTreeMap::new();
        for r in &self.records {
            *m.entry(r.review_status.as_str()).or_insert(0) += 1;
        }
        m
    }

    /// Colliding ids, and distinct records holding identical sequences.
    pub fn duplicates(&self) -> Vec<String> {
        let mut by_id: BTreeMap<&str, usize> = BTreeMap::new();
        let mut by_seq: BTreeMap<&[u8], Vec<&str>> = BTreeMap::new();
        for r in &self.records {
            *by_id.entry(&r.id).or_insert(0) += 1;
            by_seq.entry(&r.reference_nt).or_default().push(&r.id);
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

    fn feat(id: &str, nt: &str, aa: &str, class: &str, status: &str, curator: &str) -> String {
        format!("{id}\tTest\t\t{class}\tCDS\t{nt}\t{aa}\torf_atg_to_stop\tJ01749.1:3293-4153:-\tA description\t{status}\t{curator}\t2026-07-27\t0\t")
    }

    fn prov(id: &str) -> String {
        format!("{id}\treference_nt\tena\tAAB59737.1\tINSDC-free\thttps://www.ebi.ac.uk/ena/browser/api/fasta/AAB59737.1\t2026-07-27\tabc123")
    }

    #[test]
    fn a_well_formed_database_round_trips() {
        let f = format!(
            "#!version 2026.10\n{FH}\n{}\n",
            feat("PLF:0001", "ATGACGT", "MT", "cds", "reviewed", "L. Lobel")
        );
        let p = format!("{PH}\n{}\n", prov("PLF:0001"));
        let (db, errs) = Db::parse(&f, &p);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(db.version, "2026.10");
        assert_eq!(db.records.len(), 1);
        assert_eq!(db.provenance.len(), 1);

        let (f2, p2) = db.to_tsv();
        let (again, errs2) = Db::parse(&f2, &p2);
        assert!(errs2.is_empty(), "{errs2:?}");
        assert_eq!(again.records, db.records);
        assert_eq!(again.provenance, db.provenance);
    }

    #[test]
    fn a_sequence_with_no_stated_origin_is_refused() {
        // The single rule the whole schema exists to enforce.
        let f = format!(
            "{FH}\n{}\n",
            feat("PLF:0001", "ATGACGT", "", "cds", "proposed", "")
        );
        let (_, errs) = Db::parse(&f, &format!("{PH}\n"));
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
        let (db, errs) = Db::parse(&f, &format!("{PH}\n{}\n", prov("PLF:0001")));
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
        let (db, errs) = Db::parse(&f, &format!("{PH}\n{}\n", prov("PLF:0001")));
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
        let (db, errs) = Db::parse(&f, &p);
        assert!(errs.is_empty(), "{errs:?}");
        let ship = db.reviewed();
        assert_eq!(ship.records.len(), 1);
        // Provenance for the dropped record must go with it, or the release
        // carries attribution obligations for data it does not contain.
        assert_eq!(ship.provenance.len(), 1);
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
        let (db, errs) = Db::parse(&f, &p);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(db.provenance_of("PLF:0001").len(), 3);
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
        let f = format!(
            "{FH}\n{}\n",
            feat("PLF:0001", "ATGACGT", "MT", "regulatory", "proposed", "")
        );
        let (db, errs) = Db::parse(&f, &format!("{PH}\n{}\n", prov("PLF:0001")));
        assert!(db.records.is_empty());
        assert!(
            errs.iter().any(|e| e.problem.contains("only cds may")),
            "{errs:?}"
        );
    }

    #[test]
    fn a_boundary_without_evidence_is_refused() {
        let mut cells: Vec<String> = feat("PLF:0001", "ATGACGT", "", "cds", "proposed", "")
            .split('\t')
            .map(String::from)
            .collect();
        cells[8] = String::new();
        let (_, errs) = Db::parse(&format!("{FH}\n{}\n", cells.join("\t")), &format!("{PH}\n"));
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
        let (_, errs) = Db::parse(&f, &p);
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
        let (db, errs) = Db::parse(&f, &p);
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
        let (db, _) = Db::parse(&f, &format!("{PH}\n"));
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
        let (db, errs) = Db::parse(
            &format!("{FH}\n{}\n", cells.join("\t")),
            &format!("{PH}\n{}\n", prov("PLF:0001")),
        );
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(db.records[0].description, "line one\nline two\tindented");
        let (f2, p2) = db.to_tsv();
        let (again, _) = Db::parse(&f2, &p2);
        assert_eq!(again.records[0].description, db.records[0].description);
    }

    #[test]
    fn a_file_without_a_header_is_an_error_not_an_empty_database() {
        let (db, errs) = Db::parse("# just a comment\n", "");
        assert!(db.records.is_empty());
        assert!(
            errs.iter().any(|e| e.problem.contains("no header")),
            "{errs:?}"
        );
    }

    #[test]
    fn the_shipped_database_parses_and_ships_nothing() {
        // Two claims, and the second is a promise to Lior rather than a
        // property of the code: the compiled-in table is well formed, and
        // *none of it is approved*. Every row was assembled by machine from
        // public sources and no human has checked one against its accession.
        //
        // If a future change makes `reviewed()` non-empty without a curator
        // having signed those rows off, this fails — which is the point.
        // Putting `AmpR` on somebody's plasmid map is an assertion, and the
        // rule for this project is that it may propose and never assert.
        let (db, errors) = Db::builtin();
        assert!(
            errors.is_empty(),
            "the shipped table must parse: {errors:?}"
        );
        assert!(!db.records.is_empty(), "and it must not be empty");
        assert!(
            !db.version.is_empty(),
            "every annotation is stamped with it"
        );

        let counts = db.review_counts();
        assert_eq!(
            counts.get(&ReviewStatus::Proposed).copied(),
            Some(db.records.len()),
            "every row is still proposed: {counts:?}"
        );
        assert!(
            db.reviewed().records.is_empty(),
            "nothing is shippable until a named human signs each row off"
        );
        for r in &db.records {
            assert!(!r.reference_nt.is_empty(), "{} has no sequence", r.id);
            assert!(r.id.starts_with("PLF:"), "{} is not one of ours", r.id);
        }
    }
}
