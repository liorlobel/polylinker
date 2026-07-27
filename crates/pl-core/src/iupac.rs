//! IUPAC nucleotide codes: complementation, matching, and composition.
//!
//! Case is preserved throughout. Lowercase is meaningful in this field —
//! soft-masked or low-coverage assembly bases, non-annealing primer tails —
//! and silently upper-casing destroys information the user put there.
//! Matching routines fold case internally instead.

/// Complement a single base, preserving case. Unknown bytes pass through.
///
/// **This is the DNA complement**: `U` complements to `A`, and `A` complements
/// to `T`, so a round trip through [`reverse_complement`] rewrites RNA as DNA
/// and is not an involution on `U`. That is deliberate rather than an
/// oversight — changing `A => U` would alter the complement of every ordinary
/// sequence and break every duplex SEGUID — but it means an RNA sequence
/// silently changes alphabet. Use [`reverse_complement_rna`] for RNA.
///
/// The root cause is that `Molecule` has no alphabet field; until it does,
/// the caller has to know which they hold.
#[inline]
pub const fn complement(b: u8) -> u8 {
    match b {
        b'A' => b'T',
        b'a' => b't',
        b'C' => b'G',
        b'c' => b'g',
        b'G' => b'C',
        b'g' => b'c',
        b'T' => b'A',
        b't' => b'a',
        b'U' => b'A',
        b'u' => b'a',
        // Ambiguity codes complement within the IUPAC alphabet.
        b'R' => b'Y',
        b'r' => b'y', // purine  <-> pyrimidine
        b'Y' => b'R',
        b'y' => b'r',
        b'S' => b'S',
        b's' => b's', // strong (G/C) is self-complementary
        b'W' => b'W',
        b'w' => b'w', // weak   (A/T) is self-complementary
        b'K' => b'M',
        b'k' => b'm', // keto   <-> amino
        b'M' => b'K',
        b'm' => b'k',
        b'B' => b'V',
        b'b' => b'v',
        b'V' => b'B',
        b'v' => b'b',
        b'D' => b'H',
        b'd' => b'h',
        b'H' => b'D',
        b'h' => b'd',
        b'N' => b'N',
        b'n' => b'n',
        other => other,
    }
}

/// Reverse complement, preserving case.
pub fn reverse_complement(seq: &[u8]) -> Vec<u8> {
    seq.iter().rev().map(|&b| complement(b)).collect()
}

/// Reverse-complement an RNA sequence, keeping `U` as `U`.
///
/// [`reverse_complement`] is the DNA operation and maps `A -> T`, so RNA passed
/// through it comes back as DNA. This is purely additive: it does not change
/// what any existing caller gets.
///
/// Deliberately *not* implemented by sniffing T-versus-U per sequence. That
/// would destroy `rc(a ++ b) == rc(b) ++ rc(a)`, which the annotator, the
/// translator and the cloning engine all rely on when they concatenate
/// fragments — a mixed pair would complement inconsistently.
pub fn reverse_complement_rna(seq: &[u8]) -> Vec<u8> {
    seq.iter()
        .rev()
        .map(|&b| match complement(b) {
            b'T' => b'U',
            b't' => b'u',
            other => other,
        })
        .collect()
}

/// The set of concrete bases an IUPAC code stands for, as a 4-bit mask
/// (bit 0 = A, 1 = C, 2 = G, 3 = T). Returns 0 for bytes that are not
/// nucleotide codes, which makes them match nothing.
#[inline]
pub const fn code_mask(b: u8) -> u8 {
    match b.to_ascii_uppercase() {
        b'A' => 0b0001,
        b'C' => 0b0010,
        b'G' => 0b0100,
        b'T' | b'U' => 0b1000,
        b'R' => 0b0101, // A G
        b'Y' => 0b1010, // C T
        b'S' => 0b0110, // C G
        b'W' => 0b1001, // A T
        b'K' => 0b1100, // G T
        b'M' => 0b0011, // A C
        b'B' => 0b1110, // C G T
        b'D' => 0b1101, // A G T
        b'H' => 0b1011, // A C T
        b'V' => 0b0111, // A C G
        b'N' => 0b1111,
        _ => 0,
    }
}

/// Does a sequence base satisfy a (possibly ambiguous) pattern code?
///
/// Asymmetric on purpose: the pattern may be ambiguous, and the subject base
/// must be one of the bases the pattern allows. A subject `N` therefore does
/// *not* match a pattern `A` — an unknown base is not evidence of a site.
#[inline]
pub const fn matches(pattern: u8, subject: u8) -> bool {
    let p = code_mask(pattern);
    let s = code_mask(subject);
    s != 0 && (s & !p) == 0
}

/// Every start where `pattern` matches `subject`, 1-based, ascending.
///
/// The one search loop in the project. It was inside `pl_enzymes::cut_positions`
/// — the only loop over [`matches`] anywhere in the workspace — where it had
/// accumulated a Biopython oracle covering 25,400 positions but could not be
/// called by anything else. Lifting it here means the library's motif search
/// inherits that oracle instead of needing a second one, and any future scanner
/// does too.
///
/// **Circular molecules wrap.** `n` starts on a circle against `n - k + 1` on a
/// line is the whole of the wraparound handling; indices are taken modulo `n`.
/// Missing that is the classic plasmid bug — a unique cutter reported as a
/// non-cutter purely because its site straddles base 1.
///
/// **A pattern longer than the molecule never matches**, on either topology.
/// On a circle that is a deliberate, pinned divergence from Biopython, which
/// searches a doubled string and so lets a 6 bp site match a 4 bp plasmid by
/// wrapping it more than once. No enzyme binds that way; see
/// `pl_enzymes::cut_positions`.
///
/// **Forward strand only.** The caller decides whether the reverse complement
/// is a second search — for a palindromic restriction site it would only
/// double-report, and for an arbitrary motif it is required.
pub fn find_all(pattern: &[u8], subject: &[u8], circular: bool) -> Vec<u64> {
    let (n, k) = (subject.len(), pattern.len());
    if n == 0 || k == 0 || k > n {
        return Vec::new();
    }
    let starts = if circular { n } else { n - k + 1 };
    let mut out = Vec::new();
    for i in 0..starts {
        let hit = (0..k).all(|j| {
            let idx = if circular { (i + j) % n } else { i + j };
            matches(pattern[j], subject[idx])
        });
        if hit {
            out.push(i as u64 + 1);
        }
    }
    out
}

/// Is a pattern its own reverse complement, as a *set of allowed bases*?
///
/// Compared on masks rather than on the uppercased string. The two agree for
/// every pattern that is spelled canonically, but the mask test is the one that
/// is actually true: it says the two searches would return the same positions,
/// which is the property a caller collapsing them relies on. It also costs the
/// same.
///
/// The caller that needs this is a both-strand motif search: without it,
/// `GAATTC` reports every site twice.
pub fn is_palindrome_masks(pattern: &[u8]) -> bool {
    let k = pattern.len();
    (0..k).all(|i| {
        let a = code_mask(pattern[i]);
        let b = code_mask(pattern[k - 1 - i]);
        // Complement on a mask is a 4-bit reversal: A<->T is bit 0 <-> bit 3,
        // C<->G is bit 1 <-> bit 2.
        let rc =
            ((b & 0b0001) << 3) | ((b & 0b0010) << 1) | ((b & 0b0100) >> 1) | ((b & 0b1000) >> 3);
        a == rc
    })
}

/// Composition counts over the four concrete bases plus everything else.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Composition {
    pub a: u64,
    pub c: u64,
    pub g: u64,
    pub t: u64,
    pub other: u64,
}

impl Composition {
    pub fn of(seq: &[u8]) -> Self {
        let mut c = Composition::default();
        for &b in seq {
            match b.to_ascii_uppercase() {
                b'A' => c.a += 1,
                b'C' => c.c += 1,
                b'G' => c.g += 1,
                b'T' | b'U' => c.t += 1,
                _ => c.other += 1,
            }
        }
        c
    }

    /// GC as a percentage of unambiguous bases. `None` when there are none,
    /// rather than a misleading 0.0.
    pub fn gc_percent(&self) -> Option<f64> {
        let n = self.a + self.c + self.g + self.t;
        if n == 0 {
            return None;
        }
        Some(100.0 * (self.g + self.c) as f64 / n as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complement_preserves_case() {
        assert_eq!(reverse_complement(b"AaCcGgTt"), b"aAcCgGtT".to_vec());
    }

    #[test]
    fn ambiguity_codes_complement_correctly() {
        assert_eq!(reverse_complement(b"RYKMBVDHSWN"), b"NWSDHBVKMRY".to_vec());
    }

    #[test]
    fn double_complement_is_identity() {
        let s = b"AcGtRyKmBvDhSwNn";
        assert_eq!(reverse_complement(&reverse_complement(s)), s.to_vec());
    }

    #[test]
    fn pattern_matching_is_asymmetric() {
        assert!(matches(b'N', b'A')); // N pattern accepts a concrete A
        assert!(!matches(b'A', b'N')); // an unknown base is not an A
        assert!(matches(b'R', b'g')); // case-insensitive, R = A|G
        assert!(!matches(b'R', b'c'));
        assert!(!matches(b'A', b'-')); // non-nucleotides match nothing
    }

    #[test]
    fn gc_ignores_ambiguous_bases() {
        let c = Composition::of(b"GGCCAANNNN");
        assert_eq!(c.other, 4);
        // 4 of 6 unambiguous bases are G or C. Compared with a tolerance
        // because the exact bit pattern depends on the order of operations.
        let gc = c.gc_percent().expect("six unambiguous bases");
        assert!((gc - 200.0 / 3.0).abs() < 1e-9, "got {gc}");
        // No unambiguous bases means no answer, not a misleading 0.0.
        assert_eq!(Composition::of(b"NNNN").gc_percent(), None);
    }

    #[test]
    fn rna_keeps_its_alphabet_only_through_the_rna_helper() {
        // `complement` is the DNA operation: U -> A and A -> T, so a round
        // trip through `reverse_complement` rewrites RNA as DNA and is not an
        // involution on U. Deliberate — changing A -> U would alter every
        // ordinary complement and break every duplex SEGUID — but it means the
        // caller has to pick the right function.
        assert_eq!(reverse_complement(b"AUGC"), b"GCAT".to_vec());
        assert_ne!(
            reverse_complement(&reverse_complement(b"AUGC")),
            b"AUGC".to_vec(),
            "documented: the DNA operation is not an involution on U"
        );

        // The RNA helper is, and it preserves case.
        assert_eq!(reverse_complement_rna(b"AUGC"), b"GCAU".to_vec());
        assert_eq!(
            reverse_complement_rna(&reverse_complement_rna(b"AUGC")),
            b"AUGC".to_vec()
        );
        assert_eq!(reverse_complement_rna(b"augc"), b"gcau".to_vec());
        // A DNA sequence put through it comes back as RNA, which is the
        // caller's business to know.
        assert_eq!(reverse_complement_rna(b"ACGT"), b"ACGU".to_vec());

        // Both agree wherever no T or U is involved, which is what keeps the
        // ambiguity codes consistent between them.
        for s in [b"RYSWKM".as_slice(), b"BDHVN", b"CCGG"] {
            assert_eq!(reverse_complement(s), reverse_complement_rna(s));
        }
    }

    /// A tiny deterministic generator. `Math.random`'s Rust cousin is not in
    /// std and a dependency for a test is not worth it; this is xorshift64*,
    /// which is more than random enough to shuffle bases.
    fn rng(state: &mut u64) -> u64 {
        *state ^= *state >> 12;
        *state ^= *state << 25;
        *state ^= *state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn random_seq(state: &mut u64, n: usize, alphabet: &[u8]) -> Vec<u8> {
        (0..n)
            .map(|_| alphabet[(rng(state) % alphabet.len() as u64) as usize])
            .collect()
    }

    #[test]
    fn find_all_agrees_with_the_naive_loop_it_replaced() {
        // The scan moved here out of `pl_enzymes::cut_positions`, where it had
        // a Biopython oracle over 25,400 positions. This asserts the move was
        // faithful by comparing against the loop as it was written there, on
        // both topologies and over ambiguity codes the enzyme table does not
        // exercise.
        fn naive(pattern: &[u8], subject: &[u8], circular: bool) -> Vec<u64> {
            let (n, k) = (subject.len(), pattern.len());
            if n == 0 || k == 0 || k > n {
                return Vec::new();
            }
            let starts = if circular { n } else { n - k + 1 };
            let mut out = Vec::new();
            for i in 0..starts {
                let hit = (0..k).all(|j| {
                    let idx = if circular { (i + j) % n } else { i + j };
                    matches(pattern[j], subject[idx])
                });
                if hit {
                    out.push(i as u64 + 1);
                }
            }
            out
        }

        let mut st = 0x1234_5678_9abc_def0u64;
        for case in 0..4000 {
            let (sn, pn) = (
                1 + (rng(&mut st) % 40) as usize,
                1 + (rng(&mut st) % 8) as usize,
            );
            let subject = random_seq(&mut st, sn, b"ACGTNRYW");
            let pattern = random_seq(&mut st, pn, b"ACGTNRYWSKMBDHV");
            for circular in [true, false] {
                assert_eq!(
                    find_all(&pattern, &subject, circular),
                    naive(&pattern, &subject, circular),
                    "case {case}: pattern {} subject {} circular {circular}",
                    String::from_utf8_lossy(&pattern),
                    String::from_utf8_lossy(&subject)
                );
            }
        }
    }

    #[test]
    fn a_site_straddling_the_origin_is_found_on_a_circle_and_not_on_a_line() {
        //        1234567890
        let seq = b"TTCXXXXGAA";
        // GAATTC spans 8,9,10,1,2,3.
        assert_eq!(find_all(b"GAATTC", seq, true), vec![8]);
        assert!(find_all(b"GAATTC", seq, false).is_empty());
    }

    #[test]
    fn rotating_a_circle_shifts_every_hit_by_exactly_the_rotation() {
        // The property CONTRIBUTING.md names by hand, as a property rather
        // than an example: it kills the class instead of one case.
        let mut st = 0xfeed_face_dead_beefu64;
        for _ in 0..300 {
            let n = 12 + (rng(&mut st) % 60) as usize;
            let pn = 2 + (rng(&mut st) % 5) as usize;
            let seq = random_seq(&mut st, n, b"ACGT");
            let pattern = random_seq(&mut st, pn, b"ACGTRYWN");
            let base: Vec<u64> = find_all(&pattern, &seq, true);
            for r in 0..n {
                let mut rot = seq[r..].to_vec();
                rot.extend_from_slice(&seq[..r]);
                let got: Vec<u64> = find_all(&pattern, &rot, true);
                let mut want: Vec<u64> = base
                    .iter()
                    .map(|&p| ((p - 1 + (n - r) as u64) % n as u64) + 1)
                    .collect();
                want.sort_unstable();
                assert_eq!(
                    got,
                    want,
                    "rotation {r} of {}",
                    String::from_utf8_lossy(&seq)
                );
            }
        }
    }

    #[test]
    fn a_pattern_longer_than_the_molecule_never_matches() {
        // Deliberate divergence from Biopython, which searches a doubled
        // string and lets a 6 bp site wrap a 4 bp circle more than once.
        assert!(find_all(b"GAATTC", b"GAAT", true).is_empty());
        assert!(find_all(b"GAATTC", b"GAAT", false).is_empty());
        // Exactly equal is fine, and on a circle every rotation is a start.
        assert_eq!(find_all(b"ACGT", b"ACGT", false), vec![1]);
        assert_eq!(find_all(b"ACGT", b"ACGT", true), vec![1]);
        assert_eq!(find_all(b"AAAA", b"AAAA", true), vec![1, 2, 3, 4]);
    }

    #[test]
    fn nothing_matches_an_empty_pattern_or_an_empty_subject() {
        assert!(find_all(b"", b"ACGT", true).is_empty());
        assert!(find_all(b"ACGT", b"", true).is_empty());
        assert!(find_all(b"", b"", false).is_empty());
    }

    #[test]
    fn find_all_inherits_the_asymmetry_of_matches() {
        // Pattern N matches subject A; pattern A does not match subject N.
        // An unknown base is not evidence of a site, and a search that
        // pretended otherwise would report sites that are not there.
        assert_eq!(find_all(b"N", b"A", false), vec![1]);
        assert!(find_all(b"A", b"N", false).is_empty());
        assert_eq!(find_all(b"GAWTC", b"GAATC", false), vec![1]);
        assert!(find_all(b"GAATC", b"GAWTC", false).is_empty());
        // A byte that is not a code matches nothing, in either role.
        assert!(find_all(b"A", b"-", false).is_empty());
        assert!(find_all(b"-", b"A", false).is_empty());
    }

    #[test]
    fn palindromes_are_recognised_by_the_bases_they_allow() {
        for p in [
            b"GAATTC".as_slice(), // EcoRI
            b"GGATCC",            // BamHI
            b"AT",
            b"GC",
            b"",            // vacuously
            b"GGWCC",       // W is self-complementary
            b"RGCY",        // R/Y complement each other across the centre
            b"GCGGCCGC",    // NotI
            b"CCANNNNNTGG", // N is self-complementary
        ] {
            assert!(
                is_palindrome_masks(p),
                "{} should be a palindrome",
                String::from_utf8_lossy(p)
            );
        }
        for p in [
            b"GGTCTC".as_slice(), // BsaI, Type IIS
            b"GAAGAC",            // BbsI
            b"A",
            b"GAATTG",
            // `GGSCC` looks like it belongs here and does not: S is
            // self-complementary, so it reverse-complements to itself. Moving
            // one base off centre is what actually breaks the symmetry.
            b"GGSCA",
            b"ACGTA",
        ] {
            assert!(
                !is_palindrome_masks(p),
                "{} should not be a palindrome",
                String::from_utf8_lossy(p)
            );
        }
    }

    #[test]
    fn a_palindrome_is_exactly_a_pattern_whose_two_strands_find_the_same_sites() {
        // The property the mask test is a proxy for, checked directly: this is
        // what a both-strand search relies on when it collapses the two scans
        // into one to avoid reporting every site twice.
        let mut st = 0x0bad_c0de_0bad_c0deu64;
        for _ in 0..2000 {
            let (pn, sn) = (
                1 + (rng(&mut st) % 7) as usize,
                20 + (rng(&mut st) % 40) as usize,
            );
            let pattern = random_seq(&mut st, pn, b"ACGTRYSWKMN");
            let subject = random_seq(&mut st, sn, b"ACGT");
            let rc = reverse_complement(&pattern);
            let same = find_all(&pattern, &subject, true) == find_all(&rc, &subject, true);
            if is_palindrome_masks(&pattern) {
                assert!(
                    same,
                    "{} is called a palindrome but its strands disagree",
                    String::from_utf8_lossy(&pattern)
                );
            }
        }
    }

    #[test]
    fn mask_complement_and_sequence_complement_are_the_same_operation() {
        // `is_palindrome_masks` complements a 4-bit mask by reversing its bits.
        // That is only sound if it agrees with `complement` over the whole
        // alphabet -- including the bytes that are not codes at all.
        for b in 0u8..=255 {
            let m = code_mask(b);
            let reversed = ((m & 0b0001) << 3)
                | ((m & 0b0010) << 1)
                | ((m & 0b0100) >> 1)
                | ((m & 0b1000) >> 3);
            assert_eq!(
                reversed,
                code_mask(complement(b)),
                "byte {:?} ({b}): mask reversal and complement disagree",
                b as char
            );
        }
    }
}
