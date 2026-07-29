//! The answer, and — when there is no answer — why not.
//!
//! # An empty result is a first-class result
//!
//! Measured: at 27% template GC, essentially no candidate of length 18-27
//! reaches a 52 °C Tm on this crate's 50 mM Na⁺ scale. For AT-rich bacteria —
//! and this tool is for bacteria — an empty list is the *expected* output of
//! sensible defaults. A tool that returns nothing without saying which
//! constraint bound leaves the user concluding their locus is undesignable,
//! when the truth is that an 18-27 nt length range is wrong for their organism.
//!
//! So [`Tally`] is always returned, always in one fixed order, and
//! [`Tally::advice`] names the binding constraint and the remedy. It advises
//! **widening length before Tm**: length is a synthesis-cost constraint and Tm
//! is a physical one, and relaxing the physical constraint to rescue a search
//! is how a designer produces primers that pass its own checks and fail at the
//! bench.

use crate::oligo::Side;
use crate::pair::Primer;
use crate::params::{Constraints, Mode};
use crate::Region;

/// Why a candidate or a pair was refused.
///
/// A C-like enum with a fixed declaration order, and the tally is an array
/// indexed by it — so the print order is this order, on every machine, with no
/// hash iteration anywhere near a result that has to be byte-identical between
/// runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The span ran off the end of a linear molecule. Structural, not a
    /// judgement, and counted separately so it cannot be mistaken for one.
    OffTheEnd,
    Ambiguous,
    Tm,
    Gc,
    Run,
    DinucRepeat,
    ThreePrimeStability,
    Hairpin,
    SelfDimer,
    OffTarget,
    // --- pair stage ---
    PairOverlap,
    DeltaTm,
    CrossDimer,
    ProductLength,
    InternalSite,
}

impl Reason {
    /// Declaration order **is evaluation order**, and that is now load-bearing
    /// rather than tidy.
    ///
    /// [`Tally::reached`] works out how many candidates a gate ever saw by
    /// subtracting everything spent before it, which is only correct if the
    /// gates fire in this order — `oligo::evaluate` top to bottom, then the
    /// off-target scan, then `pair::run`'s inner loop top to bottom. The
    /// pair-stage half was declared in a different order from the one it runs
    /// in until that arithmetic needed it.
    pub const ALL: &'static [Reason] = &[
        Reason::OffTheEnd,
        Reason::Ambiguous,
        Reason::Tm,
        Reason::Gc,
        Reason::Run,
        Reason::DinucRepeat,
        Reason::ThreePrimeStability,
        Reason::Hairpin,
        Reason::SelfDimer,
        Reason::OffTarget,
        Reason::PairOverlap,
        Reason::DeltaTm,
        Reason::CrossDimer,
        Reason::ProductLength,
        Reason::InternalSite,
    ];

    /// Is this refusal about a pair rather than about one oligo?
    ///
    /// The two halves count out of different totals — candidates enumerated
    /// against pairs built — so printing them under one heading, which is what
    /// `NoPair` did, put "2935 Tm outside 52.0-58.0C" under a sentence saying
    /// these were pair rejections.
    pub fn pair_stage(self) -> bool {
        matches!(
            self,
            Reason::PairOverlap
                | Reason::DeltaTm
                | Reason::CrossDimer
                | Reason::ProductLength
                | Reason::InternalSite
        )
    }

    pub fn key(self) -> &'static str {
        match self {
            Reason::OffTheEnd => "off_the_end",
            Reason::Ambiguous => "ambiguity_code",
            Reason::Tm => "tm_out_of_range",
            Reason::Gc => "gc_out_of_range",
            Reason::Run => "homopolymer_run",
            Reason::DinucRepeat => "dinucleotide_repeat",
            Reason::ThreePrimeStability => "three_prime_too_stable",
            Reason::Hairpin => "hairpin",
            Reason::SelfDimer => "self_dimer",
            Reason::OffTarget => "off_target",
            Reason::DeltaTm => "delta_tm",
            Reason::ProductLength => "product_length",
            Reason::PairOverlap => "primers_overlap",
            Reason::CrossDimer => "cross_dimer",
            Reason::InternalSite => "added_site_already_in_product",
        }
    }

    /// The sentence a user can act on, with the numbers filled in.
    pub fn label(self, c: &Constraints) -> String {
        match self {
            Reason::OffTheEnd => "no template there (a linear end)".into(),
            Reason::Ambiguous => "an ambiguity code in the footprint".into(),
            Reason::Tm => format!("Tm outside {:.1}-{:.1}C", c.tm_min, c.tm_max),
            Reason::Gc => format!("GC outside {:.0}-{:.0}%", c.gc_min, c.gc_max),
            Reason::Run => format!(
                "a run of more than {} identical bases (more than {} for G)",
                c.max_poly, c.max_poly_g
            ),
            Reason::DinucRepeat => format!(
                "a dinucleotide repeat of more than {} units",
                c.max_dinuc_repeat
            ),
            Reason::ThreePrimeStability => format!(
                "a 3'-terminal pentamer at or below {:.1} kcal/mol",
                c.dg_three_prime
            ),
            Reason::Hairpin => format!("a hairpin stem at or below {:.1} kcal/mol", c.dg_hairpin),
            Reason::SelfDimer => format!(
                "a 3'-end self-dimer at or below {:.1} kcal/mol",
                c.dg_dimer_three_prime
            ),
            Reason::OffTarget => "another binding site on this template".into(),
            Reason::DeltaTm => format!("dTm above {:.1}C", c.tm_diff_max),
            Reason::ProductLength => {
                format!("product outside {}-{} bp", c.product_min, c.product_max)
            }
            Reason::PairOverlap => "the two footprints overlap".into(),
            Reason::CrossDimer => format!(
                "a 3'-end cross-dimer at or below {:.1} kcal/mol",
                c.dg_dimer_three_prime
            ),
            Reason::InternalSite => {
                "the added restriction site already occurs in the product".into()
            }
        }
    }

    fn slot(self) -> usize {
        Reason::ALL.iter().position(|r| *r == self).expect("in ALL")
    }
}

/// How many candidates each gate refused.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tally {
    counts: [u32; 15],
    labels: Vec<(usize, String)>,
}

impl Tally {
    pub fn new(c: &Constraints) -> Tally {
        Tally {
            counts: [0; 15],
            labels: Reason::ALL.iter().map(|r| (r.slot(), r.label(c))).collect(),
        }
    }
    pub fn bump(&mut self, r: Reason) {
        self.counts[r.slot()] += 1;
    }
    pub fn get(&self, r: Reason) -> u32 {
        self.counts[r.slot()]
    }
    pub fn total(&self) -> u32 {
        self.counts.iter().sum()
    }
    /// The reason that rejected the most, or `None` if nothing was rejected.
    ///
    /// Raw count, which is the wrong question on its own — see
    /// [`Tally::terminal`] for the one that matters and why.
    pub fn binding(&self) -> Option<Reason> {
        Reason::ALL
            .iter()
            .copied()
            .filter(|r| self.get(*r) > 0)
            .max_by_key(|r| self.get(*r))
    }

    /// How many candidates (or pairs) ever reached this gate.
    ///
    /// The gates run in [`Reason::ALL`] order and each returns at the first
    /// failure, so everything spent earlier never arrived here. Two funnels,
    /// counted out of two totals: the candidate-stage gates out of `enumerated`
    /// and the pair-stage gates out of `built`.
    pub fn reached(&self, r: Reason, enumerated: usize, built: usize) -> u32 {
        let pair = r.pair_stage();
        let first = Reason::ALL.iter().position(|x| x.pair_stage()).unwrap_or(0);
        let start = if pair { first } else { 0 };
        let total = if pair { built } else { enumerated };
        let spent: u32 = Reason::ALL[start..r.slot()]
            .iter()
            .map(|x| self.counts[x.slot()])
            .sum();
        (total as u32).saturating_sub(spent)
    }

    /// The last gate that rejected **everything that reached it**.
    ///
    /// This is the diagnosis a raw count hides, and hiding it produced advice
    /// that could not work and then looped. Measured case: a 9,035 bp plasmid
    /// whose target sits inside a 4.5 kb direct repeat gave 3,693 Tm, 38 run,
    /// 20 3'-stability and 269 off-target rejections. Tm "won" on count, so the
    /// tool advised widening `--len`; but 3,751 candidates had already died
    /// before the specificity gate, so all 269 that reached it were rejected
    /// there — the gate was 100% fatal, and no length on earth moves a
    /// duplicated locus. Following the advice widened `--len` three times, each
    /// time to a fully off-target result, until the fourth suggestion was an
    /// argument the CLI itself refuses.
    ///
    /// A 1%-of-all-rejections floor keeps a gate that happened to kill the last
    /// one or two candidates from outranking a gate that killed thousands.
    pub fn terminal(&self, enumerated: usize, built: usize) -> Option<Reason> {
        let floor = (self.total() / 100).max(1);
        // `rfind`, so it is the LAST such gate: a later gate acted on a
        // population everything earlier had already filtered, which makes it
        // the more specific diagnosis of the two.
        Reason::ALL.iter().copied().rfind(|r| {
            let n = self.get(*r);
            n >= floor && n > 0 && n == self.reached(*r, enumerated, built)
        })
    }

    /// Every non-zero count, under the heading of the funnel it belongs to.
    pub fn render(&self, indent: &str, enumerated: usize, built: usize) -> String {
        let mut out = String::new();
        for (pair, heading) in [
            (false, format!("of {enumerated} candidate oligos:")),
            (true, format!("of {built} pairs:")),
        ] {
            if !Reason::ALL
                .iter()
                .any(|r| r.pair_stage() == pair && self.get(*r) > 0)
            {
                continue;
            }
            out.push_str(&format!("{indent}{heading}\n"));
            for r in Reason::ALL.iter().filter(|r| r.pair_stage() == pair) {
                let n = self.get(*r);
                if n == 0 {
                    continue;
                }
                let label = self
                    .labels
                    .iter()
                    .find(|(s, _)| *s == r.slot())
                    .map(|(_, l)| l.as_str())
                    .unwrap_or(r.key());
                let reached = self.reached(*r, enumerated, built);
                // The rate, not just the count, because "269 rejected" and
                // "269 of the 269 that got here" are different findings and
                // only the second one is a diagnosis.
                out.push_str(&format!(
                    "{indent}  {n:>6}  {label}{}\n",
                    if reached > n {
                        String::new()
                    } else {
                        format!("  <- all {reached} that reached it")
                    }
                ));
            }
        }
        out
    }

    /// What to widen, and the order to widen it in.
    ///
    /// `clashes` is empty for a candidate-stage refusal and carries the
    /// unintended restriction sites for a pair-stage one, because
    /// [`Reason::InternalSite`] is the one refusal whose remedy depends on
    /// *which* site and *where* rather than on a threshold.
    pub fn advice(
        &self,
        c: &Constraints,
        enumerated: usize,
        built: usize,
        clashes: &[crate::tail::SiteClash],
    ) -> String {
        let mut out = String::new();
        // Said first when it applies, because it explains the size of the
        // search rather than any one threshold, and every other remedy below
        // reads as though the search had been a normal one.
        if c.mode == Mode::Contain && c.flank == 0 {
            out.push_str(&format!(
                "--flank 0 pins BOTH outer ends to the selection, so only {} oligos per side \
                 were possible -- one 5' end and {} lengths. That is the narrowest search \
                 this tool can be asked for. Raise --flank to a few bases, or widen --len, \
                 before touching anything physical. ",
                c.len_max + 1 - c.len_min,
                c.len_max + 1 - c.len_min
            ));
        }
        let reason = self.terminal(enumerated, built).or_else(|| self.binding());
        let lead = match reason {
            Some(Reason::Tm) => {
                // Capped at what the CLI will accept, so the remedy is never an
                // argument the tool then refuses; and once there is nothing
                // left to widen, stop suggesting it and say so.
                let lo = c.len_min.saturating_sub(3).max(15);
                let hi = (c.len_max + 13).min(Constraints::LEN_HARD_MAX);
                if hi > c.len_max || lo < c.len_min {
                    format!(
                        "Tm is the binding constraint. Widen the LENGTH range first -- \
                         --len {lo}..{hi} is the usual move -- because length is a \
                         synthesis-cost bound and Tm is a physical one; relaxing Tm to \
                         rescue a search selects primers that pass these checks and fail \
                         at the bench. If that is not enough, --tm {:.0}..{:.0}.",
                        c.tm_min - 3.0,
                        c.tm_max + 3.0
                    )
                } else {
                    format!(
                        "Tm is the binding constraint and --len is already at {}..{}, the \
                         widest this tool accepts, so there is nothing left to widen on the \
                         synthesis side. --tm {:.0}..{:.0} is the remaining move and it is a \
                         physical one: on an AT-rich template the honest answer may be that \
                         the anneal has to be run cooler, not that the primer has to be \
                         longer.",
                        c.len_min,
                        c.len_max,
                        c.tm_min - 3.0,
                        c.tm_max + 3.0
                    )
                }
            }
            Some(Reason::OffTarget) => "Every candidate that reached the specificity gate \
                 anneals somewhere else on this template. That is a property of the \
                 molecule, not of the settings: look for a repeat -- an IS element, a \
                 duplicated polylinker, an rrn operon -- move the region, or accept the \
                 risk with --no-specificity, which says so in the report."
                .into(),
            Some(Reason::OffTheEnd) => format!(
                "There is not enough template outside the region. Raise --flank past {}, \
                 or use --mode within.",
                c.flank
            ),
            Some(Reason::Gc) if c.gc_hard => format!(
                "The %GC band is a gate here because --gc-hard was passed, and it is what \
                 emptied the search. Drop --gc-hard, or widen --gc past {:.0}..{:.0}: on an \
                 AT-rich template a hard 40-60% band excludes the organism rather than the \
                 design, and it encodes no failure the Tm window does not already gate.",
                c.gc_min, c.gc_max
            ),
            Some(Reason::ProductLength) => {
                format!("Widen --product past {}..{}.", c.product_min, c.product_max)
            }
            Some(Reason::DeltaTm) => format!(
                "The two primers' Tms are what will not agree. Widen --tm-diff past \
                 {:.1}C, or widen --tm so more lengths are available to balance against.",
                c.tm_diff_max
            ),
            Some(Reason::PairOverlap) => format!(
                "Every pair's two footprints overlap: there is no room between them for a \
                 product. The shortest product two {} nt primers can make is {} bases. Use \
                 --mode contain so the primers may sit outside the region, amplify a longer \
                 region, or lower --len.",
                c.len_min,
                2 * c.len_min + 1
            ),
            Some(Reason::CrossDimer) => format!(
                "The forward and reverse 3' ends anneal to each other at or below {:.1} \
                 kcal/mol in every pair built. That is about the two ends specifically, so \
                 moving one of them is the fix: a different --flank, a narrower --region, \
                 or a different --len. If both primers carry tails, check whether it is the \
                 tails that pair.",
                c.dg_dimer_three_prime
            ),
            Some(Reason::InternalSite) => {
                let mut s = String::from(
                    "The site being added already occurs inside the product, so the enzyme \
                     would cut the insert as well as its ends. No threshold moves this. ",
                );
                for cl in clashes {
                    s.push_str(&cl.render());
                    s.push_str(". ");
                }
                s.push_str(
                    "Choose an enzyme absent from the region -- pl digest lists what cuts \
                     it -- move the region, or drop --add-5/--add-3.",
                );
                s
            }
            Some(r) => format!("The binding constraint is: {}.", r.label(c)),
            None => String::new(),
        };
        out.push_str(&lead);
        out
    }
}

/// A ranked primer pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub forward: Primer,
    pub reverse: Primer,
    /// Unrolled 0-based coordinate of each primer's 3'-terminal base — the end
    /// a polymerase extends from, and what the diversity rule separates on.
    /// Unrolled rather than reduced, so two sites either side of the origin do
    /// not read as three bases apart when they are three thousand.
    pub forward_three_prime: i64,
    pub reverse_three_prime: i64,
    pub penalty: f64,
    /// The eight scoring terms, so the total can be decomposed. A number nobody
    /// can take apart is a black box, and this project ships a methods note
    /// beside every tier-3 quantity precisely so a disagreement reads as a
    /// modelling choice.
    pub terms: Vec<(&'static str, f64)>,
    /// 1-based inclusive on the plus strand; `end < start` means the amplicon
    /// crosses the origin, which is ordinary on a plasmid.
    pub product_start: u64,
    pub product_end: u64,
    pub product_bp: u64,
    /// %GC **of the unambiguous bases**, which is `pl_core::Composition`'s
    /// definition and is only the whole amplicon when [`Pair::product_ambiguous`]
    /// is zero.
    pub product_gc: f64,
    /// Ambiguity codes inside the amplicon but outside both footprints.
    ///
    /// Legal — only the footprints are gated unambiguous — but it means
    /// `product_gc`'s denominator is `product_bp - product_ambiguous`, and a
    /// denominator that changes without saying so is the thing this crate
    /// refuses to do elsewhere (`TmError::NotUnambiguous`,
    /// `DesignError::AmbiguousTarget`).
    pub product_ambiguous: u64,
    pub delta_tm: f64,
    pub cross_dimer_any: crate::fold::Structure,
    pub cross_dimer_three: crate::fold::Structure,
    /// Did `pl_clone::pcr` agree, and on what length?
    pub pcr_check: Result<u64, String>,
    pub warnings: Vec<String>,
}

/// Whether the off-target scan ran, and over what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecNote {
    pub ran: bool,
    pub seed: usize,
    pub used_index: bool,
}

/// A whole design run.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub bp: u64,
    pub circular: bool,
    pub region: Region,
    pub region_bp: u64,
    pub mode: Mode,
    pub flank: u64,
    pub method: String,
    pub constraints: String,
    pub dg_note: String,
    pub weights: String,
    pub enumerated: usize,
    pub survivors_forward: usize,
    pub survivors_reverse: usize,
    pub pairs_built: usize,
    pub tally: Tally,
    pub pairs: Vec<Pair>,
    pub specificity: SpecNote,
    /// Caveats that are part of the **answer**, not complaints about the input.
    /// Carried as data so a `--json` consumer cannot lose them.
    pub warnings: Vec<String>,
}

/// The specificity sentence. Never the bare word "specific".
pub fn specificity_caveat(label: &str, bp: u64, circular: bool) -> String {
    format!(
        "specificity was checked against {label} only, all {bp} bp of it, {}. It says \
         nothing about the host genome; a primer unique in a plasmid is routinely not \
         unique in E. coli.",
        if circular { "circular" } else { "linear" }
    )
}

/// The RT-PCR sentence, printed unconditionally with the preset.
pub const RT_PCR_CAVEAT: &str = "\
RT-PCR, bacteria: this design CANNOT exclude genomic DNA. Bacterial genes have no \
introns, so there is no exon-exon junction for a primer to span, and any pair that \
amplifies the cDNA amplifies contaminating gDNA identically. Use a DNase-treated RNA \
prep and a no-RT control; the no-RT control is the only thing here that distinguishes \
the two. If the region sits inside an operon, a pair inside it quantifies the operon \
and not the gene.";

/// Printed when the scan was skipped.
pub const NO_SPECIFICITY_NOTE: &str = "\
the off-target scan was skipped. These primers were scored in isolation, which is what \
every designer that defers specificity to BLAST does, and it is weaker than what this \
tool can tell you.";

impl Report {
    /// The human-readable report.
    pub fn text(&self, label: &str) -> String {
        let mut o = String::new();
        o.push_str(&format!(
            "{label}, {} bp {}\n",
            self.bp,
            if self.circular { "circular" } else { "linear" }
        ));
        o.push_str(&format!(
            "target {}..{} ({} bp){} - mode {}{}\n",
            self.region.start,
            self.region.end,
            self.region_bp,
            if self.region.wraps() {
                ", crosses the origin"
            } else {
                ""
            },
            self.mode.as_str(),
            match self.mode {
                Mode::Contain => format!(", primers may begin up to {} bp outside it", self.flank),
                Mode::Within => ", both primers lie inside it".to_string(),
            }
        ));
        o.push_str(&format!("{}\n", self.method));
        o.push_str(&format!("{}\n", self.constraints));
        o.push_str(&format!("{}\n", self.dg_note));
        if self.specificity.ran {
            o.push_str(&format!(
                "off-target scan: this template only, 3'-anchored seed {}{}\n",
                self.specificity.seed,
                if self.specificity.used_index {
                    ""
                } else {
                    " (every candidate scanned in full; the seed index was not usable here)"
                }
            ));
        } else {
            o.push_str("off-target scan: NOT RUN\n");
        }
        o.push_str(&format!(
            "\n{} candidate oligos, {} forward and {} reverse survived, {} pairs built, \
             {} reported\n",
            self.enumerated,
            self.survivors_forward,
            self.survivors_reverse,
            self.pairs_built,
            self.pairs.len()
        ));
        if self.tally.total() > 0 {
            o.push_str("\nrejected:\n");
            o.push_str(&self.tally.render("  ", self.enumerated, self.pairs_built));
        }

        for (i, p) in self.pairs.iter().enumerate() {
            o.push_str(&format!(
                "\npair {}   penalty {:.2}   product {}..{}   {} bp   {:.1}% GC\n",
                i + 1,
                p.penalty,
                p.product_start,
                p.product_end,
                p.product_bp,
                p.product_gc
            ));
            o.push_str(
                "                                                nt    GC%      Tm    3'dG37  position\n",
            );
            for pr in [&p.forward, &p.reverse] {
                o.push_str(&pr.render_rows());
            }
            // The specificity clause names the model it used. "No other
            // binding site on this template" was the loosest wording in the
            // report and the one sitting where a reader acts on it: the scan
            // sees a second site only when the last `seed` bases match
            // exactly, so a near-identical locus differing by one base inside
            // that window -- measured, at 3, 6, 9 and 12 nt from the 3' end --
            // was being reported as no site at all.
            o.push_str(&format!(
                "     dTm {:.1}C - 3' cross-dimer dG37 {} - {}\n",
                p.delta_tm,
                p.cross_dimer_three.render(),
                if self.specificity.ran {
                    format!(
                        "no second site with a perfect {} nt 3' match on this template",
                        self.specificity.seed
                    )
                } else {
                    "off-target scan not run".to_string()
                }
            ));
            match &p.pcr_check {
                Ok(bp) if *bp == p.product_bp => {
                    o.push_str(&format!("     pl-clone agrees: {bp} bp\n"))
                }
                Ok(bp) => o.push_str(&format!(
                    "     WARNING: pl-design predicts {} bp and pl_clone::pcr reports {bp} bp \
                     for the same two oligos. One of the two is wrong and this tool will not \
                     choose between them; please report this with the file and the command line.\n",
                    p.product_bp
                )),
                Err(e) => o.push_str(&format!(
                    "     WARNING: pl_clone::pcr will not simulate this pair: {e}\n"
                )),
            }
            for w in &p.warnings {
                o.push_str(&format!("     warning: {w}\n"));
            }
        }

        if let Some(p) = self.pairs.first() {
            o.push_str(&format!(
                "\nannealing advice, from the lower Tm ({:.1}C):\n",
                p.forward.tm.min(p.reverse.tm)
            ));
            for pol in pl_thermo::POLYMERASES {
                let (lo, _) = pl_thermo::anneal(p.forward.tm, Some(p.reverse.tm), pol);
                o.push_str(&format!("  {:>9} {:5.0}C   {}\n", pol.name, lo, pol.note));
            }
            o.push_str("this is advice, not a measurement; the Tm above is the measurement\n");
            if p.forward.tail.is_some() || p.reverse.tail.is_some() {
                o.push_str(
                    "with a tail the oligo is longer from cycle 3 onward, when the tail is \
                     templated: run about 5 cycles at the annealing temperature above, then \
                     the rest at the tailed one on the 'order this' line\n",
                );
            }
        }

        if !self.warnings.is_empty() {
            o.push('\n');
            for w in &self.warnings {
                o.push_str(&format!("{w}\n"));
            }
        }
        o
    }

    /// One JSON document. Every caveat is a field, so a consumer cannot lose it
    /// down a redirect.
    pub fn json(&self, label: &str) -> String {
        let mut o = String::new();
        o.push_str("{\n");
        o.push_str(&format!("  \"template\": {},\n", js(label)));
        o.push_str(&format!(
            "  \"bp\": {}, \"circular\": {},\n",
            self.bp, self.circular
        ));
        o.push_str(&format!(
            "  \"target\": {{\"start\": {}, \"end\": {}, \"bp\": {}, \"crosses_origin\": {}}},\n",
            self.region.start,
            self.region.end,
            self.region_bp,
            self.region.wraps()
        ));
        o.push_str(&format!(
            "  \"mode\": {}, \"flank\": {},\n",
            js(self.mode.as_str()),
            self.flank
        ));
        o.push_str(&format!("  \"tm_method\": {},\n", js(&self.method)));
        o.push_str(&format!("  \"constraints\": {},\n", js(&self.constraints)));
        o.push_str(&format!("  \"dg_convention\": {},\n", js(&self.dg_note)));
        o.push_str(&format!("  \"weights\": {},\n", js(&self.weights)));
        o.push_str(&format!(
            "  \"specificity\": {{\"ran\": {}, \"scope\": \"this template only\", \
             \"seed\": {}, \"seed_index_used\": {}}},\n",
            self.specificity.ran, self.specificity.seed, self.specificity.used_index
        ));
        o.push_str(&format!(
            "  \"candidates_built\": {}, \"survivors_forward\": {}, \
             \"survivors_reverse\": {}, \"pairs_built\": {},\n",
            self.enumerated, self.survivors_forward, self.survivors_reverse, self.pairs_built
        ));
        o.push_str("  \"rejected\": [");
        let mut first = true;
        for r in Reason::ALL {
            let n = self.tally.get(*r);
            if n == 0 {
                continue;
            }
            if !first {
                o.push_str(", ");
            }
            first = false;
            o.push_str(&format!("{{\"reason\": {}, \"n\": {n}}}", js(r.key())));
        }
        o.push_str("],\n");
        o.push_str("  \"pairs\": [");
        for (i, p) in self.pairs.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            o.push_str("\n    {");
            o.push_str(&format!(
                "\"rank\": {}, \"penalty\": {:.6}, ",
                i + 1,
                p.penalty
            ));
            o.push_str("\"terms\": {");
            for (j, (k, v)) in p.terms.iter().enumerate() {
                if j > 0 {
                    o.push_str(", ");
                }
                o.push_str(&format!("{}: {:.6}", js(k), v));
            }
            o.push_str("}, ");
            o.push_str(&format!(
                "\"amplicon\": {{\"start\": {}, \"end\": {}, \"bp\": {}, \"gc_percent\": {:.3}, \
                 \"gc_percent_basis\": \"unambiguous bases\", \"ambiguity_codes\": {}, \
                 \"crosses_origin\": {}, \"confirmed_by_pcr\": {}}}, ",
                p.product_start,
                p.product_end,
                p.product_bp,
                p.product_gc,
                p.product_ambiguous,
                p.product_end < p.product_start,
                matches!(&p.pcr_check, Ok(bp) if *bp == p.product_bp)
            ));
            o.push_str(&format!("\"forward\": {}, ", p.forward.json()));
            o.push_str(&format!("\"reverse\": {}, ", p.reverse.json()));
            o.push_str(&format!(
                "\"delta_tm\": {:.3}, \"cross_dimer_3p_dg37\": {:.3}, \
                 \"cross_dimer_any_dg37\": {:.3}, ",
                p.delta_tm, p.cross_dimer_three.dg, p.cross_dimer_any.dg
            ));
            o.push_str("\"warnings\": [");
            for (j, w) in p.warnings.iter().enumerate() {
                if j > 0 {
                    o.push_str(", ");
                }
                o.push_str(&js(w));
            }
            o.push_str("]}");
        }
        o.push_str("\n  ],\n");
        o.push_str("  \"warnings\": [");
        for (i, w) in self.warnings.iter().enumerate() {
            if i > 0 {
                o.push_str(", ");
            }
            o.push_str(&js(w));
        }
        o.push_str("]\n}\n");
        o
    }
}

impl Primer {
    /// Two or three rows: the tail (if any), the footprint, and the whole
    /// oligo (if there is a tail).
    ///
    /// Four independent channels tell the tail from the footprint, because
    /// conflating them on screen is the same error as conflating them in the
    /// Tm: a separate row, lowercase against uppercase, blank metrics, and no
    /// position. In the GUI there is a fifth — the muted colour — and colour is
    /// never the only one.
    pub fn render_rows(&self) -> String {
        let mut o = String::new();
        let side = match self.side {
            Side::Fwd => "F",
            Side::Rev => "R",
        };
        let foot = String::from_utf8_lossy(&self.footprint).to_uppercase();
        let metrics = format!(
            "{:5.1}%  {:5.1}C    {:6.1}  {}..{} ({})",
            self.gc,
            self.tm,
            self.dg_three_prime,
            self.start,
            self.end,
            self.side.as_str()
        );
        match &self.tail {
            Some(t) => {
                let tail = String::from_utf8_lossy(&t.bases).to_lowercase();
                // Blank metrics on the tail row are not a formatting choice: a
                // GC%, a Tm or a ΔG there is the exact error the split exists
                // to prevent, and so is a position -- the tail is not anywhere
                // on this template.
                o.push_str(&format!(
                    "  {side}  {tail:<44}{:>3}      -       -         -  5' tail, adds {}\n",
                    t.len(),
                    t.enzyme.name
                ));
                o.push_str(&format!(
                    "     {foot:<44}{:>3}  {metrics}\n",
                    self.footprint.len()
                ));
                let whole = format!("{tail}{foot}");
                o.push_str(&format!(
                    "     {whole:<44}{:>3}                           order this{}\n",
                    whole.len(),
                    match self.tm_full {
                        Some(t) => format!("  ({t:.1}C from cycle 3, tail templated)"),
                        None => String::new(),
                    }
                ));
            }
            None => {
                o.push_str(&format!(
                    "  {side}  {foot:<44}{:>3}  {metrics}\n",
                    self.footprint.len()
                ));
            }
        }
        o
    }
}

pub(crate) fn js(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tally_prints_in_declaration_order_and_names_the_binding_constraint() {
        let c = Constraints::default();
        let mut t = Tally::new(&c);
        for _ in 0..3 {
            t.bump(Reason::Hairpin);
        }
        for _ in 0..9 {
            t.bump(Reason::Tm);
        }
        t.bump(Reason::Ambiguous);
        // 300 enumerated, so nothing here is a terminal gate and the count
        // rule decides; 13 would make the hairpin gate 3-of-3 fatal, which is
        // the other test below.
        let s = t.render("  ", 300, 0);
        let amb = s.find("ambiguity").unwrap();
        let tm = s.find("Tm outside").unwrap();
        let hp = s.find("hairpin stem").unwrap();
        assert!(
            amb < tm && tm < hp,
            "declaration order, not count order:\n{s}"
        );
        assert_eq!(t.binding(), Some(Reason::Tm));
        assert_eq!(t.total(), 13);
        assert_eq!(
            t.terminal(300, 0),
            None,
            "287 candidates got past everything"
        );
        assert!(t
            .advice(&c, 300, 0, &[])
            .contains("Widen the LENGTH range first"));
    }

    /// PROVEN TO FAIL: with `advice` choosing by `binding()` alone -- the raw
    /// count, which is what shipped -- this asserts the off-target sentence and
    /// gets the Tm one, because 3,693 is larger than 269 however fatal 269 is.
    #[test]
    fn a_gate_that_rejected_everything_that_reached_it_outranks_one_that_rejected_more() {
        // The measured 9,035 bp plasmid whose target sits inside a 4.5 kb
        // direct repeat: 4,020 candidates, 3,751 dead before the specificity
        // gate, and all 269 that reached it rejected there.
        let c = Constraints::default();
        let mut t = Tally::new(&c);
        for _ in 0..3693 {
            t.bump(Reason::Tm);
        }
        for _ in 0..38 {
            t.bump(Reason::Run);
        }
        for _ in 0..20 {
            t.bump(Reason::ThreePrimeStability);
        }
        for _ in 0..269 {
            t.bump(Reason::OffTarget);
        }
        assert_eq!(t.binding(), Some(Reason::Tm), "on raw count Tm still wins");
        assert_eq!(t.reached(Reason::OffTarget, 4020, 0), 269);
        assert_eq!(t.terminal(4020, 0), Some(Reason::OffTarget));
        let a = t.advice(&c, 4020, 0, &[]);
        assert!(a.contains("look for a repeat"), "{a}");
        assert!(!a.contains("Widen the LENGTH range"), "{a}");
        // And the funnel is visible in the table itself.
        let s = t.render("  ", 4020, 0);
        assert!(s.contains("all 269 that reached it"), "{s}");
    }

    /// PROVEN TO FAIL: with the widening suggestion uncapped -- `c.len_max +
    /// 13` -- this asserts no `..66` and gets `--len 15..66`, which
    /// `pl design` then refuses with "expected a number from 8 to 60".
    #[test]
    fn the_widening_advice_never_names_a_length_the_cli_would_refuse() {
        let mut c = Constraints {
            len_min: 15,
            len_max: 53,
            ..Default::default()
        };
        let mut t = Tally::new(&c);
        for _ in 0..100 {
            t.bump(Reason::Tm);
        }
        let a = t.advice(&c, 1000, 0, &[]);
        assert!(a.contains("--len 15..60"), "{a}");
        assert!(!a.contains("66"), "{a}");

        // And once there is nothing left to widen it stops saying so.
        c.len_max = Constraints::LEN_HARD_MAX;
        let a = t.advice(&c, 1000, 0, &[]);
        assert!(a.contains("nothing left to widen"), "{a}");
        assert!(!a.contains("Widen the LENGTH range first"), "{a}");
    }

    /// PROVEN TO FAIL: with the `--flank 0` clause removed, the assertion on
    /// "pins BOTH outer ends" fires -- which is what shipped, so a search
    /// narrowed to ten oligos per side by one flag was told to widen Tm.
    #[test]
    fn flank_zero_is_named_before_any_threshold_is_blamed() {
        let c = Constraints {
            flank: 0,
            ..Default::default()
        };
        let mut t = Tally::new(&c);
        for _ in 0..20 {
            t.bump(Reason::Tm);
        }
        let a = t.advice(&c, 20, 0, &[]);
        assert!(a.starts_with("--flank 0 pins BOTH outer ends"), "{a}");
        assert!(a.contains("10 oligos per side"), "{a}");

        // Not said when it does not apply.
        let d = Constraints::default();
        assert!(!t.advice(&d, 20, 0, &[]).contains("--flank 0 pins"));
    }

    /// PROVEN TO FAIL: with `NoPair`'s fixed "widen --tm-diff or --product"
    /// string -- what shipped -- neither assertion here can hold, because the
    /// remedy never looked at the tally and the enzyme and coordinate were
    /// computed and thrown away.
    #[test]
    fn a_pair_stage_refusal_names_the_enzyme_and_the_coordinate() {
        let c = Constraints::default();
        let mut t = Tally::new(&c);
        // Candidate-stage attrition too, because the bug being pinned is that
        // this half was printed under a heading calling it a pair rejection.
        for _ in 0..2935 {
            t.bump(Reason::Tm);
        }
        for _ in 0..40 {
            t.bump(Reason::DeltaTm);
        }
        for _ in 0..39441 {
            t.bump(Reason::InternalSite);
        }
        let clashes = [crate::tail::SiteClash {
            enzyme: "EcoRI",
            strand: "+",
            at: crate::tail::ClashAt::Template(4359),
        }];
        let a = t.advice(&c, 4020, 39481, &clashes);
        assert!(a.contains("EcoRI also reads at 4359"), "{a}");
        assert!(!a.contains("--tm-diff"), "the remedy that cannot work: {a}");

        // The two funnels print under their own headings and out of their own
        // totals, so a candidate-stage count is never shown as a pair one.
        let s = t.render("  ", 4020, 39481);
        assert!(s.contains("of 4020 candidate oligos:"), "{s}");
        assert!(s.contains("of 39481 pairs:"), "{s}");
    }

    #[test]
    fn the_specificity_caveat_never_says_the_bare_word() {
        let s = specificity_caveat("pUC19.gb", 2686, true);
        assert!(s.contains("2686 bp"), "{s}");
        assert!(s.contains("says nothing about the host genome"), "{s}");
    }
}
