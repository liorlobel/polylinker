//! Enumerating candidates, and the gate each one has to pass on its own.
//!
//! A candidate is a **span and a side**, not a copied sequence. That keeps
//! enumeration cheap, and — the part that matters — it makes the intended
//! binding site a *known coordinate* rather than something to be inferred
//! afterwards. [`crate::specificity::scan`] depends on it: on a molecule with a
//! repeat, the site `find_bindings` reports first need not be the one
//! enumeration meant.
//!
//! Coordinates here are **0-based and unrolled**: on a circle a span may run
//! past `n`, or start before 0, and is reduced modulo `n` only when it is
//! reported. Doing the arithmetic in one frame and the reduction in one place
//! is what stops an origin-crossing amplicon coming out as its complement arc.

use crate::fold;
use crate::params::Constraints;
use crate::report::Reason;
use pl_core::iupac::reverse_complement;
use pl_thermo::{dg37_stacks, tm};

/// Which primer of the pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Fwd,
    Rev,
}

impl Side {
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Fwd => "+",
            Side::Rev => "-",
        }
    }
}

/// One enumerated oligo, with everything the gate and the score need.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub side: Side,
    /// Unrolled 0-based plus-strand span of the **footprint**, `lo <= hi`.
    ///
    /// For [`Side::Fwd`] the 5' end is `lo` and the 3' end is `hi`; for
    /// [`Side::Rev`] it is the other way round. The tail has no coordinates on
    /// this molecule and never appears here.
    pub lo: i64,
    pub hi: i64,
    /// The footprint, 5'→3' as the primer reads.
    pub bases: Vec<u8>,
    pub tm: f64,
    pub gc: f64,
    /// ΔG°37 of the terminal pentamer's stacks (Rychlik).
    pub dg_three_prime: f64,
    pub hairpin: fold::Structure,
    /// The most stable self-dimer helix that includes the 3'-terminal base.
    ///
    /// There is deliberately no `self_dimer_any` beside it. `fold::dimer`
    /// returns both, and this struct carried both until a reviewer found that
    /// the "any" number reached nothing a user ever saw — not the report, not
    /// the JSON, not the pair ranking — and was read in exactly one place, the
    /// search bound's ordering, where it made the cut disagree with the
    /// ranking. A field whose only consumer is a criterion nothing else applies
    /// is worse than absent, so it is absent.
    pub self_dimer_three: fold::Structure,
    /// G/C among the last five bases.
    pub clamp: usize,
}

impl Candidate {
    pub fn len(&self) -> usize {
        self.bases.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bases.is_empty()
    }
    /// The 3'-terminal base's unrolled coordinate — the end a polymerase
    /// extends from, and the one the diversity rule separates on.
    pub fn three_prime(&self) -> i64 {
        match self.side {
            Side::Fwd => self.hi,
            Side::Rev => self.lo,
        }
    }
    /// 1-based plus-strand start on an `n` bp molecule.
    pub fn start(&self, n: u64) -> u64 {
        (self.lo.rem_euclid(n as i64)) as u64 + 1
    }
    pub fn end(&self, n: u64) -> u64 {
        (self.hi.rem_euclid(n as i64)) as u64 + 1
    }
}

/// The footprint bases for a span, or `None` if it falls off a linear end.
pub(crate) fn span_bases(template: &[u8], circular: bool, lo: i64, hi: i64) -> Option<Vec<u8>> {
    let n = template.len() as i64;
    if !circular && (lo < 0 || hi >= n) {
        return None;
    }
    if hi - lo + 1 > n {
        return None;
    }
    Some(
        (lo..=hi)
            .map(|i| template[i.rem_euclid(n) as usize].to_ascii_uppercase())
            .collect(),
    )
}

/// Longest run of one base, and longest run of G specifically.
fn runs(seq: &[u8]) -> (usize, usize) {
    let (mut best, mut best_g, mut cur) = (0usize, 0usize, 0usize);
    let mut prev = 0u8;
    for &b in seq {
        cur = if b == prev { cur + 1 } else { 1 };
        prev = b;
        best = best.max(cur);
        if b == b'G' {
            best_g = best_g.max(cur);
        }
    }
    (best, best_g)
}

/// Longest tandem dinucleotide repeat, in units. `X == Y` is excluded — that
/// is the homopolymer rule, and counting it twice would reject on one fact
/// under two names.
fn dinuc_units(seq: &[u8]) -> usize {
    let mut best = 0usize;
    for i in 0..seq.len().saturating_sub(1) {
        if seq[i] == seq[i + 1] {
            continue;
        }
        let mut k = 0usize;
        while i + 2 * k + 1 < seq.len()
            && seq[i + 2 * k] == seq[i]
            && seq[i + 2 * k + 1] == seq[i + 1]
        {
            k += 1;
        }
        best = best.max(k);
    }
    best
}

/// Build a candidate, or say which gate refused it.
///
/// Order is fixed, and it is the order the attrition table prints in. Ambiguity
/// is first because it decides what the other criteria are allowed to assume:
/// a candidate spanning an `N` has no Tm, no ΔG and no meaningful specificity
/// check, and scoring it on the unambiguous remainder is the substitution
/// `pl_thermo::tm` already refuses to make.
#[allow(clippy::result_large_err)]
pub(crate) fn evaluate(
    template: &[u8],
    circular: bool,
    side: Side,
    lo: i64,
    hi: i64,
    c: &Constraints,
) -> Result<Candidate, Reason> {
    let plus = span_bases(template, circular, lo, hi).ok_or(Reason::OffTheEnd)?;
    if plus.iter().any(|b| !matches!(b, b'A' | b'C' | b'G' | b'T')) {
        return Err(Reason::Ambiguous);
    }
    let bases = match side {
        Side::Fwd => plus,
        Side::Rev => reverse_complement(&plus),
    };

    let t = tm(&bases, &c.tm_method).map_err(|_| Reason::Tm)?;
    if !(c.tm_min..=c.tm_max).contains(&t.tm) {
        return Err(Reason::Tm);
    }
    if c.gc_hard && !(c.gc_min..=c.gc_max).contains(&t.gc_percent) {
        return Err(Reason::Gc);
    }

    let (run, run_g) = runs(&bases);
    if run > c.max_poly || run_g > c.max_poly_g {
        return Err(Reason::Run);
    }
    if dinuc_units(&bases) > c.max_dinuc_repeat {
        return Err(Reason::DinucRepeat);
    }

    // Rychlik: the terminal pentamer as bare stacks. Five bases, because that
    // is what the criterion is defined over; a shorter oligo cannot reach here
    // because `len_min` is 18.
    let pent = &bases[bases.len() - 5..];
    let dg_three_prime =
        dg37_stacks(pent, &Constraints::DG_TABLE).map_err(|_| Reason::Ambiguous)?;
    if dg_three_prime <= c.dg_three_prime {
        return Err(Reason::ThreePrimeStability);
    }

    let hairpin = fold::hairpin(&bases, &Constraints::DG_TABLE);
    if hairpin.dg <= c.dg_hairpin {
        return Err(Reason::Hairpin);
    }
    let (_any, self_dimer_three) = fold::dimer(&bases, &bases, &Constraints::DG_TABLE);
    if self_dimer_three.dg <= c.dg_dimer_three_prime {
        return Err(Reason::SelfDimer);
    }

    let clamp = bases[bases.len() - 5..]
        .iter()
        .filter(|b| matches!(b, b'G' | b'C'))
        .count();

    Ok(Candidate {
        side,
        lo,
        hi,
        tm: t.tm,
        gc: t.gc_percent,
        dg_three_prime,
        hairpin,
        self_dimer_three,
        clamp,
        bases,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_and_repeats_are_counted_the_way_the_thresholds_assume() {
        assert_eq!(runs(b"ACGTACGT"), (1, 1));
        assert_eq!(runs(b"AAAACGT"), (4, 1));
        assert_eq!(runs(b"ACGGGGT"), (4, 4));
        // A dinucleotide repeat is units, not bases: (AT)4 is 8 bases.
        assert_eq!(dinuc_units(b"ATATATATCG"), 4);
        assert_eq!(dinuc_units(b"CACACACACACG"), 5);
        // A homopolymer is not a dinucleotide repeat; counting it as both would
        // reject one fact under two names.
        assert_eq!(dinuc_units(b"AAAAAAAA"), 0);
    }

    #[test]
    fn a_span_that_falls_off_a_linear_end_is_refused_and_a_circle_wraps() {
        let t = b"ACGTACGTAC";
        assert!(span_bases(t, false, -1, 3).is_none());
        assert!(span_bases(t, false, 6, 10).is_none());
        assert_eq!(span_bases(t, false, 0, 3).unwrap(), b"ACGT".to_vec());
        // On a circle, -1 is the last base.
        assert_eq!(span_bases(t, true, -1, 1).unwrap(), b"CAC".to_vec());
        assert_eq!(span_bases(t, true, 8, 10).unwrap(), b"ACA".to_vec());
        // Never more than one turn.
        assert!(span_bases(t, true, 0, 10).is_none());
    }
}
