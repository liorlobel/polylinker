//! Choosing a PCR primer pair for a region, against the molecule you have open.
//!
//! Everything else in this workspace answers a question about a primer you
//! already hold. `pl-primer` says where it anneals, `pl-thermo` says what it
//! melts at, `pl-clone` says what it amplifies. Nothing chose one. This does.
//!
//! # The split, restated because design is where it gets broken
//!
//! `pl-primer`'s module doc is the thing to read first, and its rule governs
//! here: a primer is a **footprint** (the 3' portion that pairs with the
//! template) and a **tail** (the 5' portion that does not — a restriction site
//! being added, a Gibson arm, a barcode), and **a tail must not contribute to
//! Tm**.
//!
//! A designer breaks that rule in a way an analyser cannot. `find_bindings`
//! *derives* the split by annealing; a designer *declares* it, because it chose
//! those bases. The tempting implementation — build the tailed oligo, hand it
//! back to `find_bindings`, read `Binding.tm` — is subtly wrong, and
//! `pl-primer` measured how wrong: with `extend_mismatches` on (the default),
//! "the base next to the footprint mismatches and the one beyond it matches by
//! chance about a quarter of the time, so a lenient extension quietly absorbs
//! two bases of tail into the footprint. That was six of eighty cases in the
//! pydna differential." Two absorbed bases inflate the reported Tm by 1-3 °C,
//! in the direction that runs the anneal too hot. So the Tm reported here is
//! always [`Primer::footprint`]'s, computed from the bases this crate chose.
//! `find_bindings` is used for one thing only: finding sites somewhere else.
//!
//! # Bacteria only, and what that costs RT-PCR
//!
//! Scope is bacterial templates. That is a real simplification, not a
//! placeholder, and one consequence has to be stated wherever RT-PCR is
//! offered rather than buried here:
//!
//! **Bacteria have no introns. A pair designed here cannot span an exon-exon
//! junction, and therefore cannot be made to fail on genomic DNA.** Every
//! product amplifies as well from contaminating gDNA as from cDNA. This is not
//! a missing feature — for a bacterial template an intron-spanning primer is
//! not unimplemented, it is meaningless — so the docs must not say "coming
//! soon", which would tell a reader the guarantee is achievable and merely
//! absent. The only controls are a DNase-treated RNA prep and a no-RT control,
//! and [`Report::warnings`] carries that sentence on every RT-PCR run.
//!
//! # The differentiator: specificity against the actual open molecule
//!
//! Most designers score a primer in isolation and defer specificity to BLAST.
//! [`pl_primer::find_bindings`] can check a candidate against *the molecule the
//! user has open*, so a primer that also primes in their own backbone is
//! rejected during enumeration rather than discovered on a gel. That check is
//! part of the gate, not a decoration bolted on afterwards.
//!
//! Its scope is exactly one molecule, and the wording never says otherwise.
//! Never the bare word "specific"; always "unique in *pUC19-myGene* (5,386 bp,
//! circular); not checked against any genome". A primer unique in a plasmid is
//! routinely not unique in *E. coli*, and the tool's greatest strength is also
//! the source of a claim narrower than it reads.
//!
//! # Every ΔG here is one convention, said once
//!
//! **Stack sum, ΔG°37, SantaLucia & Hicks 2004 stacks, 1 M Na⁺** — computed by
//! [`pl_thermo::dg37_stacks`], which derives it from the ΔH and ΔS already
//! stored there. No initiation term, no loop term, no dangling ends, no
//! terminal mismatches, no salt correction. One convention for the 3'-end
//! stability criterion, the hairpin stem and the dimer helix, so that two ΔG
//! numbers in the same report are comparable.
//!
//! The 2004 stacks are used for ΔG whatever table was chosen for Tm, and the
//! report says so. Mixing 1998 stacks with a 2004-derived threshold would be an
//! undocumented hybrid.
//!
//! # What this is not
//!
//! - **Not a fold.** [`fold`] finds perfect ungapped helices. It has no
//!   internal loops, bulges, dangling ends, terminal mismatches, coaxial
//!   stacking or quadruplexes. `docs/PLAN.md` §7.4 schedules a Zuker DP
//!   (seqfold, Lattice Automation, MIT) that would replace it; **that has not
//!   been ported and is not cited as though it had**. Every ΔG it renders is
//!   printed with the words "or more stable" rather than as a value — an
//!   operator was overloaded here and pointed the wrong way for the one thing
//!   `render` is ever called on — and [`fold::SCREEN_NOTE`] is carried in
//!   [`Report::warnings`] on every run, so this limit reaches a `pl design`
//!   user without their having to run `pl methods design` to find it. The raw
//!   values are also in the JSON, unqualified and labelled by basis.
//! - **No Mg²⁺ or dNTP correction.** `pl-thermo` has monovalent corrections
//!   only, so every Tm here is on a 50 mM Na⁺ scale by default and an ordinary
//!   PCR buffer is about 150 mM monovalent-equivalent. Measured: the same
//!   20-mer reports 55.4 °C at 50 mM and 60.7 °C at 150 mM. The default Tm
//!   window is stated on the model's own scale for exactly this reason — see
//!   [`Constraints::TM_OPT`].
//! - **Not genome-scale.** See above.
//! - **No in-frame mode.** A tail whose length is a multiple of three is not
//!   evidence that a fusion is in frame; frame depends on the vector's reading
//!   frame at the insertion site, which this does not know. It will never pad a
//!   tail to preserve a frame, and [`Report::warnings`] says so beside every
//!   tail.
//!
//! # Provenance
//!
//! | source | licence | used for |
//! |---|---|---|
//! | SantaLucia 1998 PNAS 95:1460; SantaLucia & Hicks 2004 Annu Rev Biophys 33:415 | published science | NN parameters, via `pl-thermo` |
//! | Rychlik 1993 Methods Mol Biol 15:31-40, on Breslauer et al. 1986 PNAS 83:3746 parameters | published science | 3'-terminal stability as a criterion, and the −9 kcal/mol figure this crate does *not* import |
//! | Dieffenbach, Lowe & Dveksler 1993 PCR Methods Appl 3:S30; Innis & Gelfand 1990 | published science | length, %GC, poly-N and GC-clamp conventions |
//! | Rozen & Skaletsky 2000 Methods Mol Biol 132:365; Untergasser et al. 2012 NAR 40:e115 | published science | that a designer ranks by a weighted sum of deviations |
//! | Kaufman & Evans 1990 BioTechniques 9:304; Moreira & Noren 1995 BioTechniques 19:56 | published science | that cleavage near a fragment terminus is inefficient |
//!
//! **Not used, and nobody opens them:** `oligotm.c`, `thal.c`, `libprimer3.c`
//! or any other file of the Primer3 source tree, including the C sources
//! vendored inside the installed `primer3-py` wheel. Primer3 is GPL-2.0 and a
//! GPL-derived file inside this crate would relicense the distribution.
//! Primer3's `PRIMER_WT_*` default weights are deliberately **not** copied
//! either: they are an equally undefended set of conventions wearing an
//! authoritative-looking provenance, and importing them would launder one into
//! the other. The weights here are ours, printed by the tool, and no parity
//! with Primer3 is claimed.

pub mod fold;
pub mod oligo;
pub mod pair;
pub mod params;
pub mod report;
pub mod specificity;
pub mod tail;

pub use oligo::{Candidate, Side};
pub use pair::Primer;
pub use params::{Constraints, Mode, Weights};
pub use report::{Pair, Reason, Report};
pub use tail::Tail;

use pl_core::iupac::Composition;

/// The region to amplify: 1-based, inclusive, on the plus strand.
///
/// `end < start` means the region **wraps the origin**, which is only legal on
/// a circular molecule. That bit is load-bearing, and the GUI must derive it
/// from `Selection::through_origin` rather than from the ordering of the two
/// carets. `seqedit.rs` spells out why: a pair of carets on a circle names two
/// arcs, not one — `(40, 4961)` on a 5,386 bp plasmid is either the 4,921 bases
/// between them or the 465 across the origin, and no ordering of the pair
/// distinguishes them. Reading `(lo, hi)` off a through-origin selection
/// designs primers for the *complement arc*: a 4,921 bp amplicon where the user
/// asked for 465. Every number in that report would look entirely reasonable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub start: u64,
    pub end: u64,
}

impl Region {
    pub fn new(start: u64, end: u64) -> Self {
        Region { start, end }
    }
    /// Does this region cross the origin?
    pub fn wraps(&self) -> bool {
        self.end < self.start
    }
    /// How many bases it covers on an `n` bp molecule.
    ///
    /// Not `end - start + 1`, which reports 4,921 for a 465 bp origin-crossing
    /// region — the same trap [`pl_core`]'s callers hit and the reason
    /// `Selection::base_count` exists in the GUI.
    pub fn len(&self, n: u64) -> u64 {
        if self.wraps() {
            n - self.start + 1 + self.end
        } else {
            self.end - self.start + 1
        }
    }
    pub fn is_empty(&self, n: u64) -> bool {
        self.len(n) == 0
    }
}

/// Why no design could be produced.
///
/// Every variant names the numbers involved, because "no primers found" is a
/// useless answer to a question that has a specific arithmetic reason.
#[derive(Debug, Clone, PartialEq)]
pub enum DesignError {
    /// Features but no bases: the coordinates belong to a sequence held
    /// elsewhere.
    AnnotationTrack { features: usize },
    /// A declared length and none of the bases it names.
    SequenceAbsent { declared: u64 },
    /// The template is shorter than the shortest primer asked for.
    TemplateTooShort { bp: u64, len_min: usize },
    /// `start > end` on a linear molecule, where there is no origin to cross.
    BackwardsOnALine { start: u64, end: u64 },
    /// A coordinate past the end of the molecule.
    OutsideTemplate { start: u64, end: u64, bp: u64 },
    /// [`Mode::Within`] on a region too short to hold two primers.
    RegionTooShort {
        bp: u64,
        shortest_product: usize,
        len_min: usize,
    },
    /// [`Mode::Contain`] on a linear molecule with no room for a footprint at
    /// one end of the region.
    ///
    /// Not "no template *outside* the region": a Contain footprint needs none,
    /// because `flank` bounds the primer's outer end and `lo = s` / `hi = e`
    /// are always enumerated. `which` names the end that has no room, and
    /// `available` is how many bases of template a footprint there would have
    /// to sit in.
    NoFlank {
        which: &'static str,
        available: u64,
        needed: usize,
        bp: u64,
    },
    /// An ambiguity code where a candidate would have to span it.
    ///
    /// Worded after `TmError::NotUnambiguous`, whose doc gives the reason: a Tm
    /// over an ambiguity code is a different, shorter oligo's — "a number that
    /// looks entirely reasonable and is about something else".
    AmbiguousTarget { position: u64, base: u8 },
    /// Every candidate was rejected on its own. Carries the attrition tally.
    ///
    /// The tally is boxed, and so is `NoPair`'s. A `Tally` is 15 counters plus
    /// 15 rendered labels, and once `NoPair` also carried the site clashes and
    /// the remedy the enum crossed 128 bytes — at which point every
    /// `Result<_, DesignError>` in the crate pays for the largest refusal on
    /// the success path too. Boxing the two heavy variants keeps `design()`
    /// cheap to return from without any variant having to give up a field it
    /// needs.
    NoCandidate {
        enumerated: usize,
        tally: Box<report::Tally>,
        constraints: String,
    },
    /// Candidates survived and every pair of them was rejected.
    ///
    /// Carries `enumerated` as well as `built` because the tally spans two
    /// funnels and the candidate-stage half has to be counted out of the number
    /// of candidates; printing it under a pair heading was how "2935 Tm outside
    /// 52.0-58.0C" came to be listed as a pair rejection.
    NoPair {
        survivors: usize,
        enumerated: usize,
        built: usize,
        tally: Box<report::Tally>,
        /// The unintended restriction sites, if that is what refused the pairs.
        clashes: Vec<tail::SiteClash>,
        constraints: String,
    },
    /// The tails alone are as long as the longest product allowed.
    ///
    /// Named rather than left to come out as "0 pairs were built": once the
    /// product window gates the amplicon rather than the template span, a
    /// `--product` ceiling below the tails describes no molecule at all, and the
    /// arithmetic is the answer.
    TailsExceedProduct { tail_bp: u64, product_max: u64 },
    /// `--add-5`/`--add-3` named an enzyme whose site is not fully specified.
    AmbiguousSite {
        enzyme: &'static str,
        site: &'static str,
    },
    /// The off-target seed is longer than the shortest primer the search may
    /// build, so some candidates could not be scanned at all.
    ///
    /// Nothing enforced this relation: `--off-seed` is validated against 8..32
    /// and `--len` against 8..60, independently, and `Constraints` had no
    /// validator. A seed longer than the footprint then broke both specificity
    /// paths — the index path by a `usize` underflow, the fall-back path by
    /// silently certifying every short candidate as unique. Refused here rather
    /// than papered over downstream, because "cannot be scanned" has no safe
    /// reading as an answer: passing the candidate claims a uniqueness nothing
    /// checked, and failing it blames the molecule for a setting.
    SeedLongerThanPrimer { off_seed: usize, len_min: usize },

    /// The shortest primer `--len` allows is below five bases.
    ///
    /// A primer's 3'-stability and GC-clamp criteria read a terminal pentamer
    /// (`oligo::evaluate`), so a candidate shorter than five bases has no
    /// pentamer to slice and the evaluation underflows the `len - 5` index. The
    /// shipped `--len` floor is 8, so this only catches a hand-built
    /// `Constraints`; refused here for the same reason as the seed relation —
    /// an unscannable primer has no safe reading as an answer.
    PrimerTooShort { len_min: usize },
}

impl std::fmt::Display for DesignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DesignError::AnnotationTrack { features } => write!(
                f,
                "this is an annotation track: it carries {features} feature{} and no bases, \
                 so there is nothing here to design against. Open the sequence these \
                 coordinates describe and design against it.",
                if *features == 1 { "" } else { "s" }
            ),
            DesignError::SequenceAbsent { declared } => write!(
                f,
                "this file declares {declared} bases and carries none of them -- \
                 annotation-only GenBank. There is nothing here to design against."
            ),
            DesignError::TemplateTooShort { bp, len_min } => write!(
                f,
                "the template is {bp} bp; nothing shorter than {len_min} bases is a primer \
                 here. Lower --len, or give a longer template."
            ),
            DesignError::BackwardsOnALine { start, end } => write!(
                f,
                "--region {start}..{end} runs backwards. On a circle that would be a region \
                 crossing the origin; this molecule is linear, so there is no template \
                 between {start} and {end}."
            ),
            DesignError::OutsideTemplate { start, end, bp } => write!(
                f,
                "--region {start}..{end} is outside the {bp} bp template. Give coordinates \
                 within 1..{bp}, and on a circular molecule let the region cross the origin \
                 if that is what you mean."
            ),
            DesignError::RegionTooShort {
                bp,
                shortest_product,
                len_min,
            } => write!(
                f,
                "the region is {bp} bases. The shortest product two {len_min} nt primers can \
                 make is {shortest_product} bases, so there is no room for a pair inside it. \
                 Amplify a longer region, use --mode contain so the primers may sit outside \
                 it, or lower --len."
            ),
            DesignError::NoFlank {
                which,
                available,
                needed,
                bp,
            } => write!(
                f,
                "there is no room for a primer {which} on this {bp} bp linear sequence: a \
                 footprint there has {available} nt of template to sit in and the shortest \
                 --len is {needed}. --mode contain needs no template OUTSIDE the region -- \
                 the two footprints may sit exactly on its ends -- so this is the molecule \
                 running out, not the flank. Raise --flank so the primer may back off \
                 further, lower --len, move the region, or use --mode within (which changes \
                 what is amplified)."
            ),
            DesignError::AmbiguousTarget { position, base } => write!(
                f,
                "the target contains {:?} at {position}. A melting temperature over an \
                 ambiguity code is a different oligo's, so no candidate spanning it can be \
                 scored. Restrict the region to unambiguous bases, or fix the base.",
                *base as char
            ),
            DesignError::NoCandidate {
                enumerated,
                tally,
                constraints,
            } => {
                writeln!(
                    f,
                    "no primer meets these constraints. {enumerated} candidate oligo{} \
                     built and every one was rejected:",
                    if *enumerated == 1 { " was" } else { "s were" }
                )?;
                write!(f, "{}", tally.render("  ", *enumerated, 0))?;
                write!(f, "{constraints}")
            }
            DesignError::NoPair {
                survivors,
                enumerated,
                built,
                tally,
                clashes: _,
                constraints,
            } => {
                // `built == 0` is not a rejection, and saying "all 0 pairs were
                // rejected" described one that never happened. It means the
                // product window left no combination for the pairing loop to
                // consider at all, which is a different refusal with a
                // different remedy.
                if *built == 0 {
                    writeln!(
                        f,
                        "{survivors} oligo{} passed on their own and no two of them could be \
                         paired:",
                        if *survivors == 1 { "" } else { "s" }
                    )?;
                } else {
                    writeln!(
                        f,
                        "{survivors} oligo{} passed on their own and all {built} pair{} rejected:",
                        if *survivors == 1 { "" } else { "s" },
                        if *built == 1 { " was" } else { "s were" }
                    )?;
                }
                write!(f, "{}", tally.render("  ", *enumerated, *built))?;
                // The remedy tracks the tally rather than being a fixed
                // sentence. It used to end "widen --tm-diff or --product"
                // whatever the reason, so a run refused 39,441 times because an
                // EcoRI site sits inside the amplicon was told to widen two
                // knobs, neither of which can move a site that is already in
                // the template.
                write!(f, "{constraints}")
            }
            DesignError::TailsExceedProduct {
                tail_bp,
                product_max,
            } => write!(
                f,
                "the tails add {tail_bp} nt to every product and --product allows at most \
                 {product_max} bp, so no amplicon can satisfy both. The product window is \
                 the AMPLICON's, tails included, because that is the molecule that runs on \
                 the gel. Raise --product, shorten --spacer, or drop a tail."
            ),
            DesignError::AmbiguousSite { enzyme, site } => write!(
                f,
                "{enzyme}'s site is {site}, which is not fully specified. A tail is real DNA \
                 that has to be ordered, so an N in the site has no single oligo to write \
                 down. Choose an enzyme whose site is unambiguous."
            ),
            DesignError::SeedLongerThanPrimer { off_seed, len_min } => write!(
                f,
                "--off-seed {off_seed} is longer than the shortest primer --len allows \
                 ({len_min}). A 3'-anchored seed longer than the primer cannot anchor, so \
                 every candidate of {len_min} nt would go unscanned -- and an unscanned \
                 candidate is not a specific one. Lower --off-seed to at most {len_min}, or \
                 raise the shortest --len to at least {off_seed}. --no-specificity skips the \
                 scan outright, and the report says so."
            ),
            DesignError::PrimerTooShort { len_min } => write!(
                f,
                "the shortest primer --len allows is {len_min}, below the five bases the \
                 3'-stability and GC-clamp checks read as a terminal pentamer. Raise the \
                 shortest --len to at least 5 (the shipped floor is 8)."
            ),
        }
    }
}

impl std::error::Error for DesignError {}

/// Design against a whole molecule, refusing the two files that carry no bases.
///
/// The gate and its two sentences live here rather than in the CLI and again in
/// the GUI. `pl-doc`'s module doc names the failure that avoids: prose restated
/// in two places drifts, and the drift is invisible because the sentence still
/// reads correctly and is no longer true. `seqedit::Editability` makes the same
/// distinction for the editor, and these are deliberately the same three cases.
pub fn design_molecule(
    mol: &pl_core::Molecule,
    region: Region,
    c: &Constraints,
) -> Result<Report, DesignError> {
    if mol.is_annotation_track() {
        return Err(DesignError::AnnotationTrack {
            features: mol.features.len(),
        });
    }
    if mol.sequence_absent() {
        return Err(DesignError::SequenceAbsent {
            declared: mol.declared_len.unwrap_or(0),
        });
    }
    design(&mol.seq, mol.topology.is_circular(), region, c)
}

/// Design a primer pair for `region` on `template`.
///
/// Pure: no clock, no environment, no filesystem, no randomness. The same
/// inputs give byte-identical output, which is what makes a design diffable and
/// re-orderable — see `tests/determinism.rs`.
pub fn design(
    template: &[u8],
    circular: bool,
    region: Region,
    c: &Constraints,
) -> Result<Report, DesignError> {
    let n = template.len() as u64;
    if n == 0 {
        return Err(DesignError::SequenceAbsent { declared: 0 });
    }
    if n < c.len_min as u64 {
        return Err(DesignError::TemplateTooShort {
            bp: n,
            len_min: c.len_min,
        });
    }
    // The 3'-stability and GC-clamp checks read a terminal pentamer, so a primer
    // shorter than five bases has none to read and `oligo::evaluate` underflows
    // the `len - 5` slice. The shipped --len floor is 8; this catches a
    // hand-built `Constraints` that set it lower, before any candidate is built.
    if c.len_min < 5 {
        return Err(DesignError::PrimerTooShort { len_min: c.len_min });
    }
    // The one relation between two flags that nothing else checks. Both are
    // range-validated on their own -- `--off-seed` against 8..32, `--len`
    // against 8..60 -- and neither interface compares them, so `--off-seed 20`
    // at the default `--len 18..27` reached `specificity::scan` with an 18 nt
    // footprint and a 20 nt seed and crashed the process.
    if c.specificity && c.off_seed > c.len_min {
        return Err(DesignError::SeedLongerThanPrimer {
            off_seed: c.off_seed,
            len_min: c.len_min,
        });
    }
    if region.start == 0 || region.start > n || region.end == 0 || region.end > n {
        return Err(DesignError::OutsideTemplate {
            start: region.start,
            end: region.end,
            bp: n,
        });
    }
    if region.wraps() && !circular {
        return Err(DesignError::BackwardsOnALine {
            start: region.start,
            end: region.end,
        });
    }

    // Ambiguity in the *target* is fatal; ambiguity in the flank only removes
    // candidates, and those are counted in the tally rather than refused.
    // pl-thermo refuses an ambiguity code rather than skipping it, and the
    // reason transfers: a Tm over the unambiguous remainder is a different
    // oligo's.
    let target: Vec<u8> = (0..region.len(n))
        .map(|k| template[((region.start - 1 + k) % n) as usize])
        .collect();
    if let Some((k, &b)) = target
        .iter()
        .enumerate()
        .find(|(_, b)| !matches!(b.to_ascii_uppercase(), b'A' | b'C' | b'G' | b'T'))
    {
        return Err(DesignError::AmbiguousTarget {
            position: (region.start - 1 + k as u64) % n + 1,
            base: b.to_ascii_uppercase(),
        });
    }

    pair::run(template, circular, region, c)
}

/// Is this template free of ambiguity codes?
///
/// The precondition for [`specificity::SeedIndex`]: `find_bindings` matches
/// through `pl_core::iupac::matches`, which is IUPAC-aware, and a 2-bit code
/// lookup is not. An `N` or an `R` in the template would make the index miss
/// real bindings, and a primer reported as unique because the fast path could
/// not see its second site is precisely the failure the specificity check
/// exists to prevent.
pub(crate) fn unambiguous(template: &[u8]) -> bool {
    Composition::of(template).other == 0
}
