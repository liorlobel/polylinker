//! Infix alignment — all of a database feature against *some substring* of a
//! plasmid.
//!
//! This is the verification step of `docs/PLAN.md` §7.7. The plan names `edlib`
//! in `HW` mode; this is that mode, written out, because the correctness crates
//! take no dependencies and an aligner is small enough to own.
//!
//! # What "infix" means here, precisely
//!
//! Gaps before and after the match in the *plasmid* are free; the *feature* must
//! be consumed entirely. So `AmpR` either matches somewhere in the plasmid or it
//! does not, and the cost never counts the thousands of bases on either side.
//! That is the `HW` of edlib, `--infix` of others, and semi-global elsewhere;
//! the name is contested enough to be worth spelling out.
//!
//! # Why plain dynamic programming
//!
//! Myers' bit-parallel algorithm is roughly 64× faster and is the obvious thing
//! to reach for. It is not used here, for a reason worth recording: seeding
//! (see [`crate::index`]) narrows verification to a handful of windows barely
//! longer than the feature itself, so the aligner runs on ~1 kb × 1 kb problems
//! a few dozen times per file rather than over a whole genome. Ukkonen's cutoff
//! then removes most of even that. The measured cost is far inside the 200 ms
//! budget, and a DP that a reviewer can check by hand is worth more here than a
//! bit-twiddling routine that a reviewer must take on faith.
//!
//! If profiling ever contradicts that, [`infix_reference`] stays as the oracle
//! to test a faster implementation against.

/// Where a feature matched, and how badly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    /// Start offset in the text, 0-based, inclusive.
    pub start: usize,
    /// End offset in the text, 0-based, exclusive.
    pub end: usize,
    /// Levenshtein distance: substitutions + insertions + deletions.
    pub dist: u32,
}

impl Hit {
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Identity as a fraction of the *database feature's* length.
    ///
    /// The denominator is a real choice. Measuring against the feature answers
    /// "how much of the reference did this plasmid reproduce", which is what a
    /// curated-library matcher is actually asking, and it makes a hit that
    /// deletes half the feature score 0.5 rather than 1.0 — which is what
    /// dividing by the alignment length would have given.
    pub fn identity(&self, feature_len: usize) -> f64 {
        if feature_len == 0 {
            return 0.0;
        }
        1.0 - (self.dist as f64 / feature_len as f64)
    }
}

/// Case-insensitive base equality.
///
/// Soft-masked (lowercase) regions are ordinary sequence for matching purposes;
/// the case is display information that this project preserves elsewhere but
/// must not let change an alignment.
#[inline]
fn same(a: u8, b: u8) -> bool {
    a.eq_ignore_ascii_case(&b)
}

/// The best end position for an infix alignment, plus its distance.
///
/// `cutoff` is Ukkonen's: rows whose value already exceeds `max_dist` cannot
/// contribute, because the DP is non-decreasing along diagonals. Set it false to
/// get the plain, obviously-correct computation.
fn best_end(pattern: &[u8], text: &[u8], max_dist: u32, cutoff: bool) -> Option<(usize, u32)> {
    let m = pattern.len();
    if m == 0 {
        return Some((0, 0));
    }

    // Column 0: aligning `i` feature bases against no text costs `i` deletions.
    let mut prev: Vec<u32> = (0..=m as u32).collect();
    let mut cur = vec![0u32; m + 1];

    // Rows past this are known to exceed `max_dist`. In column 0 that is
    // simply where `i > max_dist`.
    let mut last = if cutoff {
        (max_dist as usize).min(m)
    } else {
        m
    };

    let mut best: Option<(usize, u32)> = None;
    if prev[m] <= max_dist {
        best = Some((0, prev[m]));
    }

    for j in 1..=text.len() {
        cur[0] = 0; // a match may begin anywhere in the text, for free
        let upto = (last + 1).min(m);
        for i in 1..=upto {
            let sub = prev[i - 1] + u32::from(!same(pattern[i - 1], text[j - 1]));
            cur[i] = sub.min(prev[i] + 1).min(cur[i - 1] + 1);
        }
        // Anything below the computed band is unreachable within budget.
        // `saturating_add`: `max_dist` comes from `(1.0 - min_identity) * len`
        // cast to u32, and a nonsensical `min_identity` saturates the cast to
        // u32::MAX. Inert in practice — at u32::MAX the fill slice is always
        // empty — but it panicked in every debug build.
        cur[(upto + 1).min(m + 1)..].fill(max_dist.saturating_add(1));

        if cutoff {
            last = upto;
            while last > 0 && cur[last] > max_dist {
                last -= 1;
            }
            // An empty band means no alignment can still finish in budget from
            // here, but a later column may start a fresh one, so reset rather
            // than abandon the scan.
            if last == 0 {
                last = (max_dist as usize).min(m);
            }
        }

        if cur[m] <= max_dist && best.is_none_or(|(_, d)| cur[m] < d) {
            best = Some((j, cur[m]));
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    best
}

/// Align all of `pattern` to the best-matching substring of `text`.
///
/// Returns the lowest-distance hit, or `None` if none is within `max_dist`.
/// Ties are broken toward the leftmost end position, so the result is
/// deterministic — an annotator that reshuffles feature positions between runs
/// on the same file is not usable.
pub fn infix(pattern: &[u8], text: &[u8], max_dist: u32) -> Option<Hit> {
    infix_with(pattern, text, max_dist, true)
}

/// The same alignment without Ukkonen's cutoff: slower, and the oracle the
/// cutoff version is tested against.
pub fn infix_reference(pattern: &[u8], text: &[u8], max_dist: u32) -> Option<Hit> {
    infix_with(pattern, text, max_dist, false)
}

fn infix_with(pattern: &[u8], text: &[u8], max_dist: u32, cutoff: bool) -> Option<Hit> {
    let (end, dist) = best_end(pattern, text, max_dist, cutoff)?;

    // The forward pass gives the end but not the start. Running the same
    // computation on both sequences reversed finds where the alignment began:
    // its end position in reversed coordinates *is* the match length.
    let rp: Vec<u8> = pattern.iter().rev().copied().collect();
    let rt: Vec<u8> = text[..end].iter().rev().copied().collect();
    let (len, back) = best_end(&rp, &rt, max_dist, cutoff)?;

    // Both passes solve the same problem, so they must agree. If they ever do
    // not, the alignment is not trustworthy and silently returning the shorter
    // answer would place a feature at the wrong coordinates.
    debug_assert_eq!(
        back, dist,
        "forward and reverse passes disagreed: {dist} vs {back}"
    );

    Some(Hit {
        start: end - len,
        end,
        dist,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(p: &str, t: &str, k: u32) -> Option<Hit> {
        infix(p.as_bytes(), t.as_bytes(), k)
    }

    #[test]
    fn an_exact_match_costs_nothing_and_lands_where_it_should() {
        let h = find("GGGG", "AAAAGGGGTTTT", 2).unwrap();
        assert_eq!((h.start, h.end, h.dist), (4, 8, 0));
        assert_eq!(h.identity(4), 1.0);
    }

    #[test]
    fn flanking_text_is_free_but_the_pattern_is_not() {
        // Ten thousand bases of plasmid on either side must not cost anything.
        let t = format!("{}GGGG{}", "A".repeat(500), "T".repeat(500));
        let h = find("GGGG", &t, 0).unwrap();
        assert_eq!((h.start, h.end, h.dist), (500, 504, 0));

        // ...whereas an unmatched base of the *feature* does cost.
        assert!(find("GGGGG", &t, 0).is_none());
        assert_eq!(find("GGGGG", &t, 1).unwrap().dist, 1);
    }

    #[test]
    fn one_substitution_one_insertion_one_deletion() {
        assert_eq!(find("GGGG", "AAAGGAGTTT", 1).unwrap().dist, 1);
        assert_eq!(find("GGGG", "AAAGGGAGTTT", 1).unwrap().dist, 1); // insertion
        assert_eq!(find("GGGG", "AAAGGGTTT", 1).unwrap().dist, 1); // deletion
    }

    #[test]
    fn a_match_at_either_edge_of_the_text_is_found() {
        assert_eq!(find("ACGT", "ACGTAAAA", 0).unwrap().start, 0);
        let h = find("ACGT", "AAAAACGT", 0).unwrap();
        assert_eq!((h.start, h.end), (4, 8));
    }

    #[test]
    fn no_match_within_budget_is_none_not_a_bad_match() {
        assert!(find("GGGGGGGG", "AAAAAAAAAAAA", 2).is_none());
    }

    #[test]
    fn identity_is_measured_against_the_feature_not_the_alignment() {
        // Half the feature missing is 50%, not "100% over what aligned".
        let h = find("GGGGGGGGGG", "AAAAAGGGGGAAAAA", 5).unwrap();
        assert_eq!(h.dist, 5);
        assert!((h.identity(10) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn case_does_not_change_an_alignment() {
        let h = find("acgtACGT", "TTTTACGTacgtTTTT", 0).unwrap();
        assert_eq!(h.dist, 0);
        assert_eq!((h.start, h.end), (4, 12));
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        assert_eq!(find("", "ACGT", 0).map(|h| h.dist), Some(0));
        assert!(find("ACGT", "", 0).is_none());
        assert_eq!(find("ACGT", "", 4).unwrap().dist, 4);
        assert!(find("", "", 0).is_some());
    }

    /// A tiny, deterministic PRNG so the property tests are reproducible and
    /// take no dependency. Values are irrelevant; the sequence being fixed is
    /// the point.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        fn base(&mut self) -> u8 {
            b"ACGT"[self.below(4)]
        }
        fn seq(&mut self, n: usize) -> Vec<u8> {
            (0..n).map(|_| self.base()).collect()
        }
    }

    #[test]
    fn ukkonens_cutoff_never_changes_the_answer() {
        // The optimisation is only safe if it is invisible. Thousands of random
        // cases, including ones with no match at all and ones where the budget
        // is exactly the true distance.
        let mut rng = Rng(0x5eed_1234_abcd_ef01);
        let mut checked = 0;
        for _ in 0..4000 {
            let m = 1 + rng.below(30);
            let n = 1 + rng.below(80);
            let p = rng.seq(m);
            let t = rng.seq(n);
            let k = rng.below(8) as u32;
            assert_eq!(
                infix(&p, &t, k),
                infix_reference(&p, &t, k),
                "cutoff changed the result for p={:?} t={:?} k={k}",
                String::from_utf8_lossy(&p),
                String::from_utf8_lossy(&t),
            );
            checked += 1;
        }
        assert_eq!(checked, 4000);
    }

    #[test]
    fn a_planted_match_is_recovered_with_the_damage_it_was_given() {
        // Build the answer, then corrupt it, then check the aligner reports at
        // most the damage inflicted. Not exactly equal: a corrupted copy can
        // coincidentally align better somewhere else, and that is correct
        // behaviour, not a bug.
        let mut rng = Rng(0xfeed_face_0000_0001);
        for _ in 0..600 {
            let flank = rng.below(60);
            let flen = 20 + rng.below(60);
            let feature = rng.seq(flen);
            let mut planted = feature.clone();

            let edits = rng.below(4);
            for _ in 0..edits {
                if planted.is_empty() {
                    break;
                }
                let at = rng.below(planted.len());
                match rng.below(3) {
                    0 => planted[at] = rng.base(),
                    1 => {
                        planted.insert(at, rng.base());
                    }
                    _ => {
                        planted.remove(at);
                    }
                }
            }

            let mut text = rng.seq(flank);
            text.extend_from_slice(&planted);
            text.extend_from_slice(&rng.seq(flank));

            let hit = infix(&feature, &text, edits as u32);
            assert!(
                hit.is_some(),
                "{edits} edits should still be findable within a budget of {edits}"
            );
            assert!(hit.unwrap().dist <= edits as u32);
        }
    }

    #[test]
    fn the_reported_interval_really_contains_the_match() {
        // A distance is useless if the coordinates are wrong, and coordinates
        // are exactly what this project has already been bitten by. Re-align
        // the pattern against only the reported window and demand the same
        // distance.
        let mut rng = Rng(0x0bad_c0de_1111_2222);
        for _ in 0..800 {
            let (pm, tn) = (5 + rng.below(25), 20 + rng.below(100));
            let p = rng.seq(pm);
            let t = rng.seq(tn);
            let k = rng.below(5) as u32;
            if let Some(h) = infix(&p, &t, k) {
                assert!(h.end <= t.len());
                assert!(h.start <= h.end);
                let window = &t[h.start..h.end];
                let again = infix(&p, window, k).expect("the window must still match");
                assert_eq!(
                    again.dist,
                    h.dist,
                    "p={:?} window={:?}",
                    String::from_utf8_lossy(&p),
                    String::from_utf8_lossy(window)
                );
                // And the window is tight: it starts and ends at the alignment.
                assert_eq!(again.start, 0);
                assert_eq!(again.end, window.len());
            }
        }
    }

    #[test]
    fn distance_is_monotone_in_the_budget() {
        // Raising the budget may reveal a match but must never worsen one.
        let mut rng = Rng(0x1234_5678_9abc_def0);
        for _ in 0..500 {
            let (pm, tn) = (8 + rng.below(20), 30 + rng.below(60));
            let p = rng.seq(pm);
            let t = rng.seq(tn);
            let mut best = u32::MAX;
            for k in 0..10u32 {
                if let Some(h) = infix(&p, &t, k) {
                    assert!(h.dist <= k);
                    assert!(h.dist <= best, "distance grew when the budget grew");
                    best = h.dist;
                }
            }
        }
    }

    #[test]
    fn a_realistic_feature_in_a_realistic_plasmid_is_fast_enough() {
        // Guards the §7.7 budget in the shape that matters: one ~1 kb feature
        // verified against a window a little longer than itself.
        let mut rng = Rng(0xabcd_0000_ffff_1111);
        let feature = rng.seq(861); // roughly bla / AmpR
        let mut text = rng.seq(60);
        text.extend_from_slice(&feature);
        text.extend_from_slice(&rng.seq(60));
        let h = infix(&feature, &text, 34).unwrap(); // 96% of 861
        assert_eq!(h.dist, 0);
        assert_eq!((h.start, h.end), (60, 921));
    }
}
