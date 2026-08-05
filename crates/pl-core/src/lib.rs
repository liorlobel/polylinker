//! Polylinker's sequence model.
//!
//! No I/O and no dependencies: everything that could be wrong *about a
//! molecule* lives here, so it can be tested without a GUI, a file, or a
//! network. Formats are somebody else's crate's problem.
//!
//! # Coordinates
//!
//! The model is **1-based inclusive**, matching both GenBank and the SnapGene
//! container, so neither reader has to shift on the way in. Conversion to
//! 0-based half-open happens only where an external convention demands it,
//! and is always spelled out at that site.

pub mod base64;
pub mod ed25519;
pub mod iupac;
pub mod oplog;
pub mod orf;
pub mod seguid;
pub mod sha1;
pub mod sha256;
pub mod sha512;
pub mod translate;

pub use iupac::{complement, matches, reverse_complement, Composition};
pub use oplog::{OpKind, OpLog};
pub use orf::{find_orfs, stopless_frames, Orf};
pub use seguid::{cdseguid, csseguid, ldseguid, lsseguid};

/// Whether the molecule's ends join.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Topology {
    #[default]
    Linear,
    Circular,
}

impl Topology {
    pub fn is_circular(self) -> bool {
        matches!(self, Topology::Circular)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Topology::Linear => "linear",
            Topology::Circular => "circular",
        }
    }
}

/// Which strand a feature is annotated on.
///
/// `Unoriented` is a real state that SnapGene stores and GenBank cannot
/// express — a GenBank feature is either plain or wrapped in `complement()`.
/// Modelling it honestly means the loss is visible at the boundary where it
/// happens instead of being silently invented earlier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strand {
    #[default]
    Forward,
    Reverse,
    Unoriented,
    Both,
}

impl Strand {
    /// SnapGene's `directionality` attribute.
    pub fn from_directionality(d: Option<u32>) -> Self {
        match d {
            Some(1) => Strand::Forward,
            Some(2) => Strand::Reverse,
            Some(3) => Strand::Both,
            _ => Strand::Unoriented,
        }
    }
    pub fn to_directionality(self) -> Option<u32> {
        match self {
            Strand::Forward => Some(1),
            Strand::Reverse => Some(2),
            Strand::Both => Some(3),
            Strand::Unoriented => None,
        }
    }
    pub fn is_reverse(self) -> bool {
        matches!(self, Strand::Reverse)
    }
}

/// One contiguous span of a feature. 1-based inclusive.
///
/// A feature with more than one segment is a join: an intron-split CDS, or a
/// span that crosses the origin of a circular molecule.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start: u64,
    pub end: u64,
    pub color: Option<String>,
    /// Does this segment carry a protein reading?
    ///
    /// SnapGene's per-segment `translated` bit, and the only per-segment
    /// spelling of the question anywhere in the model. GenBank has no way to
    /// say it, so a `.gb` save loses it and a reader has to fall back on
    /// `Feature::kind == "CDS"`.
    pub translated: bool,
    pub kind: String,
}

impl Segment {
    pub fn new(start: u64, end: u64) -> Self {
        Segment {
            start,
            end,
            color: None,
            translated: false,
            kind: "standard".into(),
        }
    }
    /// Number of bases covered, reading left to right. **Zero** if the span is
    /// inverted.
    ///
    /// `saturating_sub` alone returned 1 for an inverted span while
    /// `is_empty()` returned true and this comment said 0 — three answers to
    /// one question. An inverted span on a *circular* molecule is a wrap and
    /// its real length is `n - start + 1 + end`, which needs the molecule and
    /// so cannot be computed here; a caller holding one should ask the
    /// molecule. The saturating arithmetic is load-bearing either way: a
    /// hostile `.dna` can set `end` to `u64::MAX`, and `end - start + 1` would
    /// panic on it.
    pub fn len(&self) -> u64 {
        if self.end < self.start {
            0
        } else {
            (self.end - self.start).saturating_add(1)
        }
    }
    pub fn is_empty(&self) -> bool {
        self.end < self.start
    }
}

/// An annotation on the molecule.
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    pub name: String,
    /// GenBank feature key: `CDS`, `promoter`, `misc_feature`, ...
    pub kind: String,
    pub strand: Strand,
    pub segments: Vec<Segment>,
    /// Ordered and allowed to repeat, because GenBank qualifiers are.
    ///
    /// The value is `None` for a **valueless** qualifier — `/pseudo`,
    /// `/ribosomal_slippage`, `/trans_splicing` — which GenBank writes bare,
    /// with no `=`. That is a different thing from `/replace=""`, an empty
    /// *value*, and conflating the two is not cosmetic: collapsing them made
    /// the writer drop every valueless qualifier, so a `/pseudo` gene came back
    /// as an ordinary protein-coding one. There are 11,716 valueless
    /// qualifiers across this project's 328-file GenBank corpus against 4
    /// empty-valued ones, so the common case was the one being lost.
    pub qualifiers: Vec<(String, Option<String>)>,
}

impl Feature {
    pub fn new(name: impl Into<String>, kind: impl Into<String>) -> Self {
        Feature {
            name: name.into(),
            kind: kind.into(),
            strand: Strand::Forward,
            segments: Vec::new(),
            qualifiers: Vec::new(),
        }
    }

    /// The **minimum of the segment starts**, which is not the same thing as
    /// where the feature begins.
    ///
    /// Use [`extent`](Self::extent) for anything a user or a machine will
    /// read. Two shapes make this number a lie, and both arrive from ordinary
    /// files:
    ///
    /// - a single origin-crossing segment `2677..7` begins at 2677, and this
    ///   returns 2677 — the *highest* coordinate it names, not the lowest;
    /// - the same span written as a GenBank join, `join(2677..2686,1..7)`,
    ///   which is what `genbank::write` emits and `genbank::parse` reads back,
    ///   always has a part starting at base 1, so this returns exactly 1 and
    ///   [`end`](Self::end) returns the molecule length. A 17 bp promoter then
    ///   reports as spanning a whole 2,686 bp plasmid, and
    ///   `Rotate { origin: f.start() }` becomes a guaranteed no-op.
    pub fn start(&self) -> u64 {
        self.segments.iter().map(|s| s.start).min().unwrap_or(0)
    }
    /// The **maximum of the segment ends**. See [`start`](Self::start) for why
    /// that is not where the feature ends, and use [`extent`](Self::extent)
    /// instead.
    pub fn end(&self) -> u64 {
        self.segments.iter().map(|s| s.end).max().unwrap_or(0)
    }

    /// Where this feature begins and ends, read the way [`Molecule::subseq`]
    /// reads a pair: `end < start` means the span crosses the origin.
    ///
    /// `None` only when the feature has no segments at all.
    ///
    /// This exists because [`start`](Self::start)/[`end`](Self::end) are a
    /// min/max over the segments and collapse to `(1, span)` for any
    /// origin-crossing feature in its GenBank join form — a shape every
    /// save-and-reopen through `genbank::write` produces, so the answer was
    /// format-dependent. Both call sites are still correct for an ordinary
    /// multi-exon join (`join(100..200,300..400)` really does run 100..400),
    /// which is why they are not simply replaced.
    ///
    /// For a feature with introns *and* an origin crossing the result is an
    /// outer bound: the pair covers the intervening bases the segments do not.
    /// Draw from `segments`; use this to say where a feature is.
    pub fn extent(&self, span: u64, circular: bool) -> Option<(u64, u64)> {
        let first = self.segments.first()?;
        let last = self.segments.last()?;
        if self.segments.len() == 1 {
            return Some((first.start, first.end));
        }
        // GenBank has exactly one spelling for a span that crosses the origin:
        // a join whose penultimate part ends at the last base and whose final
        // part begins at base 1. `location_parts` emits that and nothing else,
        // so recognising it here is recognising our own output. The
        // `last.end < first.start` test is what keeps an ordinary join that
        // merely happens to start at base 1 out of this branch.
        if circular && span > 0 && last.start == 1 && last.end < first.start {
            let prev = &self.segments[self.segments.len() - 2];
            if prev.end == span {
                return Some((first.start, last.end));
            }
        }
        Some((self.start(), self.end()))
    }
    /// First colour any segment carries.
    pub fn color(&self) -> Option<&str> {
        self.segments.iter().find_map(|s| s.color.as_deref())
    }
    /// The value of a qualifier, or `None` if it is absent **or valueless**.
    ///
    /// Use [`Feature::has_qualifier`] to ask whether it is present at all —
    /// for `/pseudo` that is the entire question.
    pub fn qualifier(&self, key: &str) -> Option<&str> {
        self.qualifiers
            .iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.as_deref())
    }
    /// Is this qualifier present, with or without a value?
    pub fn has_qualifier(&self, key: &str) -> bool {
        self.qualifiers.iter().any(|(k, _)| k == key)
    }
    pub fn set_qualifier(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.qualifiers.push((key.into(), Some(value.into())));
    }
    /// Record a qualifier that GenBank writes bare, such as `/pseudo`.
    pub fn set_flag_qualifier(&mut self, key: impl Into<String>) {
        self.qualifiers.push((key.into(), None));
    }
}

/// Where a primer anneals.
#[derive(Debug, Clone, PartialEq)]
pub struct BindingSite {
    pub start: u64,
    pub end: u64,
    pub strand: Strand,
    /// As recorded by the source file. Recomputing it is `pl-thermo`'s job,
    /// and the two are deliberately kept apart so a disagreement is visible.
    pub tm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Primer {
    pub name: String,
    pub seq: String,
    pub description: String,
    pub sites: Vec<BindingSite>,
}

/// One entry of a source file's metadata block: a name, its text, and the named
/// parts the source recorded *beside* that text.
///
/// This was `(String, String)` and lost half of a field. `.dna` block 6 splits
/// one value across both halves — `<Created UTC="22:0:0">2022.12.13</Created>`
/// is a single timestamp whose date is the element text and whose time of day is
/// an attribute — and a pair can hold the date or the time, not both. The reader
/// kept the date, so by the time anything could ask for the time it was gone,
/// and `snapgene::from_molecule` re-emitted `<Created>2022.12.13</Created>`: a
/// `.dna` in, a `.dna` out, and the recorded creation time absent from the
/// second one with nothing printed. See docs/DNA-FORMAT.md §7.
///
/// Encoding the attribute into the key instead — `"Created UTC=22:0:0"` — would
/// have been cheaper and is why this is a type change rather than a string hack:
/// `pl-scan`, `pl-wasm` and the GUI's notes grid all render the key verbatim, so
/// that syntax would have been displayed raw to the user as the note's name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Note {
    pub key: String,
    /// The element's own text.
    ///
    /// Empty is a real state and not the same as absent: `<Empty/>` and
    /// `<Created UTC="21:55:7"/>` carry no text at all, and the reader that
    /// waited for a text event to create the note emitted nothing for either, so
    /// a note the file contained had no representation to be empty *in*.
    pub value: String,
    /// Named parts recorded alongside `value`, in file order.
    ///
    /// A `Vec` rather than a map because the source's order is not stable and
    /// there is no reason to normalise it: two `.dna` files written by the same
    /// SnapGene install spell one `<Reference>`'s attributes
    /// `title,pubMedID,journal,authors` and `authors,pubMedID,title,journal`, so
    /// a map would silently reorder one of them on rewrite.
    ///
    /// Set only by the SnapGene reader. `Molecule::notes` has exactly one
    /// producer in this workspace, and block 6 is the only metadata block in any
    /// format read here that puts half a field in an attribute — GenBank and
    /// FASTA never populate `notes` at all, they only read it — so this is empty
    /// for every molecule that did not come from a `.dna`.
    pub attrs: Vec<(String, String)>,
}

impl Note {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Note {
            key: key.into(),
            value: value.into(),
            attrs: Vec::new(),
        }
    }
    /// The value of one attribute, or `None` if it is absent.
    ///
    /// First-wins if the name repeats, matching [`Molecule::note`]. A repeat can
    /// only arrive from a file — `set_attr` below cannot produce one — and XML
    /// forbids it, so there is no second value this could legitimately prefer.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    /// Set one attribute, replacing an existing entry of that name **in place**.
    ///
    /// It appended, which made a method named `set_` the shortest route to a
    /// state XML has no spelling for: two `set_attr("UTC", ..)` calls left
    /// `attrs` holding `UTC` twice, `attr("UTC")` answered with the first and
    /// stale one, and `snapgene::notes_xml` wrote
    /// `<Created UTC="22:0:0" UTC="09:00:00">` — markup that violates XML 1.0's
    /// "Unique Att Spec" well-formedness constraint, so the reference
    /// implementation's `ET.fromstring` refuses the whole block. Replacing in
    /// place keeps the position the caller first chose, because block 6's
    /// attribute order is not stable and reordering on edit is a diff nobody
    /// asked for.
    pub fn set_attr(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        match self.attrs.iter_mut().find(|(k, _)| *k == name) {
            Some(slot) => slot.1 = value,
            None => self.attrs.push((name, value)),
        }
    }
}

/// Methylation state, which blocks some restriction enzymes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Methylation {
    pub dam: bool,
    pub dcm: bool,
    pub ecoki: bool,
    /// CpG methylation (`CG`, C5 on both strands).
    ///
    /// Not a flag the `.dna` container carries, so it defaults to false and is
    /// set by the caller — a plasmid grown in an ordinary *E. coli* is not CpG
    /// methylated, but one passed through a mammalian cell line, or treated
    /// with M.SssI, is.
    ///
    /// Added because it turns out to dominate: of the 34 (enzyme, methylase)
    /// pairs that block or impair cleavage across this project's 50 enzymes,
    /// **26 are CpG**. A methylation model without it is missing three
    /// quarters of the cases it exists to catch.
    pub cpg: bool,
}

/// A coordinate that does not describe anything real.
///
/// See [`Molecule::validate`] for why these are possible at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    /// `end < start` on a **linear** molecule, which has no origin to cross.
    ///
    /// On a circle the same shape is a legitimate wrap — `Molecule::subseq`,
    /// the annotator and the SVG renderer all read it that way — so it is not
    /// reported there. Reporting it was a contradiction that made `rotate`
    /// refuse roughly a third of real rotations.
    Inverted { what: String, start: u64, end: u64 },
    /// An endpoint of 0. Coordinates are 1-based, so there is no position 0.
    ///
    /// Reported for **either** endpoint. Checking only `start` left `end == 0`
    /// clean, and that window is reachable off disk: on a circle `end < start`
    /// is a legal wrap and `0 <= n` is not past the end, so a `.dna` carrying
    /// `<Segment range="5-0"/>` on a 16 bp plasmid validated clean while
    /// `Molecule::subseq(5, 0)` refused the very same span, and
    /// `genbank::write` emitted `misc_feature join(5..16,1..0)` — not a legal
    /// GenBank location, and Biopython "fixes" it into a feature covering the
    /// whole molecule. `what` therefore names the endpoint, which is why the
    /// `Display` text below cannot say "starts".
    ZeroStart { what: String },
    /// The coordinate is past the end of the molecule.
    PastEnd { what: String, end: u64, len: u64 },
    /// A feature that annotates nowhere.
    FeatureWithoutSegments { index: usize, name: String },
    /// The file declares one length and carries a different number of bases.
    ///
    /// Reading text with `from_utf8_lossy` turns one invalid byte into U+FFFD,
    /// three bytes the base filter then accepts: a 12 bp record read as 14 bp,
    /// and every feature after that point pointed at the wrong bases while
    /// `validate()` returned clean. Only checked when bases are present, since
    /// annotation-only GenBank declares a length and ships none by design.
    LengthMismatch { declared: u64, actual: u64 },
}

impl Invalid {
    /// A stable name for *what kind of* problem this is.
    ///
    /// The operation log needs to compare the problems before an edit against
    /// the problems after it, and `Invalid` values themselves cannot be
    /// compared for that purpose: every variant embeds data that moves under
    /// ordinary editing. `PastEnd` carries the molecule length, so any
    /// length-changing insert makes an untouched problem look new. `what`
    /// embeds both the feature index and its name, so removing feature 0 or
    /// renaming a feature does the same. Comparing values directly therefore
    /// refuses perfectly reasonable edits to a file that arrived broken.
    ///
    /// Comparing *counts per kind* is stable under all of those, and still
    /// catches an edit that trades one problem for a different one — which the
    /// old "did the total go up?" test let through, and which is exactly how a
    /// reverse-complement could commit a wrapped coordinate.
    pub fn kind(&self) -> &'static str {
        match self {
            Invalid::Inverted { .. } => "inverted",
            Invalid::ZeroStart { .. } => "zero start",
            Invalid::PastEnd { .. } => "past the end",
            Invalid::FeatureWithoutSegments { .. } => "feature without segments",
            Invalid::LengthMismatch { .. } => "declared length disagrees",
        }
    }
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Invalid::Inverted { what, start, end } => {
                write!(f, "{what}: {start}..{end} ends before it starts")
            }
            Invalid::ZeroStart { what } => {
                // Endpoint-agnostic, because `what` may name either end: the
                // old "starts at 0" read "segment 0 end: starts at 0".
                write!(f, "{what}: names position 0, but coordinates are 1-based")
            }
            Invalid::PastEnd { what, end, len } => {
                write!(f, "{what}: {end} is past the {len} bp molecule")
            }
            Invalid::FeatureWithoutSegments { index, name } => {
                write!(f, "feature {index} '{name}': has no segments")
            }
            Invalid::LengthMismatch { declared, actual } => {
                write!(f, "the file declares {declared} bases but carries {actual}")
            }
        }
    }
}

/// A nucleic-acid molecule with its annotations.
#[derive(Debug, Clone, Default)]
pub struct Molecule {
    pub name: String,
    pub description: String,
    /// Raw bases, **case preserved**. Not guaranteed to be valid IUPAC.
    pub seq: Vec<u8>,
    pub topology: Topology,
    /// `None` when the source does not record it — which is the normal case for
    /// GenBank and always the case for FASTA.
    ///
    /// Defaulting this to `false` made the viewer assert "single-stranded" about
    /// ordinary plasmids, and defaulting to `true` would invent information just
    /// as confidently. Unknown is a real third state and callers should say so.
    pub double_stranded: Option<bool>,
    pub methylation: Methylation,
    pub features: Vec<Feature>,
    pub primers: Vec<Primer>,
    /// The source file's metadata block, in file order. See [`Note`].
    ///
    /// Ordered, and duplicate keys are possible: block 6 is a list of elements,
    /// not a map, and nothing in it is schema-constrained. The Python oracle
    /// models it as a `dict` and therefore keeps the *last* of a repeated tag
    /// where this keeps every one of them.
    pub notes: Vec<Note>,
    /// Length claimed by the source when the bases themselves are absent.
    ///
    /// Annotation-only GenBank is real and common: `ORIGIN` immediately
    /// followed by `//`, with the length declared only on the LOCUS line.
    /// Recording it separately keeps "we have 2.9 Mb of bases" distinct from
    /// "the file says 2.9 Mb but shipped none", which is a distinction other
    /// tools blur.
    pub declared_len: Option<u64>,
}

impl Molecule {
    /// Number of bases actually present.
    pub fn len(&self) -> u64 {
        self.seq.len() as u64
    }
    pub fn is_empty(&self) -> bool {
        self.seq.is_empty()
    }
    /// The span annotations are drawn against: real bases if we have them,
    /// otherwise the length the file declared.
    pub fn span(&self) -> u64 {
        if self.seq.is_empty() {
            self.declared_len.unwrap_or(0)
        } else {
            self.len()
        }
    }
    /// True when the file described a molecule but carried no bases.
    pub fn sequence_absent(&self) -> bool {
        self.seq.is_empty() && self.declared_len.unwrap_or(0) > 0
    }

    /// The **text** of the first note with this key, or `None` if there is none.
    ///
    /// First-wins, deliberately, and written down here because it was previously
    /// an undocumented accident implemented twice: `snapgene::parse` takes the
    /// first `Description` and `genbank::write` the first `UUID`, and
    /// `snapgene::notes_xml`'s de-duplication of `Description` is written
    /// against that assumption. `notes` can hold the same key twice — see the
    /// field's own comment — and a caller that needs all of them iterates it.
    ///
    /// Returns the text only. A note whose value lives in an attribute, such as
    /// the time of day on `<Created UTC="22:0:0">`, is reached through
    /// [`Note::attr`]; this deliberately does not merge the two, because
    /// whatever it returned would have to disagree with what `notes_xml` writes
    /// back out.
    pub fn note(&self, key: &str) -> Option<&str> {
        self.notes
            .iter()
            .find(|n| n.key == key)
            .map(|n| n.value.as_str())
    }

    /// True for a standalone annotation track: features, but neither bases nor
    /// a declared length. UGENE and SnapGene both export these — a set of
    /// coordinates meant to be applied to a sequence held somewhere else.
    pub fn is_annotation_track(&self) -> bool {
        self.seq.is_empty() && self.declared_len.unwrap_or(0) == 0 && !self.features.is_empty()
    }

    /// A span to draw annotations against, inferring one from the features
    /// when the file supplied neither bases nor a length.
    ///
    /// Separate from [`span`](Self::span) because this one *infers*. Anything
    /// writing a file header must use `span`, which never invents a number.
    pub fn annotation_span(&self) -> u64 {
        let s = self.span();
        if s > 0 {
            return s;
        }
        self.features.iter().map(Feature::end).max().unwrap_or(0)
    }
    pub fn composition(&self) -> Composition {
        Composition::of(&self.seq)
    }
    pub fn gc_percent(&self) -> Option<f64> {
        self.composition().gc_percent()
    }

    /// Bases in `[start, end]`, 1-based inclusive, wrapping the origin when
    /// the molecule is circular. Returns `None` if the range is unusable.
    pub fn subseq(&self, start: u64, end: u64) -> Option<Vec<u8>> {
        let n = self.len();
        if n == 0 || start == 0 || start > n {
            return None;
        }
        if end >= start {
            if end > n {
                return None;
            }
            return Some(self.seq[(start - 1) as usize..end as usize].to_vec());
        }
        // end < start: only meaningful as an origin-crossing span.
        if !self.topology.is_circular() || end == 0 {
            return None;
        }
        let mut v = self.seq[(start - 1) as usize..].to_vec();
        v.extend_from_slice(&self.seq[..end as usize]);
        Some(v)
    }

    /// Check every coordinate against the molecule it describes.
    ///
    /// This exists because of a deliberate trade recorded in `docs/PLAN.md`
    /// §5.3.1. Coordinates here are 1-based inclusive `{start, end}`, chosen so
    /// that conversion to and from GenBank and the SnapGene container is the
    /// identity. The cost is that an invalid interval is *constructible* —
    /// `end < start`, `start == 0`, `end` past the sequence — where the
    /// alternative `{start, length}` representation would have made it
    /// unrepresentable. That safety has to be bought back explicitly, and this
    /// is where it is bought.
    ///
    /// Note what is *not* an error: a feature made of several segments. That is
    /// how both source formats express an intron, a fusion, and a span crossing
    /// the origin.
    pub fn validate(&self) -> Vec<Invalid> {
        let mut out = Vec::new();
        // Annotations are checked against the span they annotate, which for a
        // file carrying no bases is the length it declares.
        let n = self.annotation_span();

        // On a circle, `end < start` is not a mistake — it is a feature running
        // across the origin, and the rest of this codebase already reads it
        // that way: `Molecule::subseq`, the annotator, and the SVG renderer all
        // treat it as a wrap. Calling the same shape invalid here was a
        // contradiction with real consequences: `rotate` produces exactly this
        // for any feature straddling the new origin, and 23 of 78 rotations of
        // 13 real corpus plasmids were therefore refused outright by the
        // operation log, making "set origin" impossible on roughly a third of
        // rotations.
        //
        // A linear molecule has no origin to cross, so there it stays invalid.
        let wraps_are_legal = self.topology.is_circular();

        for (i, f) in self.features.iter().enumerate() {
            if f.segments.is_empty() {
                out.push(Invalid::FeatureWithoutSegments {
                    index: i,
                    name: f.name.clone(),
                });
                continue;
            }
            for (j, s) in f.segments.iter().enumerate() {
                if s.start == 0 {
                    out.push(Invalid::ZeroStart {
                        what: format!("feature {i} '{}' segment {j}", f.name),
                    });
                }
                // BOTH endpoints, for the same reason as the `PastEnd` pair
                // below. `end == 0` slipped through every other check on a
                // circle: `start != 0`, `end < start` is a legal wrap there,
                // and `0 <= n` is not past the end. `<Segment range="5-0"/>`
                // parses verbatim out of a `.dna`, and `validate()` called it
                // clean while `subseq(5, 0)` refused the span, `pl_draw::ranges`
                // fabricated a 1 bp arc at base 1 under the feature's name, and
                // `genbank::write` emitted `join(5..16,1..0)` with nothing on
                // `unwritable` and exit 0.
                if s.end == 0 {
                    out.push(Invalid::ZeroStart {
                        what: format!("feature {i} '{}' segment {j} end", f.name),
                    });
                }
                if s.end < s.start && !wraps_are_legal {
                    out.push(Invalid::Inverted {
                        what: format!("feature {i} '{}' segment {j}", f.name),
                        start: s.start,
                        end: s.end,
                    });
                }
                // BOTH endpoints, not just the end.
                //
                // Checking `end` alone leaves one window uncovered, and it is
                // constructible: circular topology, `start > n`, and
                // `end < start` (which the wrap rule above declares legal), so
                // no `Inverted`; and `end <= n`, so no `PastEnd`. A `.dna`
                // carrying `<Segment range="99999-100"/>` on a 5,386 bp plasmid
                // landed in exactly that window — validate() returned clean,
                // the GUI reported no coordinate problem, `subseq` refused the
                // span, and `pl_draw::ranges` clamped the start and drew a
                // fabricated 101 bp arc under the real feature's name. A
                // fabrication is worse than a loss.
                if n > 0 && s.start > n {
                    out.push(Invalid::PastEnd {
                        what: format!("feature {i} '{}' segment {j} start", f.name),
                        end: s.start,
                        len: n,
                    });
                }
                if n > 0 && s.end > n {
                    out.push(Invalid::PastEnd {
                        what: format!("feature {i} '{}' segment {j}", f.name),
                        end: s.end,
                        len: n,
                    });
                }
            }
        }

        // Guarded on non-empty: annotation-only GenBank declares a length and
        // deliberately ships no bases, and flagging that would break a
        // supported class of file.
        if let Some(declared) = self.declared_len {
            if !self.seq.is_empty() && declared != self.seq.len() as u64 {
                out.push(Invalid::LengthMismatch {
                    declared,
                    actual: self.seq.len() as u64,
                });
            }
        }

        for (i, p) in self.primers.iter().enumerate() {
            for (j, s) in p.sites.iter().enumerate() {
                if s.start == 0 {
                    out.push(Invalid::ZeroStart {
                        what: format!("primer {i} '{}' site {j}", p.name),
                    });
                }
                // The mirror of the segment check above. `snapgene.rs` cannot
                // hand a binding site a 0 (it adds 1 to the file's 0-based
                // `location`), so this is as latent as the `start == 0` check
                // beside it — but `from_molecule_reporting` already refuses to
                // write a `9..0` site, so the model must be able to say why.
                if s.end == 0 {
                    out.push(Invalid::ZeroStart {
                        what: format!("primer {i} '{}' site {j} end", p.name),
                    });
                }
                if s.end < s.start && !wraps_are_legal {
                    out.push(Invalid::Inverted {
                        what: format!("primer {i} '{}' site {j}", p.name),
                        start: s.start,
                        end: s.end,
                    });
                }
                if n > 0 && s.start > n {
                    out.push(Invalid::PastEnd {
                        what: format!("primer {i} '{}' site {j} start", p.name),
                        end: s.start,
                        len: n,
                    });
                }
                if n > 0 && s.end > n {
                    out.push(Invalid::PastEnd {
                        what: format!("primer {i} '{}' site {j}", p.name),
                        end: s.end,
                        len: n,
                    });
                }
            }
        }
        out
    }

    /// True when nothing in [`validate`](Self::validate) objects.
    pub fn is_valid(&self) -> bool {
        self.validate().is_empty()
    }

    /// Features whose strand GenBank cannot express.
    ///
    /// A GenBank location is either plain or wrapped in `complement()`, so
    /// `Unoriented` and `Both` have nowhere to go and are written as forward.
    /// That is a real loss and, for roughly half of them, a biologically wrong
    /// claim — `cat` and `TcR` on pACYC184 are on opposite strands, and an
    /// export that calls both forward publishes something untrue. 47 features
    /// across this project's own corpus are affected.
    ///
    /// The loss is unavoidable at that boundary; being silent about it is not.
    /// `Strand`'s own documentation says the loss should be "visible at the
    /// boundary where it happens", and this is how a caller sees it without
    /// the writer inventing a marker qualifier that would then be re-read as
    /// data on the next import.
    pub fn features_without_expressible_orientation(&self) -> Vec<(usize, &Feature)> {
        self.features
            .iter()
            .enumerate()
            .filter(|(_, f)| matches!(f.strand, Strand::Unoriented | Strand::Both))
            .collect()
    }

    /// Rotate a circular molecule so that 1-based position `origin` becomes 1,
    /// moving every annotation with it. No-op on a linear molecule.
    ///
    /// A coordinate that does not name a real base is left exactly where it is
    /// rather than being moved onto one; see the note on `remap` below for the
    /// fabrication that prevents.
    pub fn rotate(&mut self, origin: u64) -> bool {
        let n = self.len();
        if !self.topology.is_circular() || n == 0 || origin == 0 || origin > n {
            return false;
        }
        let shift = origin - 1;
        if shift == 0 {
            return true;
        }
        self.seq.rotate_left(shift as usize);
        // The two out-of-range directions are NOT symmetric, and treating them
        // as one `clamp(1, n)` was a fabrication.
        //
        // Below 1: `p - 1` underflowed on `start == 0`, which the SnapGene
        // reader can produce and deliberately carries through rather than
        // dropping (`<Segment range="0-4"/>`). In debug that panicked; under the
        // wasm profile, which disables overflow checks and aborts on panic, it
        // instead silently relocated the annotation somewhere else entirely.
        // Raising it to 1 is defensible because 0 is *adjacent* to a real
        // coordinate: base 0 does not exist, so `0-4` describes the four bases
        // 1..4, and raising preserves the span's *length*, which is the property
        // a reader would notice going wrong.
        //
        // Past the end: there is no such adjacency and nothing to preserve.
        // Clamping 999 down to n on a 16 bp circle turned segment `4..999` —
        // which `validate()` reports as `PastEnd { end: 999, len: 16 }` — into
        // `16..12`, a perfectly legal 13 bp wrap, so the defect report vanished
        // and the operation log committed the edit (its gate only refuses
        // problem kinds whose count went *up*). Worse, `20..25`, where both ends
        // are past the end, collapsed to a 1 bp segment at base 12 — a base the
        // feature never named. A coordinate past the end names no base, so
        // rotating it onto one invents coverage; it is left alone, which keeps
        // `validate()` reporting it and keeps the loss visible.
        let remap = |p: u64| -> u64 {
            if p > n {
                return p;
            }
            let p = p.max(1);
            ((p - 1 + n - shift) % n) + 1
        };
        for f in &mut self.features {
            for s in &mut f.segments {
                s.start = remap(s.start);
                s.end = remap(s.end);
            }
        }
        for p in &mut self.primers {
            for s in &mut p.sites {
                s.start = remap(s.start);
                s.end = remap(s.end);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circular(seq: &[u8]) -> Molecule {
        Molecule {
            seq: seq.to_vec(),
            topology: Topology::Circular,
            ..Default::default()
        }
    }

    #[test]
    fn subseq_is_one_based_inclusive() {
        let m = circular(b"ACGTACGT");
        assert_eq!(m.subseq(1, 4).unwrap(), b"ACGT".to_vec());
        assert_eq!(m.subseq(4, 4).unwrap(), b"T".to_vec());
        assert!(m.subseq(0, 4).is_none(), "position 0 does not exist");
        assert!(m.subseq(1, 9).is_none(), "runs off the end");
    }

    #[test]
    fn subseq_wraps_only_when_circular() {
        let c = circular(b"ACGTACGT");
        assert_eq!(c.subseq(7, 2).unwrap(), b"GTAC".to_vec());
        let mut l = c.clone();
        l.topology = Topology::Linear;
        assert!(l.subseq(7, 2).is_none());
    }

    #[test]
    fn rotate_moves_sequence_and_annotations_together() {
        let mut m = circular(b"AAAACCCCGGGGTTTT");
        let mut f = Feature::new("gg", "misc_feature");
        f.segments.push(Segment::new(9, 12)); // the GGGG
        m.features.push(f);

        assert_eq!(m.subseq(9, 12).unwrap(), b"GGGG".to_vec());
        assert!(m.rotate(9));
        assert_eq!(&m.seq[..4], b"GGGG");
        // the annotation followed the bases it describes
        let s = &m.features[0].segments[0];
        assert_eq!((s.start, s.end), (1, 4));
        assert_eq!(m.subseq(s.start, s.end).unwrap(), b"GGGG".to_vec());
    }

    #[test]
    fn a_rotation_that_wraps_a_feature_is_still_a_valid_molecule() {
        // 23 of 78 rotations of 13 real corpus plasmids used to leave the
        // molecule "invalid", because a feature straddling the new origin comes
        // out `end < start` and `validate()` called that a mistake — while
        // `subseq`, the annotator and the renderer all read the same shape as a
        // wrap. The operation log refused all 23, so "set origin" was
        // impossible on roughly a third of rotations.
        let mut m = circular(b"AAAACCCCGGGGTTTT");
        let mut f = Feature::new("gg", "misc_feature");
        f.segments.push(Segment::new(9, 12)); // GGGG
        m.features.push(f);

        assert!(m.rotate(11));
        let s = &m.features[0].segments[0];
        assert!(s.end < s.start, "this rotation should wrap the feature");
        assert!(
            m.is_valid(),
            "a wrap on a circle is not a defect: {:?}",
            m.validate()
        );
        // ...and it still names the bases it named before.
        assert_eq!(m.subseq(s.start, s.end).unwrap(), b"GGGG".to_vec());
    }

    #[test]
    fn a_wrapped_span_on_a_linear_molecule_is_still_invalid() {
        // A linear molecule has no origin to cross.
        let mut m = Molecule {
            seq: b"AAAACCCCGGGGTTTT".to_vec(),
            topology: Topology::Linear,
            ..Default::default()
        };
        let mut f = Feature::new("bad", "misc_feature");
        f.segments.push(Segment {
            start: 12,
            end: 3,
            ..Segment::new(12, 3)
        });
        m.features.push(f);
        assert!(!m.is_valid());
        assert!(matches!(m.validate()[0], Invalid::Inverted { .. }));
    }

    #[test]
    fn rotate_does_not_panic_on_a_zero_start_the_importer_carried_through() {
        // The SnapGene reader accepts `<Segment range="0-4"/>` and deliberately
        // carries it rather than dropping it. `p - 1` underflowed on it.
        let mut m = circular(b"AAAACCCCGGGGTTTT");
        let mut f = Feature::new("z", "misc_feature");
        f.segments.push(Segment::new(0, 4));
        m.features.push(f);
        assert!(m.rotate(5));
        let s = &m.features[0].segments[0];
        // Clamped into 1..=n. Base 0 does not exist, so `0-4` really described
        // four bases (1..4) and that is what survives — the alternative,
        // mapping 0 to 0, would have kept an impossible coordinate.
        assert!(s.start >= 1, "start {} is not a real coordinate", s.start);
        assert_eq!(s.len(), 4);
        assert!(m.is_valid(), "{:?}", m.validate());
    }

    #[test]
    fn rotating_does_not_quietly_repair_a_coordinate_past_the_end() {
        // `remap` clamped BOTH ends into `1..=n`, and a clamp downwards is a
        // fabrication. On this 16 bp circle a segment `4..999` — which
        // `validate()` reports as `PastEnd { end: 999, len: 16 }`, and which a
        // real `.dna` can carry because `<Segment range="4-999"/>` is parsed
        // verbatim — came out of `rotate(5)` as `16..12`: a legal 13 bp wrap.
        // The defect report disappeared, and the operation log committed the
        // edit because its gate only refuses problem kinds whose count rose.
        let mut m = circular(b"AAAACCCCGGGGTTTT");
        let mut f = Feature::new("past the end", "misc_feature");
        f.segments.push(Segment::new(4, 999));
        m.features.push(f);
        assert_eq!(m.validate().len(), 1, "{:?}", m.validate());

        assert!(m.rotate(5));
        let s = &m.features[0].segments[0];
        assert_eq!(
            s.end, 999,
            "a coordinate naming no base names no base after a rotation either"
        );
        assert_eq!(
            s.start, 16,
            "the end that IS real still moves with its bases"
        );
        assert!(
            m.validate().iter().any(|p| matches!(
                p,
                Invalid::PastEnd {
                    end: 999,
                    len: 16,
                    ..
                }
            )),
            "the report must survive the rotation: {:?}",
            m.validate()
        );
    }

    #[test]
    fn rotating_a_segment_entirely_past_the_end_does_not_collapse_it_onto_a_real_base() {
        // The worse half of the same clamp: with BOTH ends past the end, both
        // clamped to 16 and then remapped to the same base, leaving a 1 bp
        // segment sitting on base 12 — a base the feature never described —
        // while `validate()` went from two `PastEnd`s to none. Fabricated
        // coverage under a real feature's name is this project's own
        // worst-defect category (docs/AUDIT-2026-07.md).
        let mut m = circular(b"AAAACCCCGGGGTTTT");
        let mut f = Feature::new("entirely past the end", "misc_feature");
        f.segments.push(Segment::new(20, 25));
        m.features.push(f);
        assert_eq!(m.validate().len(), 2, "{:?}", m.validate());

        assert!(m.rotate(5));
        let s = &m.features[0].segments[0];
        assert_eq!(
            (s.start, s.end),
            (20, 25),
            "nothing here names a base, so nothing here may be moved onto one"
        );
        assert_eq!(m.validate().len(), 2, "{:?}", m.validate());
    }

    #[test]
    fn rotating_an_in_range_segment_is_unaffected_by_the_past_the_end_rule() {
        // The guard against over-correcting. Every ordinary coordinate must
        // still move with its bases, or "set origin" stops working.
        let mut m = circular(b"AAAACCCCGGGGTTTT");
        let mut f = Feature::new("gg", "misc_feature");
        f.segments.push(Segment::new(9, 12)); // the GGGG
        m.features.push(f);
        // The boundary is `n` itself, which is a real base and must move.
        let mut edge = Feature::new("last base", "misc_feature");
        edge.segments.push(Segment::new(16, 16));
        m.features.push(edge);

        assert!(m.rotate(9));
        assert_eq!(
            (
                m.features[0].segments[0].start,
                m.features[0].segments[0].end
            ),
            (1, 4)
        );
        assert_eq!(m.subseq(1, 4).unwrap(), b"GGGG".to_vec());
        assert_eq!(
            (
                m.features[1].segments[0].start,
                m.features[1].segments[0].end
            ),
            (8, 8),
            "base n is in range and must rotate like any other"
        );
        assert!(m.is_valid(), "{:?}", m.validate());
    }

    #[test]
    fn rotates_own_documentation_sits_on_rotate_and_not_on_the_strand_query() {
        // A doc comment is a claim the code has to keep. These two lines sat in
        // the same `///` block as `features_without_expressible_orientation`, so
        // rustdoc printed "Rotate a circular molecule so that 1-based position
        // `origin` becomes 1" as the method-index summary for a read-only `&self`
        // query that takes no origin, mutates nothing and returns
        // `Vec<(usize, &Feature)>` — while `rotate`, the only "set origin" entry
        // point, was listed with no description at all. Asserted rather than
        // eyeballed, because nothing else in the build reads doc comments.
        let src = include_str!("lib.rs");
        let sentence =
            "/// Rotate a circular molecule so that 1-based position `origin` becomes 1,";
        let query = src
            .find("pub fn features_without_expressible_orientation")
            .expect("the strand query is still here");
        let rotate = src
            .find("pub fn rotate(&mut self")
            .expect("rotate is still here");
        let doc = src.find(sentence).expect("rotate still describes itself");
        assert!(
            doc > query && doc < rotate,
            "rotate's summary must attach to rotate, not to the item above it"
        );
        // ...and the strand query keeps its own.
        let strand_doc = src
            .find("/// Features whose strand GenBank cannot express.")
            .expect("the strand query still describes itself");
        assert!(strand_doc < query);
    }

    #[test]
    fn strands_genbank_cannot_express_are_reported() {
        // A GenBank location is plain or wrapped in `complement()`, so
        // `Unoriented` and `Both` are written as forward. 47 features in this
        // project's own corpus are affected, and for roughly half the export
        // publishes a direction the source never claimed — `cat` and `TcR` on
        // pACYC184 really are on opposite strands.
        let mut m = Molecule {
            seq: b"ACGTACGTACGT".to_vec(),
            ..Default::default()
        };
        for (name, strand) in [
            ("fwd", Strand::Forward),
            ("rev", Strand::Reverse),
            ("unoriented", Strand::Unoriented),
            ("both", Strand::Both),
        ] {
            let mut f = Feature::new(name, "misc_feature");
            f.strand = strand;
            f.segments.push(Segment::new(1, 4));
            m.features.push(f);
        }
        let lossy = m.features_without_expressible_orientation();
        let names: Vec<&str> = lossy.iter().map(|(_, f)| f.name.as_str()).collect();
        assert_eq!(names, vec!["unoriented", "both"]);
        // Indices come back too, so a caller can point at the right row.
        assert_eq!(lossy[0].0, 2);
    }

    #[test]
    fn a_declared_length_that_disagrees_with_the_bases_is_reported() {
        // `from_utf8_lossy` turns one invalid byte into U+FFFD, three bytes the
        // base filter then accepts: a 12 bp record read as 14 bp, with every
        // feature after that point pointing at the wrong bases while
        // `validate()` returned clean.
        let m = Molecule {
            seq: b"ACGTACGTACGTAA".to_vec(),
            declared_len: Some(12),
            ..Default::default()
        };
        assert!(!m.is_valid());
        assert!(matches!(
            m.validate()[0],
            Invalid::LengthMismatch {
                declared: 12,
                actual: 14
            }
        ));

        // Agreement is silent.
        let ok = Molecule {
            seq: b"ACGTACGTACGT".to_vec(),
            declared_len: Some(12),
            ..Default::default()
        };
        assert!(ok.is_valid());

        // Annotation-only GenBank declares a length and ships no bases by
        // design; flagging that would break a supported class of file.
        let annotations_only = Molecule {
            declared_len: Some(2_900_000),
            ..Default::default()
        };
        assert!(annotations_only.is_valid());
    }

    #[test]
    fn an_inverted_segment_has_no_left_to_right_length() {
        let s = Segment::new(12, 3);
        assert_eq!(
            s.len(),
            0,
            "the doc comment says zero, so the code must too"
        );
        assert!(s.is_empty());
        // ...and a hostile coordinate does not panic.
        assert_eq!(Segment::new(1, u64::MAX).len(), u64::MAX);
    }

    #[test]
    fn rotation_that_crosses_the_origin_stays_consistent() {
        let mut m = circular(b"AAAACCCCGGGGTTTT");
        let mut f = Feature::new("wrap", "misc_feature");
        f.segments.push(Segment::new(15, 16));
        f.segments.push(Segment::new(1, 2));
        m.features.push(f);
        assert!(m.rotate(5));
        let segs = &m.features[0].segments;
        assert_eq!((segs[0].start, segs[0].end), (11, 12));
        assert_eq!((segs[1].start, segs[1].end), (13, 14));
    }

    #[test]
    fn validate_accepts_a_sound_molecule() {
        let mut m = Molecule {
            seq: b"ACGTACGTACGT".to_vec(),
            ..Default::default()
        };
        let mut f = Feature::new("ok", "CDS");
        f.segments.push(Segment::new(1, 6));
        f.segments.push(Segment::new(9, 12)); // a join, which is fine
        m.features.push(f);
        assert!(m.is_valid(), "{:?}", m.validate());
    }

    #[test]
    fn validate_catches_what_the_other_representation_would_have_prevented() {
        // Each of these is impossible to express as {start, length} mod L, and
        // constructible here. That trade is recorded in PLAN 5.3.1.
        let base = Molecule {
            seq: b"ACGTACGTACGT".to_vec(),
            ..Default::default()
        };

        let mut inverted = base.clone();
        let mut f = Feature::new("backwards", "CDS");
        f.segments.push(Segment::new(9, 4));
        inverted.features.push(f);
        assert!(matches!(
            inverted.validate().as_slice(),
            [Invalid::Inverted {
                start: 9,
                end: 4,
                ..
            }]
        ));

        let mut zero = base.clone();
        let mut f = Feature::new("zero", "CDS");
        f.segments.push(Segment::new(0, 4));
        zero.features.push(f);
        assert!(matches!(
            zero.validate().as_slice(),
            [Invalid::ZeroStart { .. }]
        ));

        let mut past = base.clone();
        let mut f = Feature::new("past", "CDS");
        f.segments.push(Segment::new(4, 999));
        past.features.push(f);
        assert!(matches!(
            past.validate().as_slice(),
            [Invalid::PastEnd {
                end: 999,
                len: 12,
                ..
            }]
        ));

        let mut empty = base.clone();
        empty.features.push(Feature::new("nowhere", "CDS"));
        assert!(matches!(
            empty.validate().as_slice(),
            [Invalid::FeatureWithoutSegments { .. }]
        ));
    }

    #[test]
    fn an_endpoint_of_zero_is_reported_at_either_end() {
        // The mirror of the `start > n` gap above, and it hid in the same way.
        // On a circle `end == 0` passes every other check: `start != 0`, and
        // `end < start` is a legal wrap there, and `0` is not past the end. A
        // `.dna` carrying `<Segment range="5-0"/>` parses verbatim, so
        // `validate()` called the molecule clean while `subseq(5, 0)` refused
        // the same span and `genbank::write` emitted `join(5..16,1..0)` — not a
        // legal GenBank location — with nothing reported and exit 0.
        let mut m = circular(b"AAAACCCCGGGGTTTT");
        let mut f = Feature::new("endzero", "misc_feature");
        f.segments.push(Segment::new(5, 0));
        m.features.push(f);
        assert!(
            m.subseq(5, 0).is_none(),
            "the model already refuses to slice this span"
        );
        assert!(
            matches!(m.validate().as_slice(), [Invalid::ZeroStart { .. }]),
            "a span the model will not slice is not a clean span: {:?}",
            m.validate()
        );

        // Both endpoints at once is two problems, not one.
        let mut both = circular(b"AAAACCCCGGGGTTTT");
        let mut f = Feature::new("bothzero", "misc_feature");
        f.segments.push(Segment::new(0, 0));
        both.features.push(f);
        assert_eq!(both.validate().len(), 2, "{:?}", both.validate());

        // And the same for a primer binding site. `snapgene.rs` cannot produce
        // one, but `snapgene::from_molecule_reporting` already refuses to write
        // a `9..0` site, so the model has to be able to say why.
        let mut p = circular(b"AAAACCCCGGGGTTTT");
        p.primers.push(Primer {
            name: "P".into(),
            sites: vec![BindingSite {
                start: 9,
                end: 0,
                strand: Strand::Forward,
                tm: None,
            }],
            ..Default::default()
        });
        assert!(
            matches!(p.validate().as_slice(), [Invalid::ZeroStart { .. }]),
            "{:?}",
            p.validate()
        );

        // The message has to work for either end, which "starts at 0" did not.
        let text = Invalid::ZeroStart {
            what: "feature 0 'endzero' segment 0 end".into(),
        }
        .to_string();
        assert!(!text.contains("starts at"), "{text}");
        assert!(text.contains("position 0"), "{text}");
    }

    #[test]
    fn extent_reads_an_origin_crossing_join_as_the_wrap_it_is() {
        // `Feature::start`/`end` are a min and a max over the segments, and an
        // origin-crossing feature in its GenBank join form always has a part
        // beginning at base 1 — so they collapse to `(1, n)` and report a 17 bp
        // promoter as spanning the whole 2,686 bp plasmid. That shape is not
        // exotic: `genbank::write` emits it for every wrapped feature and
        // `genbank::parse` reads it straight back, so a save and a reopen was
        // enough to produce it.
        let n = 2686u64;
        let mut joined = Feature::new("wrapper", "promoter");
        joined.segments.push(Segment::new(2677, n));
        joined.segments.push(Segment::new(1, 7));
        assert_eq!((joined.start(), joined.end()), (1, 2686));
        assert_eq!(joined.extent(n, true), Some((2677, 7)));

        // The single-segment spelling of the same span agrees with it.
        let mut wrapped = Feature::new("wrapper", "promoter");
        wrapped.segments.push(Segment::new(2677, 7));
        assert_eq!(wrapped.extent(n, true), Some((2677, 7)));

        // An ordinary multi-exon join is NOT this shape and must keep the
        // conventional span, which is also what Biopython reports for it.
        let mut spliced = Feature::new("spliced", "CDS");
        spliced.segments.push(Segment::new(100, 200));
        spliced.segments.push(Segment::new(300, 400));
        assert_eq!(spliced.extent(n, true), Some((100, 400)));

        // Nor is a join that merely begins at base 1.
        let mut from_one = Feature::new("from one", "CDS");
        from_one.segments.push(Segment::new(1, 200));
        from_one.segments.push(Segment::new(300, 400));
        assert_eq!(from_one.extent(n, true), Some((1, 400)));

        // On a linear molecule there is no origin to cross.
        assert_eq!(joined.extent(n, false), Some((1, 2686)));
        assert_eq!(Feature::new("nowhere", "CDS").extent(n, true), None);
    }

    #[test]
    fn an_annotation_only_file_is_checked_against_its_declared_span() {
        // No bases, so there is nothing to compare against except the length
        // the file claims. A feature inside that span is fine.
        let mut m = Molecule {
            declared_len: Some(1000),
            ..Default::default()
        };
        let mut f = Feature::new("gene", "CDS");
        f.segments.push(Segment::new(100, 400));
        m.features.push(f);
        assert!(m.is_valid(), "{:?}", m.validate());

        let mut past = m.clone();
        past.features[0].segments[0].end = 2000;
        assert!(!past.is_valid());
    }

    #[test]
    fn primer_sites_are_checked_too() {
        let mut m = Molecule {
            seq: b"ACGTACGTACGT".to_vec(),
            ..Default::default()
        };
        m.primers.push(Primer {
            name: "p".into(),
            seq: "ACGT".into(),
            description: String::new(),
            sites: vec![BindingSite {
                start: 10,
                end: 3,
                strand: Strand::Forward,
                tm: None,
            }],
        });
        assert!(matches!(
            m.validate().as_slice(),
            [Invalid::Inverted { .. }]
        ));
    }

    #[test]
    fn declared_length_is_not_confused_with_real_bases() {
        let m = Molecule {
            declared_len: Some(2_944_528),
            ..Default::default()
        };
        assert_eq!(m.len(), 0);
        assert_eq!(m.span(), 2_944_528);
        assert!(m.sequence_absent());
        assert_eq!(m.gc_percent(), None);
    }

    #[test]
    fn unoriented_strand_survives_the_model() {
        assert_eq!(Strand::from_directionality(None), Strand::Unoriented);
        assert_eq!(Strand::from_directionality(Some(2)), Strand::Reverse);
        assert_eq!(Strand::Unoriented.to_directionality(), None);
    }

    #[test]
    fn a_start_past_the_end_of_a_circle_is_reported() {
        // The one window `validate` did not cover, and it is constructible from
        // a real `.dna`: circular, `start > n`, and `end < start` — which the
        // wrap rule calls legal — with `end <= n`, so neither `Inverted` nor
        // the end-only `PastEnd` fired. validate() returned clean while
        // `subseq` refused the span and the renderer drew a fabricated arc
        // under the feature's name.
        let mut m = Molecule {
            seq: b"ACGTACGTACGT".to_vec(),
            topology: Topology::Circular,
            ..Default::default()
        };
        let mut f = Feature::new("impossible", "CDS");
        f.segments.push(Segment::new(99999, 10));
        m.features.push(f);

        let problems = m.validate();
        assert!(
            problems.iter().any(|p| matches!(
                p,
                Invalid::PastEnd {
                    end: 99999,
                    len: 12,
                    ..
                }
            )),
            "the start must be reported: {problems:?}"
        );
        // And the consumers that disagreed with it still disagree, which is
        // why reporting it matters.
        assert!(m.subseq(99999, 10).is_none());
    }

    #[test]
    fn a_legal_wrap_on_a_circle_is_still_not_a_problem() {
        // The guard against over-correcting: an ordinary origin-crossing
        // feature must stay clean, or every circular plasmid starts reporting
        // faults.
        let mut m = Molecule {
            seq: b"ACGTACGTACGT".to_vec(),
            topology: Topology::Circular,
            ..Default::default()
        };
        let mut f = Feature::new("crosses the origin", "CDS");
        f.segments.push(Segment::new(10, 3));
        m.features.push(f);
        assert_eq!(m.validate(), vec![], "a legal wrap is not a fault");
    }
}
