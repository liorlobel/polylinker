//! Hairpins and dimers, as perfect ungapped helices.
//!
//! # This is a screen, not a fold
//!
//! It enumerates every **maximal run of consecutive Watson-Crick pairs** and
//! scores it as a stack sum with [`pl_thermo::dg37_stacks`]. That is all it
//! does. It is not a thermodynamic alignment and it is not a Zuker DP.
//! `docs/PLAN.md` §7.4 schedules a port of seqfold (Lattice Automation, MIT)
//! that would replace this; **that port has not happened, seqfold has not been
//! read, and nothing here derives from it.** Saying so is the point of the
//! provenance table in the crate doc.
//!
//! What is deliberately absent, each of which removes stabilisation:
//!
//! 1. **No internal loops, no bulges.** A stem interrupted by a single
//!    mismatch is scored as its longer half alone. A real 12 bp stem with one
//!    1×1 internal loop is reported as, say, a 7 bp stem. This is the largest
//!    single error source.
//! 2. **No dangling ends, no terminal mismatches.**
//! 3. **No coaxial stacking, no multibranch loops, no branched structures.**
//!    One helix per structure. G-quadruplexes are invisible entirely, which is
//!    why `Constraints::MAX_POLY_G` exists as a separate criterion.
//! 4. **No mismatch nearest-neighbour parameters.** The Allawi & SantaLucia
//!    mismatch series is not implemented.
//! 5. **No hairpin-loop initiation term.** That table exists only in the 2004
//!    review and is not transcribed here; writing recalled numbers into a
//!    thermodynamic model is the failure this project's own record calls its
//!    most unreliable habit. The consequence is stated below.
//! 6. **No salt correction.** The stacks are 1 M Na⁺ numbers and are reported
//!    as such.
//!
//! # The bias, and which way it runs
//!
//! Omissions 1-4 all *remove* stabilisation: a structure this reports at
//! −4 kcal/mol can genuinely be −8. Omission 5 runs the other way, and by more:
//! a loop's initiation term is positive and of the same order, so leaving it
//! out makes every hairpin look **more** stable than it is.
//!
//! So the hairpin number is conservative — it over-reports hairpins rather than
//! missing them — and the dimer numbers are not. Two consequences, both acted
//! on:
//!
//! - The dimer thresholds are set tighter than a full model would need, and
//!   `Constraints::DG_DIMER_THREE_PRIME`'s doc says that is why.
//! - Every ΔG is reported as an **inequality** in the honest direction, and
//!   [`Structure::render`] writes it that way rather than as an equality.
//! - [`SCREEN_NOTE`] says the same thing in a sentence, and the report prints
//!   it beside the numbers rather than leaving it to `pl methods design`.
//!
//! Both directions are stated rather than one, because a reader who is told
//! only "conservative" will believe a clean report more than they should.

use pl_thermo::{dg37_stacks, NnTable};

/// The sentence that travels with every number this module produces.
///
/// It lives here, next to the model, so the report and the methods text cannot
/// drift from each other or from the code — `pl-doc`'s module doc names that
/// failure: prose restated in two places drifts, and the drift is invisible
/// because the sentence still reads correctly and is no longer true.
pub const SCREEN_NOTE: &str = "\
hairpin and dimer free energies are a perfect-helix SCREEN, not a fold: only ungapped runs \
of Watson-Crick pairs are found, so a stem broken by one mismatch is scored as its longer \
half and a real structure can be several kcal/mol more stable than the number shown. That \
is why each is printed as >=. Internal loops, bulges, dangling ends, terminal mismatches, \
coaxial stacking and G-quadruplexes are not modelled.";

/// The most stable helix found, and where.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Structure {
    /// kcal/mol, ≤ 0. Zero means no helix of two or more pairs exists.
    pub dg: f64,
    /// Base pairs in the helix.
    pub pairs: usize,
}

impl Structure {
    pub const NONE: Structure = Structure { dg: 0.0, pairs: 0 };

    /// The number with the inequality that makes it honest.
    ///
    /// `>=`, never a bare number. The module doc has promised this since the
    /// file was written and the body printed an equality anyway, which is the
    /// one overclaim this module exists to prevent: a reviewer measured
    /// `ATTATTATTATTATTGCGAGCG` against `ATTATTATTATTATTCGCACGC`, whose 3' ends
    /// form 3 bp + a 1×1 internal loop + 3 bp. This screen sees one of the two
    /// helices and reports −4.40 kcal/mol, which **passes** the −6.0 gate; the
    /// two helices stack to −8.79. Printed as `-4.4` that is read as the
    /// answer. Printed as `>= -4.4` it is read as what it is.
    pub fn render(&self) -> String {
        if self.pairs == 0 {
            // No number, so no inequality to hang on one: the screen found no
            // helix of two or more pairs. A real structure may still exist —
            // that is what SCREEN_NOTE says, once, where the report shows it.
            "none found".to_string()
        } else {
            format!(">= {:.1} ({} bp helix)", self.dg, self.pairs)
        }
    }
}

fn complements(a: u8, b: u8) -> bool {
    matches!(
        (a.to_ascii_uppercase(), b.to_ascii_uppercase()),
        (b'A', b'T') | (b'T', b'A') | (b'G', b'C') | (b'C', b'G')
    )
}

/// The shortest loop a helix can close.
///
/// Three unpaired nucleotides is the standard steric constraint — a helix
/// cannot be closed by fewer — and it is where every published hairpin-loop
/// table starts.
pub const MIN_LOOP: usize = 3;

/// The most stable hairpin `seq` can fold into.
///
/// For every closing pair `(i, j)` with a loop of at least [`MIN_LOOP`], the
/// stem is extended outward while the bases pair, and the whole stem is scored.
/// Every maximal helix has exactly one innermost pair, so this enumerates each
/// once. O(n²·stem), which for a primer is nothing.
///
/// The returned ΔG is the **stem only**; see the module doc for what that
/// leaves out and which way it errs.
pub fn hairpin(seq: &[u8], t: &NnTable) -> Structure {
    let n = seq.len();
    let mut best = Structure::NONE;
    for i in 0..n {
        for j in (i + MIN_LOOP + 1)..n {
            if !complements(seq[i], seq[j]) {
                continue;
            }
            let (mut a, mut b, mut stem) = (i, j, 1usize);
            while a > 0 && b + 1 < n && complements(seq[a - 1], seq[b + 1]) {
                a -= 1;
                b += 1;
                stem += 1;
            }
            if stem < 2 {
                continue;
            }
            // The stem's two strands are `seq[a..a+stem]` and its partner; a
            // perfect duplex reads the same either way, so one side is enough.
            let Ok(dg) = dg37_stacks(&seq[a..a + stem], t) else {
                continue; // an ambiguity code; the oligo gate rejects those first
            };
            if dg < best.dg {
                best = Structure { dg, pairs: stem };
            }
        }
    }
    best
}

/// The most stable duplex between two oligos, over every register.
///
/// Returns `(anywhere, at a 3' end)`.
///
/// The split matters mechanically and is not cosmetic. Only a 3' end can be
/// extended, so only a helix containing one is amplified into a primer-dimer
/// band that competes for polymerase and dNTPs through every subsequent cycle.
/// A helix in the middle sequesters primer and does nothing else. So the 3'-end
/// number is gated hard and the other is weighted softly.
///
/// `a` and `b` are both 5'→3'. Pass `b == a` for a self-dimer.
pub fn dimer(a: &[u8], b: &[u8], t: &NnTable) -> (Structure, Structure) {
    // Two antiparallel strands, so position `k` of `a` faces position
    // `nb - 1 - (k - off)` of `b`, where `off` slides one past the other. Two
    // consequences used below:
    //
    //   * `a`'s 3' terminus is `k == na - 1`;
    //   * `b`'s 3' terminus is `nb - 1 - (k - off) == nb - 1`, i.e. `k == off`.
    //
    // Writing the second as "the run contains `off`" is what makes the 3'-end
    // test one comparison rather than a reconstruction.
    let (na, nb) = (a.len(), b.len());
    if na < 2 || nb < 2 {
        return (Structure::NONE, Structure::NONE);
    }
    let mut any = Structure::NONE;
    let mut three = Structure::NONE;

    for off in -(nb as i64 - 1)..(na as i64) {
        let lo = off.max(0);
        let hi = (off + nb as i64 - 1).min(na as i64 - 1);
        if hi < lo {
            continue;
        }
        let mut run: Option<(usize, usize)> = None;
        // One past the end, so a run reaching the last aligned column is closed
        // by the same code as every other run rather than by a copy of it.
        for k in lo..=hi + 1 {
            let paired = k <= hi && {
                let bj = (nb as i64 - 1 - (k - off)) as usize;
                complements(a[k as usize], b[bj])
            };
            if paired {
                let k = k as usize;
                run = Some(match run {
                    Some((s, _)) => (s, k),
                    None => (k, k),
                });
                continue;
            }
            let Some((s, e)) = run.take() else { continue };
            if e - s + 1 < 2 {
                continue;
            }
            let Ok(dg) = dg37_stacks(&a[s..=e], t) else {
                continue;
            };
            let pairs = e - s + 1;
            if dg < any.dg {
                any = Structure { dg, pairs };
            }
            let touches_three = e == na - 1 || (s as i64 <= off && off <= e as i64);
            if touches_three && dg < three.dg {
                three = Structure { dg, pairs };
            }
        }
    }
    (any, three)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_thermo::SANTALUCIA_2004;

    fn t() -> NnTable {
        SANTALUCIA_2004
    }

    #[test]
    fn a_designed_hairpin_is_found_and_a_flat_oligo_is_not() {
        // Stem GGGGCC / GGCCCC with a 4 nt loop.
        let h = hairpin(b"GGGGCCAAAAGGCCCC", &t());
        assert!(h.pairs >= 6, "{h:?}");
        assert!(h.dg < -5.0, "a 6 bp GC stem must be very stable: {h:?}");

        // A run of one base cannot pair with itself.
        assert_eq!(hairpin(b"AAAAAAAAAAAAAAAAAAAA", &t()), Structure::NONE);
    }

    /// PROVEN TO FAIL: with the minimum loop dropped from 3 to 1, the
    /// two-base-loop fixture folds into the same 4 bp stem as the control and
    /// the comparison fires.
    #[test]
    fn a_loop_shorter_than_three_bases_is_not_a_hairpin() {
        // Steric, and the same constraint every published loop table starts at.
        // GCGC + 2 nt + GCGC would be a 4 bp stem on a 2 nt loop; it must not
        // be reported. The control below, with one more loop base, must be.
        let too_tight = hairpin(b"GCGCAAGCGC", &t());
        let ok = hairpin(b"GCGCAAAGCGC", &t());
        assert!(
            ok.pairs > too_tight.pairs,
            "a 3 nt loop closes and a 2 nt loop does not: {ok:?} vs {too_tight:?}"
        );
    }

    /// PROVEN TO FAIL: with `touches_three` forced true, an oligo whose
    /// complementary stretch sits in the MIDDLE reports the same -10.28
    /// kcal/mol for both numbers, and the split that decides whether a
    /// primer-dimer is amplified stops meaning anything.
    #[test]
    fn a_three_prime_dimer_is_told_apart_from_one_in_the_middle() {
        // The whole reason there are two numbers. Only a 3' end is extended, so
        // only a 3'-end dimer becomes a primer-dimer band.
        //
        // Two oligos whose 3' ends are exactly complementary.
        let a = b"AAAAAAAAAAAAGGCGCC";
        let bb = b"AAAAAAAAAAAAGGCGCC"; // its own 3' end is self-complementary
        let (any, three) = dimer(a, bb, &t());
        assert!(three.dg < -3.0, "3'-end dimer must be found: {three:?}");
        assert!(any.dg <= three.dg);

        // And an oligo whose complementary stretch is in the middle reports a
        // stable `any` with a much weaker `three`.
        let c = b"AAGGCGCCAAAAAAAAAA";
        let (any_c, three_c) = dimer(c, c, &t());
        assert!(any_c.dg < -3.0, "{any_c:?}");
        assert!(
            three_c.dg > any_c.dg + 2.0,
            "the 3' end is not what pairs here: any {any_c:?} three {three_c:?}"
        );
    }

    #[test]
    fn a_self_dimer_of_a_palindrome_is_the_whole_oligo() {
        // EcoRI's site repeated: perfectly self-complementary, so the most
        // stable register is the full-length one.
        let seq = b"GAATTCGAATTC";
        let (any, three) = dimer(seq, seq, &t());
        assert_eq!(any.pairs, seq.len(), "{any:?}");
        assert_eq!(three.pairs, seq.len(), "{three:?}");
    }

    /// PROVEN TO FAIL: against the shipped `render` — which formatted
    /// `"{:.1} ({} bp helix)"` — the first assertion fires with
    /// `left: "-8.8 (6 bp helix)"`. The module doc promised the inequality from
    /// the day the file was written; the body printed an equality, and a
    /// reviewer had to measure a gate-crossing case to find it.
    #[test]
    fn a_screened_helix_is_rendered_as_an_inequality_and_never_as_a_number() {
        let h = hairpin(b"GGGGCCAAAAGGCCCC", &t());
        let s = h.render();
        assert!(
            s.starts_with(">= -"),
            "a screen result must not read as a measurement: {s}"
        );

        // The measured case the inequality exists for. These two 3' ends form
        // 3 bp + a 1x1 internal loop + 3 bp; the screen sees one helix, reports
        // about -4.4, and PASSES the -6.0 gate, while the two helices stack to
        // about -8.8. The number is not wrong, the equals sign was.
        let (_, three) = dimer(b"ATTATTATTATTATTGCGAGCG", b"ATTATTATTATTATTCGCACGC", &t());
        assert!(
            three.dg > -6.0,
            "the premise: this pair passes the shipped gate ({three:?})"
        );
        assert!(three.render().starts_with(">= -"), "{}", three.render());

        // And the empty case says it found nothing rather than claiming zero.
        assert_eq!(Structure::NONE.render(), "none found");

        // The sentence that has to reach the reader alongside it.
        assert!(SCREEN_NOTE.contains("longer half"), "{SCREEN_NOTE}");
    }

    #[test]
    fn nothing_complementary_scores_zero_rather_than_a_small_number() {
        let (any, three) = dimer(b"AAAAAAAAAA", b"AAAAAAAAAA", &t());
        assert_eq!(any, Structure::NONE);
        assert_eq!(three, Structure::NONE);
    }
}
