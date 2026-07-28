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
        if o.bases.is_empty() {
            // An end that reports an overhang with no bases in it is not blunt
            // -- a blunt end is `None` and never reaches here. It is an end
            // whose designed overhang could not form because the enzyme's
            // second-strand nick fell outside the fragment, and that is a fatal
            // Golden Gate design fault (too few spacer bases past the Type IIS
            // site). It has to be said out loud: dropping the end instead let
            // `pl goldengate --enzyme BsaI` print "no structural fault found"
            // and `"usable": true` for a digest with no usable junction at all.
            faults.push(Fault::Incompatible {
                detail: "an end reports an overhang with no bases: the enzyme's second-strand \
                         nick fell outside the fragment, so that junction cannot form"
                    .into(),
            });
        }
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
    //
    // Collapsing to one index per distinct overhang *before* the loop is also
    // what bounds the work, and that is not a micro-optimisation. A four-base
    // overhang has 256 possible values, so the answer can never hold more than
    // C(256,2) = 32,640 distinct pairs however many fragments the digest
    // produced -- but the loop used to walk all n^2 positions and screen each
    // against a linear scan of a `Vec` of the pairs already done. Measured on
    // the release binary that was 3.4 s at 500 overhangs, 16.3 s at 1,000,
    // 67.2 s at 2,000 and 276.5 s at 4,000, and `pl goldengate --enzyme BsaI`
    // on a 10 Mb record (4,991 fragments) took 137.7 s. Nothing caps the
    // fragment count and nothing refuses, unlike `assembly::Options::
    // max_fragments`, so a genome-sized record simply stalled. Deduplicating
    // first makes the loop O(D^2) with D bounded by the alphabet, and reports
    // exactly the same faults: pairs of equal name were skipped anyway.
    let mut distinct: std::collections::BTreeSet<&str> = Default::default();
    let mut uniq: Vec<usize> = Vec::new();
    for (i, name) in names.iter().enumerate() {
        if distinct.insert(name.as_str()) {
            uniq.push(i);
        }
    }
    for (rank, &i) in uniq.iter().enumerate() {
        for &j in &uniq[rank + 1..] {
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
/// A `Dseq` records its left overhang as `ovhg`, and the sign says which strand
/// protrudes: **negative means watson protrudes**, which is a 5' overhang on the
/// top strand, and positive means crick does, which is a 3' one. That is the
/// convention the diagram at `lib.rs` 15-17 draws, the doc at `lib.rs` 42-44
/// states, and [`Dseq::left_end`] implements.
///
/// This function used to read the *other* strand in both branches, and the
/// header above it said so ("negative means the watson strand starts later than
/// the crick one", which is false). That is not an off-by-one; it is a different
/// four bases. On the crate's own pydna fixture
/// `{watson:"GATCCTTTT", crick:"AAAAG", ovhg:-4}` it returned `AAAG` -- the
/// reverse complement of the four *duplex* bases sitting four nt inside the
/// fragment -- where the BamHI overhang is `GATC`. Every junction that
/// `pl goldengate --enzyme BsaI <file>` reported was therefore the wrong four
/// bases, and the whole [`check`] report was computed over strings that are not
/// overhangs: a fatal design passed as clean, and unrelated fragments that
/// happened to share those interior bases were reported as `Repeated`.
///
/// Blunt ends give `None` — not an empty overhang, because "this end cannot
/// direct an assembly" and "this end pairs with nothing" are different
/// statements and only one of them is a fault. `None` now means blunt and
/// nothing else. An end whose carrying strand is shorter than `|ovhg|` — the
/// enzyme's second-strand nick fell outside the fragment, so the designed
/// overhang never forms — returns the single-stranded bases that are really
/// there, which is shorter than every other overhang in the set and so reaches
/// the reader as a fatal [`Fault::Incompatible`]. It used to return `None` and
/// vanish: the sole caller is a bare `if let Some(o) = … { push }`, so the
/// junction was deleted from the set and the CLI printed "no structural fault
/// found" with `"usable": true` over what was left.
pub fn left_overhang(f: &Dseq) -> Option<Overhang> {
    let n = f.ovhg;
    if n == 0 {
        return None;
    }
    let k = n.unsigned_abs() as usize;
    let bases: Vec<u8> = if n < 0 {
        // Watson protrudes on the left, so watson carries the single-stranded
        // bases -- and they are its *first* k, because watson runs left to
        // right and this is its 5' end.
        let w = f.watson.as_bytes();
        w[..k.min(w.len())].to_vec()
    } else {
        // Crick protrudes on the left, which is crick's 3' side, so the bases
        // are its *last* k: crick runs the other way.
        let c = f.crick.as_bytes();
        c[c.len() - k.min(c.len())..].to_vec()
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
    fn a_left_overhang_is_read_off_the_strand_that_actually_protrudes() {
        // The crate's own pydna-oracle fixture (`lib.rs`
        // `cutting_matches_the_pydna_reference_shape`): the second BamHI
        // fragment of `AAAAGGATCCTTTT` is
        // `{watson:"GATCCTTTT", crick:"AAAAG", ovhg:-4}` and the overhang is
        // `GATC`. Reading it off crick instead gave `AAAG` -- the reverse
        // complement of the four duplex bases four nt inside the fragment --
        // so every junction in a `pl goldengate --enzyme BsaI` report was the
        // wrong four bases and every fault was computed over strings that are
        // not overhangs.
        let f = Dseq::from_parts("GATCCTTTT", "AAAAG", -4, false);
        let o = left_overhang(&f).expect("a -4 end is not blunt");
        assert_eq!(o.as_str(), "GATC");
        assert!(o.five_prime, "a watson protrusion on the left is 5'");

        // A 3' end reads off crick, and the two branches must not simply have
        // been swapped: `PstI` on `AAAACTGCAGTTTT` leaves `TGCA` on crick.
        let frags = crate::cut(
            &Dseq::new("AAAACTGCAGTTTT", false),
            pl_enzymes::by_name("PstI").unwrap(),
        );
        let three = left_overhang(&frags[1]).expect("a PstI end is not blunt");
        assert_eq!(three.as_str(), "TGCA");
        assert!(!three.five_prime, "PstI leaves a 3' overhang");
    }

    #[test]
    fn a_left_overhang_says_the_same_thing_as_the_end_it_describes() {
        // The control, and the reason the bug survived: `Dseq::left_end` was
        // right all along, so the two functions were strand-swapped mirrors of
        // each other. Nothing pins them together but this.
        for (seq, enzyme) in [
            ("AAAAGGATCCTTTTGGATCCGGGGCCCC", "BamHI"),
            ("TTTTGAATTCAAAAGAATTCCCCCGGGG", "EcoRI"),
            ("AAAACTGCAGTTTTCTGCAGGGGGCCCC", "PstI"),
            ("AAAAGATATCTTTTGATATCGGGGCCCC", "EcoRV"), // blunt: None
        ] {
            let frags = crate::cut(&Dseq::new(seq, true), pl_enzymes::by_name(enzyme).unwrap());
            for f in &frags {
                match (left_overhang(f), f.left_end()) {
                    (None, crate::End::Blunt) => {}
                    (
                        Some(o),
                        crate::End::Overhang {
                            five_prime,
                            ref bases,
                        },
                    ) => {
                        assert_eq!(o.as_str(), *bases, "{enzyme} {f:?}");
                        assert_eq!(o.five_prime, five_prime, "{enzyme} {f:?}");
                    }
                    (a, b) => panic!("{enzyme}: {a:?} disagrees with {b:?} for {f:?}"),
                }
            }
        }
    }

    #[test]
    fn an_end_whose_overhang_cannot_form_is_reported_rather_than_dropped() {
        // The enzyme's second-strand nick fell outside the fragment, so the
        // designed overhang never forms. That is a fatal design fault -- too
        // few spacer bases past the Type IIS site -- and it used to return the
        // same `None` that means "blunt", so the sole caller's bare
        // `if let Some(o) = ... { push }` deleted the junction from the set and
        // the CLI printed "no structural fault found" over what was left.
        let starved = Dseq::from_parts("", "", -4, false);
        let o = left_overhang(&starved).expect("an unformed end is not a blunt end");
        assert!(o.bases.is_empty());

        let r = check(&[oh("AATG"), o]);
        assert!(
            !r.is_usable(),
            "an unformed junction is fatal: {:?}",
            r.faults
        );
        let f = r
            .faults
            .iter()
            .find(|f| matches!(f, Fault::Incompatible { .. }))
            .expect("the unformed end must be named, not swallowed");
        assert!(f.to_string().contains("cannot form"), "{f}");

        // A partly-formed end keeps the bases that really are single-stranded,
        // and its length disagreeing with the rest of the set is itself the
        // fatal report.
        let short = Dseq::from_parts("C", "", -4, false);
        let p = left_overhang(&short).expect("not blunt");
        assert_eq!(p.as_str(), "C");
        assert!(!check(&[oh("AATG"), p]).is_usable());
    }

    #[test]
    fn checking_a_genome_sized_overhang_set_finishes_in_bounded_time() {
        // A 1 Mb record gives ~500 BsaI fragments and a 10 Mb one ~5,000, and
        // nothing caps the input, so this loop is the whole cost of
        // `pl goldengate --enzyme BsaI`. Screening every one of the n^2
        // position pairs against a linear scan of a `Vec` of the pairs already
        // done measured 3.4 s at 500 overhangs and 276.5 s at 4,000 on the
        // *release* binary -- for a result that can hold at most
        // C(256,2) = 32,640 distinct pairs. There are only 256 four-base
        // overhangs, so the work is bounded by the alphabet, not by the digest.
        let alphabet: Vec<String> = (0..256usize)
            .map(|v| {
                (0..4)
                    .map(|p| b"ACGT"[(v >> (2 * p)) & 3] as char)
                    .collect()
            })
            .collect();
        let set: Vec<Overhang> = (0..600).map(|i| oh(&alphabet[i % 256])).collect();

        let t0 = std::time::Instant::now();
        let r = check(&set);
        let dt = t0.elapsed();
        assert!(
            dt < std::time::Duration::from_secs(5),
            "check() took {dt:?} for 600 overhangs drawn from 256 values"
        );
        // ...and it is still the same answer: every value repeats, so every
        // one of them is a repeat.
        assert!(!r.is_usable());
        assert_eq!(
            r.faults
                .iter()
                .filter(|f| matches!(f, Fault::Repeated { .. }))
                .count(),
            256
        );
    }

    #[test]
    fn the_empty_set_is_not_an_error() {
        let r = check(&[]);
        assert!(r.faults.is_empty());
        assert!(r.is_usable());
        assert!(r.overhangs.is_empty());
    }
}
