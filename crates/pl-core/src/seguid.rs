//! SEGUID v2 checksums — the correctness primitive.
//!
//! A plasmid has no canonical representation. Rotation, strand choice,
//! annotation order and feature-name spelling are all free, so comparing two
//! molecules by their bytes, or by their GenBank text, produces false failures
//! and hides real ones. These checksums are invariant under exactly the things
//! that do not change the molecule, and nothing else, which is what makes
//! "did this operation produce the right answer?" a decidable question.
//!
//! Every assertion about a molecule should be an assertion about its checksum.
//!
//! # Provenance
//!
//! Ported from the reference implementation, the Python `seguid` package 0.2.1
//! (MIT, Björn Johansson), and validated against it — see
//! `reference/python/tests/xcheck_seguid.py`, which runs both over the same
//! inputs and requires exact string equality. Agreement with the reference is
//! the entire point: a checksum only has value if other tools compute the same
//! one.
//!
//! # The five forms
//!
//! | function | molecule |
//! |---|---|
//! | [`seguid`]   | the original 2006 checksum; standard base64, so it can contain `+` and `/` |
//! | [`lsseguid`] | linear single-stranded |
//! | [`csseguid`] | circular single-stranded — rotation invariant |
//! | [`ldseguid`] | linear double-stranded — strand invariant |
//! | [`cdseguid`] | circular double-stranded — rotation *and* strand invariant |
//!
//! Each returns the "long form": a prefix such as `cdseguid=` followed by 27
//! base64 characters.

use crate::base64::{encode_standard_nopad, encode_urlsafe_nopad};
use crate::sha1::sha1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Empty,
    /// A symbol outside the permitted alphabet, with the offending character.
    NotInAlphabet(char),
    /// Watson and Crick must be the same length for a duplex.
    LengthMismatch {
        watson: usize,
        crick: usize,
    },
    /// The two strands are not complementary at the given 0-based offset.
    NotComplementary {
        at: usize,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Empty => write!(f, "a sequence must not be empty"),
            Error::NotInAlphabet(c) => {
                write!(
                    f,
                    "symbol {c:?} is not in the alphabet (expected A, C, G or T)"
                )
            }
            Error::LengthMismatch { watson, crick } => write!(
                f,
                "watson is {watson} nt and crick is {crick} nt; a duplex needs both the same"
            ),
            Error::NotComplementary { at } => {
                write!(f, "strands are not complementary at position {}", at + 1)
            }
        }
    }
}

impl std::error::Error for Error {}

const PREFIX_SEGUID: &str = "seguid=";
const PREFIX_LS: &str = "lsseguid=";
const PREFIX_CS: &str = "csseguid=";
const PREFIX_LD: &str = "ldseguid=";
const PREFIX_CD: &str = "cdseguid=";

/// The `{DNA}` alphabet: uppercase A, C, G, T only.
///
/// Deliberately strict, matching the reference. Lowercase is *rejected* rather
/// than folded: a checksum is an identity claim, and silently upper-casing
/// would let two sequences the caller considers different collide. Callers
/// holding mixed-case sequence should decide explicitly — see
/// [`crate::Molecule::checksum`].
fn assert_dna(seq: &str) -> Result<(), Error> {
    if seq.is_empty() {
        return Err(Error::Empty);
    }
    for c in seq.chars() {
        if !matches!(c, 'A' | 'C' | 'G' | 'T') {
            return Err(Error::NotInAlphabet(c));
        }
    }
    Ok(())
}

/// As [`assert_dna`], plus the `-` and `;` used to express overhangs and to
/// join the two strands.
fn assert_dna_extended(seq: &str) -> Result<(), Error> {
    if seq.is_empty() {
        return Err(Error::Empty);
    }
    for c in seq.chars() {
        if !matches!(c, 'A' | 'C' | 'G' | 'T' | '-' | ';') {
            return Err(Error::NotInAlphabet(c));
        }
    }
    Ok(())
}

fn complement(c: char) -> Option<char> {
    match c {
        'A' => Some('T'),
        'T' => Some('A'),
        'C' => Some('G'),
        'G' => Some('C'),
        // A dash marks a single-stranded position; it pairs with anything.
        '-' => Some('-'),
        _ => None,
    }
}

/// Watson and Crick are antiparallel, so position `i` of one pairs with
/// position `len - 1 - i` of the other. A dash on either side is an unpaired
/// overhang position and is not checked.
fn assert_complementary(watson: &str, crick: &str) -> Result<(), Error> {
    let w: Vec<char> = watson.chars().collect();
    let c: Vec<char> = crick.chars().collect();
    if w.len() != c.len() {
        return Err(Error::LengthMismatch {
            watson: w.len(),
            crick: c.len(),
        });
    }
    for (i, &wc) in w.iter().enumerate() {
        let cc = c[c.len() - 1 - i];
        if wc == '-' || cc == '-' {
            continue;
        }
        if complement(wc) != Some(cc) {
            return Err(Error::NotComplementary { at: i });
        }
    }
    Ok(())
}

/// Rotate `amount` symbols off the front and onto the back.
///
/// Matches the reference's `rotate`: `rotate(s, k) == s[k:] + s[:k]`, with
/// `k` reduced modulo the length so negative and over-long amounts are fine.
pub fn rotate(seq: &str, amount: isize) -> String {
    let chars: Vec<char> = seq.chars().collect();
    let n = chars.len();
    if n == 0 {
        return String::new();
    }
    let k = amount.rem_euclid(n as isize) as usize;
    chars[k..].iter().chain(&chars[..k]).collect()
}

/// Index at which the lexicographically smallest rotation begins.
///
/// Booth's algorithm. The reference uses Duval's; both return a starting index
/// for the same smallest *string*, and where a periodic sequence admits several
/// such indices the choice does not matter here — see the note in [`cdseguid`].
///
/// Ordering is by byte value, so uppercase sorts before lowercase, matching the
/// reference's ASCII ordering.
pub fn min_rotation(seq: &str) -> usize {
    let s: Vec<u8> = seq.bytes().collect();
    let n = s.len();
    if n == 0 {
        return 0;
    }
    // Failure function over the doubled string, tracking the best start.
    let mut f = vec![usize::MAX; 2 * n];
    let mut k = 0usize;
    for j in 1..2 * n {
        let sj = s[j % n];
        let mut i = f[j - k - 1];
        while i != usize::MAX && sj != s[(k + i + 1) % n] {
            if sj < s[(k + i + 1) % n] {
                k = j - i - 1;
            }
            i = f[i];
        }
        if i == usize::MAX && sj != s[(k + i.wrapping_add(1)) % n] {
            if sj < s[(k + i.wrapping_add(1)) % n] {
                k = j;
            }
            f[j - k] = usize::MAX;
        } else {
            f[j - k] = i.wrapping_add(1);
        }
    }
    k
}

/// The lexicographically smallest rotation of `seq`.
pub fn rotate_to_min(seq: &str) -> String {
    rotate(seq, min_rotation(seq) as isize)
}

fn checksum(seq: &str, url_safe: bool) -> String {
    let d = sha1(seq.as_bytes());
    if url_safe {
        encode_urlsafe_nopad(&d)
    } else {
        encode_standard_nopad(&d)
    }
}

/// The original 2006 SEGUID. Standard base64, so the result may contain `+`
/// and `/` — which is why [`lsseguid`] exists.
pub fn seguid(seq: &str) -> Result<String, Error> {
    assert_dna(seq)?;
    Ok(format!("{PREFIX_SEGUID}{}", checksum(seq, false)))
}

/// Linear single-stranded.
pub fn lsseguid(seq: &str) -> Result<String, Error> {
    assert_dna(seq)?;
    Ok(format!("{PREFIX_LS}{}", checksum(seq, true)))
}

/// Circular single-stranded: [`lsseguid`] of the smallest rotation, which is
/// what makes it independent of where the sequence was cut open.
pub fn csseguid(seq: &str) -> Result<String, Error> {
    assert_dna(seq)?;
    Ok(format!(
        "{PREFIX_CS}{}",
        checksum(&rotate_to_min(seq), true)
    ))
}

/// Linear double-stranded.
///
/// The two strands are ordered lexicographically and joined with `;`, so it
/// does not matter which one the caller calls Watson. Overhangs are written as
/// `-` in the strand that is absent there.
pub fn ldseguid(watson: &str, crick: &str) -> Result<String, Error> {
    assert_dna_extended(watson)?;
    assert_dna_extended(crick)?;
    assert_complementary(watson, crick)?;
    let spec = if watson < crick {
        format!("{watson};{crick}")
    } else {
        format!("{crick};{watson}")
    };
    Ok(format!("{PREFIX_LD}{}", checksum(&spec, true)))
}

/// Circular double-stranded — the one that matters for plasmids.
///
/// Invariant under rotation and under swapping the strands, which is exactly
/// the freedom a circular duplex has and no more.
///
/// Each strand is rotated to its own smallest form; whichever of the two is
/// lexicographically smaller becomes Watson, and the other strand is rotated by
/// the *same* amount so the two stay in register. Where a periodic sequence
/// offers several minimal-rotation indices they differ by a multiple of the
/// period, and rotating the partner strand by any of them yields the same
/// string — so the result does not depend on which index the algorithm picks.
pub fn cdseguid(watson: &str, crick: &str) -> Result<String, Error> {
    assert_dna(watson)?;
    assert_dna(crick)?;
    assert_complementary(watson, crick)?;

    let aw = min_rotation(watson) as isize;
    let watson_min = rotate(watson, aw);
    let ac = min_rotation(crick) as isize;
    let crick_min = rotate(crick, ac);

    let (w, c) = if watson_min < crick_min {
        (watson_min, rotate(crick, -aw))
    } else {
        (crick_min, rotate(watson, -ac))
    };

    let ld = ldseguid(&w, &c)?;
    Ok(format!("{PREFIX_CD}{}", &ld[PREFIX_LD.len()..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectors taken from the reference implementation's own docstrings and
    // from running it directly; see xcheck_seguid.py for the exhaustive check.

    #[test]
    fn reference_vectors_single_stranded() {
        assert_eq!(seguid("AT").unwrap(), "seguid=Ax/RG6hzSrMEEWoCO1IWMGska+4");
        assert_eq!(
            lsseguid("AT").unwrap(),
            "lsseguid=Ax_RG6hzSrMEEWoCO1IWMGska-4"
        );
        assert_eq!(
            seguid("GATTACA").unwrap(),
            "seguid=tp2jzeCM2e3W4yxtrrx09CMKa/8"
        );
        assert_eq!(
            lsseguid("GATTACA").unwrap(),
            "lsseguid=tp2jzeCM2e3W4yxtrrx09CMKa_8"
        );
    }

    #[test]
    fn the_two_base64_alphabets_are_the_only_difference() {
        let s = seguid("GATTACA").unwrap();
        let l = lsseguid("GATTACA").unwrap();
        assert_eq!(
            s[PREFIX_SEGUID.len()..].replace('+', "-").replace('/', "_"),
            l[PREFIX_LS.len()..]
        );
    }

    #[test]
    fn reference_vectors_circular_single_stranded() {
        // From the reference's csseguid docstring.
        assert_eq!(
            csseguid("ATTT").unwrap(),
            "csseguid=ot6JPLeAeMmfztW1736Kc6DAqlo"
        );
        assert_eq!(
            csseguid("TTTA").unwrap(),
            "csseguid=ot6JPLeAeMmfztW1736Kc6DAqlo"
        );
        // ...and the point of it: the linear form is not rotation invariant.
        assert_eq!(
            lsseguid("TTTA").unwrap(),
            "lsseguid=8zCvKwyQAEsbPtC4yTV-pY0H93Q"
        );
        assert_eq!(
            lsseguid("ATTT").unwrap(),
            "lsseguid=ot6JPLeAeMmfztW1736Kc6DAqlo"
        );
    }

    #[test]
    fn reference_vectors_double_stranded() {
        // From the reference's ldseguid docstring, including the overhang form.
        assert_eq!(
            ldseguid("-TATGCC", "-GCATAC").unwrap(),
            "ldseguid=rr65d6AYuP-CdMaVmdw3L9FPt6I"
        );
        assert_eq!(
            ldseguid("-GCATAC", "-TATGCC").unwrap(),
            "ldseguid=rr65d6AYuP-CdMaVmdw3L9FPt6I"
        );
        assert_eq!(
            ldseguid("AT", "AT").unwrap(),
            "ldseguid=odgytmQKSOnFEUorGIWK3NDjqUA"
        );
        assert_eq!(
            cdseguid("GATTACA", "TGTAATC").unwrap(),
            "cdseguid=z7GBDOjQuqwVpDiiC_CEJkmOKZo"
        );
    }

    fn rc(s: &str) -> String {
        s.chars().rev().filter_map(complement).collect()
    }

    #[test]
    fn cdseguid_is_invariant_under_rotation() {
        let w = "GATTACAGGGCCC";
        let expect = cdseguid(w, &rc(w)).unwrap();
        for i in 0..w.len() {
            let r = rotate(w, i as isize);
            assert_eq!(cdseguid(&r, &rc(&r)).unwrap(), expect, "rotation {i}");
        }
    }

    #[test]
    fn cdseguid_is_invariant_under_swapping_the_strands() {
        let w = "GATTACAGGGCCC";
        let c = rc(w);
        assert_eq!(cdseguid(w, &c).unwrap(), cdseguid(&c, w).unwrap());
    }

    #[test]
    fn csseguid_is_invariant_under_rotation() {
        let s = "GATTACAGGGCCC";
        let expect = csseguid(s).unwrap();
        for i in 0..s.len() {
            assert_eq!(csseguid(&rotate(s, i as isize)).unwrap(), expect, "rot {i}");
        }
    }

    #[test]
    fn a_periodic_sequence_is_still_stable() {
        // Several rotations are minimal here; the answer must not depend on
        // which index the algorithm happens to return.
        let w = "ATATATAT";
        let expect = cdseguid(w, &rc(w)).unwrap();
        for i in 0..w.len() {
            let r = rotate(w, i as isize);
            assert_eq!(cdseguid(&r, &rc(&r)).unwrap(), expect, "rotation {i}");
        }
    }

    #[test]
    fn min_rotation_finds_the_smallest() {
        assert_eq!(min_rotation("TAAA"), 1);
        assert_eq!(rotate_to_min("TAAA"), "AAAT");
        assert_eq!(rotate_to_min("TTTA"), "ATTT");
        assert_eq!(rotate_to_min("ACGT"), "ACGT");
        let s = "ACAACAAACAACACAAACAAACACAAC";
        assert_eq!(rotate_to_min(s), "AAACAAACACAACACAACAAACAACAC");
    }

    #[test]
    fn min_rotation_agrees_with_brute_force() {
        // The cheap oracle: generate every rotation and take the smallest.
        for s in [
            "A",
            "AT",
            "TA",
            "AAAA",
            "ATATATAT",
            "GATTACA",
            "TTTTTTTA",
            "CAGCAGCAG",
            "ACAACAAACAACACAAACAAACACAAC",
            "GGGGGGGGGGGGGGGA",
            "TGCATGCATGCA",
        ] {
            let brute = (0..s.len()).map(|i| rotate(s, i as isize)).min().unwrap();
            assert_eq!(rotate_to_min(s), brute, "{s}");
        }
    }

    #[test]
    fn rotate_matches_the_reference_semantics() {
        assert_eq!(rotate("ABCDEFGH", 0), "ABCDEFGH");
        assert_eq!(rotate("ABCDEFGH", 1), "BCDEFGHA");
        assert_eq!(rotate("ABCDEFGH", 7), "HABCDEFG");
        assert_eq!(rotate("ABCDEFGH", -1), "HABCDEFG");
        assert_eq!(rotate("ABCDEFGH", 8), "ABCDEFGH");
    }

    #[test]
    fn bad_input_is_refused_rather_than_coerced() {
        assert_eq!(lsseguid(""), Err(Error::Empty));
        assert_eq!(lsseguid("gattaca"), Err(Error::NotInAlphabet('g')));
        assert_eq!(lsseguid("GATTACN"), Err(Error::NotInAlphabet('N')));
        assert_eq!(lsseguid("ACGU"), Err(Error::NotInAlphabet('U')));
        assert_eq!(
            cdseguid("AT", "ATG"),
            Err(Error::LengthMismatch {
                watson: 2,
                crick: 3
            })
        );
        assert_eq!(cdseguid("AT", "AA"), Err(Error::NotComplementary { at: 0 }));
    }
}
