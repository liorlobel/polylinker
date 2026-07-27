//! Type IIS assembly, and whether an overhang set will actually work.
//!
//! # Why Type IIS is different
//!
//! A Type IIP enzyme leaves the same ends every time: every EcoRI cut gives the
//! same `AATT`, so every EcoRI fragment ligates to every other one, in either
//! orientation. A Type IIS enzyme cuts *outside* its site, so the overhang it
//! leaves is **whatever four bases happen to be there** — which means the
//! designer chooses them, fragments only join where they were meant to, and the
//! enzyme's own site is cut away and does not reappear in the product.
//!
//! That is the whole idea of Golden Gate, and it is also where it goes wrong:
//! the assembly is only as reliable as the overhang set someone picked.
//!
//! # What this checks, and what it does not
//!
//! Three structural faults, all computable from the overhangs alone:
//!
//! - a **repeated** overhang — two junctions that can swap, so the assembly has
//!   more than one answer;
//! - a **palindromic** overhang — one that ligates to itself, giving head-to-head
//!   dimers;
//! - a **single-mismatch neighbour** — two overhangs differing at one position,
//!   which T4 ligase mis-joins at a measurable rate.
//!
//! **The published ligation-fidelity matrices are not shipped**, and that is
//! stated rather than implied. Potapov *et al.* (PLOS ONE 2020,
//! `10.1371/journal.pone.0238592`) measured every overhang pair; with that data
//! one can predict a fidelity *percentage*. Without it this reports the
//! structural faults and refuses to put a number on the rest. A confident
//! fidelity score computed from nothing would be worse than no score.

use crate::Dseq;
use pl_core::iupac::reverse_complement;

/// One fragment's end, as the single-stranded bases that will pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overhang {
    /// The single-stranded bases, 5'->3', as they read on the strand that
    /// carries them.
    pub bases: Vec<u8>,
    /// 5' overhangs are what every Type IIS enzyme used for assembly leaves;
    /// the flag exists because a 3' overhang cannot pair with a 5' one and a
    /// mixed set is a design error rather than a low-fidelity one.
    pub five_prime: bool,
}

impl Overhang {
    pub fn as_str(&self) -> String {
        String::from_utf8_lossy(&self.bases).to_string()
    }

    /// The overhang this one pairs with.
    pub fn partner(&self) -> Overhang {
        Overhang {
            bases: reverse_complement(&self.bases),
            five_prime: self.five_prime,
        }
    }

    /// Does this overhang ligate to itself?
    ///
    /// A palindromic overhang gives head-to-head dimers: the fragment joins to
    /// a copy of itself in the wrong orientation, and the assembly is a mixture
    /// rather than a construct.
    pub fn is_palindromic(&self) -> bool {
        !self.bases.is_empty() && reverse_complement(&self.bases) == self.bases
    }
}

/// A fault in an overhang set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The same overhang appears at two junctions, which can therefore swap.
    Repeated { overhang: String, times: usize },
    /// An overhang that ligates to itself.
    Palindromic { overhang: String },
    /// Two overhangs one substitution apart. T4 ligase joins these at a
    /// measurable rate, so the assembly has a minor wrong product.
    NearNeighbour { a: String, b: String, at: usize },
    /// An overhang that pairs with another's partner — the same collision as
    /// `Repeated`, seen from the other strand.
    CrossPairing { a: String, b: String },
    /// Overhangs of different lengths, or a mix of 5' and 3'.
    Incompatible { detail: String },
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fault::Repeated { overhang, times } => write!(
                f,
                "{overhang} appears at {times} junctions; they can swap, so the \
                 assembly has more than one answer"
            ),
            Fault::Palindromic { overhang } => write!(
                f,
                "{overhang} is its own reverse complement and will ligate to \
                 itself, giving head-to-head dimers"
            ),
            Fault::NearNeighbour { a, b, at } => write!(
                f,
                "{a} and {b} differ only at position {}; T4 ligase mis-joins \
                 pairs like this at a measurable rate",
                at + 1
            ),
            Fault::CrossPairing { a, b } => write!(
                f,
                "{a} pairs with the partner of {b}; those two junctions are \
                 interchangeable"
            ),
            Fault::Incompatible { detail } => write!(f, "{detail}"),
        }
    }
}

impl Fault {
    /// Will this stop the assembly, or only reduce its yield?
    ///
    /// A repeat or a palindrome produces a *different construct*. A near
    /// neighbour produces mostly the right one with a wrong minor product. They
    /// are not the same problem and must not be reported as the same severity.
    pub fn is_fatal(&self) -> bool {
        !matches!(self, Fault::NearNeighbour { .. })
    }
}

/// Everything wrong with a set of overhangs, worst first.
///
/// An empty result means **no structural fault was found**, which is not the
/// same as "this will work": ligation fidelity also depends on the measured
/// pairwise rates this does not ship. [`Report::caveat`] says so, and callers
/// are expected to print it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub faults: Vec<Fault>,
    pub overhangs: Vec<String>,
}

impl Report {
    pub fn is_usable(&self) -> bool {
        !self.faults.iter().any(Fault::is_fatal)
    }

    /// What this check cannot tell you. Print it with the result, always.
    pub fn caveat(&self) -> &'static str {
        "structural checks only: repeats, palindromes and single-mismatch \
         neighbours. Measured ligation-fidelity rates (Potapov et al. 2020) are \
         not shipped, so no fidelity percentage is claimed."
    }
}

/// Check a set of overhangs for the faults that can be found without data.
pub fn check(overhangs: &[Overhang]) -> Report {
    let mut faults = Vec::new();
    let names: Vec<String> = overhangs.iter().map(Overhang::as_str).collect();

    if overhangs.is_empty() {
        return Report {
            faults,
            overhangs: names,
        };
    }

    // Mixed geometry is a design error, not a fidelity problem, so it is
    // reported first and separately.
    let len = overhangs[0].bases.len();
    if overhangs.iter().any(|o| o.bases.len() != len) {
        faults.push(Fault::Incompatible {
            detail: "the overhangs are not all the same length; they cannot all pair".into(),
        });
    }
    let five = overhangs[0].five_prime;
    if overhangs.iter().any(|o| o.five_prime != five) {
        faults.push(Fault::Incompatible {
            detail: "a 5' overhang cannot pair with a 3' overhang".into(),
        });
    }

    for (i, o) in overhangs.iter().enumerate() {
        if o.is_palindromic() {
            faults.push(Fault::Palindromic {
                overhang: names[i].clone(),
            });
        }
    }

    // Repeats, counted once per distinct overhang rather than once per pair.
    let mut seen: Vec<(&String, usize)> = Vec::new();
    for n in &names {
        match seen.iter_mut().find(|(s, _)| *s == n) {
            Some((_, c)) => *c += 1,
            None => seen.push((n, 1)),
        }
    }
    for (n, c) in seen {
        if c > 1 {
            faults.push(Fault::Repeated {
                overhang: n.clone(),
                times: c,
            });
        }
    }

    // Pairwise faults are reported once per *pair of overhangs*, not once per
    // pair of positions. A set holding `AATG` twice would otherwise report the
    // same near neighbour against every copy, and a real design with three
    // repeats of one overhang would bury its own diagnosis.
    let mut pairs_done: Vec<(String, String)> = Vec::new();
    for i in 0..overhangs.len() {
        for j in (i + 1)..overhangs.len() {
            if names[i] == names[j] {
                continue; // already reported as a repeat
            }
            let key = if names[i] < names[j] {
                (names[i].clone(), names[j].clone())
            } else {
                (names[j].clone(), names[i].clone())
            };
            if pairs_done.contains(&key) {
                continue;
            }
            pairs_done.push(key);
            // One overhang pairing with another's partner is the same
            // collision as a repeat, seen from the other strand -- and it is
            // the one a designer reading a list of overhangs will not spot.
            if overhangs[i].bases == overhangs[j].partner().bases {
                faults.push(Fault::CrossPairing {
                    a: names[i].clone(),
                    b: names[j].clone(),
                });
                continue;
            }
            if overhangs[i].bases.len() == overhangs[j].bases.len() {
                let diffs: Vec<usize> = overhangs[i]
                    .bases
                    .iter()
                    .zip(&overhangs[j].bases)
                    .enumerate()
                    .filter(|(_, (a, b))| !a.eq_ignore_ascii_case(b))
                    .map(|(k, _)| k)
                    .collect();
                if diffs.len() == 1 {
                    faults.push(Fault::NearNeighbour {
                        a: names[i].clone(),
                        b: names[j].clone(),
                        at: diffs[0],
                    });
                }
            }
        }
    }

    // Fatal first, then by kind, so the reader sees what stops the reaction
    // before what merely taxes it.
    faults.sort_by_key(|f| (!f.is_fatal(), format!("{f:?}")));
    Report {
        faults,
        overhangs: names,
    }
}

/// The single-stranded end at the left of a fragment, if it has one.
///
/// A `Dseq` records its left overhang as `ovhg`: negative means the watson
/// strand starts later than the crick one, which is a 5' overhang on the left.
/// Blunt ends give `None` — not an empty overhang, because "this end cannot
/// direct an assembly" and "this end pairs with nothing" are different
/// statements and only one of them is a fault.
pub fn left_overhang(f: &Dseq) -> Option<Overhang> {
    let n = f.ovhg;
    if n == 0 {
        return None;
    }
    let k = n.unsigned_abs() as usize;
    let bases: Vec<u8> = if n < 0 {
        // Watson starts later than crick, so the crick strand carries the
        // single-stranded bases at this end -- and they are its *last* k, since
        // crick runs the other way.
        let c = f.crick.as_bytes();
        if c.len() < k {
            return None;
        }
        c[c.len() - k..].to_vec()
    } else {
        let w = f.watson.as_bytes();
        if w.len() < k {
            return None;
        }
        w[..k].to_vec()
    };
    Some(Overhang {
        bases,
        five_prime: n < 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oh(s: &str) -> Overhang {
        Overhang {
            bases: s.as_bytes().to_vec(),
            five_prime: true,
        }
    }

    #[test]
    fn a_clean_set_reports_nothing_and_still_carries_its_caveat() {
        let r = check(&[oh("AATG"), oh("GCTT"), oh("CAGG"), oh("TACA")]);
        assert!(r.faults.is_empty(), "{:?}", r.faults);
        assert!(r.is_usable());
        // An empty fault list is not a promise that the assembly works.
        assert!(r.caveat().contains("not shipped"));
        assert!(r.caveat().contains("no fidelity percentage"));
    }

    #[test]
    fn a_repeated_overhang_is_fatal_because_the_assembly_has_two_answers() {
        let r = check(&[oh("AATG"), oh("GCTT"), oh("AATG")]);
        assert!(!r.is_usable());
        let f = r
            .faults
            .iter()
            .find(|f| matches!(f, Fault::Repeated { .. }));
        assert!(f.is_some(), "{:?}", r.faults);
        assert!(f.unwrap().to_string().contains("more than one answer"));
        // Counted once for the overhang, not once per pair.
        assert_eq!(
            r.faults
                .iter()
                .filter(|f| matches!(f, Fault::Repeated { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn a_palindromic_overhang_ligates_to_itself() {
        // AATT reverse-complements to AATT.
        assert!(oh("AATT").is_palindromic());
        assert!(!oh("AATG").is_palindromic());
        let r = check(&[oh("AATT"), oh("GCTT")]);
        assert!(!r.is_usable());
        assert!(r
            .faults
            .iter()
            .any(|f| matches!(f, Fault::Palindromic { .. })));
    }

    #[test]
    fn overhangs_one_substitution_apart_are_flagged_but_not_fatal() {
        // AATG and AATC differ only at the last base.
        let r = check(&[oh("AATG"), oh("AATC")]);
        let nn: Vec<&Fault> = r
            .faults
            .iter()
            .filter(|f| matches!(f, Fault::NearNeighbour { .. }))
            .collect();
        assert_eq!(nn.len(), 1, "{:?}", r.faults);
        assert!(nn[0].to_string().contains("position 4"));
        // A wrong minor product, not a wrong construct.
        assert!(!nn[0].is_fatal());
        assert!(
            r.is_usable(),
            "a near neighbour lowers yield, it does not stop the reaction"
        );
    }

    #[test]
    fn an_overhang_that_pairs_with_anothers_partner_is_caught() {
        // AATG pairs with CATT. A set holding both has two interchangeable
        // junctions -- the collision a designer reading the list will not see,
        // because the two strings look nothing alike.
        assert_eq!(oh("AATG").partner().as_str(), "CATT");
        let r = check(&[oh("AATG"), oh("CATT"), oh("GGAG")]);
        assert!(!r.is_usable());
        assert!(
            r.faults
                .iter()
                .any(|f| matches!(f, Fault::CrossPairing { .. })),
            "{:?}",
            r.faults
        );
    }

    #[test]
    fn mixed_geometry_is_a_design_error_not_a_fidelity_one() {
        let r = check(&[oh("AATG"), oh("GCT")]);
        assert!(r
            .faults
            .iter()
            .any(|f| matches!(f, Fault::Incompatible { .. })));

        let mut three = oh("GCTT");
        three.five_prime = false;
        let r = check(&[oh("AATG"), three]);
        let f = r
            .faults
            .iter()
            .find(|f| matches!(f, Fault::Incompatible { .. }))
            .expect("mixed ends");
        assert!(f.to_string().contains("3' overhang"), "{f}");
    }

    #[test]
    fn faults_that_stop_the_reaction_are_listed_before_those_that_tax_it() {
        let r = check(&[oh("AATG"), oh("AATC"), oh("AATT")]);
        assert!(r.faults.len() >= 2);
        let first_non_fatal = r.faults.iter().position(|f| !f.is_fatal());
        let last_fatal = r.faults.iter().rposition(Fault::is_fatal);
        if let (Some(fn_), Some(lf)) = (first_non_fatal, last_fatal) {
            assert!(lf < fn_, "a near neighbour is listed above a palindrome");
        }
    }

    #[test]
    fn a_repeated_overhang_does_not_multiply_every_other_report() {
        // Three copies of one overhang would otherwise report the same near
        // neighbour three times over and bury the diagnosis in its own noise.
        let r = check(&[oh("AATG"), oh("AATG"), oh("AATG"), oh("AATC")]);
        let nn = r
            .faults
            .iter()
            .filter(|f| matches!(f, Fault::NearNeighbour { .. }))
            .count();
        assert_eq!(nn, 1, "{:?}", r.faults);
        let rep = r
            .faults
            .iter()
            .filter(|f| matches!(f, Fault::Repeated { .. }))
            .count();
        assert_eq!(rep, 1, "one repeat fault, naming the count");
    }

    #[test]
    fn the_empty_set_is_not_an_error() {
        let r = check(&[]);
        assert!(r.faults.is_empty());
        assert!(r.is_usable());
        assert!(r.overhangs.is_empty());
    }
}
