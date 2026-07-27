//! The packed sequence store: one nibble per base.
//!
//! # Why four bits, and why these four bits
//!
//! The nibble is [`pl_core::iupac::code_mask`] of the byte — the set of
//! concrete bases that byte stands for. That is not a compression scheme that
//! happens to fit; it is the exact quotient of the byte alphabet under the
//! relation the search uses. `iupac::matches` is
//!
//! ```text
//! let p = code_mask(pattern);
//! let s = code_mask(subject);
//! s != 0 && (s & !p) == 0
//! ```
//!
//! so the stored nibble *is* `s`. Two bytes the engine can tell apart get
//! different nibbles; two it cannot — `T` and `U`, `A` and `a`, `-` and `x` and
//! `?`, all already folded by `matches` — get the same one. Nothing a query
//! could observe is lost, and `packing_preserves_matching_over_all_65536_byte_pairs`
//! asserts exactly that, rather than trusting the argument.
//!
//! **Two bits per base is ruled out on correctness, not on size.** It cannot
//! represent `N` or any of the fifteen ambiguity codes, so it would have to
//! fold or drop them — a silent wrong answer in a search box. A byte with
//! `code_mask == 0` stores nibble 0 and matches nothing, including a pattern of
//! `N`, which is the right answer for a byte we could not interpret.
//!
//! # Measured
//!
//! The real corpus (`docs/FINDINGS.md`) is 23.2 Mbase, which packs to 11.1 MiB
//! and scans at ~335 Mbase/s single-threaded — 69 ms, inside the 100 ms budget
//! that makes a search box feel instant. Degeneracy costs about 3%; early exit
//! dominates either way. **The budget is exhausted at roughly 33 Mbase**, which
//! is the number to re-measure before claiming this design scales.

use pl_core::iupac::code_mask;

/// Pack bases into nibbles, low nibble first.
///
/// Low-nibble-first so that `unpack_mask(pack(s), i)` is index arithmetic a
/// reader can check by eye, and so an odd-length run leaves the *high* nibble
/// of its last byte zero — which the file checksum then covers. Leaving that
/// padding undefined would make a bit-flip test flake intermittently instead of
/// failing, which is worse than failing.
pub fn pack(seq: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; seq.len().div_ceil(2)];
    for (i, &b) in seq.iter().enumerate() {
        let m = code_mask(b);
        if i % 2 == 0 {
            out[i / 2] = m;
        } else {
            out[i / 2] |= m << 4;
        }
    }
    out
}

/// The mask stored at 0-based base index `i`.
#[inline]
pub fn mask_at(packed: &[u8], i: usize) -> u8 {
    let byte = packed[i / 2];
    if i % 2 == 0 {
        byte & 0x0F
    } else {
        byte >> 4
    }
}

/// Unpack back to bases, canonically.
///
/// **Not an inverse of [`pack`].** `pack` is deliberately lossy about anything
/// no query can observe: case, `T` against `U`, and every byte that is not a
/// code. This returns the canonical uppercase spelling of each mask, and `-`
/// for mask 0 — a placeholder for "some byte we could not interpret", not a
/// claim that the file contained a gap character. Useful for showing a hit in
/// context; never for reconstructing a file. The file is on disk; read it.
pub fn unpack(packed: &[u8], bases: usize) -> Vec<u8> {
    (0..bases).map(|i| base_for(mask_at(packed, i))).collect()
}

/// The canonical IUPAC letter for a 4-bit mask.
pub fn base_for(mask: u8) -> u8 {
    const LETTERS: [u8; 16] = [
        b'-', // 0000 nothing
        b'A', b'C', b'M', // A, C, AC
        b'G', b'R', b'S', b'V', // G, AG, CG, ACG
        b'T', b'W', b'Y', b'H', // T, AT, CT, ACT
        b'K', b'D', b'B', b'N', // GT, AGT, CGT, ACGT
    ];
    LETTERS[(mask & 0x0F) as usize]
}

/// Complement a mask: a 4-bit reversal, since A↔T is bit 0↔3 and C↔G is 1↔2.
///
/// Exact for degenerate codes, which is what lets a both-strand search
/// complement the *pattern* and scan the forward store once, instead of
/// materialising a reverse-complemented copy of a 24 Mbase corpus.
#[inline]
pub const fn mask_complement(m: u8) -> u8 {
    ((m & 0b0001) << 3) | ((m & 0b0010) << 1) | ((m & 0b0100) >> 1) | ((m & 0b1000) >> 3)
}

/// The reverse complement of a pattern, as masks.
pub fn masks_reverse_complement(pattern_masks: &[u8]) -> Vec<u8> {
    pattern_masks
        .iter()
        .rev()
        .map(|&m| mask_complement(m))
        .collect()
}

/// A pattern as masks, or `None` naming the first byte that is not a code.
///
/// Refusing is the point. A pattern containing a byte that can never match —
/// `5'-GAATTC-3'`, a stray space, an `X` — is unsatisfiable, and answering it
/// with a clean empty result is a silent failure wearing a smaller costume:
/// the user reads "not present" where the truth is "not asked".
pub fn pattern_masks(pattern: &[u8]) -> Result<Vec<u8>, (usize, u8)> {
    let mut out = Vec::with_capacity(pattern.len());
    for (i, &b) in pattern.iter().enumerate() {
        let m = code_mask(b);
        if m == 0 {
            return Err((i, b));
        }
        out.push(m);
    }
    Ok(out)
}

/// Every start where `pattern_masks` matches the packed store, 1-based.
///
/// The nibble twin of [`pl_core::iupac::find_all`], with the same contract:
/// `n` starts on a circle against `n - k + 1` on a line, indices modulo `n`,
/// nothing when the pattern is longer than the molecule, forward strand only.
///
/// Those are two implementations of one search, and the byte one carries the
/// Biopython oracle. `agrees_with_the_byte_scan` in the tests is what connects
/// them; without it the oracle guards code no user query reaches.
pub fn find_all_nib(pattern_masks: &[u8], packed: &[u8], bases: usize, circular: bool) -> Vec<u64> {
    let (n, k) = (bases, pattern_masks.len());
    if n == 0 || k == 0 || k > n {
        return Vec::new();
    }
    let starts = if circular { n } else { n - k + 1 };
    let mut out = Vec::new();
    for i in 0..starts {
        let hit = (0..k).all(|j| {
            let idx = if circular { (i + j) % n } else { i + j };
            let s = mask_at(packed, idx);
            s != 0 && (s & !pattern_masks[j]) == 0
        });
        if hit {
            out.push(i as u64 + 1);
        }
    }
    out
}

/// Is a mask pattern its own reverse complement?
///
/// On masks rather than on letters, because that is the property a both-strand
/// search relies on when it collapses two scans into one: without it every
/// `GAATTC` site is reported twice.
pub fn is_palindrome(pattern_masks: &[u8]) -> bool {
    let k = pattern_masks.len();
    (0..k).all(|i| pattern_masks[i] == mask_complement(pattern_masks[k - 1 - i]))
}

/// Count of bases whose mask is not exactly one of A, C, G, T.
///
/// Carried into every coverage footer. Because `matches` is asymmetric — an
/// unknown base is not evidence of a site — a record containing `N` can
/// silently *lose* a hit, so the user is told how many such bases were in the
/// records searched rather than being left to assume there were none.
pub fn ambiguous_count(packed: &[u8], bases: usize) -> u64 {
    (0..bases)
        .filter(|&i| !matches!(mask_at(packed, i), 0b0001 | 0b0010 | 0b0100 | 0b1000))
        .count() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::iupac::{complement, find_all, is_palindrome_masks, matches, reverse_complement};

    /// The byte predicate, named so the exhaustive test below reads as the
    /// comparison it is.
    fn byte_matches(p: u8, s: u8) -> bool {
        matches(p, s)
    }

    const ALPHABET: &[u8] = b"ACGTUacgtuRYSWKMBDHVNryswkmbdhvn-nN.xX? ";

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

    /// Test 1 of the brief: the store must not change search semantics.
    ///
    /// A motif test with a handful of patterns cannot see a byte where
    /// `code_mask` collapses two things `matches` distinguishes. This is
    /// exhaustive over every (pattern, subject) byte pair, so an edit to
    /// `code_mask` — adding an alphabet, say — cannot silently change every
    /// search in the product.
    #[test]
    fn packing_preserves_matching_over_all_65536_byte_pairs() {
        for p in 0u8..=255 {
            for s in 0u8..=255 {
                let stored = code_mask(s);
                let nib = stored != 0 && (stored & !code_mask(p)) == 0;
                assert_eq!(
                    byte_matches(p, s),
                    nib,
                    "pattern {p} ({:?}) subject {s} ({:?})",
                    p as char,
                    s as char
                );
            }
        }
    }

    #[test]
    fn mask_complement_agrees_with_base_complement_over_all_256_bytes() {
        // `mask_complement` is what lets the search complement the pattern
        // instead of the corpus. It is only sound if it is the same operation.
        for b in 0u8..=255 {
            assert_eq!(
                mask_complement(code_mask(b)),
                code_mask(complement(b)),
                "byte {b} ({:?})",
                b as char
            );
        }
    }

    #[test]
    fn a_packed_run_round_trips_to_canonical_bases() {
        let seq = b"ACGTacgtRYSWKMBDHVN";
        let packed = pack(seq);
        assert_eq!(packed.len(), seq.len().div_ceil(2));
        assert_eq!(unpack(&packed, seq.len()), b"ACGTACGTRYSWKMBDHVN".to_vec());

        // U packs as T, because no query can tell them apart.
        assert_eq!(unpack(&pack(b"AUGC"), 4), b"ATGC".to_vec());
        // A byte that is not a code becomes mask 0, shown as `-`.
        assert_eq!(unpack(&pack(b"A?C"), 3), b"A-C".to_vec());
    }

    #[test]
    fn an_odd_length_run_leaves_its_last_high_nibble_zero() {
        // Not cosmetic: the file checksum covers these bytes, so undefined
        // padding would make the bit-flip test flake rather than fail.
        for n in [1usize, 3, 5, 7, 101] {
            let packed = pack(&vec![b'N'; n]);
            assert_eq!(packed.len(), n.div_ceil(2));
            assert_eq!(packed[packed.len() - 1] >> 4, 0, "n = {n}");
        }
    }

    /// Test 2 of the brief, and the single most important one in the feature.
    ///
    /// `cut_positions` carries a Biopython oracle over 25,400 positions and it
    /// scans **bytes**. This scans **nibbles**. Nothing else connects them, so
    /// without this the oracle guards code that no user query ever reaches.
    #[test]
    fn agrees_with_the_byte_scan_it_is_a_twin_of() {
        let mut st = 0x5eed_1234_abcd_0001u64;
        for case in 0..6000 {
            let (sn, pn) = (
                1 + (rng(&mut st) % 60) as usize,
                1 + (rng(&mut st) % 10) as usize,
            );
            let subject = random_seq(&mut st, sn, ALPHABET);
            let pattern = random_seq(&mut st, pn, b"ACGTRYSWKMBDHVN");
            let packed = pack(&subject);
            let pm: Vec<u8> = pattern.iter().map(|&b| code_mask(b)).collect();
            for circular in [true, false] {
                assert_eq!(
                    find_all_nib(&pm, &packed, subject.len(), circular),
                    find_all(&pattern, &subject, circular),
                    "case {case}: pattern {} subject {} circular {circular}",
                    String::from_utf8_lossy(&pattern),
                    String::from_utf8_lossy(&subject)
                );
            }
        }
    }

    #[test]
    fn a_reverse_strand_hit_lands_on_bases_that_really_reverse_complement() {
        // Asserting the *bases*, not the index: an off-by-k in the minus-strand
        // coordinate is completely invisible in a hit count.
        let mut st = 0xfeed_0bad_1234_5678u64;
        for _ in 0..500 {
            let (sn, pn) = (
                20 + (rng(&mut st) % 60) as usize,
                3 + (rng(&mut st) % 6) as usize,
            );
            let subject = random_seq(&mut st, sn, b"ACGT");
            let pattern = random_seq(&mut st, pn, b"ACGT");
            let packed = pack(&subject);
            let pm: Vec<u8> = pattern.iter().map(|&b| code_mask(b)).collect();
            let rc = masks_reverse_complement(&pm);

            for &start in &find_all_nib(&rc, &packed, subject.len(), true) {
                let i = (start - 1) as usize;
                let span: Vec<u8> = (0..pattern.len())
                    .map(|j| subject[(i + j) % subject.len()])
                    .collect();
                assert_eq!(
                    reverse_complement(&span),
                    pattern,
                    "a Reverse hit at {start} does not reverse-complement to the pattern"
                );
            }
        }
    }

    #[test]
    fn the_two_strands_are_mirror_images_of_each_other() {
        let mut st = 0x0f0f_0f0f_1111_2222u64;
        for _ in 0..500 {
            let subject = random_seq(&mut st, 40, b"ACGT");
            let pn = 3 + (rng(&mut st) % 5) as usize;
            let pattern = random_seq(&mut st, pn, b"ACGTRYWN");
            let packed = pack(&subject);
            let pm: Vec<u8> = pattern.iter().map(|&b| code_mask(b)).collect();
            let rc = masks_reverse_complement(&pm);
            // Searching rc(P) is the same as searching P against the reverse
            // complement of the subject, read backwards. The cheap version of
            // that claim: rc(rc(P)) finds exactly what P finds.
            assert_eq!(masks_reverse_complement(&rc), pm);
            assert_eq!(
                find_all_nib(&pm, &packed, subject.len(), true),
                find_all_nib(&masks_reverse_complement(&rc), &packed, subject.len(), true)
            );
        }
    }

    #[test]
    fn palindromy_on_masks_matches_pl_cores_answer() {
        let mut st = 0xdead_beef_cafe_0001u64;
        for _ in 0..3000 {
            let pn = 1 + (rng(&mut st) % 9) as usize;
            let pattern = random_seq(&mut st, pn, b"ACGTRYSWKMBDHVN");
            let pm: Vec<u8> = pattern.iter().map(|&b| code_mask(b)).collect();
            assert_eq!(
                is_palindrome(&pm),
                is_palindrome_masks(&pattern),
                "{}",
                String::from_utf8_lossy(&pattern)
            );
        }
    }

    #[test]
    fn a_pattern_with_a_byte_that_can_never_match_is_refused_by_name() {
        assert_eq!(pattern_masks(b"5'-GAATTC-3'"), Err((0, b'5')));
        assert_eq!(pattern_masks(b"GAATTC GG"), Err((6, b' ')));
        assert_eq!(pattern_masks(b"GAATTX"), Err((5, b'X')));
        assert_eq!(
            pattern_masks(b"GAATTC"),
            Ok(vec![0b0100, 0b0001, 0b0001, 0b1000, 0b1000, 0b0010])
        );
        // Lowercase and RNA are patterns, not errors.
        assert!(pattern_masks(b"gaauuc").is_ok());
    }

    #[test]
    fn ambiguous_bases_are_counted_because_they_can_hide_a_hit() {
        assert_eq!(ambiguous_count(&pack(b"ACGT"), 4), 0);
        assert_eq!(ambiguous_count(&pack(b"acgtu"), 5), 0);
        assert_eq!(ambiguous_count(&pack(b"ACNGT"), 5), 1);
        // A byte that is not a code is not an unambiguous base either.
        assert_eq!(ambiguous_count(&pack(b"AC-GT"), 5), 1);
        assert_eq!(ambiguous_count(&pack(b"NNNN"), 4), 4);
    }

    #[test]
    fn an_n_in_the_molecule_loses_a_hit_and_that_is_the_documented_answer() {
        // Not a bug to fix here: `matches` is asymmetric on purpose, because an
        // unknown base is not evidence of a site. It is a fact to *report*,
        // which is why `ambiguous_count` exists.
        let clean = pack(b"AAGAATTCAA");
        let dirty = pack(b"AAGAANTCAA");
        let pm: Vec<u8> = b"GAATTC".iter().map(|&b| code_mask(b)).collect();
        assert_eq!(find_all_nib(&pm, &clean, 10, false), vec![3]);
        assert!(find_all_nib(&pm, &dirty, 10, false).is_empty());
        assert_eq!(ambiguous_count(&dirty, 10), 1);
    }
}
