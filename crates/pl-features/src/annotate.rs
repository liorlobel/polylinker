//! Auto-annotation — the §7.7 pipeline.
//!
//! Open a plasmid, get its features back. This is the "magic moment" the plan
//! identifies as the product's centre of gravity, and its most important
//! scoping decision is what it *does not* do: no BLAST+, no DIAMOND, no
//! Infernal, no gigabyte database. SnapGene's behaviour is documented as
//! approximate matching against a curated library at ≥96% identity, tolerant of
//! occasional mismatches and indels, plus perfect-protein matching so
//! codon-optimised genes still resolve. That is string matching over a few
//! megabytes, and it belongs in-process.
//!
//! # Coordinates
//!
//! Output is **1-based inclusive**, matching `pl_core` and both file formats we
//! read (`docs/PLAN.md` §5.3.1 as amended). Internally everything is 0-based
//! half-open, because that is what alignment wants; the conversion happens once,
//! at the boundary, in [`Annotation::from_span`].
//!
//! # Circularity
//!
//! A circular plasmid has no beginning, so a feature may straddle whatever base
//! the file happens to number 1. The query is therefore doubled before matching
//! and hits are folded back, which is the same trick the digester uses. The
//! cost is that every feature is found twice; [`dedupe`] resolves that by
//! position modulo length rather than by identity, so the *origin-spanning* copy
//! is the one that survives.
//!
//! # Peptide parts, and the fusion rule
//!
//! Since 2026-07-28 a `synthetic_part` row may be a peptide and nothing else —
//! FLAG is `DYKDDDDK` and has dozens of synonymous encodings, so a nucleotide
//! reference for it would be one arbitrary choice that misses every re-coded
//! copy. Such a row is found only through the translated scan, and only under
//! two extra rules this module applies and the loader does not:
//!
//! 1. **Exactly and wholly, at zero edit distance**, regardless of
//!    [`Config::min_identity`]. `features/SOURCING.md` §3's false-positive
//!    arithmetic — eight residues over twenty letters against ~10,000 residue
//!    positions — holds only under exact matching. A scored aligner on an
//!    8-mer reports FLAG tags that are not there.
//! 2. **Fused to an ORF.** The hit must lie in frame inside an open reading
//!    frame of the *query*, with at least [`PARTNER_MIN`] residues of that ORF
//!    outside the tag. See [`fused_orf`] for the predicate and what it costs.
//!
//! The second is the PI's decision of 2026-07-28, in his words: "add these
//! sequences, but make sure they are fused to an ORF, otherwise ignored."
//! *Ignored* is meant literally — a hit that fails the predicate is dropped
//! from the results with no annotation and no fragment.

use pl_core::orf::{self, Orf, Params};
use pl_core::translate::{self, Code, Frame};
use pl_core::{iupac, Molecule, Strand};

use crate::align;
use crate::index::{Index, K_DNA, K_PROTEIN};
use crate::{Db, Record};

/// Shortest `reference_aa` this annotator will accept **on any record**,
/// enforced in [`Annotator::new`].
///
/// Named for the designed parts it was written for, and applied to every class
/// because the route it guards is blind to class — see
/// [`Annotator::new`]'s own docs for the `cds` row that walked through the
/// class-keyed version of this floor.
///
/// It used to be inherited rather than enforced, and the derivation was: a
/// peptide shorter than one seed word produces no window at all, lands in the
/// protein index's `short` list, and is honestly reported unreachable — so
/// nothing below this length could reach the predicate by any route, whatever a
/// database said.
///
/// [`Index::unchainable`] and the exact scan in [`Annotator::scan_protein`]
/// falsified that sentence: a 4-residue peptide is now matched exactly, in every
/// frame, like any other. What did **not** move is [`ORF_MIN_AA`], which is the
/// `Params::min_aa` handed to [`orf::find_orfs`] — while [`fused_orf`] itself
/// only asks for `aa_len >= tag_aa + PARTNER_MIN`. A 4-residue part in a 24 aa
/// ORF therefore satisfies the predicate over an ORF the search was told not to
/// return, and is dropped with no diagnostic: findable at 25 residues of ORF,
/// invisible at 24. A length-dependent hole, opened by a derivation nobody
/// updated.
///
/// So the floor is now a refusal rather than a fact about the index. Admitting
/// a shorter part means moving [`ORF_MIN_AA`] with it and saying so in
/// `features/build/stage_curated.py`'s `self_test`; the const assert below ties
/// the two numbers together so the pair cannot drift.
///
/// The value is the shortest part the builder issues — enterokinase's `DDDDK`,
/// five residues — and no longer [`K_PROTEIN`], which it used to equal and which
/// it no longer has any reason to. The loader enforces no length floor at all,
/// so a hand-authored table may carry a shorter peptide than the build would
/// issue, and refusing it out loud is the only answer that is neither a silent
/// drop nor a silent hole.
const MIN_PART_AA: usize = 5;

/// Residues of the ORF that must be something **other than** the tag.
///
/// This is the "fused to" clause: a fusion requires a partner, and the partner
/// must exist. A flat count rather than a ratio, deliberately. The ratio
/// version — tag at most half the ORF — demands the *most* corroboration from
/// the tags that need it least: an exact 38-residue SBP match is astronomically
/// unlikely by chance, and requiring it to sit in a 76 aa ORF buys nothing
/// while costing real SBP-tagged small proteins. A flat floor puts the
/// evidential burden on the short peptides, which is where it belongs: FLAG
/// needs a 28 aa ORF, SBP a 58 aa one.
///
/// Why 20 and not 50 or 100, measured rather than asserted. The quantity is
/// the share of the positions at which an 8-residue tag could start in the six
/// translated frames that the predicate admits — which is exactly the share of
/// chance exact matches the gate lets through — under the shipped defaults
/// ([`Config::code`] = table 11):
///
/// ```text
///                            partner 20   partner 50   partner 100
///   random, 20 x 5 kb           2.7x         6.4x         40x
///   pBR322   J01749 4361 bp     2.1x         3.0x        5.2x
///   pUC19    L09137 2686 bp     2.3x         4.0x       10.3x
///   pTrc99A  U13872 4176 bp     2.1x         3.5x        8.3x
/// ```
///
/// An earlier version of this comment claimed 4.7x here and ~64x at 100, from
/// an estimate rather than a run; the numbers above come from `find_orfs`
/// itself over the three ENA records named. **Real vectors are the ones that
/// matter, and there the gate is worth about 2.1x** — they are far denser in
/// coding sequence than random DNA, so more of them is inside some ORF.
///
/// The conclusion survives the correction and in fact strengthens. If a
/// 100-residue floor buys 5-10x on a real vector rather than 64x, then paying
/// for it with every bacterial small protein — the PI's own field — is a worse
/// bargain than the old number suggested, and a 50-residue floor drops a
/// His-tagged 45 aa protein for about 1.6x. **The ORF rule is not the
/// false-positive control** — exact matching is, and anyone who describes it
/// otherwise is wrong. This rule is what makes the *claim* ("this is a tag on a
/// protein") mean something.
///
/// Note which constant is doing the work: [`ORF_MIN_AA`] is *not*. Varying
/// `Params::min_aa` between 25 and 28 moves none of the numbers above, because
/// this clause already requires `aa_len >= tag_aa + 20` and discards whatever
/// `min_aa` would have let through. `min_aa` exists to save work, not to
/// filter.
///
/// What it costs, plainly: a genuine fusion whose partner is shorter than 20
/// residues — a tagged peptide antigen, a tagged peptide hormone, a 12-residue
/// display construct — is dropped, silently. A 30 aa small protein with a
/// C-terminal His6 has a 24-residue partner and survives, which is why 20 was
/// chosen over 50.
const PARTNER_MIN: usize = 20;

/// [`Params::min_aa`] for the fusion search: the smallest ORF the predicate can
/// possibly accept, so the search discards nothing the predicate would take.
const ORF_MIN_AA: usize = MIN_PART_AA + PARTNER_MIN;

// Tied together rather than written as a literal, because the two numbers that
// decide it live thirty lines apart and a literal would drift from them.
//
// The second clause used to be `MIN_PART_AA >= K_PROTEIN`, on the grounds that
// no peptide below one seed word could reach the predicate by any route. The
// exact scan makes that false, so the clause is gone and `Annotator::new`
// enforces the floor instead. What survives is the relation that actually
// matters: the ORF search must not discard an ORF the predicate would accept,
// and it does not, because the shortest acceptable ORF is the shortest
// acceptable tag plus its shortest acceptable partner.
const _: () = assert!(
    ORF_MIN_AA == MIN_PART_AA + PARTNER_MIN,
    "the ORF floor must be exactly the shortest acceptable tag plus its \
     shortest acceptable partner, or a part is findable in a 25-residue ORF and \
     silently invisible in a 24-residue one"
);

/// Knobs, all with the plan's defaults.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// §7.7 step 4. User-adjustable by design: a cloning scar or a silent
    /// mutation should not hide a marker, but neither should 80% identity
    /// invent one.
    pub min_identity: f64,
    /// Seeds a chain needs before it is worth aligning.
    pub min_seeds: usize,
    /// §7.7 step 8. Below this fraction of the database feature's length, a hit
    /// is a **fragment** — a real thing to report, drawn as an unfilled arrow,
    /// not a whole feature and not nothing.
    pub fragment_coverage: f64,
    /// Below this fraction of the feature's length, a hit is not a fragment but
    /// a coincidence, and is discarded.
    ///
    /// §7.7 specifies no such floor and needs one. Three 12-mers are enough to
    /// nominate a chain, so without it the annotator reports a "15 bp KanR
    /// fragment" — an 816 bp gene claimed on the evidence of fifteen bases.
    /// Measured against 108 real molecules that produced **10,146 fragments and
    /// 5 whole features**: not a coverage result, a wall of noise deep enough
    /// to bury the real answers and make every map untrustworthy.
    pub min_coverage: f64,
    /// An absolute floor in symbols as well, so a short feature must be found
    /// nearly whole rather than on a lucky seed. Clamped to the feature's own
    /// length, so a 34 bp lox site stays findable — at 34 bp.
    pub min_match_len: usize,
    /// §7.7 step 7's overlap rule: shrink each hit by this fraction at each end
    /// before asking whether two hits collide, so features that merely abut are
    /// both kept.
    ///
    /// The rule is applied **across records**, per the plan, with one
    /// exception: a hit lying strictly inside a better-scoring one is kept.
    /// See [`resolve_overlaps`].
    pub overlap_trim: f64,
    /// Match by six-frame translation.
    ///
    /// This finds a marker whose nucleotides were rewritten for expression in
    /// another organism — and, since 2026-07-28, it is the **only** route to a
    /// peptide-only synthetic part. Turning it off does not merely make the
    /// tags harder to find; it makes them matchable by nothing at all, because
    /// they are in no other index. [`Annotator::unseedable`] reports them under
    /// that setting, correctly and alarmingly.
    pub protein: bool,
    /// The genetic code, used by the six-frame scan **and** by the fusion
    /// rule's ORF search.
    ///
    /// One code for both halves, not two. If they differed, the stop that ends
    /// an ORF would not be the stop the translated frame renders as `*`, and
    /// the two halves of the fusion predicate would disagree about where the
    /// protein ends — 13 of the 27 tables do not stop at `TGA`.
    ///
    /// # This field decides whether a tag is reported at all
    ///
    /// Until the fusion rule existed, `code` reached only
    /// [`translate::six_frames`], and tables 1 and 11 have the *same* 64 amino
    /// acids and the *same* three stops — so the start-codon half of the table
    /// had literally no effect on `annotate()` and the default was inert. It is
    /// not inert now: [`orf::find_orfs`] runs with `require_start`, so which
    /// codons may initiate decides which ORFs exist, and an ORF that does not
    /// exist admits no tag. See [`Config::default`] for why that made table 1
    /// the wrong default.
    pub code: Code,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            min_identity: 0.96,
            min_seeds: 3,
            fragment_coverage: 0.95,
            min_coverage: 0.30,
            min_match_len: 50,
            overlap_trim: 0.15,
            protein: true,
            // Table 11, not table 1, and the fusion rule is why.
            //
            // The concrete failure: splice a FLAG tag in frame after the
            // initiator of the shipped `lacI` row (PLF:1000) and annotate the
            // result. Under table 1 the tag is NOT REPORTED; under table 11 it
            // is found at 4..27, in frame with a 368 aa ORF. Same for `TetA`
            // (PLF:0006, 407 aa) and lambda `int` (PLF:1007, 613 aa). All three
            // begin `GTG`, which table 1 does not accept as an initiator, so
            // `find_orfs` reports no ORF over the gene and the fusion predicate
            // has nothing to admit the tag on. Five of the 38 CDS rows in this
            // project's own table start GTG. `orf.rs`'s module doc names the
            // trap by name: "tet(A) starts GTG, which this project has already
            // been caught by once."
            //
            // Nothing else moves. `TABLE11` has the identical amino-acid string
            // and the identical stop set to `TABLE1` — verified in
            // `translate.rs`'s own `table_11_differs_from_table_1_only_in_starts`
            // — so `six_frames` output is byte-for-byte unchanged and the "one
            // code for both halves" invariant above still holds: what has to
            // agree between the ORF finder and the frame is the *stop* set, and
            // it does.
            //
            // The cost, stated: seven initiators instead of three admits more
            // chance ORFs, and the gate is worth about 2.1x on a real vector
            // under table 11 against 2.4x under table 1 (see `PARTNER_MIN`).
            // That is the right trade — a silently missing tag on a real
            // bacterial gene is a worse answer than a marginally weaker gate,
            // and the gate was never the false-positive control. A user
            // annotating a eukaryotic construct, where GTG initiation is not
            // the norm, can ask for table 1 with `pl annotate --code 1`.
            code: translate::TABLE11,
        }
    }
}

/// One found feature.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    /// Index into [`Db::records`].
    pub record: usize,
    /// 1-based inclusive.
    pub start: u64,
    /// 1-based inclusive.
    pub end: u64,
    pub strand: Strand,
    /// Fraction of the *aligned region* that matched — a **local** identity.
    ///
    /// Not the fraction of the database feature reproduced; that is
    /// [`coverage`](Self::coverage), and the two only mean anything together.
    /// The denominator is the seed-supported core plus whatever the outward
    /// `walk` added, so a plasmid carrying the first 300 bp of a 600 bp
    /// marker exactly reports `identity = 1.0` alongside `coverage = 0.50`:
    /// 300 bases of the feature, reproduced perfectly. A caller that prints
    /// one of these without the other is not saying anything.
    ///
    /// This line used to read "Fraction of the database feature reproduced.
    /// See [`Hit::identity`](crate::align::Hit::identity)", which is the opposite convention and describes
    /// a function nothing on this path calls — [`Hit::identity`](crate::align::Hit::identity) divides by the
    /// feature length *precisely so* a half-deleted feature scores 0.5. Under
    /// that sentence the shipped fixture
    /// `a_truncated_feature_is_reported_as_a_fragment_not_dropped` reads as a
    /// bug rather than the intended result, and a UI trusting it would label a
    /// half-length fragment 100%.
    pub identity: f64,
    /// How much of the database feature this hit spans: aligned symbols over
    /// the record's length. This is the number that falls when a feature is
    /// truncated; [`identity`](Self::identity) does not.
    pub coverage: f64,
    /// `match_length × identity × coverage`, per §7.7 step 7.
    pub score: f64,
    /// Coverage below [`Config::fragment_coverage`].
    pub is_fragment: bool,
    /// Found by translation rather than by nucleotide identity — i.e. this is
    /// probably a codon-optimised or otherwise recoded copy.
    pub via_protein: bool,
    /// Runs across the origin of a circular molecule, so `start > end`.
    pub wraps_origin: bool,
    /// For a peptide-only synthetic part: the ORF the fusion rule admitted it
    /// on. `None` for every other record, which reaches no fusion rule.
    ///
    /// Carried as evidence because the rule is otherwise correct and
    /// inexplicable. ORF display is a separate feature, so a user can see a
    /// FLAG tag appear with no visible protein under it and no way to find out
    /// why. `features/SOURCING.md` §3's stated differentiator is "a hit plus
    /// how we found it"; this is the how, and a UI that says "in frame with a
    /// 312 aa ORF at 1204..2139" turns a mysterious result into a checkable
    /// one.
    pub fusion_orf: Option<FusionOrf>,
}

/// The open reading frame a peptide part was admitted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FusionOrf {
    /// 1-based inclusive, plus-strand, exactly as [`pl_core::orf::Orf`] reports
    /// them — so `end < start` may mean it crosses the origin, and the
    /// inclusive range is not its extent. [`FusionOrf::aa_len`] is the length.
    pub start: u64,
    pub end: u64,
    pub strand: Strand,
    /// Amino acids, excluding the stop.
    pub aa_len: usize,
}

impl Annotation {
    /// Build from a 0-based half-open span in the *unwrapped* molecule.
    fn from_span(record: usize, span: (usize, usize), len: usize, strand: Strand) -> Annotation {
        let wraps = span.1 > len;
        let start = (span.0 % len) as u64 + 1;
        let end = if wraps {
            ((span.1 - 1) % len) as u64 + 1
        } else {
            span.1 as u64
        };
        Annotation {
            record,
            start,
            end,
            strand,
            identity: 0.0,
            coverage: 0.0,
            score: 0.0,
            is_fragment: false,
            via_protein: false,
            wraps_origin: wraps,
            fusion_orf: None,
        }
    }

    /// Length in bases, correct across the origin.
    pub fn len(&self, molecule_len: u64) -> u64 {
        if self.wraps_origin {
            molecule_len - self.start + 1 + self.end
        } else {
            self.end.saturating_sub(self.start) + 1
        }
    }

    pub fn is_empty(&self, molecule_len: u64) -> bool {
        self.len(molecule_len) == 0
    }
}

/// The [`pl_core::Feature`] an annotation becomes, provenance and all.
///
/// **ONE function because there must be ONE format.** This body lived inside
/// `cmd_annotate`'s `--genbank` arm and had exactly one caller, which was fine
/// until the desktop application grew an Accept button and needed to write the
/// same thing. Two copies of a provenance string is two provenance strings: the
/// next person to add a clause adds it to one of them, and a `.gb` written by
/// the app then says something different from a `.gb` written by the command
/// line about the same hit in the same database. The note is the only record a
/// reader has of where a name on their map came from, so "the same" has to be
/// mechanical rather than intended.
///
/// `db` must be the database `a.record` indexes — the same one the
/// [`Annotator`] was built over. Passing the full table for an annotation
/// produced against [`Db::reviewed`](crate::Db::reviewed) does not fail; it
/// silently names a different record.
///
/// # What the note says, and why both numbers are in it
///
/// Identity and coverage always travel together, because either alone is
/// unreadable: see [`Annotation::identity`], which is a *local* identity over
/// the aligned region, against [`Annotation::coverage`], the fraction of the
/// database feature reproduced. The first 300 bp of a 600 bp marker, copied
/// perfectly, is `100.0% identity, 50% coverage` — and "100%" on its own would
/// be read as "this is that feature".
///
/// # Origin-crossing hits
///
/// One segment, `start..end`, even when `end < start`. That is the shape
/// [`pl_core::Feature::extent`] reads as a wrap on a circular molecule, and it
/// is what [`Annotation::start`] and [`Annotation::end`] already mean. Turning
/// it into a GenBank `join(...)` is the writer's business and happens on the
/// way to the file, not here.
pub fn to_feature(db: &Db, a: &Annotation) -> pl_core::Feature {
    let r = &db.records[a.record];
    let mut feat = pl_core::Feature::new(r.name.clone(), r.genbank_key.clone());
    feat.strand = a.strand;
    feat.segments = vec![pl_core::Segment::new(a.start, a.end)];
    // Provenance travels with the annotation. A map that cannot say where a
    // name came from is a map nobody can check, and an unreviewed row must
    // carry that fact into the file it lands in — otherwise the caveat stops at
    // the terminal.
    feat.qualifiers.push((
        "note".into(),
        Some(format!(
            "{} {}: {:.1}% identity, {:.0}% coverage, polylinker feature db {}{}",
            r.id,
            if a.via_protein {
                "protein match"
            } else {
                "nucleotide match"
            },
            a.identity * 100.0,
            a.coverage * 100.0,
            db.version,
            if r.review_status == crate::ReviewStatus::Proposed {
                "; PROPOSED, not reviewed by a human"
            } else {
                ""
            }
        )),
    ));
    // The evidence a peptide part was admitted on, carried into the written
    // file rather than left in the terminal. A tag called by the fusion rule
    // with no ORF drawn under it is otherwise unexplainable to whoever opens
    // the file next.
    if let Some(o) = a.fusion_orf {
        feat.qualifiers.push((
            "note".into(),
            Some(format!(
                "peptide reference, admitted because it lies in frame inside \
                 a {} aa ORF at {}..{} on the {} strand",
                o.aa_len,
                o.start,
                o.end,
                if o.strand == Strand::Reverse {
                    "minus"
                } else {
                    "plus"
                }
            )),
        ));
    }
    feat
}

/// An annotator with its indexes already built.
///
/// Building the index is the expensive part and does not depend on the
/// molecule, so it is done once and reused across every file opened.
pub struct Annotator<'a> {
    db: &'a Db,
    dna: Index,
    protein: Index,
    config: Config,
}

impl<'a> Annotator<'a> {
    /// # Panics
    ///
    /// If `db` carries **any** record whose `reference_aa` is shorter than
    /// [`MIN_PART_AA`], whatever its class.
    ///
    /// Loud on purpose, and this is the one place that can be loud. Such a row
    /// is not unmatchable — the exact scan finds it in every frame — it is
    /// matchable *inconsistently*: [`fused_orf`] would admit it in an ORF of
    /// `len + PARTNER_MIN` residues, but [`ORF_MIN_AA`] means no ORF that short
    /// is ever searched, so the row works in a 25 aa ORF and vanishes in a 24 aa
    /// one with nothing said. `unseedable()` cannot report it, because it is not
    /// unreachable; returning a `Result` would make every caller in the tree
    /// handle a case no shipped table can produce. So it refuses.
    ///
    /// # Why it is not keyed on `Record::is_designed_peptide`
    ///
    /// It was, and the two keys did not line up: the refusal read
    /// `is_designed_peptide()` (`synthetic_part` only) while
    /// [`Annotator::scan_protein_exact`] is driven by [`Index::unchainable`],
    /// which is blind to class. A `cds` row carrying a 4-residue
    /// `reference_aa` — which [`Db::parse`] admits, checking the alphabet and
    /// nothing else — therefore slipped past the floor and into the scan, and
    /// `make` applies neither the exactness rule nor the fusion gate to a
    /// non-designed row. Measured: on a molecule of pure `GCC` filler with the
    /// four residues spliced in, a `cds` row carrying `reference_aa` of length 4
    /// was reported at `91..102`, `via_protein`, identity 1.000, coverage 1.000,
    /// `fusion_orf: None` — an ungated six-frame call on twelve bases. At
    /// b340b18 the same row was found nowhere, for the reason this whole change
    /// is about: four residues index no 5-mer at all, so there were no seeds,
    /// no chain, and no scan to fall back on.
    ///
    /// The floor is deliberately *not* the whole answer for a `cds` row: at 5
    /// and 6 residues such a row is now scanned and reported ungated, where at
    /// b340b18 it was silently unfindable. That is the same treatment a 7 aa
    /// `cds` peptide already got by seed-and-chain at b340b18, extended down two
    /// residues by this change and not a new kind of behaviour. What the floor
    /// buys is that no class gets a reference so short that an exact six-frame
    /// match means nothing.
    pub fn new(db: &'a Db, config: Config) -> Annotator<'a> {
        if let Some(r) = db.records.iter().find(|r| {
            r.reference_aa
                .as_ref()
                .is_some_and(|p| !p.is_empty() && p.len() < MIN_PART_AA)
        }) {
            panic!(
                "{}: class {} carries a protein reference of {} residue(s), below \
                 MIN_PART_AA = {MIN_PART_AA}. A designed peptide that short would be \
                 findable inside an ORF of {} residues and silently invisible inside \
                 one of {}, because ORF_MIN_AA = {ORF_MIN_AA} is what the ORF search \
                 is given; on any other class it is worse, because the fusion gate \
                 does not apply at all and the exact scan would report it in six \
                 frames of everything. Lower ORF_MIN_AA with it, or leave the row out.",
                r.id,
                r.class.as_str(),
                r.reference_aa.as_ref().map_or(0, |p| p.len()),
                ORF_MIN_AA,
                ORF_MIN_AA - 1,
            );
        }
        Annotator {
            dna: Index::build(db, false, K_DNA),
            protein: Index::build(db, true, K_PROTEIN),
            db,
            config,
        }
    }

    pub fn db(&self) -> &Db {
        self.db
    }

    /// Database entries that neither index can reach.
    ///
    /// Reported rather than swallowed: a caller that believes it searched the
    /// whole database when it did not will report a confident empty result.
    pub fn unseedable(&self) -> Vec<&Record> {
        // `dna.short()` narrowed by `has_protein()`, and NOT by
        // `protein.short()`. A record only one route can reach is still
        // reachable, and listing it here was worse than saying nothing: a
        // well-formed 5-codon CDS was reported unsearchable and then found at
        // coverage 1.0 in the same run.
        //
        // This used to intersect the two indexes' short lists, and that formula
        // stopped being true when the exact scan landed. `scan_protein` reaches
        // a record either by seed-and-chain (`words >= min_seeds`) or by the
        // scan (`Index::unchainable`, evaluated at `min_seeds.max(1)`), and
        // those two are exhaustive over records with residues: `has_protein()`
        // means `lengths > 0` in the protein index, so every such record has
        // either `words >= min_seeds` or `words < min_seeds.max(1)`. There is
        // no third case. So "the protein index cannot seed it" is no longer a
        // reason it cannot be found, and `protein.short()` is no longer the
        // rescue predicate. `has_protein()` is.
        //
        // The case that makes the difference concrete, and it is not
        // hypothetical: a peptide whose every 5-residue window carries an `X`
        // indexes zero words, so `protein.short()` holds it and the old formula
        // reported it "too short to seed and cannot be found". The scan searches
        // for it in all six frames and finds it wherever the query really
        // translates to those residues — an `NNN` codon translates to `X` — so
        // the old answer was false, in the same shape as the 5-codon CDS failure
        // above. That is why this is a narrowing rather than a widening.
        //
        // Gated on `config.protein`, because with translated matching switched
        // off the protein index is never consulted and a record the DNA index
        // cannot seed really is unreachable. Under-reporting is the worse of
        // the two failures. A peptide-only synthetic part is in `dna.short()`
        // **always**, having no bases to seed, so under `--no-protein` every
        // one of them is reported — true, and alarming to anyone who does not
        // know that translation is their only route.
        let dna: std::collections::BTreeSet<u32> = self.dna.short().iter().copied().collect();
        let kept: Vec<u32> = if self.config.protein {
            dna.into_iter()
                .filter(|i| !self.db.records[*i as usize].has_protein())
                .collect()
        } else {
            dna.into_iter().collect()
        };
        // BTreeSet throughout, so this is record order rather than hash order.
        kept.into_iter()
            .map(|i| &self.db.records[i as usize])
            .collect()
    }

    /// Annotate a molecule.
    pub fn annotate(&self, mol: &Molecule) -> Vec<Annotation> {
        let len = mol.seq.len();
        if len == 0 {
            return Vec::new();
        }
        // §7.7 step 2 — duplicate for circularity so origin-spanning features
        // are contiguous in the text being searched.
        let doubled: Vec<u8> = if mol.topology.is_circular() {
            let mut v = mol.seq.clone();
            v.extend_from_slice(&mol.seq);
            v
        } else {
            mol.seq.clone()
        };

        // The ORFs the fusion rule adjudicates against, computed once and only
        // when the database actually holds a row the rule applies to —
        // otherwise this is a six-frame ORF search wasted on every call.
        //
        // Run on `mol.seq`, **not** on `doubled`. Running it on the 2L text
        // would treat a circle as linear and throw away the stop
        // synchronisation that makes ORF calls independent of where the file
        // was cut; it would report every ORF twice; and it would invent
        // incomplete ORFs running off the end of an artefact. So the two
        // subsystems live in different coordinate systems — ORFs in molecule
        // space, hits in doubled space — and `fused_orf` normalises once.
        // `is_designed_peptide`, the same predicate `make` gates on. These two
        // must be the same question: ask it here with `is_peptide_only` and a
        // row carrying both references gets an EMPTY ORF list, so every hit on
        // it fails the fusion rule for want of anything to be fused to — a hit
        // silently dropped by an optimisation rather than by a decision.
        let orfs: Vec<Orf> = if self.db.records.iter().any(|r| r.is_designed_peptide()) {
            orf::find_orfs(
                &mol.seq,
                self.config.code,
                mol.topology.is_circular(),
                &Params {
                    min_aa: ORF_MIN_AA,
                    // A linear fragment carrying a tagged ORF that runs off the
                    // end is a real fusion. On a circle this does nothing:
                    // `find_orfs` deliberately reports no stopless circular ORF.
                    include_incomplete: true,
                    // orf.rs's own module doc: a stop-to-stop run "has no start,
                    // so it is not a thing that could be translated". "Fused to
                    // an ORF" means fused to something a ribosome makes. The
                    // cost is a 5'-truncated fragment — a sequencing read or a
                    // Gibson piece covering the middle of a tagged gene has no
                    // initiator, so no ORF, so no tag. That is the commonest
                    // real miss this rule has.
                    require_start: true,
                    // A nested ORF is always a suffix of a reported one, so it
                    // can never change the predicate's answer: a tag inside the
                    // suffix is also inside the parent. Pure cost — and
                    // therefore, by the same argument, unfalsifiable: setting
                    // it true leaves every test green, as it must. Named here
                    // so nobody reads the suite's silence as coverage.
                    nested: false,
                },
            )
        } else {
            Vec::new()
        };

        let mut hits = Vec::new();
        self.scan_dna(&doubled, len, &mut hits);
        if self.config.protein {
            self.scan_protein(&doubled, len, &orfs, mol.topology.is_circular(), &mut hits);
        }

        let hits = dedupe(hits, len as u64);
        resolve_overlaps(hits, self.config.overlap_trim, len as u64)
    }

    fn scan_dna(&self, doubled: &[u8], len: usize, out: &mut Vec<Annotation>) {
        let rc = iupac::reverse_complement(doubled);
        for reverse in [false, true] {
            let text: &[u8] = if reverse { &rc } else { doubled };
            let seeds = self.dna.seeds(text);
            let slack = 40;
            for chain in self
                .dna
                .chain(&seeds, text.len(), slack, self.config.min_seeds)
            {
                let rec = &self.db.records[chain.record as usize];
                let Some(m) = self.verify(&rec.reference_nt, text, &chain) else {
                    continue;
                };
                // Map back through the reverse complement if we searched it.
                let span = if reverse {
                    (text.len() - m.span.1, text.len() - m.span.0)
                } else {
                    m.span
                };
                if let Some(a) = self.make(
                    Candidate {
                        record: chain.record as usize,
                        span,
                        strand: if reverse {
                            Strand::Reverse
                        } else {
                            Strand::Forward
                        },
                        m,
                        db_units: rec.reference_nt.len(),
                        protein: false,
                    },
                    len,
                    // A peptide-only record has no nucleotides, so it cannot
                    // reach this scan at all: `db_units == 0` and `make`
                    // rejects it. There is nothing here for the fusion rule to
                    // adjudicate, which is why tier 1 needs no ORF list.
                    &[],
                    false,
                ) {
                    out.push(a);
                }
            }
        }
    }

    /// §7.7 step 5 — six-frame translation, which is what finds a marker whose
    /// nucleotides were rewritten for expression in another organism.
    ///
    /// It is also the only route to a peptide-only synthetic part, which is
    /// why `orfs` is threaded down here and nowhere else: those records are
    /// adjudicated by the fusion rule inside [`Annotator::make`].
    ///
    /// # Two routes, and why the second one exists
    ///
    /// Seed-and-chain reaches a record only if it indexed at least
    /// [`Config::min_seeds`] words. [`Index::unchainable`] names the rest, and
    /// they get an exact substring scan of the same frame. Before that scan a
    /// 6-residue peptide indexed two 5-mers, needed three, was absent from
    /// `Index::short` because two is not zero, and was therefore shipped,
    /// seeded, unchainable and never found — with nothing anywhere reporting it
    /// unreachable.
    ///
    /// The scan is exact substring search and nothing else, per
    /// `features/SOURCING.md` §3 ("features under ~15 aa are matched exactly,
    /// never by scored alignment"). It hands `make` the same [`Match`] shape
    /// `verify` produces for an exact whole hit, so the exactness assertion, the
    /// fusion rule, `min_coverage`, the `min_match_len` floor, [`dedupe`] and
    /// [`resolve_overlaps`] all apply unchanged and nothing downstream learns a
    /// second route exists.
    fn scan_protein(
        &self,
        doubled: &[u8],
        len: usize,
        orfs: &[Orf],
        circular: bool,
        out: &mut Vec<Annotation>,
    ) {
        for frame in translate::six_frames(doubled, self.config.code) {
            let strand = if frame.reverse {
                Strand::Reverse
            } else {
                Strand::Forward
            };
            self.scan_protein_exact(&frame, doubled, len, strand, orfs, circular, out);
            // NOT an early return above this line, and the reachable case is
            // narrower than the one first written here. It is *not* "a table of
            // only short peptides": `Annotator::new` refuses anything below
            // MIN_PART_AA = K_PROTEIN, so the shortest peptide that gets this
            // far indexes one word and `words()` is 1.
            //
            // What does reach it is a peptide every one of whose 5-residue
            // windows carries an `X` — [`seedable`] rejects `X`, so such a row
            // indexes zero words however long it is, and a deposited sequence
            // with an unassigned position is where they come from. A one-row
            // table of such a peptide has `words() == 0`, is yielded by
            // `Index::unchainable`, and the guard that used to sit at the top of
            // this function skipped its scan entirely while every mixed-database
            // test stayed green.
            if self.protein.words() == 0 {
                continue;
            }
            let seeds = self.protein.seeds(&frame.protein);
            if seeds.is_empty() {
                continue;
            }
            for chain in self
                .protein
                .chain(&seeds, frame.protein.len(), 12, self.config.min_seeds)
            {
                let rec = &self.db.records[chain.record as usize];
                let Some(aa) = rec.reference_aa.as_deref() else {
                    continue;
                };
                let Some(m) = self.verify(aa, &frame.protein, &chain) else {
                    continue;
                };
                let span = residues_to_bases(&frame, m.span.0, m.span.1, doubled.len());
                if let Some(mut a) = self.make(
                    Candidate {
                        record: chain.record as usize,
                        span,
                        strand,
                        m,
                        db_units: aa.len(),
                        protein: true,
                    },
                    len,
                    orfs,
                    circular,
                ) {
                    a.via_protein = true;
                    out.push(a);
                }
            }
        }
    }

    /// The exact-scan half of [`Annotator::scan_protein`], for one frame.
    ///
    /// Driven by [`Index::unchainable`] — by the number of words a record
    /// indexed, not by its length and not by `Record::is_designed_peptide`.
    /// Keying on the shape of a row is the hole that predicate's own
    /// documentation warns about, and keying on length silently mis-answers for
    /// a peptide carrying an `X`.
    ///
    /// `min_seeds.max(1)` so that a record with **zero** indexed words — the
    /// `Index::short` case — is always scanned, whatever a caller sets. At
    /// `min_seeds = 0` the raw predicate yields nothing, and a record with no
    /// seedable word produces no seeds either, so the two together would leave
    /// it unreachable with nothing saying so. That is the defect this whole
    /// route exists to close, one layer up.
    #[allow(clippy::too_many_arguments)]
    fn scan_protein_exact(
        &self,
        frame: &Frame,
        doubled: &[u8],
        len: usize,
        strand: Strand,
        orfs: &[Orf],
        circular: bool,
        out: &mut Vec<Annotation>,
    ) {
        for i in self.protein.unchainable(self.config.min_seeds.max(1)) {
            let record = i as usize;
            // Non-empty by construction: `unchainable` only yields records the
            // protein index gave a non-zero length, which is `reference_aa`'s.
            let Some(aa) = self.db.records[record].reference_aa.as_deref() else {
                continue;
            };
            // One record overlapping itself is one call, not competing calls,
            // and this is the only place that can say so. `resolve_overlaps`
            // cannot: it compares `core()` intervals shrunk by `overlap_trim`,
            // and `contained_in` is gated on `k.record != h.record`, so two
            // 18 bp hits of the same record 12 bp apart no longer meet once
            // trimmed and both survive. Measured on a synthetic ORF ending in a
            // histidine tract, with the shipped 6-residue row: His6, His7, His8
            // and His9 gave one annotation, His10 through His13 gave **two**
            // overlapping boxes and His14 three — and `His10`/`10xHis` is an
            // alias this very row advertises, so pET-16b would have been drawn
            // with the tag annotated twice.
            //
            // Advanced only on an occurrence that actually became an
            // annotation, not on every occurrence: the gate rejects a hit whose
            // ORF does not contain it, and the leftmost copy of a tract that
            // begins just before an ORF's initiator is exactly that hit. Letting
            // it suppress the copy one residue along would trade a duplicate for
            // a miss.
            let mut next_free = 0usize;
            for pos in exact_occurrences(&frame.protein, aa) {
                if pos < next_free {
                    continue;
                }
                let m = Match {
                    span: (pos, pos + aa.len()),
                    aligned: aa.len(),
                    identity: 1.0,
                };
                let span = residues_to_bases(frame, m.span.0, m.span.1, doubled.len());
                if let Some(mut a) = self.make(
                    Candidate {
                        record,
                        span,
                        strand,
                        m,
                        db_units: aa.len(),
                        protein: true,
                    },
                    len,
                    orfs,
                    circular,
                ) {
                    a.via_protein = true;
                    out.push(a);
                    next_free = pos + aa.len();
                }
            }
        }
    }

    /// Edit distance budget implied by the identity threshold.
    fn budget(&self, aligned_len: usize) -> u32 {
        ((1.0 - self.config.min_identity) * aligned_len as f64).floor() as u32
    }

    /// Align the seed-supported part of a record, then push the boundaries
    /// outward as far as the identity threshold allows.
    ///
    /// The two halves do different jobs. Alignment adjudicates whether this is
    /// the feature at all, over a region seeding says is present. The outward
    /// walk then recovers the true edges, because a mismatch near an end
    /// suppresses every seed overlapping it and would otherwise leave the
    /// feature reported a dozen bases short at each end — a wrong coordinate
    /// dressed as a right one.
    ///
    /// # Why the alignment is anchored on the chain's own diagonal
    ///
    /// `Index::chain` widens each window to `(diagonal - slack, diagonal +
    /// rec_len + slack)` so an indel still fits inside it. For a feature in
    /// direct tandem with a period no larger than `slack` that window holds
    /// whole neighbouring copies, all of them matching at distance 0 — and
    /// [`align::infix`] breaks such ties toward the leftmost end, so every
    /// chain returned the *same* leftmost copy however correctly
    /// `collinear_runs` had separated them. [`dedupe`] then merged the
    /// duplicates and n tandem copies became n − 1 annotations, the last one
    /// gone without a diagnostic. So the chain's diagonal is carried down here
    /// and used as [`align::infix_near`]'s tie-break; it is the only thing that
    /// knows which copy this chain was built on.
    fn verify(&self, record: &[u8], text: &[u8], chain: &crate::index::Chain) -> Option<Match> {
        let (rlo, rhi) = chain.record_span;
        let (wlo, whi) = chain.window;
        let core = &record[rlo..rhi];
        // Where record offset `rhi` falls if the chain's diagonal is right,
        // expressed as an offset into the window actually being aligned.
        // Signed and unclamped on purpose: a chain whose diagonal sits left of
        // the text start yields a negative anchor, which orders candidates
        // perfectly well and must not be silently pulled to 0.
        let anchor = chain.diagonal + rhi as i64 - wlo as i64;
        let hit = align::infix_near(core, &text[wlo..whi], self.budget(core.len()), anchor)?;

        let mut aligned = core.len();
        let mut dist = hit.dist as usize;
        let mut tstart = wlo + hit.start;
        let mut tend = wlo + hit.end;

        // Walk left, then right, taking the furthest point that still satisfies
        // the identity threshold over the whole extended alignment.
        let (add, mism) = walk(record[..rlo].iter().rev(), text[..tstart].iter().rev());
        tstart -= add;
        aligned += add;
        dist += mism;

        let (add, mism) = walk(record[rhi..].iter(), text[tend..].iter());
        tend += add;
        aligned += add;
        dist += mism;

        let identity = 1.0 - dist as f64 / aligned as f64;
        if identity < self.config.min_identity {
            return None;
        }
        Some(Match {
            span: (tstart, tend),
            aligned,
            identity,
        })
    }

    /// Turn a verified match into an annotation, or reject it as too thin.
    ///
    /// `orfs` and `circular` are only consulted for a peptide-only synthetic
    /// part; every other record ignores them entirely, so no existing
    /// behaviour depends on them.
    fn make(&self, c: Candidate, len: usize, orfs: &[Orf], circular: bool) -> Option<Annotation> {
        let Candidate {
            record,
            span,
            strand,
            m,
            db_units,
            protein,
        } = c;
        // A hit lying wholly in the second copy is the same feature as one in
        // the first; keep only those that start in the real molecule.
        if span.0 >= len || span.1 <= span.0 || db_units == 0 {
            return None;
        }
        // A database record longer than a circular molecule can match more
        // bases than the molecule has: a 40 bp terminal repeat matched 1240
        // bases of a 1200 bp plasmid, which came back flagged `wraps_origin`
        // while `start <= end`, with `len()` bigger than the molecule and
        // coverage 1.000. Clamp the span so those three agree.
        let span = (span.0, span.1.min(span.0 + len));

        // The two rules that apply to a TRANSLATED hit on a designed peptide
        // part, and to nothing else. Placed here, after the second-copy
        // rejection and after the clamp, because that placement is
        // load-bearing: `span.0 >= len` has already dropped the copy of an
        // origin-spanning hit that lives wholly in the doubled text's second
        // half, so each occurrence is adjudicated exactly once, with `span.0`
        // in `[0, L)` — which is what `fused_orf`'s anchor arithmetic assumes.
        //
        // The scope is [`Record::is_designed_peptide`] — a `synthetic_part`
        // carrying residues — and NOT `is_peptide_only`. The difference is a
        // row carrying both a nucleotide reference and a peptide, which the
        // relaxed schema permits and no shipped row has. Keyed on the absence
        // of nucleotides, such a row would take an eight-residue peptide into
        // the six-frame scan with no exactness rule and no ORF anywhere: a hole
        // opened by the shape of a row rather than by anything anyone decided,
        // and the loader is this project's answer to "discipline is not a
        // control". Keyed on the peptide, the rules follow the thing they are
        // about.
        //
        // `protein &&` rather than `!protein || … return None`, and that is the
        // other half of the same fix. The extra rules are about the TRANSLATED
        // route; a `synthetic_part` that carries real nucleotides — the eight
        // parented tags, HA through F2A — must keep finding them by tier 1
        // exactly as it did before any of this, ungated.
        let mut fusion = None;
        let rec = &self.db.records[record];
        if protein && rec.is_designed_peptide() {
            let aa = rec.reference_aa.as_deref().unwrap_or_default();
            // EXACT AND WHOLE, regardless of `Config::min_identity`.
            //
            // Today this falls out of the arithmetic — `budget()` returns
            // `floor((1 - 0.96) * len)`, which is 0 for anything under 25
            // residues — but `min_identity` is documented as user-adjustable,
            // and at 0.80 an 8-mer gets a budget of 1 and a 7-of-8 match passes
            // the identity test. The annotator would then report FLAG tags that
            // are not there, which is precisely what SOURCING.md §3's rule
            // ("features under ~15 aa are matched exactly, never by scored
            // alignment") exists to prevent. Asserted rather than inherited.
            //
            // Whole as well as exact, and at every length rather than only
            // under 15 aa: these are *designed* parts whose boundary is the
            // design, so a 12-residue partial of the 38-residue SBP tag is not
            // a fragment of anything and must not be drawn as one. A useful
            // side effect is that `is_fragment` below can never be true here,
            // so the fragment machinery needs no special case.
            if m.aligned != aa.len() || m.identity < 1.0 {
                return None;
            }
            // FUSED TO AN ORF, or ignored. The PI's decision of 2026-07-28.
            let hit = fused_orf(orfs, strand, span, len, m.aligned, circular)?;
            fusion = Some(FusionOrf {
                start: hit.start,
                end: hit.end,
                strand: hit.strand,
                aa_len: hit.aa_len,
            });
        }

        let coverage = (m.aligned.min(db_units) as f64 / db_units as f64).min(1.0);
        // Too little of the feature to be a claim about the feature at all.
        if coverage < self.config.min_coverage {
            return None;
        }
        let floor = if protein {
            self.config.min_match_len / 3
        } else {
            self.config.min_match_len
        };
        if m.aligned < floor.min(db_units) {
            return None;
        }

        let mut a = Annotation::from_span(record, span, len, strand);
        debug_assert!(
            !a.wraps_origin || a.start > a.end,
            "wraps_origin must mean start > end, got {}..{}",
            a.start,
            a.end
        );
        a.identity = m.identity;
        a.coverage = coverage;
        // §7.7 step 7.
        a.score = (span.1 - span.0) as f64 * a.identity * a.coverage;
        a.is_fragment = a.coverage < self.config.fragment_coverage;
        a.fusion_orf = fusion;
        Some(a)
    }
}

/// The fusion predicate: is this peptide hit in frame inside an ORF, with a
/// partner?
///
/// Returns the ORF it was admitted on, or `None`. **Any** ORF satisfying it is
/// enough — `.any()`, never one emission per ORF, because a tag sitting inside
/// a forward ORF and an overlapping reverse one must produce one annotation
/// rather than two.
///
/// # Inputs
///
/// `span` is 0-based half-open in **doubled** coordinates, with `span.0 < len`
/// guaranteed by the caller; `len` is the molecule's length; `tag_aa` is the
/// residues actually matched, which the caller's exactness rule has already
/// forced to be the whole peptide.
///
/// # Five things it deliberately does not do
///
/// 1. **It never compares frame numbers.** `Frame::offset` is an offset into
///    the doubled text; `Orf::frame` is an offset into the *reverse complement*
///    for a reverse ORF, and for a merged circular frame (`L % 3 != 0`) it is
///    not a frame index at all but the offset from the origin. Comparing the
///    two is a category error that would be right about a third of the time.
///    `d.is_multiple_of(3)` asks the same question in one coordinate system and is
///    immune to all of it — which is also why the doubled text's six frames not
///    being the circle's six frames never has to be reasoned about.
/// 2. **It never branches on `Orf::wrapped`.** It is not needed.
/// 3. **It never uses `end - start` as an extent.** orf.rs is explicit that the
///    inclusive range is not the extent when `laps != 0`, and that `end < start`
///    is not a test for origin-crossing.
/// 4. **It uses `3 * aa_len`, not `Orf::bases()`.** `bases()` includes the stop
///    codon and `aa_len` excludes it. The two differ observably in tables 27,
///    28 and 31, where a codon can be both a terminator and a residue — in
///    table 31 `TAA` and `TAG` are stops *and* encode `E`, so a C-terminal
///    AviTag or ALFA-tag whose final E sits on such a codon is rejected. That
///    is a named miss and the right call: orf.rs's own `is_ambiguous_stop` doc
///    says an ORF ending there is a guess.
///
///    **Honest status: no test in this crate distinguishes the two.** Swapping
///    `3 * aa_len` for `bases()` changes no fixture's verdict, because every
///    fixture uses table 1, where the two differ by exactly the stop codon and
///    no peptide can occupy it. This clause is defensive, and saying it is
///    covered would be a claim the suite does not support.
/// 5. **`d >= 0` and `<=`, not `d > 0` and `<`.** Both boundaries are hit by
///    ordinary real constructs; see the containment test's own comment.
///
/// # Internal stops need no separate clause
///
/// `find_orfs` with `require_start: true` returns a run from a start codon to
/// the **first** in-frame stop, so by construction there is no in-frame stop
/// strictly inside `[0, 3 * aa_len)`. `d >= 0` puts the tag downstream of the
/// initiator and `d + 3 * tag_aa <= 3 * aa_len` puts it upstream of the
/// terminator, so nothing terminating lies between them. This is a property a
/// tier-1 CDS annotation would *not* have had: a database CDS mapped onto a
/// user's construct carries the database's extent, so a clone that has acquired
/// a nonsense mutation would still be certified as a fusion straight through a
/// stop codon that is really there.
fn fused_orf(
    orfs: &[Orf],
    strand: Strand,
    span: (usize, usize),
    len: usize,
    tag_aa: usize,
    circular: bool,
) -> Option<&Orf> {
    // `Strand` has four variants and neither subsystem produces the other two,
    // but a `_ =>` arm falling through to Forward would silently adjudicate an
    // unoriented hit on the plus strand. Refuse instead.
    //
    // HONEST STATUS: defensive and uncovered. (The `laps` loop below used to be
    // named here as the same kind of thing; it is not — see its own note — and
    // `a_fusion_admitted_only_on_an_orfs_second_lap` covers it.)
    // `scan_protein` derives `strand` from `frame.reverse`
    // alone, so only Forward and Reverse ever arrive, and turning this arm into
    // an ordinary non-match leaves the whole suite green. It is kept because
    // the cost is one line and the alternative — a wildcard — is the shape of
    // bug that produces a confident answer about the wrong strand, not because
    // a test says it fires.
    let rev = match strand {
        Strand::Forward => false,
        Strand::Reverse => true,
        Strand::Unoriented | Strand::Both => return None,
    };
    let (s, e) = span;
    if len == 0 || e <= s {
        return None;
    }

    orfs.iter().find(|o| {
        if o.strand != strand {
            return false;
        }
        // Both anchors are 0-based, plus-strand, and inside [0, L).
        //
        // A reverse ORF's 5' end is its HIGH plus-strand coordinate:
        // `orf::make` maps reverse-complement index `from` — the start codon —
        // to plus-strand `n - 1 - from`, which lands on `Orf::end`.
        // Symmetrically `residues_to_bases` returns `(last.0, first.1)` for a
        // reverse frame, so the tag's first residue occupies `[e - 3, e)` and
        // its 5'-most base is `e - 1`. The `% len` is required because `e` may
        // exceed `len` for an origin-spanning tag; `s` never does. Swapping
        // these two anchors is the mirrored-coordinate bug that
        // `residues_to_bases` already carries a comment about.
        let (o5, t5) = if rev {
            (o.end as usize - 1, (e - 1) % len)
        } else {
            (o.start as usize - 1, s)
        };

        // Distance from the ORF's initiator to the tag's first base, measured
        // along the ORF's own reading direction.
        let d0 = if circular {
            if rev {
                (o5 + len - t5) % len
            } else {
                (t5 + len - o5) % len
            }
        } else {
            // Deliberately not `% len` on a linear molecule. It happens to be
            // harmless — the wrapped value always overshoots containment — but
            // relying on that coincidence is unreadable and one edit from wrong.
            if rev {
                if o5 < t5 {
                    return false;
                }
                o5 - t5
            } else {
                if t5 < o5 {
                    return false;
                }
                t5 - o5
            }
        };

        let coding = 3 * o.aa_len;
        // `laps` is enumerated because when 3 does not divide L the frame
        // visits every position, one turn is L codons, and the ORF can walk up
        // to 3L bases — so the same physical base is visited up to three times
        // at different codon offsets, and `d0` alone cannot say which visit the
        // tag is on. Since `laps != 0` only when `3 ∤ L`, at most one k in any
        // three consecutive values can satisfy `d.is_multiple_of(3)`, so this is not
        // ambiguous.
        //
        // NOT a small-circle guard, whatever the size of the molecule suggests.
        // This used to claim it was, deriving `L <= 78` from `3 * aa_len + 3 >
        // L` together with the partner floor `aa_len >= 25` — an inequality run
        // backwards, since a *lower* bound on `aa_len` bounds `L` from above by
        // nothing at all. The real condition is `bases() > L`: a frame that runs
        // stopless for more than one lap, which `find_orfs` can only produce
        // when `3 ∤ L` and which is governed by stop-codon spacing rather than
        // by `L`. `a_fusion_admitted_only_on_an_orfs_second_lap` below is a 223
        // bp circle whose sole Forward ORF has `aa_len = 148` and `laps = 2`,
        // and whose tag is admitted at k = 1 and at no other k — delete this
        // loop and that annotation disappears. The same construct still needs
        // k = 1 at 9103 bp. What DOES keep it off real DNA is the stop density:
        // a stopless run of more than L/3 consecutive codons against roughly
        // 3/64 per position does not happen in a plasmid.
        (0..=o.laps as usize).any(|k| {
            let d = d0 + k * len;
            d.is_multiple_of(3)                                     // in frame with the ORF
                && d + 3 * tag_aa <= coding                // inside its coding part
                && o.aa_len >= tag_aa + PARTNER_MIN // fused to something
        })
    })
}

/// Every start offset at which `needle` occurs in `hay`, **overlaps included**.
///
/// Reporting the overlaps is this function's job and collapsing them is not:
/// stepping by `needle.len()` here would make the answer depend on where the
/// scan started, and on the doubled text of a circle that is origin-dependent —
/// the class of bug [`dedupe`] exists for. The caller
/// ([`Annotator::scan_protein_exact`]) takes the leftmost of each overlapping
/// group *after* the fusion gate has spoken, which is the only point at which
/// "this hit survived" is known.
///
/// It used to be [`resolve_overlaps`] that was expected to collapse them, and
/// that was wrong from a 10-residue tract upward — the note is left here because
/// the mistake is easy to make twice. `resolve_overlaps` compares `core()`
/// intervals shrunk by `overlap_trim`, so at 15% two 18 bp hits 12 bp apart stop
/// meeting; `contained_in` cannot rescue them because it is gated on
/// `k.record != h.record`. Measured: His6 through His9 collapsed to one
/// annotation, His10 through His13 to two, His14 to three.
///
/// A hand-written double loop. No external crate is permitted in this workspace
/// and `memchr` is one; the needles here are at most 38 residues, where KMP buys
/// nothing worth its complexity.
///
/// Case-insensitive, like [`walk`] and like the index's own hash. Honest status:
/// unreachable through [`Db::parse`], which upper-cases `reference_aa`, and
/// through [`translate::six_frames`], whose residues come from the code table
/// and are upper-case. It is reachable through a hand-constructed [`Record`] —
/// the fields are public — and that is what the test pins. Cheaper than a rule
/// somebody has to remember.
fn exact_occurrences(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    if needle.is_empty() || hay.len() < needle.len() {
        return out;
    }
    for start in 0..=hay.len() - needle.len() {
        if hay[start..start + needle.len()]
            .iter()
            .zip(needle)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            out.push(start);
        }
    }
    out
}

/// Everything needed to decide whether a verified match becomes an annotation.
///
/// A struct rather than seven positional arguments, because two of them are
/// lengths in different units and one is a flag saying which — exactly the
/// shape that produces a silent unit bug. `db_units` is the reference length
/// **in the same units as `Match::aligned`**: residues for a translated hit,
/// bases for a nucleotide one. Mixing them understates a protein hit's coverage
/// threefold, which marks every full-length translated match as a fragment.
struct Candidate {
    record: usize,
    span: (usize, usize),
    strand: Strand,
    m: Match,
    db_units: usize,
    protein: bool,
}

/// A verified match: where in the text, how much of the record, how well.
#[derive(Debug, Clone, Copy)]
pub struct Match {
    span: (usize, usize),
    /// Symbols of the record actually accounted for — the numerator of coverage.
    aligned: usize,
    identity: f64,
}

/// Extend an alignment outward one symbol at a time, stopping when the
/// extension stops looking like sequence and starts looking like coincidence.
///
/// Ungapped on purpose: this runs *outside* an already-verified core, where the
/// only question is where the feature ends, and allowing gaps there lets an
/// alignment wander into unrelated sequence to buy a few more matches.
///
/// # Why a local rule and not the identity threshold
///
/// The obvious criterion — "extend while overall identity stays ≥96%" — is
/// wrong, and wrong in a way that looks fine in a unit test. A 300 bp core
/// matching perfectly can absorb twelve consecutive mismatches and still report
/// 96%, so the walk marches a dozen bases into the random flank and reports a
/// feature that is measurably longer than the feature. The budget must not be
/// spendable on the edges.
///
/// So this is BLAST's X-drop instead: score `+1` per match, `-3` per mismatch,
/// keep the best-scoring point, and give up once the score falls `X` below it.
/// Unrelated DNA matches a quarter of the time, worth `-2` per base on average,
/// so it stops within a few bases; a genuine 96% extension gains `+0.84` per
/// base and runs as far as the feature does.
fn walk<'a>(
    record: impl Iterator<Item = &'a u8>,
    text: impl Iterator<Item = &'a u8>,
) -> (usize, usize) {
    const MISMATCH: i32 = -3;
    const X_DROP: i32 = 10;

    let mut best = (0usize, 0usize);
    let mut best_score = 0i32;
    let mut score = 0i32;
    let mut mismatches = 0usize;

    for (i, (r, t)) in record.zip(text).enumerate() {
        if r.eq_ignore_ascii_case(t) {
            score += 1;
        } else {
            score += MISMATCH;
            mismatches += 1;
        }
        if score > best_score {
            best_score = score;
            best = (i + 1, mismatches);
        }
        if best_score - score > X_DROP {
            break;
        }
    }
    best
}

/// Map a residue range in a frame back to base coordinates.
///
/// The reverse frames run *backwards* along the sequence, so the first residue
/// of a protein hit is at the high end. Getting this the wrong way round places
/// every translated hit at a mirrored coordinate — plausible-looking and wrong.
fn residues_to_bases(frame: &Frame, aa_start: usize, aa_end: usize, len: usize) -> (usize, usize) {
    if aa_end <= aa_start {
        return (0, 0);
    }
    let first = frame.to_source(aa_start, len);
    let last = frame.to_source(aa_end - 1, len);
    if frame.reverse {
        (last.0, first.1)
    } else {
        (first.0, last.1)
    }
}

/// §7.7 step 9 — one feature at one place, once.
///
/// Keyed by `(record, start mod L)` so the two copies produced by doubling a
/// circular molecule collapse. Where they differ, the longer hit wins, which is
/// what keeps the origin-spanning version rather than the truncated one.
fn dedupe(mut hits: Vec<Annotation>, len: u64) -> Vec<Annotation> {
    hits.sort_by(|a, b| {
        (a.record, a.start)
            .cmp(&(b.record, b.start))
            .then(b.len(len).cmp(&a.len(len)))
            .then(
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    let mut out: Vec<Annotation> = Vec::new();
    for h in hits {
        let dup = out
            .iter()
            .any(|k| k.record == h.record && k.start == h.start && k.strand == h.strand);
        if !dup {
            out.push(h);
        }
    }
    out
}

/// §7.7 step 7's overlap rule, applied across records.
///
/// Each hit is shrunk by `trim` of its length at both ends before overlap is
/// tested, so features that merely abut survive together while two competing
/// calls for the same span do not.
///
/// Two hits nest rather than compete when one lies strictly inside the other —
/// an operator within a promoter, an M13 site within lacZ-alpha — and both are
/// kept. That is a deliberate departure from pLannotate, which drops the inner
/// one; SnapGene shows both, and so do we.
fn resolve_overlaps(mut hits: Vec<Annotation>, trim: f64, len: u64) -> Vec<Annotation> {
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then((a.record, a.start).cmp(&(b.record, b.start)))
    });

    let core = |a: &Annotation| -> (i64, i64) {
        let l = a.len(len) as i64;
        let cut = (l as f64 * trim).round() as i64;
        let s = a.start as i64 + cut;
        (s, a.start as i64 + l - cut)
    };

    // On a circle, "overlapping" cannot be decided by comparing two intervals
    // as written: an origin-spanning feature runs past `len` in unwrapped
    // coordinates and so never numerically meets the copy of itself sitting at
    // base 1. Comparing against the ±L translates is what closes the circle.
    let overlaps = |a: (i64, i64), b: (i64, i64)| -> bool {
        let l = len as i64;
        [-l, 0, l]
            .iter()
            .any(|shift| a.0 < b.1 + shift && b.0 + shift < a.1)
    };

    // Is `a` *strictly* inside `b`, on any of the circle's translates?
    //
    // Strictly: two hits with the identical span are competing calls, not a
    // nesting, and treating equality as containment let three near-identical
    // records stack three annotations on one locus — the very thing the
    // cross-record rule exists to prevent.
    let contained_in = |a: (i64, i64), b: (i64, i64)| -> bool {
        let l = len as i64;
        (a.1 - a.0) < (b.1 - b.0)
            && [-l, 0, l]
                .iter()
                .any(|shift| b.0 + shift <= a.0 && a.1 <= b.1 + shift)
    };

    let mut kept: Vec<Annotation> = Vec::new();
    for h in hits {
        let hc = core(&h);
        let clash = kept.iter().any(|k| {
            if !overlaps(hc, core(k)) {
                return false;
            }
            // Containment is not competition.
            //
            // §7.7 step 7 is a cross-hit rule -- "drop lower-scoring hits that
            // still overlap" -- and this was gated on `k.record == h.record`,
            // so it never fired between different database records and three
            // near-identical records at one locus produced three stacked
            // annotations of the same span.
            //
            // Applying it across records unmodified goes too far the other
            // way: a *nested* record, a lac operator inside a lac promoter or
            // an M13 site inside lacZ-alpha, would silently vanish. Those are
            // real annotations rather than duplicate calls, SnapGene shows
            // them, and PLAN 8.3 plans to add exactly that shape. So a hit
            // wholly inside a better-scoring one of a *different* record is
            // kept; only partial overlap -- two calls competing for one span --
            // resolves to the higher score.
            if k.record != h.record && contained_in(hc, core(k)) {
                return false;
            }
            true
        });
        if !clash {
            kept.push(h);
        }
    }
    kept.sort_by_key(|a| (a.start, a.end, a.record));
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoundaryRule, Class, Record, ReviewStatus};
    use pl_core::Topology;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn dna(&mut self, n: usize) -> String {
            (0..n)
                .map(|_| b"ACGT"[(self.next() % 4) as usize] as char)
                .collect()
        }
    }

    fn rec(id: &str, seq: &str, protein: bool) -> Record {
        Record {
            id: id.into(),
            name: id.into(),
            aliases: vec![],
            class: Class::Cds,
            genbank_key: "CDS".into(),
            // A protein-only test record still needs a nucleotide reference,
            // because the schema requires every row to have one; it is made
            // deliberately unmatchable so it cannot satisfy a test by accident.
            reference_nt: if protein {
                b"ATG".to_vec()
            } else {
                seq.as_bytes().to_ascii_uppercase()
            },
            reference_aa: if protein {
                Some(seq.as_bytes().to_ascii_uppercase())
            } else {
                None
            },
            boundary_rule: BoundaryRule::OrfAtgToStop,
            boundary_evidence: "test".into(),
            description: String::new(),
            review_status: ReviewStatus::Proposed,
            curator: String::new(),
            date_added: "2026-07-27".into(),
            patent_flag: false,
            notes: String::new(),
        }
    }

    /// A peptide-only synthetic part: the shape decision 1 created.
    ///
    /// No nucleotides at all, which is the point — a tag has dozens of
    /// synonymous encodings and any one of them would be an arbitrary choice.
    fn peptide(id: &str, aa: &str) -> Record {
        Record {
            id: id.into(),
            name: id.into(),
            aliases: vec![],
            class: Class::SyntheticPart,
            genbank_key: "misc_feature".into(),
            reference_nt: Vec::new(),
            reference_aa: Some(aa.as_bytes().to_ascii_uppercase()),
            boundary_rule: BoundaryRule::DesignedSequence,
            boundary_evidence: "test".into(),
            description: String::new(),
            review_status: ReviewStatus::Proposed,
            curator: String::new(),
            date_added: "2026-07-28".into(),
            patent_flag: false,
            notes: String::new(),
        }
    }

    /// The FLAG epitope, as `features/build/stage_curated.py` declares it and
    /// as RCSB polymer entity 8RMO_1 ("FLAG-tag") carries it in full.
    ///
    /// Not recalled: this is the same eight residues the shipped table holds,
    /// and the row it belongs to is PLF:3000.
    const FLAG: &str = "DYKDDDDK";

    /// The SBP-tag, from RCSB polymer entity 4JO6_2 ("SBP-Tag", the entire
    /// entity) — PLF:3011 in the shipped table.
    ///
    /// Used here for one property no other shipped part has: it **begins with
    /// M**, so it can be placed with its own first residue as the ORF's
    /// initiator, which is the `d == 0` boundary of the containment test.
    const SBP: &str = "MDEKTTGWRGGHVVEGLAGELEQLRARLEHHPQGQREP";

    /// The polyhistidine tag, six residues, as `features/build/stage_curated.py`
    /// declares it and as wwPDB polymer entity 1KTR_2 ("Oligohistidine peptide
    /// Antigen") carries it — the entire entity.
    ///
    /// Not recalled: the build locates this string in that entity at fetch time
    /// before it will issue the row, and this constant is the same six residues.
    /// The row is the one this change issues.
    const HIS6: &str = "HHHHHH";

    /// The thrombin cleavage site, six residues, from
    /// `features/build/stage_curated.py`; witnessed by wwPDB 10EE_1, which
    /// carries the pET-28a cassette around it.
    ///
    /// Used here for a mechanical property rather than a biological one: six
    /// residues is two 5-mer windows, one short of `Config::min_seeds`, so it is
    /// the middle of the band that used to be silently unfindable. Unlike
    /// [`HIS6`] it contains no residue whose codons are all table-11
    /// initiators, so [`encode`]'s strict start-free constraint can express it —
    /// which the "outside any ORF" case needs.
    const THROMBIN: &str = "LVPRGS";

    /// The fourteen residues immediately upstream of a His6 tag on the
    /// maintainer's own plasmids.
    ///
    /// Measured, not recalled. Every peptide in the shipped table was counted
    /// against 73 real plasmid and contig files from this machine —
    /// 17,061,931 residues of ATG-to-stop ORFs of at least 25 aa on both
    /// strands — counting only occurrences the shipped fusion gate would report.
    /// Two peptides occurred at all: `IEGR` 154 times, and this tag 8 times.
    /// All eight were the same shape — the tag at exactly -0 residues from the
    /// stop, behind a GG linker, in files named for it — across two constructs,
    /// one ending in this context and one a 3796 aa ORF. Zero were chance.
    ///
    /// TWO ORF LENGTHS, and they are not the same measurement. `pl annotate
    /// --include-proposed "pKoV with His decR.dna"` reports the tag at
    /// 5885..5902 "in frame with a **258 aa** ORF at 5129..5905 +". That ORF
    /// opens on `ATT`, one of table 11's seven initiators, which is the code
    /// [`Config::default`] uses. The first `ATG` in the same frame is 98 codons
    /// further in, leaving **160** residues to the stop — so 160 is the ATG-only
    /// reading and 258 is what this tool says. The fixture below builds its own
    /// ATG-started ORF and is therefore 160 aa by construction; that number
    /// describes the fixture, not the file.
    ///
    /// That measurement is why the row ships, and this fixture is the reason
    /// the whole change exists: at HEAD the shipped tool cannot find any of the
    /// eight.
    const HIS6_CONTEXT: &str = "EQIKYTTSLPIEGG";

    /// (GGGGS)4, the flexible linker shipped as PLF:3026.
    ///
    /// Here for one mechanical property rather than a biological one: 20
    /// residues — long enough to clear `min_match_len / 3` — and encodable
    /// under [`encode`]'s start-free constraint, which SBP is not. See
    /// [`encode`]'s note on tryptophan.
    const GS_LINKER: &str = "GGGGSGGGGSGGGGSGGGGS";

    /// Filler for the sense codons of a fixture ORF.
    ///
    /// GC-only, and `GCC` rather than `GCT`. `GCT` repeated spells `CTG` in
    /// frame 1, which is a table-1 start codon, so a fixture meant to hold one
    /// ORF quietly grows a second and a negative fixture passes for the wrong
    /// reason. pl-core's own `orf.rs` carries the same constant and asserts the
    /// property; this is the second place that trap bites.
    const FILLER: &str = "GCC";

    /// Back-translate `peptide` into DNA that cannot fabricate an ORF around
    /// itself.
    ///
    /// The constraint is the trap this whole fixture family is built on. An
    /// encoding is usable only if no **forward** frame of
    /// `GCCGCC + dna + GCCGCC` spells a start or a stop:
    ///
    /// - without the no-stop half, the out-of-frame fixture's displaced tag
    ///   codons spell a terminator and truncate the very ORF the fixture
    ///   needs — so it passes while testing nothing;
    /// - without the no-start half, the outside-any-ORF fixture grows an ORF of
    ///   its own, and the tag is then admitted *correctly*, for a reason the
    ///   fixture did not intend.
    ///
    /// Forward frames only, deliberately. A start or stop on the reverse strand
    /// can only produce a reverse ORF, and the predicate's first clause refuses
    /// an ORF whose strand differs from the hit's — so a reverse artefact
    /// cannot change any answer here. Constraining six frames instead of three
    /// makes some peptides unencodable for no gain.
    ///
    /// A depth-first search with backtracking, not random draws. The constraint
    /// is genuinely tight — codon boundaries interact across residues, so
    /// `...GGA` followed by `TGG` for Trp spells `ATG` in the next frame along
    /// and only `GGG` before it works — and a 37-residue peptide is never going
    /// to satisfy it by chance. Randomised option order with a fixed seed keeps
    /// the result deterministic without making it look hand-picked.
    ///
    /// # Which peptides this is impossible for, and why it got worse
    ///
    /// Because the three forward frames together cover every offset, the
    /// constraint is really "no three-base window anywhere spells a start or a
    /// stop". Under the shipped table 11 the forbidden set is ten words — the
    /// three stops plus seven initiators — and that makes whole residues
    /// unencodable rather than merely awkward:
    ///
    /// - **M**, whose only codon `ATG` is an initiator in every table;
    /// - **I**, whose three codons `ATT`/`ATC`/`ATA` are *all* table-11 starts;
    /// - **W** anywhere but the first position: its only codon is `TGG`, and
    ///   every base that could precede it makes `ATG`, `CTG`, `GTG` or `TTG`.
    ///
    /// SBP-tag contains W, so no start-free encoding of it exists at all — a
    /// fact discovered when [`Config::default`] moved to table 11 and three
    /// fixtures stopped building. Those fixtures use [`encode_stopless`] and
    /// say why.
    fn encode(peptide: &str, code: Code, rng: &mut Rng) -> String {
        encode_in(peptide, code, rng, FILLER, false, true)
    }

    /// [`encode`], forbidding stops but **not** starts.
    ///
    /// For the peptides the full constraint cannot express (see above). The
    /// weaker guarantee is enough wherever a fixture's tag sits in a frame
    /// whose *first* start is the fixture's own initiator: an extra start in
    /// another frame produces an ORF the predicate cannot use, because
    /// containment demands `d.is_multiple_of(3)` in the ORF's own frame, and an extra
    /// start further into the same frame is nested and suppressed by
    /// `Params::nested: false`. It is **not** enough for a fixture that needs
    /// "no ORF covers this at all"; those must use a peptide [`encode`] can
    /// handle, and each such call site says which property it is relying on.
    fn encode_stopless(peptide: &str, code: Code, rng: &mut Rng) -> String {
        encode_in(peptide, code, rng, FILLER, false, false)
    }

    /// `encode`, with the surrounding filler, the reverse-frame constraint and
    /// the start constraint under the caller's control.
    ///
    /// `rev0` additionally forbids a stop in frame 0 of the encoding's *reverse
    /// complement*, which one fixture needs: the reverse-ORF test plants the
    /// tag so that its reverse complement sits inside a reverse-strand reading
    /// frame, and a stop there would truncate the very ORF the fixture is built
    /// around.
    fn encode_in(
        peptide: &str,
        code: Code,
        rng: &mut Rng,
        filler: &str,
        rev0: bool,
        no_starts: bool,
    ) -> String {
        assert!(
            !no_starts || !peptide.contains('M'),
            "methionine's only codon is ATG, which is a start codon in every \
             table, so no encoding of {peptide} can avoid fabricating an ORF"
        );
        let options: Vec<Vec<[u8; 3]>> = peptide
            .bytes()
            .map(|aa| {
                let mut v = Vec::new();
                for b1 in b"TCAG" {
                    for b2 in b"TCAG" {
                        for b3 in b"TCAG" {
                            let c = [*b1, *b2, *b3];
                            if code.codon(&c) == aa {
                                v.push(c);
                            }
                        }
                    }
                }
                assert!(!v.is_empty(), "no codon encodes {}", aa as char);
                // Shuffled so the fixtures are not all first-codon-in-TCAG-order,
                // which would make them agree with each other by construction.
                for i in (1..v.len()).rev() {
                    v.swap(i, (rng.next() % (i as u64 + 1)) as usize);
                }
                v
            })
            .collect();

        // The context the tag will actually sit in, so the joins at both ends
        // are constrained too.
        let pad = filler.repeat(2);
        let mut dna: Vec<u8> = pad.as_bytes().to_vec();
        // Every codon that became complete when three bytes were appended at
        // `p` starts at `p - 2`, `p - 1` or `p`.
        let forbidden = |c: &[u8]| code.is_stop(c) || (no_starts && code.is_start(c));
        let joins_clean = |dna: &[u8], p: usize| -> bool {
            (p.saturating_sub(2)..=p).all(|i| i + 3 > dna.len() || !forbidden(&dna[i..i + 3]))
        };
        let rev_clean = |dna: &[u8]| -> bool {
            !rev0
                || iupac::reverse_complement(dna)
                    .chunks_exact(3)
                    .all(|c| !code.is_stop(c))
        };
        let mut choice = vec![0usize; options.len()];
        let mut at = 0usize;
        let mut steps = 0u64;
        while at < options.len() {
            steps += 1;
            assert!(
                steps < 5_000_000,
                "the search for an encoding of {peptide} did not terminate"
            );
            if choice[at] >= options[at].len() {
                choice[at] = 0;
                assert!(at > 0, "no encoding of {peptide} satisfies the constraint");
                at -= 1;
                dna.truncate(dna.len() - 3);
                choice[at] += 1;
                continue;
            }
            let p = dna.len();
            dna.extend_from_slice(&options[at][choice[at]]);
            // The join into the TRAILING pad, checked while the search can
            // still back out of it. Nothing checked it before: the pad is
            // appended after the loop, so the final codon's two overhanging
            // frames were constrained by nothing and the definitive assertion
            // below was the first thing to see them. `LVPRGS` is the peptide
            // that exposed it — the search committed to `...AGT` for the serine,
            // which spells `GTG` against the pad, and the helper reported the
            // peptide unencodable when four of its six serine codons are fine.
            // Every encoding this helper had produced until then was clean by
            // luck at that one join.
            let tail_clean = at + 1 < options.len() || {
                let mut probe = dna.clone();
                probe.extend_from_slice(pad.as_bytes());
                joins_clean(&probe, p + 3)
            };
            // `rev_clean` is re-checked over the whole prefix rather than
            // incrementally: reverse-complementing reverses the codon
            // boundaries, so appending three bases changes which reverse codons
            // exist from the front, not only at the end.
            if tail_clean && joins_clean(&dna, p) && rev_clean(&dna[pad.len()..]) {
                at += 1;
            } else {
                dna.truncate(p);
                choice[at] += 1;
            }
        }
        dna.extend_from_slice(pad.as_bytes());

        // The definitive check, over the whole padded string in all three
        // forward frames. The incremental test above is an optimisation and
        // this is the claim.
        for f in 0..3 {
            for c in dna[f..].chunks_exact(3) {
                assert!(
                    !forbidden(c),
                    "the encoding of {peptide} spells {} in frame {f}",
                    String::from_utf8_lossy(c)
                );
            }
        }
        let out = String::from_utf8(dna[pad.len()..dna.len() - pad.len()].to_vec()).unwrap();
        assert_eq!(
            code.translate(out.as_bytes()),
            peptide.as_bytes(),
            "the encoding does not spell the peptide it was asked for"
        );
        out
    }

    /// The code every fusion fixture below must be built under.
    ///
    /// Read off `Config::default()` rather than written as `TABLE1`, and that
    /// is load-bearing rather than tidy. `encode` guarantees its output spells
    /// no start codon *for the code it is given*; if that code is not the one
    /// the annotator runs `find_orfs` with, a fixture meant to hold one ORF can
    /// quietly grow another, and a negative case then passes for the wrong
    /// reason. Tying the two together means changing the annotator's default —
    /// which is exactly what the GTG regression below forced — cannot leave a
    /// fixture silently mis-built.
    fn fixture_code() -> Code {
        Config::default().code
    }

    /// Does some translated frame of `seq` contain `peptide` verbatim?
    ///
    /// The guard every negative fixture below needs. Without it, "the tag was
    /// not reported" is equally consistent with "the fusion rule rejected it"
    /// and with "the tag is not in this molecule at all", and only the first is
    /// what the test claims to show.
    fn six_frame_contains(seq: &[u8], peptide: &str) -> bool {
        translate::six_frames(seq, translate::TABLE1)
            .iter()
            .any(|f| {
                f.protein
                    .windows(peptide.len())
                    .any(|w| w == peptide.as_bytes())
            })
    }

    fn db_of(r: Vec<Record>) -> Db {
        Db {
            records: r,
            provenance: vec![],
            version: "test".into(),
        }
    }

    fn mol(seq: &str, circular: bool) -> Molecule {
        Molecule {
            seq: seq.as_bytes().to_vec(),
            topology: if circular {
                Topology::Circular
            } else {
                Topology::Linear
            },
            ..Default::default()
        }
    }

    #[test]
    fn a_planted_feature_is_found_at_the_right_coordinates() {
        let mut rng = Rng(0x1234_0000_0000_0001);
        let feature = rng.dna(400);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        let left = rng.dna(1000);
        let m = mol(&format!("{left}{feature}{}", rng.dna(1000)), false);
        let found = ann.annotate(&m);

        assert_eq!(found.len(), 1, "{found:?}");
        let f = &found[0];
        // 1-based inclusive: the feature occupies bases 1001..=1400.
        assert_eq!((f.start, f.end), (1001, 1400));
        assert_eq!(f.strand, Strand::Forward);
        assert!((f.identity - 1.0).abs() < 1e-9);
        assert!(!f.is_fragment);
        // ...and the coordinates really do point at the feature.
        assert_eq!(
            &m.seq[(f.start - 1) as usize..f.end as usize],
            feature.as_bytes()
        );
    }

    #[test]
    fn a_feature_on_the_reverse_strand_is_found_and_labelled_reverse() {
        let mut rng = Rng(0x2222_0000_0000_0002);
        let feature = rng.dna(300);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        let rc = String::from_utf8(iupac::reverse_complement(feature.as_bytes())).unwrap();
        let m = mol(&format!("{}{rc}{}", rng.dna(500), rng.dna(500)), false);
        let found = ann.annotate(&m);

        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].strand, Strand::Reverse);
        assert_eq!((found[0].start, found[0].end), (501, 800));
        assert_eq!(
            iupac::reverse_complement(&m.seq[500..800]),
            feature.as_bytes().to_vec()
        );
    }

    #[test]
    fn a_feature_spanning_the_origin_of_a_circle_is_found_whole() {
        // The case that justifies doubling the query, and the one every naive
        // annotator reports as two fragments or misses entirely.
        let mut rng = Rng(0x3333_0000_0000_0003);
        let feature = rng.dna(200);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        // Put the last 80 bases of the feature at the start of the file and the
        // first 120 at the end, so it runs across base 1.
        let middle = rng.dna(600);
        let m = mol(
            &format!("{}{middle}{}", &feature[120..], &feature[..120]),
            true,
        );
        let found = ann.annotate(&m);

        assert_eq!(found.len(), 1, "{found:?}");
        let f = &found[0];
        assert!(f.wraps_origin, "{f:?}");
        assert_eq!(f.len(m.seq.len() as u64), 200);
        assert!(!f.is_fragment);
        assert_eq!(f.start, 681);
        assert_eq!(f.end, 80);
    }

    #[test]
    fn a_linear_molecule_does_not_wrap() {
        let mut rng = Rng(0x4444_0000_0000_0004);
        let feature = rng.dna(200);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());
        let middle = rng.dna(600);
        let m = mol(
            &format!("{}{middle}{}", &feature[120..], &feature[..120]),
            false,
        );
        assert!(ann.annotate(&m).iter().all(|a| !a.wraps_origin));
    }

    #[test]
    fn a_codon_optimised_gene_is_found_by_its_protein() {
        // The whole reason step 5 exists. Recode a CDS with synonymous codons
        // until nucleotide identity is hopeless; the protein is untouched.
        let protein = "MKWVTFISLLFLFSSAYSRGVFRRDAHKSEVAHRFKDLGEENFKALVLIAFAQYLQQCPFEDHVKLVNEVTEFAK";
        let db = db_of(vec![rec("pf:prot", protein, true)]);
        let ann = Annotator::new(&db, Config::default());

        // Build a DNA sequence encoding exactly that protein, choosing the last
        // synonymous codon available for each residue so it looks nothing like
        // any "usual" coding sequence.
        let code = translate::TABLE1;
        let mut cds = String::new();
        for aa in protein.bytes() {
            let mut chosen = None;
            for b1 in b"TCAG" {
                for b2 in b"TCAG" {
                    for b3 in b"TCAG" {
                        let c = [*b1, *b2, *b3];
                        if code.codon(&c) == aa {
                            chosen = Some(c);
                        }
                    }
                }
            }
            let c = chosen.expect("every residue has a codon");
            cds.push_str(std::str::from_utf8(&c).unwrap());
        }
        assert_eq!(code.translate(cds.as_bytes()), protein.as_bytes().to_vec());

        let mut rng = Rng(0x5555_0000_0000_0005);
        let m = mol(&format!("{}{cds}{}", rng.dna(400), rng.dna(400)), false);
        let found = ann.annotate(&m);

        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].via_protein);
        assert_eq!(found[0].strand, Strand::Forward);
        assert_eq!(
            (found[0].start, found[0].end),
            (401, 400 + cds.len() as u64)
        );
    }

    #[test]
    fn a_protein_on_the_reverse_strand_maps_back_to_the_right_bases() {
        let protein = "MKWVTFISLLFLFSSAYSRGVFRRDAHKSEVAHRFKDLGEENFKALVLIAF";
        let db = db_of(vec![rec("pf:prot", protein, true)]);
        let ann = Annotator::new(&db, Config::default());
        let code = translate::TABLE1;

        let mut cds = String::new();
        for aa in protein.bytes() {
            'outer: for b1 in b"TCAG" {
                for b2 in b"TCAG" {
                    for b3 in b"TCAG" {
                        let c = [*b1, *b2, *b3];
                        if code.codon(&c) == aa {
                            cds.push_str(std::str::from_utf8(&c).unwrap());
                            break 'outer;
                        }
                    }
                }
            }
        }
        let rc = String::from_utf8(iupac::reverse_complement(cds.as_bytes())).unwrap();
        let mut rng = Rng(0x6666_0000_0000_0006);
        let m = mol(&format!("{}{rc}{}", rng.dna(300), rng.dna(300)), false);

        let found = ann.annotate(&m);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].strand, Strand::Reverse);
        assert_eq!(
            (found[0].start, found[0].end),
            (301, 300 + cds.len() as u64)
        );
        // The bases named really do translate to the protein.
        let region = &m.seq[(found[0].start - 1) as usize..found[0].end as usize];
        assert_eq!(
            code.translate(&iupac::reverse_complement(region)),
            protein.as_bytes().to_vec()
        );
    }

    #[test]
    fn a_truncated_feature_is_reported_as_a_fragment_not_dropped() {
        // §7.7 step 8. A user who cloned half a marker should see half a
        // marker, drawn differently — not silence.
        let mut rng = Rng(0x7777_0000_0000_0007);
        let feature = rng.dna(600);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(
            &format!("{}{}{}", rng.dna(300), &feature[..300], rng.dna(300)),
            false,
        );

        let found = ann.annotate(&m);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].is_fragment, "{:?}", found[0]);
        assert!(found[0].coverage < 0.95);
        assert!((found[0].coverage - 0.5).abs() < 0.02);
    }

    #[test]
    fn a_fragments_identity_is_local_and_its_coverage_carries_the_truncation() {
        // Pins which of the two numbers means what, because the doc on
        // `Annotation::identity` used to describe `coverage` instead and point
        // at `Hit::identity`, which divides by the feature length — the
        // opposite convention, and one nothing on this path calls. Half a
        // 600 bp marker reproduced perfectly is identity 1.0 with coverage
        // 0.50, and a reader who believed the old sentence would have called
        // that a bug or, worse, printed "100% identity" under the label
        // "fraction of the feature reproduced".
        let mut rng = Rng(0x7777_0000_0000_0107);
        let feature = rng.dna(600);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        let half = mol(
            &format!("{}{}{}", rng.dna(300), &feature[..300], rng.dna(300)),
            false,
        );
        let found = ann.annotate(&half);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            (found[0].identity - 1.0).abs() < 1e-9,
            "identity is local: the 300 bases present matched exactly, so it is \
             1.0 and not 0.5: {:?}",
            found[0]
        );
        assert!((found[0].coverage - 0.5).abs() < 0.02, "{:?}", found[0]);
        assert!(found[0].is_fragment);

        // The control: whole feature, same identity, coverage now 1.0. So the
        // truncation moves `coverage` and only `coverage`.
        let whole = mol(&format!("{}{feature}{}", rng.dna(300), rng.dna(300)), false);
        let found = ann.annotate(&whole);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!((found[0].identity - 1.0).abs() < 1e-9, "{:?}", found[0]);
        assert!((found[0].coverage - 1.0).abs() < 1e-9, "{:?}", found[0]);
        assert!(!found[0].is_fragment);
    }

    #[test]
    fn a_feature_below_the_identity_threshold_is_not_reported() {
        let mut rng = Rng(0x8888_0000_0000_0008);
        let feature = rng.dna(400);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        // 10% of positions changed — far below 96% identity.
        let mut damaged: Vec<u8> = feature.bytes().collect();
        for i in (0..damaged.len()).step_by(10) {
            damaged[i] = if damaged[i] == b'A' { b'C' } else { b'A' };
        }
        let m = mol(
            &format!(
                "{}{}{}",
                rng.dna(200),
                String::from_utf8(damaged).unwrap(),
                rng.dna(200)
            ),
            false,
        );
        assert!(ann.annotate(&m).is_empty());
    }

    #[test]
    fn a_feature_just_inside_the_threshold_is_reported() {
        let mut rng = Rng(0x9999_0000_0000_0009);
        let feature = rng.dna(500);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        // Ten substitutions in 500 bases is 98% identity.
        let mut damaged: Vec<u8> = feature.bytes().collect();
        for i in (0..500).step_by(50) {
            damaged[i] = if damaged[i] == b'G' { b'T' } else { b'G' };
        }
        let m = mol(
            &format!(
                "{}{}{}",
                rng.dna(200),
                String::from_utf8(damaged).unwrap(),
                rng.dna(200)
            ),
            false,
        );
        let found = ann.annotate(&m);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].identity >= 0.96);
    }

    #[test]
    fn unrelated_sequence_yields_nothing() {
        let mut rng = Rng(0xaaaa_0000_0000_000a);
        let db = db_of(vec![
            rec("pf:a", &rng.dna(500), false),
            rec("pf:b", &rng.dna(300), false),
        ]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(&rng.dna(8000), true);
        assert!(ann.annotate(&m).is_empty(), "false positives on random DNA");
    }

    #[test]
    fn two_features_at_the_same_place_do_not_both_survive() {
        // Overlap resolution: near-identical database entries must not produce
        // two stacked annotations of the same span.
        let mut rng = Rng(0xbbbb_0000_0000_000b);
        let feature = rng.dna(400);
        let mut variant: Vec<u8> = feature.bytes().collect();
        variant[10] = if variant[10] == b'A' { b'C' } else { b'A' };
        let db = db_of(vec![
            rec("pf:a", &feature, false),
            rec("pf:b", std::str::from_utf8(&variant).unwrap(), false),
        ]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(&format!("{}{feature}{}", rng.dna(300), rng.dna(300)), false);

        let found = ann.annotate(&m);
        // Both records legitimately match; the rule is that the *span* is not
        // reported twice for the same record, and the better call wins overall.
        assert!(found.len() <= 2, "{found:?}");
        assert!(found.iter().any(|f| f.record == 0));
    }

    #[test]
    fn two_records_competing_for_one_span_do_not_both_survive() {
        // PLAN 7.7 step 7 is a cross-hit rule, and it was gated on
        // `k.record == h.record` so it never fired between different database
        // records. Three near-identical records at one locus produced three
        // stacked annotations of the identical span.
        let mut rng = Rng(0xe3e3_0000_0000_0001);
        let feature = rng.dna(400);
        let mut a: Vec<u8> = feature.bytes().collect();
        let mut b: Vec<u8> = feature.bytes().collect();
        a[7] = if a[7] == b'A' { b'C' } else { b'A' };
        b[9] = if b[9] == b'A' { b'C' } else { b'A' };
        let db = db_of(vec![
            rec("pf:a", &feature, false),
            rec("pf:b", std::str::from_utf8(&a).unwrap(), false),
            rec("pf:c", std::str::from_utf8(&b).unwrap(), false),
        ]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(&format!("{}{feature}{}", rng.dna(300), rng.dna(300)), false);

        let found = ann.annotate(&m);
        assert_eq!(
            found.len(),
            1,
            "three records calling the same span should resolve to one: {found:?}"
        );
    }

    #[test]
    fn a_nested_record_is_kept_rather_than_swallowed() {
        // The reason the cross-record rule is not applied unmodified. An
        // operator inside a promoter, or an M13 site inside lacZ-alpha, is a
        // real annotation rather than a duplicate call -- SnapGene shows both,
        // and PLAN 8.3 plans to add exactly that shape.
        let mut rng = Rng(0xe3e3_0000_0000_0002);
        let big = rng.dna(600);
        let inner = big[200..260].to_string(); // wholly inside `big`
        let db = db_of(vec![
            rec("pf:big", &big, false),
            rec("pf:inner", &inner, false),
        ]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(&format!("{}{big}{}", rng.dna(200), rng.dna(200)), false);

        let found = ann.annotate(&m);
        let names: Vec<&str> = found
            .iter()
            .map(|f| db.records[f.record].id.as_str())
            .collect();
        assert!(names.contains(&"pf:big"), "{names:?}");
        assert!(
            names.contains(&"pf:inner"),
            "the nested feature was swallowed: {names:?}"
        );
    }

    #[test]
    fn adjacent_features_are_both_kept() {
        // The 15% trim exists so that abutting elements are not treated as a
        // collision — an MCS immediately after a promoter is the normal case.
        let mut rng = Rng(0xcccc_0000_0000_000c);
        let a = rng.dna(300);
        let b = rng.dna(300);
        let db = db_of(vec![rec("pf:a", &a, false), rec("pf:b", &b, false)]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(&format!("{}{a}{b}{}", rng.dna(200), rng.dna(200)), false);

        let found = ann.annotate(&m);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!((found[0].start, found[0].end), (201, 500));
        assert_eq!((found[1].start, found[1].end), (501, 800));

        // ...and the trim is what saves them: with no trim the two abutting
        // cores still must not collide, so this asserts the rule rather than
        // the default. Previously this test passed even at trim 0.0, which
        // means it was validating nothing.
        let strict = Config {
            overlap_trim: 0.0,
            ..Config::default()
        };
        let found = Annotator::new(&db, strict).annotate(&m);
        assert_eq!(
            found.len(),
            2,
            "abutting features must not be treated as overlapping: {found:?}"
        );
    }

    #[test]
    fn the_same_feature_twice_in_one_plasmid_is_found_twice() {
        let mut rng = Rng(0xdddd_0000_0000_000d);
        let feature = rng.dna(250);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(
            &format!(
                "{}{feature}{}{feature}{}",
                rng.dna(300),
                rng.dna(500),
                rng.dna(300)
            ),
            true,
        );
        let found = ann.annotate(&m);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].start, 301);
        assert_eq!(found[1].start, 1051);
    }

    #[test]
    fn a_tandem_repeat_is_counted_the_same_from_every_origin_of_a_circle() {
        // A circle has no first base, so the feature count must not depend on
        // which base the file numbers 1. It did: seeds were grouped by
        // `diagonal / bucket_width`, a fixed grid, and two copies of a 60 bp
        // feature sit 60 apart on the diagonal — inside one 80-wide cell for
        // some rotations and across two for others. When they shared a cell the
        // group was reduced to its median diagonal and one copy was dropped
        // with no diagnostic. Measured on this molecule before the fix: 160 of
        // the 580 rotations reported one copy, the other 420 reported two.
        let mut rng = Rng(0x7a4d_0000_0000_0011);
        let feature = rng.dna(60);
        let filler = rng.dna(460);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        let base = format!("{feature}{feature}{filler}");
        assert_eq!(base.len(), 580);
        let mut counts: std::collections::BTreeMap<usize, usize> =
            std::collections::BTreeMap::new();
        for r in 0..base.len() {
            let rotated = format!("{}{}", &base[r..], &base[..r]);
            *counts
                .entry(ann.annotate(&mol(&rotated, true)).len())
                .or_insert(0) += 1;
        }
        assert_eq!(
            counts,
            [(2usize, 580usize)].into_iter().collect(),
            "feature count changed with the origin: {counts:?}"
        );
    }

    #[test]
    fn a_single_copy_is_rotation_invariant_too() {
        // The control: one copy of the same feature on the same size of circle.
        // If the count above had wobbled for some unrelated reason — a seeding
        // artefact at the origin, say — this would wobble with it.
        let mut rng = Rng(0x7a4d_0000_0000_0012);
        let feature = rng.dna(60);
        let filler = rng.dna(520);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        let base = format!("{feature}{filler}");
        for r in 0..base.len() {
            let rotated = format!("{}{}", &base[r..], &base[..r]);
            assert_eq!(
                ann.annotate(&mol(&rotated, true)).len(),
                1,
                "rotation {r} of a single-copy molecule"
            );
        }
    }

    /// The 60 bp fixtures above are structurally unable to see this, which is
    /// why it needs its own: 60 is longer than the DNA scan's `slack = 40`, so
    /// each chain's widened window holds one whole copy and the tie-break inside
    /// `align::infix` never gets a second one to choose from. Below the slack it
    /// does, and it chose wrong.
    ///
    /// Measured before `verify` was anchored on the chain's diagonal, on the
    /// shipped 27 bp HA row and on this fixture alike: x2 reported ONE box, x3
    /// two, x4 three, x5 four — always the last copy missing, because every
    /// chain aligned to the leftmost copy in its window and [`dedupe`] then
    /// merged the identical results. It stopped at 42 bp (V5), i.e. exactly
    /// where the period passes the slack.
    #[test]
    fn tandem_copies_closer_together_than_the_alignment_slack_are_all_reported() {
        let mut rng = Rng(0x7a4d_0000_0000_0013);
        let feature = rng.dna(27); // period 27 < slack 40, unlike the 60 above
        let left = rng.dna(300);
        let right = rng.dna(300);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        for n in 1..=5usize {
            let m = mol(&format!("{left}{}{right}", feature.repeat(n)), false);
            let found = ann.annotate(&m);
            assert_eq!(found.len(), n, "{n} tandem copies reported as {found:?}");
            for (i, f) in found.iter().enumerate() {
                let start = 301 + 27 * i;
                assert_eq!(
                    (f.start as usize, f.end as usize),
                    (start, start + 26),
                    "copy {} of {n} is at the wrong coordinates",
                    i + 1
                );
            }
        }

        // ...and the spacer sweep, because the threshold is the *period* — the
        // feature plus whatever sits between copies — against the slack, not
        // the feature length. Three copies at every spacing from butt-joined up
        // to one slack apart.
        for spacer in [0usize, 3, 6, 9, 12, 15, 30] {
            let gap = rng.dna(spacer);
            let m = mol(
                &format!("{left}{feature}{gap}{feature}{gap}{feature}{right}"),
                false,
            );
            assert_eq!(
                ann.annotate(&m).len(),
                3,
                "three copies {spacer} bp apart must be three annotations"
            );
        }
    }

    #[test]
    fn a_short_tandem_repeat_is_counted_the_same_from_every_origin_of_a_circle() {
        // The same invariant as `a_tandem_repeat_is_counted_the_same_from_every_
        // origin_of_a_circle`, at a period BELOW the slack, where that test
        // cannot reach. Before `verify` was anchored on the chain's diagonal
        // this molecule reported 2 boxes for 573 of its 600 rotations and 3 for
        // the other 27 — the feature count depending on which base the file
        // numbers 1, which is the whole failure the older test is named for.
        let mut rng = Rng(0x7a4d_0000_0000_0014);
        let feature = rng.dna(27);
        let filler = rng.dna(519);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        let base = format!("{}{filler}", feature.repeat(3));
        assert_eq!(base.len(), 600);
        let mut counts: std::collections::BTreeMap<usize, usize> =
            std::collections::BTreeMap::new();
        for r in 0..base.len() {
            let rotated = format!("{}{}", &base[r..], &base[..r]);
            *counts
                .entry(ann.annotate(&mol(&rotated, true)).len())
                .or_insert(0) += 1;
        }
        assert_eq!(
            counts,
            [(3usize, 600usize)].into_iter().collect(),
            "feature count changed with the origin: {counts:?}"
        );
    }

    #[test]
    fn a_tandem_array_of_a_peptide_tag_is_reported_once_per_copy() {
        // The translated route has the same defect and a worse constant: its
        // slack is 12 RESIDUES against an 8-residue tag, so every window holds
        // two or three whole copies. Measured before the fix, with the shipped
        // table rather than this one-row fixture: n copies gave n - 1 "FLAG tag"
        // boxes, and for 3 copies the vacated span came back labelled
        // "Enterokinase cleavage site" over FLAG's own DDDDK — a protease site
        // drawn where the user put a tag.
        //
        // The whole array is encoded in one call so the joins between copies are
        // start- and stop-free too; encoding one copy and repeating it
        // constrains the join against the filler pad, not against the next copy.
        let mut rng = Rng(0x0f1a_0000_0000_0010);
        let code = fixture_code();
        let db = db_of(vec![peptide("pf:flag", FLAG)]);
        let ann = Annotator::new(&db, Config::default());

        for n in 1..=4usize {
            let array = encode(&FLAG.repeat(n), code, &mut rng);
            // 1 initiator + 26 filler residues leaves a partner of at least
            // PARTNER_MIN outside the tag, so the fusion gate admits every copy.
            let construct = format!("ATG{}{array}TAA", FILLER.repeat(26));
            let m = mol(
                &format!("{}{construct}{}", FILLER.repeat(30), FILLER.repeat(30)),
                false,
            );
            assert!(six_frame_contains(&m.seq, FLAG), "n={n}");
            let found = ann.annotate(&m);
            assert_eq!(
                found.len(),
                n,
                "{n} tandem FLAG copies reported as {found:?}"
            );
            for (i, f) in found.iter().enumerate() {
                assert!(f.via_protein);
                assert_eq!(
                    (f.end - f.start + 1) as usize,
                    24,
                    "copy {} of {n} is not 8 residues wide",
                    i + 1
                );
            }
        }
    }

    #[test]
    fn a_circular_molecule_does_not_report_each_feature_twice() {
        // The doubling in step 2 must not leak into the output.
        let mut rng = Rng(0xeeee_0000_0000_000e);
        let feature = rng.dna(300);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(
            &format!("{}{feature}{}", rng.dna(1000), rng.dna(1000)),
            true,
        );
        let found = ann.annotate(&m);
        assert_eq!(found.len(), 1, "doubling leaked: {found:?}");
    }

    #[test]
    fn results_are_stable_between_runs() {
        let mut rng = Rng(0xffff_0000_0000_000f);
        let db = db_of(vec![
            rec("pf:a", &rng.dna(300), false),
            rec("pf:b", &rng.dna(200), false),
            rec("pf:c", &rng.dna(400), false),
        ]);
        let ann = Annotator::new(&db, Config::default());
        let seq: String = db
            .records
            .iter()
            .map(|r| String::from_utf8(r.reference_nt.clone()).unwrap())
            .collect::<Vec<_>>()
            .join(&rng.dna(200));
        let m = mol(&seq, true);
        let first = ann.annotate(&m);
        assert_eq!(first.len(), 3);
        for _ in 0..10 {
            assert_eq!(ann.annotate(&m), first);
        }
    }

    #[test]
    fn an_empty_molecule_and_an_empty_database_are_not_errors() {
        let empty = db_of(vec![]);
        let ann = Annotator::new(&empty, Config::default());
        assert!(ann.annotate(&mol("ACGTACGT", false)).is_empty());

        let db = db_of(vec![rec("pf:a", "ACGTACGTACGTACGT", false)]);
        let ann = Annotator::new(&db, Config::default());
        assert!(ann.annotate(&mol("", false)).is_empty());
    }

    // ----------------------------------------------------------------
    // The fusion rule. PI decision, 2026-07-28: "add these sequences, but make
    // sure they are fused to an ORF, otherwise ignored."

    #[test]
    fn a_tag_is_found_in_frame_in_an_orf_and_nowhere_else() {
        // THE test that carries the PI's requirement, so it is built to be
        // impossible to explain away: ONE base molecule, ONE tag sequence, and
        // the only thing that changes between the three cases is the byte
        // offset the tag is inserted at.
        //
        //   A  at a codon boundary inside the ORF   -> found
        //   B  one base later, so out of frame      -> not found
        //   C  in the 5' flank, outside any ORF     -> not found
        //
        // Every case is guarded, because "no annotation" is otherwise equally
        // consistent with "the gate worked" and "the matcher never saw it".
        let mut rng = Rng(0x0f1a_0000_0000_0001);
        let code = fixture_code();
        let tag = encode(FLAG, code, &mut rng);
        assert_eq!(tag.len(), 24);

        let flank = FILLER.repeat(30); // 90 bases, no start and no stop
        let orf = format!("ATG{}TAA", FILLER.repeat(80));
        let base = format!("{flank}{orf}{flank}");
        let at = |pos: usize| mol(&format!("{}{tag}{}", &base[..pos], &base[pos..]), false);

        // 90 flank + 3 initiator + 40 filler codons: the boundary of codon 42.
        let in_frame = flank.len() + 3 + 120;
        let cases = [
            ("in frame inside the ORF", at(in_frame), true),
            ("one base out of frame", at(in_frame + 1), false),
            ("outside any ORF", at(45), false),
        ];

        let db = db_of(vec![peptide("pf:flag", FLAG)]);
        let ann = Annotator::new(&db, Config::default());
        for (label, m, want) in &cases {
            // Guard 1: the tag really is in this molecule, translated, in some
            // frame. If this ever fails the negative cases prove nothing.
            assert!(
                six_frame_contains(&m.seq, FLAG),
                "{label}: the fixture does not contain the peptide at all"
            );
            let found = ann.annotate(m);
            assert_eq!(
                !found.is_empty(),
                *want,
                "{label}: expected found={want}, got {found:?}"
            );
            if *want {
                let f = &found[0];
                assert!(f.via_protein);
                assert_eq!(f.strand, Strand::Forward);
                assert_eq!(f.len(m.seq.len() as u64), 24, "eight residues of bases");
                // The evidence the UI needs in order to explain the call.
                let ev = f.fusion_orf.expect("the ORF it was admitted on");
                assert_eq!(ev.strand, Strand::Forward);
                assert_eq!(ev.aa_len, 89, "1 initiator + 80 filler + 8 tag");
                assert_eq!(ev.start, flank.len() as u64 + 1);
            }
        }

        // Guard 2, for case B specifically: the ORF is still there and still
        // covers the tag. Without this the fixture could be passing because the
        // displaced tag codons truncated the ORF — the exact trap the `encode`
        // constraint exists to avoid, so it is asserted rather than assumed.
        let out = &cases[1].1;
        let orfs = orf::find_orfs(
            &out.seq,
            code,
            false,
            &Params {
                min_aa: ORF_MIN_AA,
                include_incomplete: true,
                require_start: true,
                nested: false,
            },
        );
        let covering = orfs.iter().find(|o| {
            o.strand == Strand::Forward
                && (o.start as usize) <= in_frame + 1
                && in_frame + 1 + 24 <= o.end as usize
        });
        let covering = covering.expect("the ORF must still span the tag; it was truncated");
        assert_eq!(covering.aa_len, 89, "and it must still be full length");

        // Guard 3, for case C: no ORF covers the tag there, which is what
        // "outside any ORF" is supposed to mean.
        let outside = &cases[2].1;
        let orfs = orf::find_orfs(
            &outside.seq,
            code,
            false,
            &Params {
                min_aa: ORF_MIN_AA,
                include_incomplete: true,
                require_start: true,
                nested: false,
            },
        );
        assert!(
            !orfs
                .iter()
                .any(|o| (o.start as usize) <= 46 && 46 + 24 <= o.end as usize),
            "case C grew an ORF of its own, so it tests the wrong thing: {orfs:?}"
        );
    }

    #[test]
    fn a_tag_separated_from_its_orf_by_an_in_frame_stop_is_not_found() {
        // A tag downstream of an in-frame terminator, inside what *looks* like
        // one long coding region. `find_orfs` with `require_start` returns the
        // run to the FIRST in-frame stop, so the containment test alone
        // excludes this and no separate clause is needed — which is a property
        // a tier-1 CDS rule would not have had. A database CDS mapped onto a
        // clone that has acquired a nonsense mutation still carries the
        // database's extent, so a containment-in-the-annotation test would
        // certify a fusion straight through a stop codon that is really there.
        let mut rng = Rng(0x0f1a_0000_0000_0002);
        let code = fixture_code();
        let tag = encode(FLAG, code, &mut rng);
        let f30 = FILLER.repeat(30);

        // The ONE codon that differs between the two molecules.
        let build = |middle: &str| {
            mol(
                &format!(
                    "{}ATG{f30}{middle}{f30}{tag}{f30}TAA{}",
                    FILLER.repeat(10),
                    FILLER.repeat(10)
                ),
                false,
            )
        };
        let broken = build("TAA");
        let whole = build(FILLER);
        assert_eq!(broken.seq.len(), whole.seq.len(), "one codon, not one base");

        let db = db_of(vec![peptide("pf:flag", FLAG)]);
        let ann = Annotator::new(&db, Config::default());

        assert!(six_frame_contains(&broken.seq, FLAG));
        assert!(
            ann.annotate(&broken).is_empty(),
            "a tag past an in-frame stop was called a fusion"
        );
        // The ORF list shows *why*: the run ends at the first stop, before the
        // tag.
        let orfs = orf::find_orfs(
            &broken.seq,
            code,
            false,
            &Params {
                min_aa: ORF_MIN_AA,
                include_incomplete: true,
                require_start: true,
                nested: false,
            },
        );
        assert!(
            orfs.iter()
                .any(|o| o.strand == Strand::Forward && o.aa_len == 31 && o.complete),
            "expected a 31 aa ORF ending at the planted stop: {orfs:?}"
        );

        // The control: remove the stop, change nothing else, and the same tag
        // at the same place is found.
        let found = ann.annotate(&whole);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].fusion_orf.unwrap().aa_len, 100);
    }

    #[test]
    fn an_n_terminal_and_a_c_terminal_tag_are_both_found_on_both_strands() {
        // The boundary arithmetic, and both boundaries are the NORMAL case
        // rather than an edge:
        //
        //   C-terminal  ->  d + 3*tag_aa == 3*aa_len exactly. An implementer
        //                   who writes `<` here ships a tool that finds no
        //                   C-terminal tag at all, which is about half of all
        //                   tagging.
        //   N-terminal  ->  d == 3, the initiator then the tag.
        //
        // Run on both strands, because the reverse anchor is the ORF's HIGH
        // plus-strand coordinate and swapping the two anchors is the
        // mirrored-coordinate bug `residues_to_bases` already carries a comment
        // about.
        let mut rng = Rng(0x0f1a_0000_0000_0003);
        let code = fixture_code();
        let tag = encode(FLAG, code, &mut rng);
        let f40 = FILLER.repeat(40);

        let n_terminal = format!("ATG{tag}{f40}TAA");
        let c_terminal = format!("ATG{f40}{tag}TAA");
        let db = db_of(vec![peptide("pf:flag", FLAG)]);
        let ann = Annotator::new(&db, Config::default());

        for (label, construct) in [("N-terminal", &n_terminal), ("C-terminal", &c_terminal)] {
            for reverse in [false, true] {
                let body = if reverse {
                    String::from_utf8(iupac::reverse_complement(construct.as_bytes())).unwrap()
                } else {
                    construct.clone()
                };
                let m = mol(
                    &format!("{}{body}{}", FILLER.repeat(30), FILLER.repeat(30)),
                    false,
                );
                let found = ann.annotate(&m);
                assert_eq!(found.len(), 1, "{label}, reverse={reverse}: {found:?}");
                assert_eq!(
                    found[0].strand,
                    if reverse {
                        Strand::Reverse
                    } else {
                        Strand::Forward
                    },
                    "{label}, reverse={reverse}"
                );
                let ev = found[0].fusion_orf.unwrap();
                assert_eq!(ev.aa_len, 49, "{label}, reverse={reverse}");
                // ...and the bases named really do translate to the tag, read
                // the way the strand says to.
                let region = &m.seq[(found[0].start - 1) as usize..found[0].end as usize];
                let read = if reverse {
                    iupac::reverse_complement(region)
                } else {
                    region.to_vec()
                };
                assert_eq!(code.translate(&read), FLAG.as_bytes());
            }
        }
    }

    #[test]
    fn a_tag_whose_own_first_residue_is_the_initiator_is_found() {
        // The N-terminal off-by-one that actually bites: `d == 0`, reached by a
        // shipped part rather than by a hypothetical. SBP-tag begins with M, so
        // a construct can place its own first residue as the initiator, and
        // `d > 0` would reject it.
        let mut rng = Rng(0x0f1a_0000_0000_0004);
        let code = fixture_code();
        // The initiator must be a literal ATG for the tag's first residue to be
        // both the M of the peptide and the ORF's start codon, so the tag's
        // remaining residues are encoded and prepended with ATG.
        //
        // `encode_stopless`, because SBP contains W and no start-free encoding
        // of it exists under table 11. Safe here, and the assertion on
        // `aa_len` below is what proves it: the tag's frame is fixed, so only
        // an ORF in that frame can admit it, the first start in that frame is
        // this fixture's own ATG, and anything further in is nested and
        // suppressed. If a stray start ever did displace the winning ORF, the
        // 78 would move.
        let rest = encode_stopless(&SBP[1..], code, &mut rng);
        let construct = format!("ATG{rest}{}TAA", FILLER.repeat(40));
        let m = mol(
            &format!("{}{construct}{}", FILLER.repeat(30), FILLER.repeat(30)),
            false,
        );
        assert!(six_frame_contains(&m.seq, SBP));

        let db = db_of(vec![peptide("pf:sbp", SBP)]);
        let found = Annotator::new(&db, Config::default()).annotate(&m);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].fusion_orf.unwrap().aa_len, 38 + 40);
        assert_eq!(found[0].start, 91, "the tag starts at the initiator");
    }

    #[test]
    fn a_fusion_across_the_origin_of_a_circle_is_found() {
        // The case a linear reading cannot see, and the one where the anchor
        // arithmetic has to reduce a hit whose span runs past the molecule's
        // length back into `[0, L)`. Both the ORF and the tag straddle base 1.
        let mut rng = Rng(0x0f1a_0000_0000_0005);
        let code = fixture_code();
        let tag = encode(FLAG, code, &mut rng);
        let f40 = FILLER.repeat(40);
        let construct = format!("ATG{f40}{tag}{f40}TAA");
        assert_eq!(construct.len(), 270);

        // Cut through the middle of the tag itself, so the tag crosses the
        // origin as well as the ORF.
        let cut = 3 + 120 + 12;
        let seq = format!(
            "{}{}{}",
            &construct[cut..],
            FILLER.repeat(30),
            &construct[..cut]
        );
        assert_eq!(seq.len() % 3, 0, "distinct frames keeps the fixture simple");
        let m = mol(&seq, true);

        let db = db_of(vec![peptide("pf:flag", FLAG)]);
        let found = Annotator::new(&db, Config::default()).annotate(&m);
        assert_eq!(found.len(), 1, "{found:?}");
        let f = &found[0];
        assert!(f.wraps_origin, "{f:?}");
        assert_eq!(f.len(m.seq.len() as u64), 24);
        assert_eq!(f.fusion_orf.unwrap().aa_len, 89);

        // The linear reading of the same bases must NOT find it, which is what
        // makes this a test of the circular path rather than of luck.
        let flat = mol(&seq, false);
        assert!(
            Annotator::new(&db, Config::default())
                .annotate(&flat)
                .is_empty(),
            "a linear molecule has no way round"
        );
    }

    #[test]
    fn a_fusion_admitted_only_on_an_orfs_second_lap() {
        // Coverage for `fused_orf`'s `laps` loop, which its own note used to
        // call "defensive, not demonstrated" and to bound to circles under
        // 78 bp. Both halves of that were wrong, and the second was wrong by two
        // orders of magnitude: `laps != 0` needs a frame that runs stopless for
        // more than one lap, which `find_orfs` produces only when `3 ∤ L`, and
        // which is a statement about stop spacing rather than about `L`.
        //
        // The fixture above cannot reach it — it asserts `seq.len() % 3 == 0`,
        // which forces `laps == 0` — so this is a separate molecule: 223 bases,
        // `223 % 3 == 1`, with the tag inside an ATG..TAA cassette and a long
        // tract of stops behind it. The merged frame opens on an `ATA` (a
        // table-11 initiator) inside that tract and runs 148 codons, i.e. 444
        // coding bases on a 223 bp molecule.
        let code = fixture_code();
        // Written out rather than encoded: this fixture needs an exact length
        // and an exact frame, and `encode`'s search would give neither.
        // GAC TAC AAG GAC GAC GAC GAC AAG = DYKDDDDK.
        let tag = "GACTACAAGGACGACGACGACAAG";
        let seq = format!(
            "ATG{}{tag}{}TAA{}A",
            FILLER.repeat(12),
            FILLER.repeat(12),
            "TAA".repeat(40)
        );
        assert_eq!(seq.len(), 223);
        assert_eq!(seq.len() % 3, 1, "a merged frame is the whole point");
        let m = mol(&seq, true);
        assert!(six_frame_contains(&m.seq, FLAG));

        let db = db_of(vec![peptide("pf:flag", FLAG)]);
        let found = Annotator::new(&db, Config::default()).annotate(&m);
        assert_eq!(found.len(), 1, "{found:?}");
        let f = &found[0];
        assert!(f.via_protein);
        let fo = f.fusion_orf.expect("admitted by the fusion gate");
        assert_eq!((fo.start, fo.end, fo.aa_len), (102, 102, 148));

        // ...and it really is the second lap that admits it, which is what
        // makes deleting the loop a behaviour change rather than a tidy-up.
        // `d0` is `fused_orf`'s own forward anchor arithmetic: the distance
        // from the ORF's initiator to the tag's first base.
        let len = m.seq.len();
        let o5 = fo.start as usize - 1;
        let t5 = f.start as usize - 1;
        let d0 = (t5 + len - o5) % len;
        assert_ne!(
            d0 % 3,
            0,
            "k = 0 already admits this tag, so the loop is not what carries it"
        );
        assert_eq!((d0 + len) % 3, 0, "k = 1 is the lap that admits it");

        // And the ORF genuinely laps: `Orf::laps` is what bounds the loop, and
        // a fixture with `laps == 0` would exercise only k = 0 however the
        // arithmetic above came out.
        let orfs = orf::find_orfs(
            &m.seq,
            code,
            true,
            &Params {
                min_aa: ORF_MIN_AA,
                include_incomplete: true,
                require_start: true,
                nested: false,
            },
        );
        let admitting = orfs
            .iter()
            .find(|o| o.strand == Strand::Forward && o.aa_len == 148)
            .expect("the 148 aa merged-frame ORF");
        assert_eq!(admitting.laps, 2);
        assert!(admitting.bases() > len, "an ORF longer than its molecule");
    }

    #[test]
    fn a_tag_with_no_partner_is_not_a_fusion() {
        // "Fused to" needs something to be fused to. The floor is on the ORF's
        // residues OUTSIDE the tag, so it asks the most of the shortest
        // peptides, which is where the evidential burden belongs.
        let mut rng = Rng(0x0f1a_0000_0000_0006);
        let code = fixture_code();
        let tag = encode(FLAG, code, &mut rng);
        let db = db_of(vec![peptide("pf:flag", FLAG)]);
        let ann = Annotator::new(&db, Config::default());

        // partner = 1 initiator + n filler residues. The rule is
        // `aa_len - tag_aa >= 20`, so n = 19 fails and n = 20 passes.
        for (n, want) in [(18usize, false), (19, false), (20, true), (21, true)] {
            let construct = format!("ATG{}{tag}TAA", FILLER.repeat(n - 1));
            let m = mol(
                &format!("{}{construct}{}", FILLER.repeat(30), FILLER.repeat(30)),
                false,
            );
            assert!(six_frame_contains(&m.seq, FLAG), "n={n}");
            assert_eq!(
                !ann.annotate(&m).is_empty(),
                want,
                "a {n}-residue partner: expected found={want}"
            );
        }
    }

    #[test]
    fn a_peptide_part_is_matched_exactly_however_the_identity_threshold_is_set() {
        // SOURCING.md §3: features under ~15 aa are matched exactly, never by
        // scored alignment, and its false-positive arithmetic holds only under
        // that rule. Today exactness falls out of `budget()` returning 0 for
        // short cores at the default 0.96 — but `min_identity` is documented as
        // user-adjustable, and at 0.80 an 8-mer gets a budget of 1. So the rule
        // is asserted rather than inherited.
        let mut rng = Rng(0x0f1a_0000_0000_0007);
        let code = fixture_code();
        let tag = encode(FLAG, code, &mut rng);
        let f40 = FILLER.repeat(40);

        // One residue of the tag changed, by changing one codon. The construct
        // is otherwise the fixture that IS found.
        let mut damaged: Vec<u8> = tag.bytes().collect();
        damaged[0..3].copy_from_slice(b"GCC"); // D -> A at position 1
        let damaged = String::from_utf8(damaged).unwrap();

        // ...and the same on a LONG peptide, which is the case that really
        // bites. On an 8-mer the rule is currently satisfied by an accident:
        // `min_match_len / 3` is clamped to the record's own length, so a
        // 7-of-8 match falls one residue short of the floor and is dropped
        // before the identity threshold is consulted. A 37-residue tag with one
        // residue changed in the MIDDLE still seeds on both sides, still chains
        // over the whole record, and at the DEFAULT 0.96 scores 0.973 — so
        // before this rule existed the annotator reported a 36-of-37 match as
        // an SBP-tag, with `identity: 0.972…` and `coverage: 1.0`, and no
        // threshold a user could set would have stopped it.
        // `encode_stopless`: SBP contains W, which has no start-free encoding
        // under table 11. Enough here — every case below either asserts the
        // control is found (one ORF, in the tag's own frame, starting at this
        // fixture's ATG) or asserts a damaged tag is found NOWHERE, and no
        // extra ORF can make an inexact peptide match.
        let long_tag = encode_stopless(&SBP[1..], code, &mut rng);
        let mid = (SBP.len() - 1) / 2;
        let mut long_damaged: Vec<u8> = long_tag.bytes().collect();
        long_damaged[mid * 3..mid * 3 + 3].copy_from_slice(b"GCC");
        let long_damaged = String::from_utf8(long_damaged).unwrap();

        for (label, part, whole, broken) in [
            ("FLAG, 8 aa", FLAG, &tag, &damaged),
            ("SBP-tag, 37 aa", &SBP[1..], &long_tag, &long_damaged),
        ] {
            let db = db_of(vec![peptide("pf:part", part)]);
            for min_identity in [0.96, 0.80, 0.50] {
                let cfg = Config {
                    min_identity,
                    ..Config::default()
                };
                let ann = Annotator::new(&db, cfg);
                let good = mol(
                    &format!(
                        "{}ATG{f40}{whole}TAA{}",
                        FILLER.repeat(30),
                        FILLER.repeat(30)
                    ),
                    false,
                );
                assert_eq!(
                    ann.annotate(&good).len(),
                    1,
                    "{label}: the control must still be found at min_identity {min_identity}"
                );
                let bad = mol(
                    &format!(
                        "{}ATG{f40}{broken}TAA{}",
                        FILLER.repeat(30),
                        FILLER.repeat(30)
                    ),
                    false,
                );
                let got = ann.annotate(&bad);
                assert!(
                    got.is_empty(),
                    "{label}: a one-residue mismatch was reported as the tag at \
                     min_identity {min_identity}: {got:?}"
                );
            }
        }
    }

    #[test]
    fn a_tag_inside_an_orf_on_the_other_strand_is_not_a_fusion() {
        // The strand clause, which nothing else here can break. `Strand` has
        // four variants and a `_ =>` arm falling through to Forward would
        // silently adjudicate an unoriented hit on the plus strand -- but the
        // failure this fixture demonstrates is coarser and more likely: drop
        // the `orf.strand != strand` test and a FORWARD tag is admitted on the
        // strength of a REVERSE ORF whose arithmetic happens to line up.
        //
        // Built inside out. The construct is written as the ORF, with the tag's
        // reverse complement inside it, and then the whole thing is reverse
        // complemented into the molecule -- so the plus strand carries the tag
        // readable forward while the only long ORF is on the minus strand.
        let mut rng = Rng(0x0f1a_0000_0000_0009);
        let code = fixture_code();
        // GGC filler, because the tag's plus-strand neighbours here are the
        // reverse complement of the ORF's filler; and rev0, because a stop in
        // the reverse complement of the tag would truncate the reverse ORF.
        let tag = encode_in(FLAG, code, &mut rng, "GGC", true, true);
        let rc_tag = String::from_utf8(iupac::reverse_complement(tag.as_bytes())).unwrap();
        let f40 = "GGC".repeat(40);
        let gene = format!("ATG{f40}{rc_tag}{f40}TAA");
        let region = String::from_utf8(iupac::reverse_complement(gene.as_bytes())).unwrap();
        assert!(
            region.contains(&tag),
            "the plus strand must carry the tag verbatim"
        );
        let m = mol(
            &format!("{}{region}{}", "GGC".repeat(30), "GGC".repeat(30)),
            false,
        );

        // Guard: the tag really is there, on the plus strand, and the reverse
        // ORF really is there and really does span it. Without both, the
        // negative result below is about the wrong thing.
        assert!(six_frame_contains(&m.seq, FLAG));
        let orfs = orf::find_orfs(
            &m.seq,
            code,
            false,
            &Params {
                min_aa: ORF_MIN_AA,
                include_incomplete: true,
                require_start: true,
                nested: false,
            },
        );
        let tag_at = 90 + 3 + 120;
        let rev = orfs
            .iter()
            .find(|o| o.strand == Strand::Reverse && o.aa_len == 89)
            .unwrap_or_else(|| panic!("the reverse ORF was truncated: {orfs:?}"));
        assert!(
            (rev.start as usize) <= tag_at + 1 && tag_at + 24 <= rev.end as usize,
            "the reverse ORF must span the tag: {rev:?}"
        );
        assert!(
            !orfs.iter().any(|o| o.strand == Strand::Forward
                && (o.start as usize) <= tag_at + 1
                && tag_at + 24 <= o.end as usize),
            "a forward ORF spans the tag, so this fixture tests nothing: {orfs:?}"
        );

        let db = db_of(vec![peptide("pf:flag", FLAG)]);
        let found = Annotator::new(&db, Config::default()).annotate(&m);
        assert!(
            found.is_empty(),
            "a forward tag was admitted on the strength of a reverse ORF: {found:?}"
        );
    }

    #[test]
    fn a_tag_is_found_on_an_orf_that_initiates_at_any_of_its_codes_start_codons() {
        // The GTG regression. `Config::code` reaches `find_orfs` with
        // `require_start`, so which codons may initiate decides whether an ORF
        // exists at all — and while the default was table 1, a tag fused to any
        // GTG-, ATT-, ATC- or ATA-started gene was silently dropped. That is
        // not exotic: five of this project's own 38 CDS rows start GTG (TetA,
        // AprR, HygR, lacI, lambda int), and an N-terminal tag on one of them
        // is the commonest thing anyone does with a tag.
        //
        // Sweeps the code's OWN start set rather than a hard-coded list, so it
        // states the property — "every initiator this code accepts can carry a
        // fusion" — instead of a table's contents.
        let mut rng = Rng(0x0f1a_0000_0000_000a);
        let code = fixture_code();
        let tag = encode(FLAG, code, &mut rng);
        let db = db_of(vec![peptide("pf:flag", FLAG)]);
        let ann = Annotator::new(&db, Config::default());

        let starts: Vec<[u8; 3]> = {
            let mut v = Vec::new();
            for b1 in b"TCAG" {
                for b2 in b"TCAG" {
                    for b3 in b"TCAG" {
                        let c = [*b1, *b2, *b3];
                        if code.is_start(&c) {
                            v.push(c);
                        }
                    }
                }
            }
            v
        };
        assert!(
            starts.len() >= 4,
            "a code with three initiators cannot demonstrate this; \
             Config::code is {} ({:?} start codons)",
            code.id,
            starts.len()
        );

        for start in &starts {
            let init = std::str::from_utf8(start).unwrap();
            let seq = format!(
                "{}{init}{}{tag}{}TAA{}",
                FILLER.repeat(30),
                FILLER.repeat(40),
                FILLER.repeat(10),
                FILLER.repeat(30)
            );
            let m = mol(&seq, false);
            assert!(
                six_frame_contains(&m.seq, FLAG),
                "{init}: the fixture does not contain the peptide at all"
            );
            let found = ann.annotate(&m);
            assert_eq!(
                found.len(),
                1,
                "an ORF initiating at {init} carried no fusion, but {init} is a \
                 start codon of the code the annotator is using: {found:?}"
            );
            let ev = found[0].fusion_orf.expect("the ORF it was admitted on");
            assert_eq!(ev.start, 91, "{init}: admitted on the planted ORF");
            assert_eq!(ev.aa_len, 59, "{init}: 1 initiator + 40 + 8 tag + 10");
        }
    }

    #[test]
    fn an_exact_partial_of_a_peptide_part_is_reported_as_nothing_at_all() {
        // WHOLE as well as exact, which is a separate clause from the identity
        // one and needs its own fixture: deleting `m.aligned != aa.len()` and
        // keeping `m.identity < 1.0` leaves the whole suite green while the
        // annotator starts reporting a 16-of-37-residue prefix of SBP-tag as an
        // SBP-tag, at identity 1.0, coverage 0.43, drawn as a fragment.
        //
        // It must not. These are DESIGNED parts whose boundary is the design,
        // so a prefix of one is not a fragment of anything — there is no
        // truncated SBP-tag, only a different peptide. The fragment machinery
        // needs no special case precisely because this can never fire.
        let mut rng = Rng(0x0f1a_0000_0000_000b);
        let code = fixture_code();
        let db = db_of(vec![peptide("pf:sbp", SBP)]);
        let ann = Annotator::new(&db, Config::default());

        // The control first, so a bug that found nothing anywhere could not
        // pass the negatives below.
        let whole = encode_stopless(&SBP[1..], code, &mut rng);
        let good = mol(
            &format!(
                "{}ATG{whole}{}TAA{}",
                FILLER.repeat(30),
                FILLER.repeat(40),
                FILLER.repeat(30)
            ),
            false,
        );
        assert_eq!(ann.annotate(&good).len(), 1, "the whole tag must be found");

        // Prefixes long enough to clear BOTH floors that could reject them for
        // an unrelated reason: `min_coverage` 0.30 (16/38 = 0.42) and
        // `min_match_len / 3` = 16 residues. Anything shorter would be dropped
        // by arithmetic rather than by the wholeness rule, and would prove
        // nothing.
        // 36 is the longest that is still a prefix: `ATG` + 36 encoded residues
        // is 37 of the 38, one residue short of the whole thing.
        for take in [16usize, 20, 24, 30, 36] {
            let part = &SBP[1..=take]; // residues 2..=take+1, so `take` of them
            let dna = encode_stopless(part, code, &mut rng);
            let m = mol(
                &format!(
                    "{}ATG{dna}{}TAA{}",
                    FILLER.repeat(30),
                    FILLER.repeat(40),
                    FILLER.repeat(30)
                ),
                false,
            );
            // Guard: the prefix really is present, translated, in a frame.
            assert!(
                six_frame_contains(&m.seq, &SBP[..=take]),
                "{take}: the fixture does not contain the prefix at all"
            );
            let got = ann.annotate(&m);
            assert!(
                got.is_empty(),
                "a {}-of-{} residue prefix of SBP-tag was reported as an \
                 SBP-tag: {got:?}",
                take + 1,
                SBP.len()
            );
        }
    }

    #[test]
    fn a_synthetic_part_carrying_both_references_is_gated_on_the_peptide_route_only() {
        // The shape the relaxed schema permits and no shipped row has: a
        // `synthetic_part` with nucleotides AND a peptide. Keyed on
        // `is_peptide_only`, the two extra rules missed it entirely, so an
        // eight-residue epitope went into the six-frame scan with no exactness
        // rule and no ORF requirement — a hole opened by the shape of a row
        // rather than by anyone's decision. Keyed on `is_designed_peptide` it
        // is closed, and the nucleotide route is left exactly as it was.
        let mut rng = Rng(0x0f1a_0000_0000_000d);
        let code = fixture_code();
        let tag = encode(FLAG, code, &mut rng);

        // A row of both shapes at once. The nucleotide reference is unrelated
        // to the tag's codons AND to the filler, so neither route can be
        // mistaken for the other in the results below.
        let mut both = peptide("pf:both", FLAG);
        let dna_ref = rng.dna(60);
        both.reference_nt = dna_ref.as_bytes().to_vec();
        let db = db_of(vec![both]);
        let ann = Annotator::new(&db, Config::default());

        // 1. The PEPTIDE route is gated. No ORF anywhere near the tag, so
        //    nothing may be reported for it.
        let no_orf = mol(
            &format!("{}TAA{tag}TAA{}", FILLER.repeat(20), FILLER.repeat(20)),
            false,
        );
        assert!(
            six_frame_contains(&no_orf.seq, FLAG),
            "the fixture does not contain the peptide at all"
        );
        let got = ann.annotate(&no_orf);
        assert!(
            got.is_empty(),
            "a synthetic part carrying both references took its peptide through \
             the six-frame scan ungated: {got:?}"
        );

        // 2. ...and it is gated by the ORF rule rather than by being ignored:
        //    the same tag inside a qualifying ORF IS reported, with evidence.
        let with_orf = mol(
            &format!(
                "{}ATG{}{tag}{}TAA{}",
                FILLER.repeat(20),
                FILLER.repeat(40),
                FILLER.repeat(10),
                FILLER.repeat(20)
            ),
            false,
        );
        let got = ann.annotate(&with_orf);
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].via_protein);
        assert!(got[0].fusion_orf.is_some());

        // 3. The NUCLEOTIDE route is untouched. Tier 1 on the same row finds
        //    its own reference with no ORF in sight — which is what the eight
        //    parented tags depend on, and what 44 suites of existing behaviour
        //    would have lost to a gate placed on the record rather than on the
        //    route.
        let nt_only = mol(
            &format!("{}{dna_ref}{}", FILLER.repeat(15), FILLER.repeat(15)),
            false,
        );
        let got = ann.annotate(&nt_only);
        assert_eq!(
            got.len(),
            1,
            "the nucleotide route on a synthetic part must not be ORF-gated: {got:?}"
        );
        assert!(!got[0].via_protein, "found by DNA, not by translation");
        assert!(got[0].fusion_orf.is_none(), "a DNA hit claims no fusion");
    }

    #[test]
    fn a_tagged_orf_running_off_the_end_of_a_linear_fragment_is_still_a_fusion() {
        // `include_incomplete: true`, which nothing else here can break. A
        // sequencing read or a Gibson piece that carries an initiator and a tag
        // but is cut before the stop is a real fusion, and the commonest shape
        // of real input that is not a whole plasmid.
        let mut rng = Rng(0x0f1a_0000_0000_000c);
        let code = fixture_code();
        let tag = encode(FLAG, code, &mut rng);
        // No stop anywhere: ATG, 40 filler codons, the tag, 5 more, end of file.
        let seq = format!("ATG{}{tag}{}", FILLER.repeat(40), FILLER.repeat(5));
        let m = mol(&seq, false);

        // Guard: the fixture really does depend on the flag. With incomplete
        // ORFs excluded there is no ORF here at all, so if this ever stopped
        // being true the test below would pass without exercising anything.
        let strict = orf::find_orfs(
            &m.seq,
            code,
            false,
            &Params {
                min_aa: ORF_MIN_AA,
                include_incomplete: false,
                require_start: true,
                nested: false,
            },
        );
        assert!(
            strict.is_empty(),
            "the fragment reached a stop, so it is not the case this tests: {strict:?}"
        );

        let db = db_of(vec![peptide("pf:flag", FLAG)]);
        let found = Annotator::new(&db, Config::default()).annotate(&m);
        assert_eq!(
            found.len(),
            1,
            "a tag on an ORF that runs off the 3' end of a linear fragment is a \
             fusion and must be reported: {found:?}"
        );
        let ev = found[0].fusion_orf.expect("the ORF it was admitted on");
        assert_eq!(ev.aa_len, 54, "1 initiator + 40 + 8 tag + 5, and no stop");
    }

    #[test]
    fn a_record_carrying_nucleotides_is_not_gated_by_the_fusion_rule() {
        // The scope check. The rule applies to peptide-only synthetic parts and
        // to nothing else, so a codon-optimised CDS found by translation with
        // no ORF anywhere near it must still be reported — that is §7.7 step 5,
        // and 44 suites of existing behaviour depend on it.
        //
        // One molecule, one peptide, two records: the difference is the shape
        // of the row, and nothing else.
        let mut rng = Rng(0x0f1a_0000_0000_0008);
        let code = fixture_code();
        // Long enough to clear `min_match_len / 3`, and in a frame with no
        // initiator and no stop, so no ORF can contain it.
        //
        // (GGGGS)4 rather than SBP, and this is the fixture that forced the
        // distinction: "no ORF can contain it" is exactly the property
        // `encode_stopless` does NOT provide, so this call must be the strict
        // `encode` — and SBP is unencodable under it, because W's only codon
        // `TGG` makes a table-11 start with every base that could precede it.
        let protein = GS_LINKER.to_string();
        let cds = encode(&protein, code, &mut rng);
        let m = mol(
            &format!("{}{cds}{}", FILLER.repeat(40), FILLER.repeat(40)),
            false,
        );
        assert!(six_frame_contains(&m.seq, &protein));

        let as_cds = db_of(vec![rec("pf:cds", &protein, true)]);
        let found = Annotator::new(&as_cds, Config::default()).annotate(&m);
        assert_eq!(
            found.len(),
            1,
            "a translated CDS hit must not be gated: {found:?}"
        );
        assert!(found[0].via_protein);
        assert!(
            found[0].fusion_orf.is_none(),
            "a CDS hit must not claim fusion evidence"
        );

        let as_tag = db_of(vec![peptide("pf:tag", &protein)]);
        assert!(
            Annotator::new(&as_tag, Config::default())
                .annotate(&m)
                .is_empty(),
            "the same peptide as a peptide-only part must be gated"
        );
    }

    #[test]
    fn unseedable_entries_are_reported_rather_than_silently_unsearched() {
        let db = db_of(vec![
            rec("pf:minus35", "TTGACA", false),
            rec("pf:long", "ACGTACGTACGTACGTACGTACGT", false),
        ]);
        let ann = Annotator::new(&db, Config::default());
        let un = ann.unseedable();
        assert_eq!(un.len(), 1);
        assert_eq!(un[0].id, "pf:minus35");
    }

    /// Build the standard three-case fixture around `peptide_aa`.
    ///
    /// Returns `(molecule_in_frame, molecule_out_of_frame, molecule_outside,
    /// flank_len, orf_aa_len)`. The same shape
    /// `a_tag_is_found_in_frame_in_an_orf_and_nowhere_else` uses, factored out
    /// because the routing change needs it for a second peptide and copying it
    /// would let the two drift.
    fn three_cases(
        peptide_aa: &str,
        rng: &mut Rng,
    ) -> (Molecule, Molecule, Molecule, usize, usize) {
        let code = fixture_code();
        let tag = encode(peptide_aa, code, rng);
        let flank = FILLER.repeat(30); // 90 bases, no start and no stop
        let orf = format!("ATG{}TAA", FILLER.repeat(80));
        let base = format!("{flank}{orf}{flank}");
        let at = |pos: usize| mol(&format!("{}{tag}{}", &base[..pos], &base[pos..]), false);
        let in_frame = flank.len() + 3 + 120;
        (
            at(in_frame),
            at(in_frame + 1),
            at(45),
            flank.len(),
            1 + 80 + peptide_aa.len(),
        )
    }

    #[test]
    fn a_six_residue_peptide_is_found_in_frame_in_an_orf_and_nowhere_else() {
        // THE defect, and its guard rail in the same test.
        //
        // Six residues is two 5-mer windows. `Config::min_seeds` is 3, so no
        // query could ever chain it; and `Index::short` lists records that
        // indexed ZERO words, so two is not zero and nothing reported it either.
        // Shipped, seeded, unchainable, unreported, never found. At b340b18 the
        // first case below returns no annotation at all.
        //
        // The other two cases are the guard rail: the fusion gate is untouched
        // by the new route, so the same peptide one base out of frame, and the
        // same peptide outside any ORF, must still be nothing.
        //
        // THE SEED IS CHOSEN, not arbitrary. `encode`'s trailing-pad fix landed
        // in the same change as the routing, and 347 of 1024 seeds make the two
        // versions of that helper disagree. `0x..0011`, which this test was
        // first written with, is one of them: at b340b18 the encoder commits to
        // `...AGT` for the serine and its own definitive assertion fires with
        // "the encoding of LVPRGS spells GTG in frame 1" before `annotate` is
        // ever called, so the test would have died in its fixture builder and
        // proved nothing about the annotator. This seed is one of the 677 where
        // both versions of `encode` return the same eighteen bases, so at
        // b340b18 the fixture builds and the failure is the missing annotation,
        // which is the claim. The pad fix keeps its own regression test:
        // `the_encoder_constrains_the_join_into_the_trailing_pad`.
        let mut rng = Rng(0x0f1a_0000_0000_0021);
        let (inf, off, outside, flank, orf_aa) = three_cases(THROMBIN, &mut rng);
        assert_eq!(orf_aa, 87, "1 initiator + 80 filler + 6 tag");

        let db = db_of(vec![peptide("pf:thrombin", THROMBIN)]);
        let ann = Annotator::new(&db, Config::default());
        for (label, m, want) in [
            ("in frame inside the ORF", &inf, true),
            ("one base out of frame", &off, false),
            ("outside any ORF", &outside, false),
        ] {
            // Without this the negative cases are equally consistent with "the
            // matcher never saw it", which is what they used to mean.
            assert!(
                six_frame_contains(&m.seq, THROMBIN),
                "{label}: the fixture does not contain the peptide at all"
            );
            let found = ann.annotate(m);
            assert_eq!(
                !found.is_empty(),
                want,
                "{label}: expected found={want}, got {found:?}"
            );
            if want {
                let f = &found[0];
                assert!(f.via_protein, "the only route to a peptide-only row");
                assert_eq!(f.strand, Strand::Forward);
                assert_eq!(f.len(m.seq.len() as u64), 18, "six residues of bases");
                assert_eq!(f.identity, 1.0, "exact, as the scan emits it");
                assert_eq!(f.coverage, 1.0, "and whole");
                assert!(!f.is_fragment);
                let ev = f.fusion_orf.expect("the ORF it was admitted on");
                assert_eq!(ev.aa_len, orf_aa);
                assert_eq!(ev.start, flank as u64 + 1);
            }
        }

        // Guard for the out-of-frame case: the ORF is still there and still
        // spans the displaced tag, so the fixture is testing the frame rule and
        // not an accidentally truncated ORF.
        let orfs = orf::find_orfs(
            &off.seq,
            fixture_code(),
            false,
            &Params {
                min_aa: ORF_MIN_AA,
                include_incomplete: true,
                require_start: true,
                nested: false,
            },
        );
        let covering = orfs
            .iter()
            .find(|o| {
                o.strand == Strand::Forward
                    && (o.start as usize) <= flank + 124
                    && flank + 124 + 18 <= o.end as usize
            })
            .expect("the ORF must still span the tag; it was truncated");
        assert_eq!(covering.aa_len, orf_aa, "and must still be full length");

        // Guard for the outside case: nothing grew an ORF of its own there.
        let orfs = orf::find_orfs(
            &outside.seq,
            fixture_code(),
            false,
            &Params {
                min_aa: ORF_MIN_AA,
                include_incomplete: true,
                require_start: true,
                nested: false,
            },
        );
        // Forward only, and that is not laxity. `encode` constrains the forward
        // frames alone, on the stated grounds that a reverse-strand start can
        // only produce a reverse ORF and `fused_orf`'s first clause refuses an
        // ORF whose strand differs from the hit's. The tag here reads forward,
        // so a reverse ORF over it cannot admit it — and this fixture does grow
        // one.
        assert!(
            !orfs.iter().any(|o| o.strand == Strand::Forward
                && (o.start as usize) <= 46
                && 46 + 18 <= o.end as usize),
            "the outside case grew a forward ORF of its own: {orfs:?}"
        );
    }

    #[test]
    fn the_his6_tag_on_a_real_construct_of_the_maintainers_is_found() {
        // The measurement that drove this change, turned into a fixture.
        //
        // 73 real plasmids, 17,061,931 ORF residues: His6 occurred eight times
        // and every one was a genuine tag — C-terminal at exactly -0 residues
        // from the stop, behind a GG linker. This rebuilds the shorter of the
        // two constructs from the context read off the file (see
        // `HIS6_CONTEXT`), as an ATG-started ORF of 160 residues ending
        // `...EQIKYTTSLPIEGG` + six histidines + stop.
        //
        // 160 is this FIXTURE's length, not the file's. On the real file `pl`
        // reports a 258 aa ORF, because table 11 opens it at an `ATT` 98 codons
        // upstream of the first `ATG`; `HIS6_CONTEXT`'s docs carry the
        // measurement. What the two share is the property under test — the tag
        // is the last thing before the stop — and that is what is asserted
        // below.
        //
        // At b340b18 this returns nothing, which is the whole point: the
        // shipped tool could not find a single one of the eight.
        //
        // It also pins the `<=` in `fused_orf`'s containment clause. The tag
        // ends on the last coding base, so `d + 3 * tag_aa == 3 * aa_len`
        // exactly; tightening that to `<` looks like an off-by-one tidy-up and
        // would delete all eight measured true positives.
        let mut rng = Rng(0x0f1a_0000_0000_0012);
        let code = fixture_code();
        // `encode_stopless`, not `encode`: the context carries an isoleucine,
        // and all three of its codons are table-11 initiators, so no start-free
        // encoding of it exists. The weaker guarantee is enough here because
        // frame 0 of this fixture spells no methionine after the initiator, so
        // the ORF the predicate uses is the one the ATG below opens; a start in
        // another frame yields an ORF the predicate cannot use (`d.is_multiple_of(3)` is
        // asked in the ORF's own frame) and a later start in this frame is
        // nested and suppressed by `Params::nested: false`.
        let tail = encode_stopless(&format!("{HIS6_CONTEXT}{HIS6}"), code, &mut rng);
        // 1 initiator + 139 filler + 14 context + 6 histidines = 160 residues.
        let cds = format!("ATG{}{tail}TAA", FILLER.repeat(139));
        let flank = FILLER.repeat(30);
        let m = mol(&format!("{flank}{cds}{flank}"), false);
        assert!(
            six_frame_contains(&m.seq, HIS6),
            "the fixture does not contain the tag at all"
        );

        let db = db_of(vec![peptide("pf:his6", HIS6)]);
        let found = Annotator::new(&db, Config::default()).annotate(&m);
        assert_eq!(found.len(), 1, "one tag, one annotation: {found:?}");
        let f = &found[0];
        assert!(f.via_protein);
        assert_eq!(f.strand, Strand::Forward);
        assert_eq!(f.identity, 1.0);
        assert_eq!(f.coverage, 1.0);
        // Residues 155..160 of the ORF, 1-based inclusive in the molecule.
        assert_eq!(f.start, (flank.len() + 3 * 154) as u64 + 1);
        assert_eq!(f.end, (flank.len() + 3 * 160) as u64);
        let ev = f.fusion_orf.expect("the ORF it was admitted on");
        assert_eq!(
            ev.aa_len, 160,
            "the fixture's own ORF length; the real file reads 258 under table 11"
        );
        assert_eq!(ev.start, flank.len() as u64 + 1);
        assert_eq!(
            f.end as usize,
            flank.len() + 3 * ev.aa_len,
            "the tag must end on the ORF's last coding base — the boundary all \
             eight measured occurrences sit on"
        );
    }

    #[test]
    fn no_record_is_both_unfindable_and_unreported() {
        // The invariant the whole routing change exists to restore, asserted
        // rather than described: every record is EITHER reachable by some route
        // OR named by `unseedable()`. Never neither.
        //
        // At b340b18 the peptide row below is neither. It is not in
        // `dna.short()`'s rescue path (it has residues), it is not in
        // `protein.short()` (two windows is not zero), and it cannot chain
        // (two is fewer than three). It is simply absent, and nothing says so.
        //
        // The DNA row is the control: it really is unreachable — six bases
        // against a 12-mer index, no residues — and must stay in the report.
        //
        // Seed chosen for the same reason as
        // `a_six_residue_peptide_is_found_in_frame_in_an_orf_and_nowhere_else`:
        // one of the 677 in 1024 where `encode` returns the same bases before
        // and after its trailing-pad fix, so this fails at b340b18 on the
        // invariant rather than inside the fixture builder.
        let mut rng = Rng(0x0f1a_0000_0000_001d);
        let (inf, ..) = three_cases(THROMBIN, &mut rng);
        let db = db_of(vec![
            peptide("pf:thrombin", THROMBIN),
            rec("pf:minus35", "TTGACA", false),
        ]);
        let ann = Annotator::new(&db, Config::default());

        let reported: Vec<&str> = ann.unseedable().iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            reported,
            vec!["pf:minus35"],
            "the DNA-only row is unreachable and must be named; the peptide row \
             is reachable and must not be"
        );

        let found = ann.annotate(&inf);
        let reachable: std::collections::BTreeSet<usize> = found.iter().map(|a| a.record).collect();
        for (i, r) in db.records.iter().enumerate() {
            assert!(
                reachable.contains(&i) || reported.contains(&r.id.as_str()),
                "{} is neither findable nor reported unreachable — the exact \
                 failure this route closes: {found:?}",
                r.id
            );
        }
    }

    #[test]
    fn a_shipped_tag_stays_findable_when_min_seeds_is_raised() {
        // `Config::min_seeds` is a public field with no clamp and no validation
        // anywhere, and nothing in this tree raises it — which is precisely why
        // nobody would write this test. Raise it to 5 and FLAG, eight residues
        // and four windows, stops chaining: at b340b18 it becomes silently
        // unfindable, exactly like a 6-residue peptide at the default, and it
        // is a row that has already shipped and been signed.
        //
        // This is what makes the fix `min_seeds`-correct rather than merely
        // 7-correct. No constant in the feature builder could have bought it.
        let mut rng = Rng(0x0f1a_0000_0000_0014);
        let (inf, off, outside, _, orf_aa) = three_cases(FLAG, &mut rng);
        assert_eq!(orf_aa, 89);

        let db = db_of(vec![peptide("pf:flag", FLAG)]);
        for min_seeds in [3usize, 4, 5, 9] {
            let cfg = Config {
                min_seeds,
                ..Config::default()
            };
            let ann = Annotator::new(&db, cfg);
            assert!(
                ann.unseedable().is_empty(),
                "min_seeds={min_seeds}: the row is reachable and must not be \
                 reported unreachable"
            );
            let found = ann.annotate(&inf);
            assert_eq!(
                found.len(),
                1,
                "min_seeds={min_seeds}: FLAG must not stop being findable \
                 because a caller asked for more seed support: {found:?}"
            );
            assert_eq!(found[0].identity, 1.0);
            // And the gate is not weakened on the way past: the same tag out of
            // frame and outside any ORF stays nothing at every setting.
            assert!(ann.annotate(&off).is_empty(), "min_seeds={min_seeds}");
            assert!(ann.annotate(&outside).is_empty(), "min_seeds={min_seeds}");
        }
    }

    #[test]
    #[should_panic(expected = "below MIN_PART_AA")]
    fn a_designed_peptide_below_the_part_floor_is_refused_rather_than_half_supported() {
        // Not unmatchable — the scan would find a 4-mer in every frame — but
        // matchable inconsistently. `fused_orf` would admit it in an ORF of 24
        // residues, and `ORF_MIN_AA = 25` is what `find_orfs` is given, so no
        // such ORF is ever offered: findable at 25 and invisible at 24, with
        // nothing said. `unseedable()` cannot report it, because it is not
        // unreachable. So the constructor refuses it instead.
        let db = db_of(vec![peptide("pf:xa", "IEGR")]);
        let _ = Annotator::new(&db, Config::default());
    }

    #[test]
    fn the_encoder_constrains_the_join_into_the_trailing_pad() {
        // A test of the fixture builder, not of the annotator, and it earns its
        // place because two regression tests above depend on the builder being
        // right about exactly this.
        //
        // `encode_in` appends the trailing pad AFTER its search finishes, so the
        // final codon's two overhanging frames used to be constrained by
        // nothing and the definitive assertion was the first thing to see them.
        // At b340b18 this call panics inside `encode` with "the encoding of
        // LVPRGS spells GTG in frame 1": the search commits to `...AGT` for the
        // serine, which spells `GTG` against the pad, and the helper declares
        // the peptide unencodable when four of its six serine codons are fine.
        // This is the seed that exposed it.
        let mut rng = Rng(0x0f1a_0000_0000_0011);
        let code = fixture_code();
        let dna = encode(THROMBIN, code, &mut rng);
        assert_eq!(code.translate(dna.as_bytes()), THROMBIN.as_bytes());

        // Stated here rather than left to the helper's internal assertion, so
        // the property is a claim this test makes and not one it borrows.
        let padded = format!("{}{dna}{}", FILLER.repeat(2), FILLER.repeat(2));
        for f in 0..3 {
            for c in padded.as_bytes()[f..].chunks_exact(3) {
                assert!(
                    !code.is_stop(c) && !code.is_start(c),
                    "frame {f} of the padded encoding spells {}",
                    String::from_utf8_lossy(c)
                );
            }
        }
    }

    #[test]
    fn a_histidine_tract_longer_than_the_record_is_not_annotated_twice() {
        // A shipped alias of the row this change issues is `His10`, and a
        // 10-histidine tract was drawn as TWO overlapping "Polyhistidine tag"
        // boxes 12 bases apart, neither of them covering the tract.
        //
        // `resolve_overlaps` was expected to collapse them and cannot: it
        // compares `core()` intervals shrunk by `overlap_trim`, so at 15% an
        // 18 bp hit loses 2 bases at each end and two such hits 12 bases apart
        // stop meeting; `contained_in` is gated on `k.record != h.record` and so
        // never fires between two copies of one record. Measured before the fix,
        // tract length -> annotations: 6,7,8,9 -> 1; 10,11,12,13 -> 2; 14 -> 3.
        //
        // What is asserted is `n / 6`: the number of DISJOINT copies of a
        // six-residue record the tract really contains. Tiling rather than
        // collapsing to one box is deliberate -- the same rule over a (GGGGS)8
        // stretch reports the two (GGGGS)4 copies that are genuinely there.
        // Reporting the tract's own length instead of the record's needs greedy
        // run extension in the matcher, which is a separate change with its own
        // tests; PLF:3004's caveat says so.
        let code = fixture_code();
        for n in 6usize..=14 {
            let mut rng = Rng(0x0f1a_0000_0000_0015 + n as u64);
            let tail = encode_stopless(&format!("{HIS6_CONTEXT}{}", "H".repeat(n)), code, &mut rng);
            let cds = format!("ATG{}{tail}TAA", FILLER.repeat(139));
            let flank = FILLER.repeat(30);
            let m = mol(&format!("{flank}{cds}{flank}"), false);
            assert!(six_frame_contains(&m.seq, &"H".repeat(n)));

            let db = db_of(vec![peptide("pf:his6", HIS6)]);
            let found = Annotator::new(&db, Config::default()).annotate(&m);
            assert_eq!(
                found.len(),
                n / HIS6.len(),
                "a {n}-histidine tract: {found:?}"
            );
            // Whatever the count, no two of them may overlap.
            let mut spans: Vec<(u64, u64)> = found.iter().map(|a| (a.start, a.end)).collect();
            spans.sort();
            for w in spans.windows(2) {
                assert!(
                    w[0].1 < w[1].0,
                    "a {n}-histidine tract produced overlapping boxes: {spans:?}"
                );
            }
        }
    }

    #[test]
    #[should_panic(expected = "below MIN_PART_AA")]
    fn a_short_protein_reference_is_refused_on_a_cds_row_too() {
        // The floor used to be keyed on `Record::is_designed_peptide`, which is
        // `synthetic_part` only, while the route it guards --
        // `Index::unchainable` -> `scan_protein_exact` -- is blind to class.
        // `Db::parse` checks `reference_aa`'s alphabet and nothing else, so a
        // `cds` row could carry four residues, walk past the floor, and be
        // reported by an exact six-frame scan with neither the exactness rule
        // nor the fusion gate applied to it: measured at 91..102 with
        // `fusion_orf: None` on a molecule of pure filler with no ATG anywhere,
        // where b340b18 reported nothing at all.
        //
        // Without the widening this constructor returns normally and the test
        // fails for want of a panic.
        let db = db_of(vec![rec("pf:cds-with-a-tetrapeptide", "IEGR", true)]);
        let _ = Annotator::new(&db, Config::default());
    }

    #[test]
    fn a_peptide_that_indexes_no_word_is_scanned_rather_than_written_off() {
        // `unseedable()` says "nothing can find this". For a record with
        // residues that claim has to be checked against the scan, not against
        // the protein index's seed list, and this is the record where the two
        // disagree: `index::seedable` rejects `X`, so a peptide of nothing but
        // `X` indexes ZERO words and sits in `protein.short()` -- while the
        // exact scan searches for it in all six frames and finds it wherever the
        // query really translates to those residues, which an `NNN` codon does.
        //
        // Three mutations die here. Restoring `unseedable()`'s old
        // `dna.short() & protein.short()` intersection reports this row as "too
        // short to seed and cannot be found" while the run below is finding it.
        // Restoring the `return` on `words() == 0` at the top of `scan_protein`
        // skips the scan entirely, because this one-row table indexes no words
        // at all. And dropping `.max(1)` from `unchainable(min_seeds.max(1))`
        // leaves the record with no route at `min_seeds = 0`, which is the
        // original defect reopened at a setting a caller may legitimately pick.
        let x8 = "XXXXXXXX";
        // 1 initiator + 20 filler + 8 X = 29 residues, and the fusion gate wants
        // 8 + PARTNER_MIN = 28.
        let cds = format!("ATG{}{}TAA", FILLER.repeat(20), "NNN".repeat(8));
        let flank = FILLER.repeat(30);
        let m = mol(&format!("{flank}{cds}{flank}"), false);
        assert!(
            six_frame_contains(&m.seq, x8),
            "NNN must translate to X, or this fixture tests nothing"
        );

        let db = db_of(vec![peptide("pf:all-x", x8)]);
        let protein = Index::build(&db, true, K_PROTEIN);
        assert_eq!(protein.words(), 0, "the premise: seedable() rejects X");
        assert_eq!(
            protein.short(),
            [0],
            "and so the old rescue predicate held it"
        );

        for min_seeds in [0usize, 1, 3, 9] {
            let ann = Annotator::new(
                &db,
                Config {
                    min_seeds,
                    ..Config::default()
                },
            );
            assert!(
                ann.unseedable().is_empty(),
                "min_seeds={min_seeds}: the scan reaches this row, so nothing may \
                 call it unreachable"
            );
            let found = ann.annotate(&m);
            assert_eq!(
                found.len(),
                1,
                "min_seeds={min_seeds}: and the scan must actually reach it: {found:?}"
            );
            assert!(found[0].via_protein);
            assert_eq!(found[0].identity, 1.0);
            assert!(
                found[0].fusion_orf.is_some(),
                "still gated on the ORF, like every other designed peptide"
            );
        }
    }

    #[test]
    fn routing_is_on_indexed_words_and_a_length_test_would_lose_this_one() {
        // `Index::unchainable` takes a word count rather than a length, and the
        // difference is only visible on a peptide that indexes fewer words than
        // its length implies. `X_AT_FIVE` is twelve residues and eight 5-mer
        // windows, of which the five covering offset 5 are rejected by
        // `seedable`, leaving 3 indexed words.
        //
        // At `min_seeds = 4` it cannot chain and must be scanned. The length
        // form of the predicate, `len < K_PROTEIN + min_seeds - 1`, asks
        // `12 < 8`, answers "this chains fine", and leaves the row with no route
        // at all -- the same silent hole this whole change closes, at a
        // different residue count.
        const X_AT_FIVE: &str = "MDEKTXGWRGGH";
        let mut rng = Rng(0x0f1a_0000_0000_0031);
        let code = fixture_code();
        // `encode_stopless`: the peptide starts with M, whose only codon is a
        // start. The nested ATG is harmless here for the same reason as in
        // `the_his6_tag_on_a_real_construct_of_the_maintainers_is_found` --
        // `Params::nested: false` suppresses a later start in this frame.
        let head = encode_stopless(&X_AT_FIVE[..5], code, &mut rng);
        let tail = encode_stopless(&X_AT_FIVE[6..], code, &mut rng);
        // 1 initiator + 20 filler + 12 tag = 33 residues; the gate wants 32.
        let cds = format!("ATG{}{head}NNN{tail}TAA", FILLER.repeat(20));
        let flank = FILLER.repeat(30);
        let m = mol(&format!("{flank}{cds}{flank}"), false);
        assert!(six_frame_contains(&m.seq, X_AT_FIVE));

        let db = db_of(vec![peptide("pf:x-at-five", X_AT_FIVE)]);
        assert_eq!(
            Index::build(&db, true, K_PROTEIN)
                .unchainable(4)
                .collect::<Vec<_>>(),
            [0],
            "3 indexed words cannot make a run of 4"
        );
        for min_seeds in [3usize, 4, 5] {
            let ann = Annotator::new(
                &db,
                Config {
                    min_seeds,
                    ..Config::default()
                },
            );
            let found = ann.annotate(&m);
            assert_eq!(
                found.len(),
                1,
                "min_seeds={min_seeds}: chaining at 3, scanning from 4, findable \
                 at every setting: {found:?}"
            );
            assert!(ann.unseedable().is_empty(), "min_seeds={min_seeds}");
        }
    }

    #[test]
    fn a_lower_cased_reference_still_matches() {
        // `exact_occurrences` compares case-insensitively, and nothing showed
        // that it had to: `Db::parse` upper-cases `reference_aa` and
        // `translate::six_frames` reads its residues out of the code table, so
        // through the shipped path both sides are already upper-case. The state
        // is reachable anyway -- `Record`'s fields are public and this workspace
        // constructs them directly -- so the property is pinned rather than left
        // as an assumption a `==` would quietly break.
        let mut rng = Rng(0x0f1a_0000_0000_0032);
        let (inf, ..) = three_cases(THROMBIN, &mut rng);
        let lower = Record {
            reference_aa: Some(THROMBIN.to_ascii_lowercase().into_bytes()),
            ..peptide("pf:thrombin", THROMBIN)
        };
        let db = db_of(vec![lower]);
        let found = Annotator::new(&db, Config::default()).annotate(&inf);
        assert_eq!(
            found.len(),
            1,
            "a lower-cased cell must still match: {found:?}"
        );
        assert_eq!(found[0].identity, 1.0);
        assert_eq!(found[0].coverage, 1.0);
    }

    /// One note qualifier, and it names both numbers.
    ///
    /// PROVEN TO FAIL by deleting the `coverage` clause from the format string:
    ///
    /// ```text
    /// the note says nothing checkable: "PLF:X nucleotide match: 100.0%
    /// identity, polylinker feature db test"
    /// ```
    ///
    /// The pairing is the property under test and it is not decoration.
    /// `identity` is a LOCAL identity over the aligned region and `coverage` is
    /// the fraction of the database feature reproduced, so the first 300 bp of
    /// a 600 bp marker copied perfectly is `100.0% identity, 50% coverage`.
    /// A note carrying only the first would say "this IS that feature" about
    /// half of one.
    #[test]
    fn the_provenance_note_carries_identity_and_coverage_together() {
        let mut rng = Rng(0x51de_0000_0000_0001);
        let gene = rng.dna(600);
        let db = db_of(vec![rec("PLF:X", &gene, false)]);
        // Only the first half of the record is present, so the two numbers
        // genuinely differ and a test that printed one could not pass by
        // reading the other.
        let m = mol(&format!("{}{}", rng.dna(200), &gene[..300]), false);
        let found = Annotator::new(&db, Config::default()).annotate(&m);
        assert_eq!(found.len(), 1, "the premise: one hit: {found:?}");
        assert!(found[0].is_fragment, "the premise: half a feature");

        let feat = to_feature(&db, &found[0]);
        assert_eq!(feat.name, "PLF:X");
        assert_eq!(feat.kind, "CDS");
        assert_eq!(feat.segments.len(), 1);
        assert_eq!(
            (feat.segments[0].start, feat.segments[0].end),
            (found[0].start, found[0].end)
        );
        let notes: Vec<&str> = feat
            .qualifiers
            .iter()
            .filter(|(k, _)| k == "note")
            .filter_map(|(_, v)| v.as_deref())
            .collect();
        assert_eq!(notes.len(), 1, "one note for a nucleotide hit: {notes:?}");
        assert!(
            notes[0].contains("identity") && notes[0].contains("coverage"),
            "the note says nothing checkable: {:?}",
            notes[0]
        );
        assert_eq!(
            notes[0],
            "PLF:X nucleotide match: 100.0% identity, 50% coverage, polylinker \
             feature db test; PROPOSED, not reviewed by a human",
            "the exact string is the contract: `pl annotate --genbank` and the desktop \
             app's Accept button both emit it, and a reader compares the two"
        );
    }

    /// A reviewed record does NOT carry the proposed caveat, and an unreviewed
    /// one does.
    ///
    /// PROVEN TO FAIL by inverting the `==` in `to_feature`: the reviewed row
    /// gained "; PROPOSED, not reviewed by a human" and the proposed row lost
    /// it, which is the failure that matters — a machine-assembled name landing
    /// in somebody's file with nothing saying no human ever checked it.
    #[test]
    fn only_an_unreviewed_record_carries_the_caveat() {
        let mut rng = Rng(0x51de_0000_0000_0002);
        let gene = rng.dna(400);
        for (status, wanted) in [
            (ReviewStatus::Proposed, true),
            (ReviewStatus::Reviewed, false),
            (ReviewStatus::Verified, false),
        ] {
            let db = db_of(vec![Record {
                review_status: status,
                ..rec("PLF:Y", &gene, false)
            }]);
            let m = mol(&format!("{}{gene}", rng.dna(50)), false);
            let found = Annotator::new(&db, Config::default()).annotate(&m);
            assert_eq!(found.len(), 1, "{status:?}: the premise: {found:?}");
            let note = to_feature(&db, &found[0]).qualifiers[0]
                .1
                .clone()
                .expect("the note has a value");
            assert_eq!(
                note.contains("PROPOSED, not reviewed by a human"),
                wanted,
                "{status:?} produced {note:?}"
            );
        }
    }

    /// A peptide part admitted by the fusion rule gets a SECOND note saying
    /// which ORF admitted it.
    ///
    /// PROVEN TO FAIL by dropping the `fusion_orf` arm: one note instead of
    /// two, and a FLAG tag then appears on a map with no visible protein under
    /// it and nothing anywhere explaining why it is there — which is the exact
    /// situation `Annotation::fusion_orf`'s own doc exists to prevent.
    #[test]
    fn a_fused_peptide_part_says_which_orf_admitted_it() {
        let mut rng = Rng(0x51de_0000_0000_0003);
        let (inf, ..) = three_cases(FLAG, &mut rng);
        let db = db_of(vec![peptide("PLF:FLAG", FLAG)]);
        let found = Annotator::new(&db, Config::default()).annotate(&inf);
        assert_eq!(found.len(), 1, "the premise: {found:?}");
        assert!(found[0].fusion_orf.is_some(), "the premise: fused");

        let feat = to_feature(&db, &found[0]);
        let notes: Vec<String> = feat
            .qualifiers
            .iter()
            .filter(|(k, _)| k == "note")
            .map(|(_, v)| v.clone().unwrap_or_default())
            .collect();
        assert_eq!(notes.len(), 2, "the ORF is not stated: {notes:?}");
        assert!(
            notes[1].starts_with("peptide reference, admitted because it lies in frame inside"),
            "{:?}",
            notes[1]
        );
        assert!(
            notes[1].contains(" aa ORF at ") && notes[1].contains(" strand"),
            "{:?}",
            notes[1]
        );
    }

    /// An origin-crossing hit becomes ONE inverted segment, not two.
    ///
    /// PROVEN TO FAIL by writing `Segment::new(a.start.min(a.end),
    /// a.start.max(a.end))`, which produces a segment that looks ordinary,
    /// covers the whole plasmid the wrong way round, and reads as a feature
    /// spanning everything except the one it names.
    #[test]
    fn an_origin_crossing_hit_is_one_inverted_segment() {
        let mut rng = Rng(0x51de_0000_0000_0004);
        let gene = rng.dna(300);
        let db = db_of(vec![rec("PLF:Z", &gene, false)]);
        // The feature straddles base 1: its last 150 bases are written first.
        let m = mol(
            &format!("{}{}{}", &gene[150..], rng.dna(400), &gene[..150]),
            true,
        );
        let found = Annotator::new(&db, Config::default()).annotate(&m);
        assert_eq!(found.len(), 1, "the premise: {found:?}");
        assert!(found[0].wraps_origin, "the premise: it wraps: {found:?}");

        let feat = to_feature(&db, &found[0]);
        assert_eq!(feat.segments.len(), 1, "a wrap is not a join here");
        assert!(
            feat.segments[0].end < feat.segments[0].start,
            "the wrap was straightened out: {:?}",
            feat.segments[0]
        );
        assert_eq!(
            feat.extent(m.len(), true),
            Some((found[0].start, found[0].end)),
            "the feature no longer says where the hit was"
        );
    }
}
