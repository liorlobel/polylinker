//! Homology-overlap assembly — the Gibson/HiFi class.
//!
//! `docs/PLAN.md` §7.9. Fragments that share terminal homology are joined at
//! that homology; a set of fragments whose overlaps close a cycle gives a
//! circular product, and one that does not gives a linear one.
//!
//! # Why terminal, and why that is the whole design
//!
//! Gibson, HiFi, In-Fusion and CPEC all work by chewing back or annealing at
//! the *ends* of fragments. A shared 30 bp run in the middle of two fragments
//! is not an assembly junction — it is a repeat, and treating it as a junction
//! is how an assembler produces a confident product that no reaction can make.
//! So an overlap only counts when it is a suffix of one fragment and a prefix
//! of the next.
//!
//! # Orientation
//!
//! Every fragment may be used in either orientation, because nothing about a
//! PCR product or a gel-purified band tells you which strand the tube contains.
//! Products are deduplicated by `cdseguid`, which is invariant to both rotation
//! and strand — so the same construct discovered by two different routes is
//! reported once, and reported as the same thing.
//!
//! # What this deliberately does not do
//!
//! No Type IIS layer yet (§7.9's second half: overhangs as nodes, plus the
//! Potapov/Pryor fidelity checks). No suffix array — the search is the naive
//! O(n·m) scan, which is fine for a handful of fragments of a few kb and is
//! replaceable without changing the interface.

use pl_core::cdseguid;

use crate::{rc, Dseq};

/// Knobs for an assembly.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Shortest homology that counts as a junction, in bases.
    ///
    /// 25 is pydna's default and the plan's. Below about 20 the odds of a
    /// chance match in a plasmid-sized molecule stop being negligible, and a
    /// chance match here does not produce a wrong length — it produces a
    /// confidently wrong *construct*.
    pub limit: usize,
    /// Refuse rather than enumerate beyond this many fragments.
    ///
    /// The search is over orderings and orientations, so it grows as
    /// `n! · 2ⁿ`. Ten fragments is already 3.7 billion arrangements. A tool
    /// that quietly runs for an hour is worse than one that says no.
    pub max_fragments: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            limit: 25,
            max_fragments: 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssemblyError {
    /// More fragments than the search will enumerate.
    TooManyFragments { given: usize, max: usize },
    /// Fewer than two fragments to join.
    NotEnoughFragments,
}

impl std::fmt::Display for AssemblyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssemblyError::TooManyFragments { given, max } => write!(
                f,
                "{given} fragments is more than this assembler will enumerate (max {max}); \
                 the search grows as n! * 2^n"
            ),
            AssemblyError::NotEnoughFragments => {
                write!(f, "an assembly needs at least two fragments")
            }
        }
    }
}

impl std::error::Error for AssemblyError {}

/// One way the fragments can be joined.
#[derive(Debug, Clone, PartialEq)]
pub struct Product {
    pub seq: Dseq,
    /// The fragments in the order used, and whether each was flipped.
    pub order: Vec<(usize, bool)>,
    /// Homology length at each junction, in the same order. For a circular
    /// product the last entry is the junction that closes the circle.
    pub junctions: Vec<usize>,
}

impl Product {
    /// Rotation- and strand-invariant identity. Two routes to the same
    /// construct give the same value.
    ///
    /// `None` when the product contains anything outside `ACGT` — the SEGUID
    /// reference rejects ambiguity codes rather than guessing what they mean,
    /// and so do we. [`Product::identity`] is what deduplication uses, and it
    /// says what it fell back to.
    pub fn checksum(&self) -> Option<String> {
        let s = self.seq.watson.to_ascii_uppercase();
        if self.seq.circular {
            cdseguid(&s, &rc(&s)).ok()
        } else {
            pl_core::ldseguid(&s, &rc(&s)).ok()
        }
    }

    /// A key for deduplication.
    ///
    /// The checksum where one can be computed, because it is invariant to
    /// rotation and strand and so collapses the same construct found by
    /// different routes. Where it cannot — an ambiguity code in a fragment —
    /// the raw sequence, which is deterministic but will report a rotated
    /// duplicate as a second product. Marked so the difference is visible
    /// rather than silently weaker.
    pub fn identity(&self) -> String {
        match self.checksum() {
            Some(c) => c,
            None => format!("raw:{}", self.seq.watson.to_ascii_uppercase()),
        }
    }
}

/// The longest suffix of `a` that is also a prefix of `b`, at least `limit`.
///
/// Bounded below `a.len()` and `b.len()`: an "overlap" equal to a whole
/// fragment means one fragment is contained in the other, which is a
/// containment rather than a junction and would let a fragment vanish into its
/// neighbour without trace.
fn terminal_overlap(a: &str, b: &str, limit: usize) -> Option<usize> {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let max = ab.len().min(bb.len());
    if max == 0 || limit == 0 {
        return None;
    }
    // Longest first: a 40 bp homology should be reported as 40, not as the 25
    // bp suffix of itself.
    let upper = if max >= ab.len() || max >= bb.len() {
        max.saturating_sub(1)
    } else {
        max
    };
    for k in (limit..=upper).rev() {
        if ab[ab.len() - k..] == bb[..k] {
            return Some(k);
        }
    }
    None
}

/// Enumerate the products these fragments can form.
///
/// `circular` selects which kind to look for: cycles that use every fragment
/// once, or paths that do. Results are deduplicated by [`Product::checksum`]
/// and returned in a deterministic order.
pub fn assemble(
    fragments: &[Dseq],
    circular: bool,
    opts: Options,
) -> Result<Vec<Product>, AssemblyError> {
    if fragments.len() < 2 {
        return Err(AssemblyError::NotEnoughFragments);
    }
    if fragments.len() > opts.max_fragments {
        return Err(AssemblyError::TooManyFragments {
            given: fragments.len(),
            max: opts.max_fragments,
        });
    }

    // Each fragment in both orientations, as plain strings. Gibson joins by
    // homology, so end shapes play no part here.
    let seqs: Vec<[String; 2]> = fragments
        .iter()
        .map(|f| {
            let w = f.to_string_full().to_ascii_uppercase();
            let r = rc(&w);
            [w, r]
        })
        .collect();

    let n = fragments.len();
    let mut out: Vec<Product> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = Default::default();

    // Fragment 0 is pinned in the forward orientation. Every arrangement is
    // reachable that way once products are deduplicated by a checksum that is
    // itself rotation- and strand-invariant, and pinning removes the n·2
    // symmetric copies of each answer.
    let mut used = vec![false; n];
    let mut path: Vec<(usize, bool)> = Vec::new();
    let mut junctions: Vec<usize> = Vec::new();
    used[0] = true;
    path.push((0, false));

    walk(
        &seqs,
        circular,
        opts,
        &mut used,
        &mut path,
        &mut junctions,
        &mut out,
        &mut seen,
    );

    out.sort_by_key(|p| p.identity());
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn walk(
    seqs: &[[String; 2]],
    circular: bool,
    opts: Options,
    used: &mut Vec<bool>,
    path: &mut Vec<(usize, bool)>,
    junctions: &mut Vec<usize>,
    out: &mut Vec<Product>,
    seen: &mut std::collections::BTreeSet<String>,
) {
    let n = seqs.len();
    if path.len() == n {
        if let Some(p) = finish(seqs, circular, path, junctions) {
            let ck = p.identity();
            if seen.insert(ck) {
                out.push(p);
            }
        }
        return;
    }

    let (li, lr) = *path.last().expect("path is never empty");
    let last = &seqs[li][lr as usize];

    for next in 0..n {
        if used[next] {
            continue;
        }
        for flip in [false, true] {
            let cand = &seqs[next][flip as usize];
            let Some(k) = terminal_overlap(last, cand, opts.limit) else {
                continue;
            };
            used[next] = true;
            path.push((next, flip));
            junctions.push(k);
            walk(seqs, circular, opts, used, path, junctions, out, seen);
            junctions.pop();
            path.pop();
            used[next] = false;
        }
    }
}

/// Turn a complete ordering into a product, if it closes.
fn finish(
    seqs: &[[String; 2]],
    circular: bool,
    path: &[(usize, bool)],
    junctions: &[usize],
) -> Option<Product> {
    let mut acc = seqs[path[0].0][path[0].1 as usize].clone();
    for (step, &(i, r)) in path.iter().enumerate().skip(1) {
        let k = junctions[step - 1];
        acc.push_str(&seqs[i][r as usize][k..]);
    }

    let mut js = junctions.to_vec();
    if circular {
        // The last fragment must run back into the first, and that closing
        // homology is present twice in `acc` — once at each end — so one copy
        // comes off.
        let first = &seqs[path[0].0][path[0].1 as usize];
        let last = &seqs[path[path.len() - 1].0][path[path.len() - 1].1 as usize];
        let k = terminal_overlap(last, first, 1)?;
        if k > acc.len() {
            return None;
        }
        acc.truncate(acc.len() - k);
        js.push(k);
    }

    Some(Product {
        seq: Dseq::new(&acc, circular),
        order: path.to_vec(),
        junctions: js,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic non-repeating DNA. Assembly fixtures must not repeat, or
    /// every fragment overlaps every other and the test measures nothing.
    fn dna(seed: u64, n: usize) -> String {
        let mut x = seed | 1;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                b"ACGT"[(x % 4) as usize] as char
            })
            .collect()
    }

    #[test]
    fn terminal_overlap_is_terminal_and_longest() {
        assert_eq!(terminal_overlap("AAAAGGGGG", "GGGGGTTTT", 3), Some(5));
        // Longest, not merely sufficient.
        assert_eq!(terminal_overlap("AAAAGGGGG", "GGGGGTTTT", 5), Some(5));
        assert_eq!(terminal_overlap("AAAAGGGGG", "GGGGGTTTT", 6), None);
        // A shared run in the MIDDLE is a repeat, not a junction.
        assert_eq!(terminal_overlap("AAGGGGGAA", "TTGGGGGTT", 5), None);
        // Containment is not a junction either.
        assert_eq!(terminal_overlap("ACGT", "ACGT", 4), None);
        assert_eq!(terminal_overlap("", "ACGT", 1), None);
    }

    #[test]
    fn two_fragments_with_terminal_homology_circularise() {
        // The shape the bench uses: two fragments sharing homology at both
        // ends, closing a circle.
        let overlap_a = dna(0xa1, 30);
        let overlap_b = dna(0xb2, 30);
        let middle_1 = dna(0xc3, 200);
        let middle_2 = dna(0xd4, 250);

        // f1 = [A][middle1][B],  f2 = [B][middle2][A]
        let f1 = format!("{overlap_a}{middle_1}{overlap_b}");
        let f2 = format!("{overlap_b}{middle_2}{overlap_a}");

        let products = assemble(
            &[Dseq::new(&f1, false), Dseq::new(&f2, false)],
            true,
            Options::default(),
        )
        .unwrap();

        assert_eq!(products.len(), 1, "{:?}", products.len());
        let p = &products[0];
        assert!(p.seq.circular);
        // Each overlap is counted once, not twice.
        assert_eq!(p.seq.watson.len(), 30 + 200 + 30 + 250);
        assert_eq!(p.junctions, vec![30, 30]);
    }

    #[test]
    fn a_fragment_supplied_on_the_other_strand_still_assembles() {
        // Nothing about a gel band tells you which strand is in the tube.
        let overlap_a = dna(0x11, 30);
        let overlap_b = dna(0x22, 30);
        let f1 = format!("{overlap_a}{}{overlap_b}", dna(0x33, 150));
        let f2 = format!("{overlap_b}{}{overlap_a}", dna(0x44, 150));

        let forward = assemble(
            &[Dseq::new(&f1, false), Dseq::new(&f2, false)],
            true,
            Options::default(),
        )
        .unwrap();
        let flipped = assemble(
            &[Dseq::new(&f1, false), Dseq::new(&rc(&f2), false)],
            true,
            Options::default(),
        )
        .unwrap();

        assert_eq!(forward.len(), 1);
        assert_eq!(flipped.len(), 1);
        assert_eq!(
            forward[0].identity(),
            flipped[0].identity(),
            "the same construct reached two ways must be one answer"
        );
        assert!(forward[0].checksum().is_some(), "plain ACGT must checksum");
    }

    #[test]
    fn three_fragments_assemble_in_the_only_order_that_works() {
        let a = dna(0x101, 30);
        let b = dna(0x202, 30);
        let c = dna(0x303, 30);
        let f1 = format!("{a}{}{b}", dna(1, 120));
        let f2 = format!("{b}{}{c}", dna(2, 130));
        let f3 = format!("{c}{}{a}", dna(3, 140));

        let p = assemble(
            &[
                Dseq::new(&f1, false),
                Dseq::new(&f2, false),
                Dseq::new(&f3, false),
            ],
            true,
            Options::default(),
        )
        .unwrap();
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].seq.watson.len(), 3 * 30 + 120 + 130 + 140);
    }

    #[test]
    fn fragments_that_share_nothing_produce_nothing() {
        let p = assemble(
            &[
                Dseq::new(&dna(0x900, 300), false),
                Dseq::new(&dna(0x901, 300), false),
            ],
            true,
            Options::default(),
        )
        .unwrap();
        assert!(p.is_empty(), "unrelated fragments must not assemble: {p:?}");
    }

    #[test]
    fn homology_below_the_limit_is_not_a_junction() {
        // 20 bp of shared end, with the default limit of 25.
        let a = dna(0x55, 20);
        let f1 = format!("{}{a}", dna(0x56, 200));
        let f2 = format!("{a}{}", dna(0x57, 200));
        let p = assemble(
            &[Dseq::new(&f1, false), Dseq::new(&f2, false)],
            false,
            Options::default(),
        )
        .unwrap();
        assert!(p.is_empty());

        // ...and is one if the caller lowers the limit knowingly.
        let p = assemble(
            &[Dseq::new(&f1, false), Dseq::new(&f2, false)],
            false,
            Options {
                limit: 20,
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].seq.watson.len(), 200 + 20 + 200);
    }

    #[test]
    fn a_shared_run_in_the_middle_is_not_an_assembly() {
        // The failure that matters: an internal repeat is not a junction, and
        // treating it as one invents a construct no reaction can make.
        let repeat = dna(0x77, 40);
        let f1 = format!("{}{repeat}{}", dna(1, 100), dna(2, 100));
        let f2 = format!("{}{repeat}{}", dna(3, 100), dna(4, 100));
        let p = assemble(
            &[Dseq::new(&f1, false), Dseq::new(&f2, false)],
            true,
            Options::default(),
        )
        .unwrap();
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn too_many_fragments_is_refused_rather_than_attempted() {
        let frags: Vec<Dseq> = (0..12).map(|i| Dseq::new(&dna(i, 100), false)).collect();
        assert!(matches!(
            assemble(&frags, true, Options::default()),
            Err(AssemblyError::TooManyFragments { given: 12, max: 8 })
        ));
        assert!(matches!(
            assemble(&frags[..1], true, Options::default()),
            Err(AssemblyError::NotEnoughFragments)
        ));
    }

    #[test]
    fn the_result_is_deterministic() {
        let a = dna(0xaa, 30);
        let b = dna(0xbb, 30);
        let f1 = format!("{a}{}{b}", dna(1, 100));
        let f2 = format!("{b}{}{a}", dna(2, 100));
        let frags = [Dseq::new(&f1, false), Dseq::new(&f2, false)];
        let first = assemble(&frags, true, Options::default()).unwrap();
        for _ in 0..10 {
            assert_eq!(assemble(&frags, true, Options::default()).unwrap(), first);
        }
    }
}
