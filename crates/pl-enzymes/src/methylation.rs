//! Which sites a methylase blocks.
//!
//! `docs/PLAN.md` §7.1: "For each candidate site, expand a window covering the
//! recognition site and test for an overlapping motif, then look up Blocked /
//! Impaired / Not sensitive. **Render blocked sites struck through, not
//! hidden.**"
//!
//! Struck through rather than hidden is the whole design. A site that will not
//! cut is still a site — it is there in the sequence, it will appear in anyone
//! else's map, and it will cut the moment the plasmid is passed through a
//! `dam⁻` strain. Hiding it produces a map that disagrees with every other tool
//! for reasons the user cannot see.
//!
//! # Three traps, each of which produces a confidently wrong answer
//!
//! **Presence of the motif is not the verdict.** `BamHI` (`GGATCC`), `BglII`
//! (`AGATCT`) and `PvuI` (`CGATCG`) all contain `GATC` unconditionally, and all
//! three cut perfectly well in dam⁺ DNA. Detecting an overlap must never by
//! itself set the flag; the (enzyme, methylase) pair is always consulted.
//!
//! **Key on the enzyme, never on the recognition sequence.** `SmaI` and `XmaI`
//! both read `CCCGGG`. SmaI is CpG-blocked; XmaI is only impaired. The same
//! REBASE reference appears under both with opposite verdicts, because the
//! property belongs to the protein.
//!
//! **Most of it is CpG.** 26 of the 34 affected pairs here are CpG, not Dam or
//! Dcm — which is why [`pl_core::Methylation`] gained a `cpg` field.
//!
//! # Provenance
//!
//! Compiled from REBASE `damlist` (all 50 enzymes retrieved) and NEB's Dam/Dcm/
//! CpG chart, then independently re-checked. Biopython carries nothing usable:
//! its `is_methylable()` is a constant per enzyme class with no methylase
//! identity and no grading, and it correlates with actual sensitivity at
//! φ = +0.02 over these 50 — it is noise, and must not be used or inverted.

use pl_core::{iupac, Methylation, Topology};

use crate::Enzyme;

/// A methylase, as a motif plus which of its bases carry the methyl group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Methylase {
    /// `GATC`, N6-adenine.
    Dam,
    /// `CCWGG`, C5 of the internal cytosine.
    Dcm,
    /// `CG`, C5 — both strands, so both offsets.
    Cpg,
}

impl Methylase {
    pub fn motif(self) -> &'static str {
        match self {
            Methylase::Dam => "GATC",
            Methylase::Dcm => "CCWGG",
            Methylase::Cpg => "CG",
        }
    }

    /// Offsets within the motif that actually carry a methyl group.
    fn offsets(self) -> &'static [usize] {
        match self {
            Methylase::Dam => &[1, 2],
            Methylase::Dcm => &[1, 3],
            Methylase::Cpg => &[0, 1],
        }
    }

    /// How far outside the recognition site a relevant motif can begin, and
    /// how far past its end one can extend.
    ///
    /// A methylated base at motif offset `o` lands on site index `j` when the
    /// motif starts at `j - o`, so the leftmost useful start is `-max(o)` and
    /// the rightmost useful end is `(k-1) + (m-1-min(o))`. Measured across all
    /// 50 enzymes the bound is tight — no wider window ever finds anything.
    fn flanks(self) -> (usize, usize) {
        let o = self.offsets();
        let m = self.motif().len();
        (*o.iter().max().unwrap(), m - 1 - *o.iter().min().unwrap())
    }

    pub fn name(self) -> &'static str {
        match self {
            Methylase::Dam => "Dam",
            Methylase::Dcm => "Dcm",
            Methylase::Cpg => "CpG",
        }
    }

    fn active_in(self, m: &Methylation) -> bool {
        match self {
            Methylase::Dam => m.dam,
            Methylase::Dcm => m.dcm,
            Methylase::Cpg => m.cpg,
        }
    }
}

/// What the methylation does to cleavage at a site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Will not cut.
    Blocked,
    /// Cuts slowly or partially.
    Impaired,
    /// Sources disagree, or none exists.
    ///
    /// A real state, kept separate from "not sensitive". NEB says PmeI and
    /// SacI are blocked by some overlapping combinations; REBASE records only
    /// "cut" for every context it documents and knows of no blocking
    /// reference. Reporting either answer as fact would be inventing one.
    Unknown,
}

impl Effect {
    pub fn as_str(self) -> &'static str {
        match self {
            Effect::Blocked => "blocked",
            Effect::Impaired => "impaired",
            Effect::Unknown => "possibly affected",
        }
    }
}

/// When the effect applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// The motif is intrinsic to the recognition site: always affected.
    Unconditional,
    /// Any single overlapping motif is enough.
    AnyOverlap,
    /// Only when the site is overlapped at **both** ends; one alone still cuts.
    BothEnds,
}

struct Rule {
    enzyme: &'static str,
    methylase: Methylase,
    effect: Effect,
    scope: Scope,
}

/// The verified table. Every (enzyme, methylase) pair not listed is unaffected.
///
/// Names are matched exactly. Near-misses are dense here and a fuzzy match
/// would be silently wrong: `SmlI` is not `SmaI`, `BsrFI` is not `BsrGI`,
/// `EarI` is not `EagI`, `HpaII` is not `HpaI`, `NarI` is not `AscI`.
const RULES: &[Rule] = &[
    // --- Dam (GATC) ---
    Rule {
        enzyme: "BclI",
        methylase: Methylase::Dam,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "BspEI",
        methylase: Methylase::Dam,
        effect: Effect::Blocked,
        scope: Scope::AnyOverlap,
    },
    Rule {
        enzyme: "ClaI",
        methylase: Methylase::Dam,
        effect: Effect::Blocked,
        scope: Scope::AnyOverlap,
    },
    Rule {
        enzyme: "NruI",
        methylase: Methylase::Dam,
        effect: Effect::Blocked,
        scope: Scope::AnyOverlap,
    },
    Rule {
        enzyme: "XbaI",
        methylase: Methylase::Dam,
        effect: Effect::Blocked,
        scope: Scope::AnyOverlap,
    },
    // --- Dcm (CCWGG) ---
    Rule {
        enzyme: "ApaI",
        methylase: Methylase::Dcm,
        effect: Effect::Blocked,
        scope: Scope::AnyOverlap,
    },
    Rule {
        enzyme: "StuI",
        methylase: Methylase::Dcm,
        effect: Effect::Blocked,
        scope: Scope::AnyOverlap,
    },
    Rule {
        enzyme: "FseI",
        methylase: Methylase::Dcm,
        effect: Effect::Impaired,
        scope: Scope::BothEnds,
    },
    // --- CpG (CG), 26 of the 34 ---
    Rule {
        enzyme: "AatII",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "AgeI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "AscI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "BsiWI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "BspEI",
        methylase: Methylase::Cpg,
        effect: Effect::Impaired,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "BstBI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "ClaI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "EagI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "FseI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "MluI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "NotI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "NruI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "PvuI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "SacII",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "SalI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "SmaI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "SnaBI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "XhoI",
        methylase: Methylase::Cpg,
        effect: Effect::Impaired,
        scope: Scope::Unconditional,
    },
    // Same site as SmaI, different protein, different verdict.
    Rule {
        enzyme: "XmaI",
        methylase: Methylase::Cpg,
        effect: Effect::Impaired,
        scope: Scope::Unconditional,
    },
    Rule {
        enzyme: "ApaI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::AnyOverlap,
    },
    Rule {
        enzyme: "NheI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::AnyOverlap,
    },
    Rule {
        enzyme: "EcoRI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::BothEnds,
    },
    Rule {
        enzyme: "EcoRV",
        methylase: Methylase::Cpg,
        effect: Effect::Impaired,
        scope: Scope::BothEnds,
    },
    Rule {
        enzyme: "HpaI",
        methylase: Methylase::Cpg,
        effect: Effect::Blocked,
        scope: Scope::BothEnds,
    },
    // Sources conflict; see `Effect::Unknown`.
    Rule {
        enzyme: "PmeI",
        methylase: Methylase::Cpg,
        effect: Effect::Unknown,
        scope: Scope::AnyOverlap,
    },
    Rule {
        enzyme: "SacI",
        methylase: Methylase::Cpg,
        effect: Effect::Unknown,
        scope: Scope::AnyOverlap,
    },
];

/// What methylation does to one site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SiteEffect {
    pub methylase: Methylase,
    pub effect: Effect,
}

/// Does an active methylation affect this particular site?
///
/// `site_start` is the 0-based index where the recognition site begins.
/// Returns the most severe applicable effect, or `None` when the site cuts
/// normally.
///
/// The scan is top-strand only, which is complete here and not an
/// approximation: `GATC`, `CCWGG` and `CG` are each self-complementary, and
/// every one of the 50 recognition sites is a palindrome. Adding a
/// non-palindromic enzyme, or EcoKI/EcoBI, breaks that and needs a two-strand
/// scan — with care, because a self-complementary motif would then be counted
/// twice.
pub fn site_effect(
    enzyme: &Enzyme,
    seq: &[u8],
    site_start: usize,
    topology: Topology,
    meth: &Methylation,
) -> Option<SiteEffect> {
    let n = seq.len();
    let k = enzyme.site.len();
    if n == 0 || k == 0 {
        return None;
    }

    let mut worst: Option<SiteEffect> = None;
    for rule in RULES {
        if rule.enzyme != enzyme.name || !rule.methylase.active_in(meth) {
            continue;
        }
        let applies = match rule.scope {
            Scope::Unconditional => true,
            Scope::AnyOverlap => !overlaps(rule.methylase, seq, site_start, k, topology).is_empty(),
            Scope::BothEnds => {
                // One overlap at each end. A single end still cuts, so
                // requiring both is the difference between a true warning and
                // a false one.
                let o = overlaps(rule.methylase, seq, site_start, k, topology);
                let m = rule.methylase.motif().len() as isize;
                o.iter().any(|&s| s < 0) && o.iter().any(|&s| s + m > k as isize)
            }
        };
        if !applies {
            continue;
        }
        let cand = SiteEffect {
            methylase: rule.methylase,
            effect: rule.effect,
        };
        // Blocked beats impaired beats unknown.
        let rank = |e: Effect| match e {
            Effect::Blocked => 2,
            Effect::Impaired => 1,
            Effect::Unknown => 0,
        };
        if worst.is_none_or(|w| rank(cand.effect) > rank(w.effect)) {
            worst = Some(cand);
        }
    }
    worst
}

/// Motif start offsets, relative to the site, that put a methylated base
/// **inside** the site.
///
/// The "inside" condition is what stops a motif merely near the site from
/// counting: a `GATC` two bases away is methylated, but not on any base the
/// enzyme reads.
fn overlaps(
    meth: Methylase,
    seq: &[u8],
    site_start: usize,
    k: usize,
    topology: Topology,
) -> Vec<isize> {
    let n = seq.len();
    let motif = meth.motif().as_bytes();
    let m = motif.len();
    let (left, right) = meth.flanks();
    let circular = topology.is_circular();

    let mut out = Vec::new();
    let lo = -(left as isize);
    let hi = (k + right) as isize - m as isize;
    for s in lo..=hi {
        // Every methylated base must land somewhere; at least one must land
        // inside the site.
        let inside = meth.offsets().iter().any(|&o| {
            let p = s + o as isize;
            p >= 0 && p < k as isize
        });
        if !inside {
            continue;
        }
        let mut hit = true;
        for (j, &mb) in motif.iter().enumerate() {
            let abs = site_start as isize + s + j as isize;
            let idx = if circular {
                abs.rem_euclid(n as isize) as usize
            } else if abs < 0 || abs >= n as isize {
                hit = false;
                break;
            } else {
                abs as usize
            };
            if !iupac::matches(mb, seq[idx]) {
                hit = false;
                break;
            }
        }
        if hit {
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{by_name, cut_positions, ENZYMES};

    fn meth(dam: bool, dcm: bool, cpg: bool) -> Methylation {
        Methylation {
            dam,
            dcm,
            cpg,
            ecoki: false,
        }
    }

    /// Effect at the first site of `enzyme` in `seq`, if any.
    fn at_first_site(name: &str, seq: &str, m: &Methylation) -> Option<SiteEffect> {
        let e = by_name(name).unwrap();
        let up = seq.to_ascii_uppercase();
        let start = up.find(e.site).expect("the fixture must contain the site");
        site_effect(e, up.as_bytes(), start, Topology::Linear, m)
    }

    #[test]
    fn a_motif_inside_the_site_does_not_by_itself_block() {
        // The trap that produces the most confident wrong answer. BamHI,
        // BglII and PvuI all contain GATC and all cut perfectly well in dam+
        // DNA. Only the (enzyme, methylase) lookup decides.
        let dam = meth(true, false, false);
        for (name, seq) in [
            ("BamHI", "AAAAGGATCCAAAA"),
            ("BglII", "AAAAAGATCTAAAA"),
            ("PvuI", "AAAACGATCGAAAA"),
        ] {
            assert!(
                at_first_site(name, seq, &dam).is_none(),
                "{name} contains GATC but is not Dam-blocked"
            );
        }
        // BclI is the one unconditional Dam block in this table.
        let b = at_first_site("BclI", "AAAATGATCAAAAA", &dam).unwrap();
        assert_eq!(b.effect, Effect::Blocked);
        assert_eq!(b.methylase, Methylase::Dam);
    }

    #[test]
    fn the_same_site_can_have_two_verdicts_because_it_is_two_proteins() {
        // SmaI and XmaI both read CCCGGG. Keying on the sequence would give
        // them one answer; keying on the enzyme gives the right one.
        let cpg = meth(false, false, true);
        let sma = at_first_site("SmaI", "AAAACCCGGGAAAA", &cpg).unwrap();
        let xma = at_first_site("XmaI", "AAAACCCGGGAAAA", &cpg).unwrap();
        assert_eq!(sma.effect, Effect::Blocked);
        assert_eq!(xma.effect, Effect::Impaired);
    }

    #[test]
    fn a_context_dependent_block_needs_the_context() {
        // ClaI is Dam-blocked only when a GATC actually overlaps it. Marking
        // every ClaI site blocked is as wrong as marking none.
        let dam = meth(true, false, false);
        assert!(at_first_site("ClaI", "AAAAATCGATAAAA", &dam).is_none());
        let hit = at_first_site("ClaI", "AAAGATCGATAAAA", &dam).unwrap();
        assert_eq!(hit.effect, Effect::Blocked);
        assert_eq!(hit.methylase, Methylase::Dam);
        assert_eq!(
            at_first_site("ClaI", "AAAAATCGATCAAA", &dam)
                .unwrap()
                .effect,
            Effect::Blocked
        );
    }

    #[test]
    fn both_ends_means_both() {
        // EcoRI is CpG-blocked only when overlapped at BOTH ends; a single
        // flank still cuts. Treating one end as enough would warn on a great
        // many sites that cut perfectly well.
        let cpg = meth(false, false, true);
        assert!(
            at_first_site("EcoRI", "AAAACGAATTCAAAA", &cpg).is_none(),
            "one flank alone must not block"
        );
        assert!(at_first_site("EcoRI", "AAAAGAATTCGAAAA", &cpg).is_none());
        assert_eq!(
            at_first_site("EcoRI", "AAACGAATTCGAAAA", &cpg)
                .unwrap()
                .effect,
            Effect::Blocked,
            "both flanks together do"
        );
    }

    #[test]
    fn nothing_applies_when_the_methylation_is_off() {
        let none = meth(false, false, false);
        for e in ENZYMES {
            let seq = format!("AAAAAA{}AAAAAA", e.site);
            let r = site_effect(e, seq.as_bytes(), 6, Topology::Linear, &none);
            assert!(r.is_none(), "{} reacted to no methylation at all", e.name);
        }
    }

    #[test]
    fn the_rules_only_name_enzymes_we_have() {
        // A typo here is silent: the rule never fires, and a blocked site is
        // reported as a clean cutter. Near-misses are dense — SmlI/SmaI,
        // BsrFI/BsrGI, EarI/EagI, HpaII/HpaI, NarI/AscI.
        for r in RULES {
            assert!(
                by_name(r.enzyme).is_some(),
                "rule names {:?}, which is not in the table",
                r.enzyme
            );
        }
        let cpg = RULES
            .iter()
            .filter(|r| r.methylase == Methylase::Cpg)
            .count();
        assert_eq!(RULES.len(), 34);
        assert_eq!(
            cpg, 26,
            "CpG dominates; a model without it misses most cases"
        );
    }

    #[test]
    fn a_palindromic_site_reacts_the_same_read_either_way() {
        // For a **palindromic** site and a self-complementary motif, a rule
        // that fires on a sequence must fire on its reverse complement. A
        // one-sided rule is physically impossible for a Type IIP enzyme, and
        // this is the check that catches one.
        //
        // Type IIS enzymes are excluded, and the exclusion is the point rather
        // than a convenience: `GGTCTC` does not appear in the reverse
        // complement of a sequence containing it -- `GAGACC` does -- so the
        // symmetry this asserts is not a claim about them at all. Before they
        // were added, this loop ran over every enzyme and the premise happened
        // to hold.
        let all = meth(true, true, true);
        let mut checked = 0;
        for e in ENZYMES {
            if !pl_core::iupac::is_palindrome_masks(e.site.as_bytes()) {
                continue;
            }
            checked += 1;
            for flank in ["G", "C", "GA", "TC", "CC", "GG", "CCAGG", "GATC"] {
                for seq in [
                    format!("TTTT{flank}{}TTTT", e.site),
                    format!("TTTT{}{flank}TTTT", e.site),
                ] {
                    let start = seq.find(e.site).unwrap();
                    let fwd = site_effect(e, seq.as_bytes(), start, Topology::Linear, &all);
                    let rc =
                        String::from_utf8(pl_core::reverse_complement(seq.as_bytes())).unwrap();
                    let rstart = rc.find(e.site).unwrap();
                    let back = site_effect(e, rc.as_bytes(), rstart, Topology::Linear, &all);
                    assert_eq!(
                        fwd.map(|x| x.effect),
                        back.map(|x| x.effect),
                        "{} is strand-asymmetric on {seq}",
                        e.name
                    );
                }
            }
        }
        assert!(
            checked >= 50,
            "only {checked} palindromic enzymes were exercised"
        );
    }

    #[test]
    fn an_overlap_across_the_origin_is_seen_on_a_circle() {
        // The site sits at the start of the molecule and its blocking GATC
        // wraps past base 1. A linear-only window calls a blocked site clean.
        let dam = meth(true, false, false);
        let e = by_name("ClaI").unwrap();
        let seq = "ATCGATAAAAAAAAAG";
        assert_eq!(
            cut_positions(seq.as_bytes(), Topology::Circular, e).len(),
            1
        );
        assert_eq!(
            site_effect(e, seq.as_bytes(), 0, Topology::Circular, &dam).map(|x| x.effect),
            Some(Effect::Blocked),
            "the GATC formed across the origin was missed"
        );
        // The same bases on a line do not form that overlap.
        assert!(site_effect(e, seq.as_bytes(), 0, Topology::Linear, &dam).is_none());
    }

    #[test]
    fn unresolved_pairs_say_so_rather_than_claiming_either_answer() {
        // NEB says PmeI and SacI are blocked by some overlapping combinations;
        // REBASE records only "cut" and knows of no blocking reference.
        // Asserting either would be inventing an answer.
        let cpg = meth(false, false, true);
        let hit = at_first_site("SacI", "AAAACGAGCTCGAAA", &cpg).unwrap();
        assert_eq!(hit.effect, Effect::Unknown);
        assert_eq!(hit.effect.as_str(), "possibly affected");
    }
}
