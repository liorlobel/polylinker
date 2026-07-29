//! Every number that decides an answer, and where each one came from.
//!
//! # The rule these defaults were chosen under
//!
//! **No threshold ships until the achievable range of the quantity it gates has
//! been computed with this project's own tables, and the threshold shown to sit
//! inside it.** That is not a style preference. Rychlik's 3'-end stability
//! limit is published as −9 kcal/mol; on the SantaLucia scale `pl-thermo`
//! stores, the most stable pentamer that exists reaches −8.79, so the rule
//! fires on 0 of 1,024 pentamers. A tool importing it ships a filter that never
//! fires while printing "3'-end stability: PASS" — a true statement about a
//! check that did nothing. This repo's own rule, that a check which cannot fail
//! proves nothing, applies to production thresholds and not only to tests.
//!
//! The measurements behind each default below are quoted beside it, and
//! `pl_thermo`'s `no_pentamer_on_this_scale_reaches_minus_nine_kcal_per_mole`
//! pins the one that matters most so that "correcting" it back to the
//! literature value breaks the build.
//!
//! # Hard and soft are different kinds of thing
//!
//! A **hard** criterion has a physical failure mode: the primer does not prime,
//! or primes somewhere else. It is a gate, with no weight — a primer that binds
//! in three places is not "worse", it is wrong, and no weight makes that
//! commensurable with being one degree off optimum.
//!
//! A **soft** criterion is a stated preference between options that are all
//! already acceptable. It is normalised by the width of its own accepted range,
//! so every term lands in `[0, 1]`, and the weights are then honestly
//! dimensionless.

use pl_thermo::{Method, NnTable, SANTALUCIA_2004};

/// What the amplicon has to do with the region the user selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The product contains the whole region. **The default.**
    ///
    /// The forward primer's footprint *begins* at or before the region's first
    /// base, and the reverse primer's footprint *ends* at or after its last, so
    /// the region is inside the product by construction. `flank` is how far
    /// outside the region those outer ends may sit, which makes `flank = 0` the
    /// seamless-cloning case: the two primers are pinned exactly to the ends of
    /// the selection, and only their lengths vary.
    ///
    /// **`flank = 0` is also the setting most likely to return nothing, and
    /// that is arithmetic rather than bad luck.** Pinning both outer ends
    /// leaves `len_max − len_min + 1` candidates per side — ten at the
    /// defaults — so the Tm window has to be hit by one of ten lengths at one
    /// fixed 5' end. Measured over 35 real plasmids, region 600..1400,
    /// `--flank 0`: 12 designed and 23 refused, against 34 of 35 at the default
    /// flank. When it does succeed it is exactly right — 22 of 22 products
    /// equalled the selection to the base and `pl_clone::pcr` agreed
    /// byte-for-byte — so the answer to an empty `--flank 0` search is to raise
    /// the flank a few bases, not to relax Tm. [`crate::report::Tally::advice`]
    /// says so first when this is the case, ahead of the generic remedy.
    ///
    /// The default because when someone selects a CDS and asks for primers,
    /// they are cloning or verifying that CDS. A mode where the forward primer
    /// may sit *inside* the selection produces a product missing the 5' end of
    /// the gene — the start codon and the RBS go with it — and nothing about
    /// the output looks wrong: the pair passes every thermodynamic check, the
    /// band is the predicted size, and the failure surfaces at expression,
    /// weeks later.
    Contain,
    /// Both primers lie inside the region; the product is a sub-interval.
    ///
    /// qPCR and RT-PCR, screening, sequencing-primer placement. What
    /// [`Constraints::rt_pcr`] switches to.
    Within,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Contain => "contain",
            Mode::Within => "within",
        }
    }
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "contain" => Some(Mode::Contain),
            "within" => Some(Mode::Within),
            _ => None,
        }
    }
}

/// Dimensionless preferences between already-acceptable options.
///
/// Only one of these is defended by a mechanism. The rest are conventional and
/// the docs say the word: 2.0/1.0/0.5 encode "thermodynamics before
/// composition, composition before cosmetics". They are not measured, they are
/// not derived, and they are **not Primer3's** — see the crate doc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    /// **3.0, and this is the one with an argument behind it.**
    ///
    /// `pl_thermo::anneal` takes the *lower* of the two Tms and says why:
    /// "the weaker primer is the one that fails to anneal". ΔTm is therefore
    /// precisely the number of degrees by which the stronger primer is run
    /// below its own optimum. At ΔTm = 2 that is inconsequential; at ΔTm = 10
    /// the stronger primer is deep into the range where it tolerates
    /// 3'-proximal mismatches and primes at sites the specificity scan had no
    /// reason to consider, *because at its own Tm they would not bind*. A ΔTm
    /// failure does not merely reduce yield — it silently widens the effective
    /// specificity of a primer that was checked for specificity at a different
    /// temperature. Nothing else on this list has that property.
    pub delta_tm: f64,
    /// Distance of each Tm from the optimum.
    pub tm: f64,
    /// Hairpin and dimer stability.
    pub structure: f64,
    /// 3'-terminal pentamer stability.
    pub three_prime: f64,
    /// Distance from a requested product size, on a log scale.
    pub product: f64,
    /// The 3' G/C band.
    pub gc_clamp: f64,
    /// Distance from the optimum length.
    pub length: f64,
    /// Distance outside the %GC band.
    pub gc: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Weights {
            delta_tm: 3.0,
            tm: 2.0,
            structure: 2.0,
            three_prime: 1.0,
            product: 1.0,
            gc_clamp: 0.5,
            length: 0.5,
            gc: 0.5,
        }
    }
}

impl Weights {
    pub fn describe(&self) -> String {
        format!(
            "dTm {:.1}, Tm {:.1}, structure {:.1}, 3'-end {:.1}, product {:.1}, \
             GC clamp {:.1}, length {:.1}, GC {:.1} (ours, conventional except dTm)",
            self.delta_tm,
            self.tm,
            self.structure,
            self.three_prime,
            self.product,
            self.gc_clamp,
            self.length,
            self.gc
        )
    }
}

/// Everything the search is allowed to accept.
#[derive(Debug, Clone, PartialEq)]
pub struct Constraints {
    pub mode: Mode,
    /// How far outside the region a primer's outer end may sit, in
    /// [`Mode::Contain`]. Ignored in [`Mode::Within`].
    pub flank: u64,
    pub len_min: usize,
    pub len_max: usize,
    pub len_opt: usize,
    pub tm_min: f64,
    pub tm_max: f64,
    pub tm_opt: f64,
    pub tm_diff_max: f64,
    pub gc_min: f64,
    pub gc_max: f64,
    /// Is the %GC band a gate, or only a preference? **Preference by default.**
    pub gc_hard: bool,
    pub gc_clamp_min: usize,
    pub gc_clamp_max: usize,
    pub max_poly: usize,
    pub max_poly_g: usize,
    pub max_dinuc_repeat: usize,
    pub dg_three_prime: f64,
    pub dg_hairpin: f64,
    pub dg_dimer_three_prime: f64,
    /// Soft only: a primer sequestered in an internal duplex is unavailable but
    /// does not generate a product. Used to normalise the structure term.
    pub dg_dimer_any: f64,
    pub product_min: u64,
    pub product_max: u64,
    pub product_target: Option<u64>,
    /// How many pairs to report.
    pub max_pairs: usize,
    /// Diversity bound on the 3' ends of the **pair**, not of either side.
    ///
    /// A pair is dropped only when its forward 3' end AND its reverse 3' end are
    /// both within `min_separation` of an already-accepted pair's, so two
    /// reported pairs may share one side outright — a genuinely different
    /// reverse primer on the same forward is still offered, which is the point.
    ///
    /// Adjacent candidates differing by one base score almost identically, so a
    /// naive top-5 is five views of the same primer — the illusion of choice.
    /// This field bounds that for the pair; it does **not** guarantee that two
    /// reported forward primers differ, and it used to say it did. On an
    /// ordinary default run (`--region 250..350` on 600 bp) pairs 1 and 2 came
    /// back with a byte-identical forward and pairs 1 and 3 with a
    /// byte-identical reverse, so both halves of that claim failed in one
    /// invocation.
    pub min_separation: u64,
    /// How many survivors per side are carried into pairing.
    ///
    /// **A search bound, not a criterion**, and the report says so whenever it
    /// bound. The pair stage is quadratic in survivors, and in [`Mode::Within`]
    /// over a 1 kb gene the two sides yield thousands each: measured, one such
    /// run took 104 seconds before this existed, which is a progress bar rather
    /// than a feature. Survivors are ranked by the per-oligo half of the same
    /// penalty the pairs are ranked by, ties broken by coordinate, so the cut
    /// is deterministic and is not a fresh set of criteria.
    pub max_per_side: usize,
    /// The 3'-anchored seed for the off-target scan.
    pub off_seed: usize,
    /// Run the off-target scan at all.
    ///
    /// Turning it off is allowed and is *said out loud* in the report, because
    /// it is the thing this tool does that a designer deferring specificity to
    /// BLAST cannot.
    pub specificity: bool,
    /// Thermodynamics for Tm. ΔG always uses [`SANTALUCIA_2004`] regardless.
    pub tm_method: Method,
    pub weights: Weights,
    /// A 5' tail on the forward primer.
    pub tail_five: Option<Tailspec>,
    /// A 5' tail on the reverse primer.
    pub tail_three: Option<Tailspec>,
    /// Was the RT-PCR preset applied? Decides whether the bacteria caveat is
    /// carried into the report.
    pub rt_pcr: bool,
}

/// A restriction site to add, and whatever the user wants 5' of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tailspec {
    pub enzyme: &'static pl_enzymes::Enzyme,
    /// Bases 5' of the site. **Empty by default** — see
    /// [`crate::tail::NO_SPACER_WARNING`].
    pub spacer: Vec<u8>,
}

/// The one relation between this crate's advice floor and another crate's
/// constant, checked where it is stated rather than in a test.
///
/// [`Constraints::LEN_ADVICE_MIN`]'s doc justifies 15 by `pl_clone::MIN_ANNEAL`
/// being 12. If that ever rises past 15, the widening advice starts
/// recommending oligos `pl_clone::pcr` will not anneal — the exact thing the
/// constant exists to prevent — and the doc above becomes a false claim. A
/// runtime `assert!` in a test could only report that after the fact, and
/// clippy is right that it is not an assertion at all; this one fails the
/// build.
const _: () = assert!(
    Constraints::LEN_ADVICE_MIN > pl_clone::MIN_ANNEAL,
    "the shortest primer this crate recommends has to clear the simulator's \
     annealing floor, or `pl design` advises its way to oligos `pl_clone::pcr` refuses"
);

impl Constraints {
    /// 18 nt. Derivable rather than conventional, which is why it is the one
    /// length bound with an argument: a primer is uninformative unless its
    /// expected chance occurrence in the template is small, and
    /// `log4(2n)` is 6.7 nt for a 5,386 bp plasmid and 11.5 nt for a 4.6 Mb
    /// genome. 18 nt carries about six orders of magnitude of margin, and it is
    /// also why [`Constraints::OFF_SEED`] at 12 is enough for a bacterial
    /// chromosome. Innis & Gelfand (1990), *Optimization of PCRs*, in *PCR
    /// Protocols*, Academic Press.
    pub const LEN_MIN: usize = 18;
    /// 27 nt. **Conventional, and a synthesis-cost bound rather than a
    /// thermodynamic one**: phosphoramidite coupling at ~99%/step leaves 74.8%
    /// of a 30-mer full-length and 55% of a 60-mer. It is therefore the first
    /// thing to widen when a search comes back empty — see
    /// [`crate::report::Tally::advice`]. Relaxing the physical constraint (Tm)
    /// to rescue a search is how a designer produces primers that pass its own
    /// checks and fail at the bench.
    pub const LEN_MAX: usize = 27;
    /// The longest primer any interface here will accept, 60 nt.
    ///
    /// Not a design default — [`Constraints::LEN_MAX`] is that — but the bound
    /// `pl design --len` validates against, restated here so that
    /// [`crate::report::Tally::advice`] cannot suggest widening past it. It
    /// could, and did: following its own advice three times produced `--len
    /// 15..66`, which the CLI then refused. Advice that the tool rejects is
    /// worse than no advice, because the user has no way to tell which of the
    /// two is wrong. 60 because coupling at ~99%/step leaves 55% of a 60-mer
    /// full-length, and below half is not an oligo anyone should be sent.
    pub const LEN_HARD_MAX: usize = 60;
    /// The shortest primer any interface here will accept, 8 nt.
    ///
    /// The mirror of [`Constraints::LEN_HARD_MAX`], and named for the same
    /// reason: [`crate::report::Tally::advice`] used to say `--len` was "already
    /// at 15..60, the widest this tool accepts", which is false — both
    /// interfaces validate `--len` against 8..60, so 8..60 is wider and is
    /// accepted, and following this function's own advice from the defaults
    /// parks a user at exactly `len_min = 15`, the state where the claim fires.
    pub const LEN_HARD_MIN: usize = 8;
    /// 15 nt: the shortest primer this crate will **recommend**, as distinct
    /// from the shortest it will accept.
    ///
    /// The widening advice floors its suggestion here rather than at
    /// [`Constraints::LEN_HARD_MIN`], and that is deliberate rather than
    /// arbitrary: `pl_clone::MIN_ANNEAL` is 12, so a primer shorter than that
    /// does not anneal in this project's own simulator, and a designer that
    /// advised its way down to 8 would be recommending oligos the rest of the
    /// toolchain then refuses to run. 15 leaves three bases of margin over that
    /// floor. Advice that the tool rejects is worse than no advice, and advice
    /// that the tool *accepts* and then cannot simulate is worse still.
    pub const LEN_ADVICE_MIN: usize = 15;
    /// 20 nt. **Conventional, and labelled so rather than defended.** It is the
    /// midpoint of 18-27 to within a base and the number every protocol book
    /// orders by; nothing here derives it, and it only ever moves a soft length
    /// term normalised by the width of the range, so its influence is bounded
    /// by `Weights::length` at 0.5. Said plainly because the alternative — a
    /// paragraph of physical-sounding reasoning around a number that is
    /// actually a habit — is the failure mode this file is organised against.
    pub const LEN_OPT: usize = 20;

    /// 55 °C, **on this model's own 50 mM Na⁺ scale, and that qualifier is the
    /// whole point.**
    ///
    /// The familiar target is 60 °C, and copying it here would be wrong by
    /// about five degrees. `pl-thermo` has monovalent corrections only — there
    /// is no `SaltCorrection::Owczarzy2008` yet — so its default is 50 mM Na⁺,
    /// while an ordinary PCR buffer (50 mM K⁺, 1.5 mM Mg²⁺, 0.8 mM dNTP) is
    /// ~150 mM monovalent-equivalent by von Ahsen, Wittwer & Schütz (2001) Clin
    /// Chem 47:1956, `[Na⁺]eq = [K⁺] + 120·sqrt([Mg²⁺] − [dNTP])`.
    ///
    /// Measured on this crate's own tables: ACGTGCATGCATGCATCGTA reports
    /// 55.37 °C at 50 mM and 60.66 °C at 150 mM — a 5.29 °C offset, reproduced
    /// within 0.05 °C on three ordinary primers. So selecting for 55 °C *here*
    /// selects the primers a bench protocol calls 60 °C. Selecting for 60 °C
    /// here would select primers that are ~65 °C in the tube: longer and more
    /// GC-rich than needed, with `anneal()`'s advice five degrees out as well.
    ///
    /// `pinned by tm_window_is_stated_on_the_model_s_own_salt_scale`. Someone
    /// wanting the bench scale should pass `--na 150`, and the report's method
    /// line will say so.
    pub const TM_OPT: f64 = 55.0;

    /// ±3 °C, so the accepted window is 52-58 °C. **Measured, because this
    /// number and not `TM_OPT` is what decides whether a search returns
    /// anything** — Tm is the binding constraint in most runs, and the
    /// half-width alone sets how much of the space survives it. It is also the
    /// `half` denominator of the Tm term in both scoring functions.
    ///
    /// Fraction of random 18-27mers landing inside `TM_OPT ± h`, 500,000 per
    /// level on this crate's default method (`SANTALUCIA_1998`, 50 mM Na⁺,
    /// i.i.d. bases):
    ///
    /// | h | 27% GC | 50% GC | 65% GC |
    /// |---|---|---|---|
    /// | ±1 | 2.5% | 12.8% | 6.8% |
    /// | ±2 | 5.3% | 25.2% | 13.6% |
    /// | **±3** | **8.5%** | **37.2%** | **20.6%** |
    /// | ±5 | 17.1% | 58.3% | 34.8% |
    ///
    /// ±2 would cost a third of the surviving space at 50% GC and 38% of it at
    /// 27%, which is where an AT-rich organism runs out of candidates
    /// altogether. ±5 opens a 10 °C spread, and at that width the window has
    /// stopped doing the work: two primers at opposite edges differ by 10 °C
    /// and only [`Constraints::TM_DIFF_MAX`] would be left holding the pair
    /// together. ±3 is already one degree wider than `TM_DIFF_MAX`, so at the
    /// extremes it is the ΔTm gate that binds and not this one — deliberate,
    /// since ΔTm carries the top weight for the reason [`Weights::delta_tm`]
    /// gives, and a **pair**-level constraint is the right thing to be binding.
    /// Widening further makes that true of the whole window rather than only
    /// its edges.
    ///
    /// The 65% row being *below* the 50% row is not an error and is worth
    /// knowing before widening anything: at high GC the whole 18-27 nt length
    /// range has already melted past 58 °C, so the loss is at the top of the
    /// window, not the bottom, and widening downward does not help.
    pub const TM_HALFWIDTH: f64 = 3.0;
    /// 5 °C. Conventional; the *ordering* argument that gives ΔTm the top
    /// weight is in [`Weights::delta_tm`] and is not.
    pub const TM_DIFF_MAX: f64 = 5.0;

    /// 40-60%. **Conventional, and soft.** No primary derivation exists; the
    /// band appears in every protocol book as a proxy for Tm and propagates
    /// from Innis & Gelfand (1990) onward.
    ///
    /// It is not a gate by default, and the reason is measured — **end to end,
    /// because the marginal rate is the wrong number and quoting it here was
    /// wrong before.** For the record, the marginal rates: of 200,000 i.i.d.
    /// random 20-mers drawn at 27% GC — *Fusobacterium* territory — 14.4% land
    /// inside 40-60%, and 39.2% at 35% GC; enumerating every 18-27mer of a real
    /// 401 bp window the way this crate does, 10.4% pass at 27.4% GC and 30.1%
    /// at 34.9%. Those are costs, not exclusions, and an earlier draft of this
    /// comment claimed "about 1 in 1,000" and "about 1 in 9", which is one to
    /// two orders of magnitude out and reproduces under no reading.
    ///
    /// What actually justifies the default is what the gate does to a *search*,
    /// after the Tm window has already taken its cut. Measured on this crate,
    /// `Mode::Contain`, a 700 bp region, default constraints: on a 27.6% GC 3 kb
    /// template `gc_hard` rejects 311 oligos that had passed the Tm window and
    /// cuts the survivors from 156 forward + 432 reverse to 59 + 241; on a
    /// 21.9% GC template it cuts them from 17 + 199 to **1** + 71 — one forward
    /// candidate away from an empty result, on a criterion that encodes no
    /// physical failure the Tm window does not already gate. A bacteria-only
    /// tool that hard-gates GC has quietly excluded a large part of the
    /// bacterial kingdom. Report it, weight it gently, do not reject on it —
    /// unless the user asks, which is what `gc_hard` is for.
    pub const GC_MIN: f64 = 40.0;
    pub const GC_MAX: f64 = 60.0;

    /// 1 to 3 G/C among the last five bases: **a band, not a floor.**
    ///
    /// The literature contradicts itself here and most tools ship both sides of
    /// the contradiction. The "GC clamp" convention (Innis & Gelfand 1990;
    /// Sharrocks 1994) says a G/C-rich 3' end anchors the primer; Rychlik's
    /// 3'-end stability rule says a G/C-rich 3' end causes mispriming, because
    /// an end stable enough to hold on through mismatches will prime anywhere
    /// it partially matches. Both cannot be maximised, and resolving it as a
    /// band is the honest reading.
    ///
    /// Soft, because the strong form is expensive and arguably
    /// counterproductive: requiring a terminal G/C rejects 49.9% of random
    /// 50%-GC 20-mers (measured), and the ceiling is enforced by
    /// [`Constraints::DG_THREE_PRIME`] anyway, which is a thermodynamic
    /// statement rather than a count.
    pub const GC_CLAMP_MIN: usize = 1;
    pub const GC_CLAMP_MAX: usize = 3;

    /// Reject a run of 5 or more identical bases. Hard.
    ///
    /// Slippage in mononucleotide tracts is standard polymerase biochemistry
    /// and the limit is conventional. Measured bite on random 50%-GC 20-mers:
    /// run ≥ 4 occurs in 19.4%, run ≥ 5 in 4.7%. Rejecting at ≥ 5 costs 4.7% of
    /// the space, which is mild enough to be a gate; rejecting at ≥ 4 would
    /// cost 19.4%, too much to spend on a failure mode that is real but not
    /// certain.
    pub const MAX_POLY: usize = 4;
    /// G is stricter — reject 4 or more — and not for symmetry.
    ///
    /// A G-tract is the only homopolymer with a distinct structural failure
    /// mode: G-quadruplexes require G-tracts, canonically G≥3 per tract (Burge,
    /// Parkinson, Hazel, Todd & Neidle (2006) Nucleic Acids Res 34:5402). The
    /// helix model in [`crate::fold`] cannot see a quadruplex at all — it
    /// models Watson-Crick pairs only — so this filter does work nothing else
    /// in the pipeline does. Measured cost: 4.9% of random 50%-GC 20-mers.
    pub const MAX_POLY_G: usize = 3;
    /// Reject `(XY)n` for n ≥ 5, excluding `X == Y`, which is the rule above.
    ///
    /// Conventional. Measured, and **the measurement is a warning about how to
    /// test it**: on random 50%-GC 20-mers a ≥5-unit dinucleotide repeat occurs
    /// in 0.010% of sequences. Against random sequence this is very nearly a
    /// check that cannot fail. Its whole value is on real templates —
    /// microsatellites, poly-(CA) tracts, low-complexity intergenic regions in
    /// AT-rich bacteria — so its regression test uses a template carrying an
    /// actual `(AT)n` tract. A property test over random sequence would pass
    /// whether the code worked or not.
    pub const MAX_DINUC_REPEAT: usize = 4;

    /// −7.5 kcal/mol over the terminal pentamer's **stacks**, SantaLucia 2004,
    /// 1 M Na⁺. Hard. **Not −9.**
    ///
    /// **Rychlik, W. (1993), "Selection of primers for polymerase chain
    /// reaction", Methods Mol Biol 15:31-40** is the citation, and getting it
    /// right matters: that is the paper carrying the 3'-terminal stability
    /// criterion and the −9 kcal/mol pentamer limit. Rychlik, Spencer & Rhoads
    /// (1990) Nucleic Acids Res 18:6409 — which this comment cited until a
    /// reviewer checked it — is the *annealing-temperature* paper
    /// (`Ta = 0.3·Tm_primer + 0.7·Tm_product − 25`) and says nothing about
    /// 3'-terminal stability. `docs/research/dossier.md` had it right; the
    /// error was introduced on the way into this crate, which is exactly the
    /// class of thing a provenance table is supposed to make impossible.
    ///
    /// The −9 kcal/mol figure is defined on **Breslauer, Frank, Blöcker & Marky
    /// (1986) PNAS 83:3746** parameters, which are systematically ~1.5-2×
    /// more negative than SantaLucia's, and it does not transfer: measured over
    /// all 1,024 pentamers with `pl_thermo::dg37_stacks`, the most stable is
    /// CGCGC at −8.79, so −9 rejects 0 of 1,024.
    ///
    /// −7.5 is **a chosen fraction, not a physical threshold**: it rejects
    /// 6.1% (62/1024). −8.0 rejects 2.5%, −7.0 rejects 10.9%. The distribution
    /// is: most stable −8.79, 5th percentile −7.54, 10th −7.14, median −5.57,
    /// least stable TATAT −2.93.
    pub const DG_THREE_PRIME: f64 = -7.5;
    /// −5.0 kcal/mol over the hairpin **stem's stacks**. Hard.
    ///
    /// The loop's initiation term is **not modelled** (see [`crate::fold`]), so
    /// this number is more negative than the folded structure's true ΔG. That
    /// error is in the safe direction — it over-reports hairpins rather than
    /// missing them — which is why a threshold can be set on it at all. It is
    /// set from the measured distribution on this scale, not imported from a
    /// whole-structure vendor convention, because the two are not the same
    /// quantity. Measured rejection over 200,000 **i.i.d.** random 20-mers per
    /// level: 1.3% at 27% GC, 5.0% at 50%, 14.2% at 65%. (Exactly-fixed
    /// composition instead of i.i.d. bases: 1.0% / 3.4% / 10.4% — the same
    /// scheme dependence recorded on [`Constraints::DG_DIMER_THREE_PRIME`],
    /// which is why the scheme is now named beside every one of these rates.)
    pub const DG_HAIRPIN: f64 = -5.0;
    /// −6.0 kcal/mol for a dimer helix that includes either oligo's 3'-terminal
    /// base. Hard.
    ///
    /// The 3'-end split is mechanistic rather than cosmetic: only a 3' end can
    /// be extended, so only a 3'-end dimer is amplified into a primer-dimer
    /// band that competes for polymerase and dNTPs through every subsequent
    /// cycle. Measured rejection, over exactly the quantity the gate reads —
    /// `fold::dimer(seq, seq).1.dg <= -6.0` — on 200,000 **i.i.d.** random
    /// 20-mers per level: **1.7% at 27% GC, 2.9% at 50%, 6.3% at 65%.**
    ///
    /// The sampling scheme is part of the measurement and not a footnote: draw
    /// the same 20-mers with an *exactly* fixed G/C count instead of i.i.d.
    /// bases and the same criterion rejects 1.5% / 2.3% / 4.9%, a fifth to a
    /// quarter lower, because fixing the composition removes the GC-rich tail
    /// of the binomial that supplies most of the stable helices. An earlier
    /// draft of this comment quoted 4.5% / 7.1% / 14.0% here, which reproduces
    /// under no reading of the shipped code — not at other lengths, not for the
    /// cross-dimer between two oligos, and not for `dimer_any` at either
    /// threshold. Numbers in this file are the evidence a reader judges a gate
    /// by, so an unreproducible one is the failure the module rule exists to
    /// prevent, not a typo.
    pub const DG_DIMER_THREE_PRIME: f64 = -6.0;
    /// −10.0 kcal/mol for the most stable helix anywhere. **Soft** — a primer
    /// sequestered in an internal duplex is unavailable but does not generate a
    /// product. Used only to normalise the cross-dimer term of the pair score,
    /// in [`crate::pair`]'s `score`; nothing gates on it.
    pub const DG_DIMER_ANY: f64 = -10.0;

    /// 100 bp. Below this the product runs with the primer-dimer front on an
    /// ordinary agarose gel and cannot be told apart from it; `pl-gel` can be
    /// used to check a specific case rather than guessing.
    pub const PRODUCT_MIN: u64 = 100;
    /// 3,000 bp. Conventional: the practical ceiling for standard PCR with a
    /// non-processive polymerase. Long-range needs a different enzyme and a
    /// different protocol.
    pub const PRODUCT_MAX: u64 = 3_000;

    /// 200 bp of flank each side, in [`Mode::Contain`]. **Conventional, and it
    /// buys search space rather than biology.**
    ///
    /// `flank` bounds the primer's *outer* end, so it is exactly the number of
    /// distinct 5' starts each side may try: `flank + 1` starts × `len_max −
    /// len_min + 1` lengths, which at the defaults is 201 × 10 = 2,010
    /// candidates per side. At `flank = 0` that collapses to ten — see
    /// [`Mode::Contain`], where the measured consequence is recorded. 200 is
    /// enough that the Tm window is essentially never the reason a
    /// `Mode::Contain` search comes back empty, and small enough that the
    /// product carries at most 400 bp of sequence the user did not ask for,
    /// which still sizes on an ordinary gel against a 100 bp ladder.
    ///
    /// It is a *bound on the search*, not a preference: no term in the score
    /// rewards a primer for sitting closer to the selection.
    pub const FLANK: u64 = 200;

    /// The %GC term's normalising width, in percentage points.
    ///
    /// A primer 20 points outside the band scores the full 1.0; anything
    /// further clamps. 20 rather than a derived number because the band itself
    /// is conventional (see [`Constraints::GC_MIN`]) and a normaliser cannot be
    /// better founded than the quantity it normalises — but it is **one named
    /// constant** because `pair::cap` and `pair::score` must use the same one.
    /// They are required to agree (`cap` is a projection of `score`) and two
    /// copies of a bare `20.0` in two functions is how they stop agreeing.
    pub const GC_NORM: f64 = 20.0;

    /// 200 survivors per side carried into pairing, giving at most 40,000
    /// pairs.
    ///
    /// **It bites on ordinary input, and the report says so whenever it does.**
    /// In `Mode::Contain` at the default flank the two sides enumerate 2,010
    /// candidates each and typically leave 300-700 standing per side, so the cut
    /// fires on any template of ordinary composition: measured across template
    /// GC from 25% to 75%, it bit at every step from 30% to 70% and failed to
    /// bite only at the two extremes, where the Tm window has already emptied
    /// the search. This doc used to say it "almost never bites there", while
    /// `tests/scoring.rs`'s own module doc recorded the opposite from a measured
    /// 3 kb / 400 bp run — two files in one crate disagreeing about one
    /// constant.
    ///
    /// The cut is harmless in `Mode::Contain` because every candidate lies
    /// inside a `2 * flank + region` band, so the survivors are mutually within
    /// the product window whichever 200 are kept. In `Mode::Within` they are
    /// not: `pair::cap` orders by the per-oligo penalty with the coordinate only
    /// as a tie-break, so over a long region the retained candidates spread out
    /// and the expected pair count is `max_per_side^2 * window / region_bp` —
    /// 1.6 over 2 Mb under `--rt`. That is why `pair::run` conditions the
    /// reverse cut on the forwards that survived theirs; see `retain_pairable`.
    pub const MAX_PER_SIDE: usize = 200;

    /// 12, deliberately shorter than `pl_primer::Params::default().seed_len`
    /// of 14.
    ///
    /// Those two seeds answer different questions. 14 is "where does this
    /// primer anneal", where a long exact seed is the point. 12 is "where might
    /// this primer *mis*prime", and mispriming is a partial 3' match, so the
    /// scan has to be more permissive than the analyser — which is the
    /// conservative direction for a rejection test. It is also exactly
    /// `pl_clone::pcr`'s own `MIN_ANNEAL`, and that is the constraint that
    /// pins it: the simulator refuses a pair as `NotSpecific` on a second site
    /// with a 12 nt exact 3' match, so a designer scanning with a *longer* seed
    /// would hand back pairs its own simulator will not run.
    ///
    /// # What 12 costs on a chromosome, measured, and why it is not raised
    ///
    /// An earlier draft of this comment said `log4(2 · 4.6e6) = 11.5, so 12
    /// still carries margin`. That is wrong about what `log4(2n)` is: it is the
    /// length at which the *expected* number of occurrences reaches one — the
    /// break-even point — and there is no margin above it. Measured on
    /// *Agrobacterium tumefaciens* C58 (2,075,577 bp), 1,005 of 2,000 random
    /// 20-mers have a 12 nt 3' seed that occurs more than once, so half of all
    /// candidates fall through the prefilter to a full `find_bindings` scan and
    /// a default design takes about 100 seconds; on a 9.1 Mb *Myxococcus
    /// xanthus* chromosome it is about five and a half minutes.
    ///
    /// **Raising the seed with template length is the obvious fix and it is
    /// the wrong one.** 16 or 17 would restore the prefilter's exclusion rate,
    /// and it would do so by making the scan blind to exactly the second sites
    /// `pl_clone::pcr` objects to — a 12 nt exact 3' match with a mismatch
    /// beyond it is a real mispriming site, and the point of this crate is that
    /// it is caught during enumeration rather than on a gel. Trading the answer
    /// for the runtime is not available here. What is done instead is to say
    /// the cost out loud *before* it is paid: `pl design` prints the estimate
    /// and names `--no-specificity`, `--flank` and a narrower `--region` when
    /// the template is big enough for the scan to dominate. A user who chooses
    /// to wait gets the right answer; a user who cannot is told which knob
    /// buys what.
    pub const OFF_SEED: usize = 12;

    /// Templates above this size make the off-target scan the dominant cost.
    ///
    /// Not a limit and nothing is refused — it is the threshold at which
    /// `pl design` volunteers the cost described on [`Constraints::OFF_SEED`]
    /// **before** paying it. A warning printed after a five-minute wait is not
    /// a warning.
    ///
    /// Measured, one default `Mode::Contain` design over a 600 bp region
    /// (4,020 candidates every time), release build, random unrepetitive
    /// template — so these are the *optimistic* figures; a real chromosome is
    /// repetitive and the verifier measured 100 s on 2.08 Mb of
    /// *Agrobacterium* where random sequence gives 65:
    ///
    /// | template | time |
    /// |---|---|
    /// | 100 kb | 0.4 s |
    /// | 250 kb | 1.9 s |
    /// | **500 kb** | **3.1 s** |
    /// | 1 Mb | 17 s |
    /// | 2 Mb | 65 s |
    ///
    /// It grows faster than the template because the proportion of candidates
    /// whose 12 nt seed is not unique grows with it, so each one costs a full
    /// scan. 500 kb is where the curve turns and where the wait first becomes
    /// something a user would want to have been asked about.
    pub const SCAN_NOTICE_BP: u64 = 500_000;

    /// The ΔG table, fixed regardless of the Tm table. See the crate doc.
    pub const DG_TABLE: NnTable = SANTALUCIA_2004;

    /// The RT-PCR / qPCR preset.
    ///
    /// Returns the constraints **and** switches [`Constraints::rt_pcr`] on, so
    /// the bacteria caveat cannot be lost between here and the report.
    pub fn rt_pcr(self) -> Self {
        Constraints {
            // The single largest behavioural change: you are quantifying a
            // transcript, not cloning a CDS, so both primers sit inside the
            // selected gene.
            mode: Mode::Within,
            // 70-150 bp, optimum 100. Conventional qPCR chemistry: short
            // amplicons amplify to completion within a two-step cycle. Bustin
            // et al. (2009) Clin Chem 55:611 (MIQE) requires amplicon length to
            // be *reported*; it does not prescribe a range, so the citation is
            // for the reporting obligation and the range is labelled
            // conventional.
            product_min: 70,
            product_max: 150,
            product_target: Some(100),
            // Two-step cycling anneals and extends at one temperature, so both
            // primers must work there and there is no lower-Tm primer to set Ta
            // around.
            tm_diff_max: 2.0,
            // 25 rather than 27. Conventional, and labelled so: a 70-150 bp
            // amplicon leaves little room between two footprints, and every
            // base of primer is a base of product that is primer rather than
            // template. Nothing here derives 25 over 26; it only narrows a
            // range whose other end is unchanged.
            len_max: 25,
            // Mispriming costs more in qPCR than in endpoint PCR, because a
            // spurious product contributes to SYBR signal indistinguishably
            // from the real one. A stated preference, not a measurement.
            dg_three_prime: -7.0,
            dg_hairpin: -4.5,
            rt_pcr: true,
            ..self
        }
    }

    /// The one line that names everything behind the answer.
    ///
    /// The [`pl_thermo::Method::describe`] pattern, for the same reason: without
    /// it, a result differing from another tool's is indistinguishable from a
    /// bug, and this crate promises parity with nobody.
    pub fn describe(&self) -> String {
        format!(
            "len {}-{} opt {}, Tm {:.1}-{:.1}C opt {:.1}, dTm <= {:.1}C, GC {:.0}-{:.0}% ({}), \
             poly-N <= {} (G <= {}), dinucleotide repeats <= {} units, \
             G/C in last 5: {}-{}, product {}-{} bp{}{}",
            self.len_min,
            self.len_max,
            self.len_opt,
            self.tm_min,
            self.tm_max,
            self.tm_opt,
            self.tm_diff_max,
            self.gc_min,
            self.gc_max,
            if self.gc_hard {
                "a gate"
            } else {
                "reported, not a gate"
            },
            self.max_poly,
            self.max_poly_g,
            self.max_dinuc_repeat,
            self.gc_clamp_min,
            self.gc_clamp_max,
            self.product_min,
            self.product_max,
            // The window bounds the amplicon and not the template span, so
            // with tails it bounds fewer template bases than the number
            // printed. Say which, next to the number, because the constraint
            // line and the amplicon line used to disagree in one document.
            match self.tail_bp() {
                0 => String::new(),
                t => format!(
                    " INCLUDING the {t} nt of tail, so {}-{} bp of template",
                    self.product_min.saturating_sub(t).max(1),
                    self.product_max.saturating_sub(t)
                ),
            },
            // The requested size, printed next to the window it is measured
            // against. It reached no surface at all before -- not the text
            // report, not the JSON, not an error -- so when the size term was
            // dropped for sitting at or above `product_max` the only trace was
            // `product 1.0` in the weights line, which asserts the opposite.
            match self.product_target {
                None => String::new(),
                Some(t) if (self.product_min..=self.product_max).contains(&t) =>
                    format!(", target {t} bp"),
                Some(t) => format!(", target {t} bp OUTSIDE that window"),
            }
        )
    }

    /// How many bases the two tails add to every product.
    pub fn tail_bp(&self) -> u64 {
        let one = |t: &Option<Tailspec>| {
            t.as_ref()
                .map(|s| (s.spacer.len() + s.enzyme.site.len()) as u64)
                .unwrap_or(0)
        };
        one(&self.tail_five) + one(&self.tail_three)
    }

    /// The ΔG conventions, spelled out where a number can be read next to them.
    pub fn describe_dg(&self) -> String {
        format!(
            "dG37 is a stack sum on SantaLucia & Hicks 2004 at 1 M Na+, no initiation, \
             loop or salt term: 3'-pentamer >= {:.1}, hairpin stem >= {:.1}, \
             3'-end dimer >= {:.1} kcal/mol",
            self.dg_three_prime, self.dg_hairpin, self.dg_dimer_three_prime
        )
    }
}

impl Default for Constraints {
    fn default() -> Self {
        Constraints {
            mode: Mode::Contain,
            flank: Constraints::FLANK,
            len_min: Constraints::LEN_MIN,
            len_max: Constraints::LEN_MAX,
            len_opt: Constraints::LEN_OPT,
            tm_min: Constraints::TM_OPT - Constraints::TM_HALFWIDTH,
            tm_max: Constraints::TM_OPT + Constraints::TM_HALFWIDTH,
            tm_opt: Constraints::TM_OPT,
            tm_diff_max: Constraints::TM_DIFF_MAX,
            gc_min: Constraints::GC_MIN,
            gc_max: Constraints::GC_MAX,
            gc_hard: false,
            gc_clamp_min: Constraints::GC_CLAMP_MIN,
            gc_clamp_max: Constraints::GC_CLAMP_MAX,
            max_poly: Constraints::MAX_POLY,
            max_poly_g: Constraints::MAX_POLY_G,
            max_dinuc_repeat: Constraints::MAX_DINUC_REPEAT,
            dg_three_prime: Constraints::DG_THREE_PRIME,
            dg_hairpin: Constraints::DG_HAIRPIN,
            dg_dimer_three_prime: Constraints::DG_DIMER_THREE_PRIME,
            dg_dimer_any: Constraints::DG_DIMER_ANY,
            product_min: Constraints::PRODUCT_MIN,
            product_max: Constraints::PRODUCT_MAX,
            product_target: None,
            max_pairs: 5,
            min_separation: 3,
            max_per_side: Constraints::MAX_PER_SIDE,
            off_seed: Constraints::OFF_SEED,
            specificity: true,
            tm_method: Method::default(),
            weights: Weights::default(),
            tail_five: None,
            tail_three: None,
            rt_pcr: false,
        }
    }
}
