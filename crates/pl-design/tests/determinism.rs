//! The same input gives byte-identical output, and ties are broken by
//! something that is a property of the pair rather than of the search.
//!
//! A design tool whose output changes between runs cannot be diffed,
//! cross-checked or re-ordered, and a ranking that reshuffles when the search
//! window is widened is worse: the user saw pair 2, widened the flank to look
//! for something better, and pair 2 is now pair 4 with a different partner.

use pl_design::{design, Constraints, Mode, Region};

/// A template whose region carries an exact tandem duplication.
///
/// The duplication is what makes ties **guaranteed** rather than hoped for: a
/// candidate at `lo` and one at `lo + block` have identical sequences, so
/// identical Tm, %GC, ΔG, hairpin and self-dimer, so an identical penalty. Every
/// component of the score is equal and only the coordinate differs — which is
/// precisely the case the structural tie-break exists for.
fn tandem(block: usize, total: usize, seed: u64) -> Vec<u8> {
    let mut s = seed;
    let mut b = Vec::with_capacity(block);
    for _ in 0..block {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        b.push(b"ACGT"[((s >> 24) & 3) as usize]);
    }
    let mut out = Vec::with_capacity(total);
    while out.len() < total {
        let take = block.min(total - out.len());
        out.extend_from_slice(&b[..take]);
    }
    out
}

fn constraints(flank: u64) -> Constraints {
    Constraints {
        mode: Mode::Within,
        // The duplication means every candidate binds twice, so the off-target
        // gate would refuse all of them. It is switched off here **because the
        // fixture is deliberately repetitive**, and that is the only reason.
        specificity: false,
        max_pairs: 40,
        // Report near-neighbours too, so the ordering under test is visible
        // rather than filtered away by the diversity rule.
        min_separation: 1,
        // Lower than the shipped 200 only to keep a hundred repeats inside a
        // sane test runtime. It is a bound on the search, so it changes how
        // many pairs exist and not how two of them compare, which is what is
        // under test -- and `the_fixture_really_produces_ties` re-checks that
        // ties survive it.
        max_per_side: 60,
        flank,
        ..Default::default()
    }
}

#[test]
fn a_hundred_runs_of_one_input_are_byte_identical() {
    let template = tandem(300, 3_000, 41);
    let region = Region::new(401, 1_400);
    let c = constraints(200);

    let first = design(&template, false, region, &c).expect("pairs exist");
    let text = first.text("fixture");
    let json = first.json("fixture");
    for i in 1..100 {
        let again = design(&template, false, region, &c).expect("pairs exist");
        assert_eq!(again.text("fixture"), text, "run {i} differs");
        assert_eq!(again.json("fixture"), json, "run {i} differs");
    }
    assert!(!first.pairs.is_empty());
}

/// PROVEN TO FAIL: with the attrition table built from a `HashSet` instead of
/// the fixed array indexed by `Reason`, `a_hundred_runs_of_one_input_are_byte_identical`
/// reports `run 1 differs`. `RandomState` is seeded per instance, so two
/// `design()` calls in one process disagree on the order of the same rows.
///
/// Ties really occur — asserted **first**, because without it everything below
/// is vacuous the day the fixture changes.
///
/// This repo has already been bitten by exactly that: `pl-primer`'s `flip()`
/// helper exists because three tests silently became no-ops when a fixture
/// changed.
#[test]
fn the_fixture_really_produces_ties() {
    let template = tandem(300, 3_000, 41);
    let c = constraints(200);
    let r = design(&template, false, Region::new(401, 1_400), &c).expect("pairs exist");
    let mut q: Vec<i64> = r
        .pairs
        .iter()
        .map(|p| (p.penalty * 1e6).round() as i64)
        .collect();
    let before = q.len();
    q.sort_unstable();
    q.dedup();
    let ties = before - q.len();
    assert!(
        ties > 0,
        "the fixture must produce equal penalties, or the tie-break is untested \
         ({before} pairs, {} distinct penalties)",
        q.len()
    );
}

/// Two pairs with the same penalty keep their relative order when the search is
/// widened.
///
/// PROVEN TO FAIL: with the sort key truncated to the quantised penalty alone —
/// the obvious simplification, and the one a reader would make if the tuple
/// looked like belt-and-braces — this fires. `sort_unstable_by_key` is pdqsort,
/// whose arrangement of equal keys depends on the whole array, so widening the
/// flank from 40 to 200 changes how many candidates precede the tied pair and
/// the two swap: the run reports `tied pairs 766..1110 and 466..1110 came back
/// in the other order`. With the full key the order is a property of the two
/// pairs and cannot move.
#[test]
fn tied_pairs_keep_their_order_when_the_search_widens() {
    let template = tandem(300, 3_000, 41);
    let region = Region::new(401, 1_400);

    let narrow = design(&template, false, region, &constraints(40)).expect("pairs");
    let wide = design(&template, false, region, &constraints(200)).expect("pairs");

    // Group by quantised penalty and compare the order within each group.
    let key = |p: &pl_design::Pair| (p.penalty * 1e6).round() as i64;
    let seq = |r: &pl_design::Report| -> Vec<(i64, u64, u64)> {
        r.pairs
            .iter()
            .map(|p| (key(p), p.forward.start, p.reverse.end))
            .collect()
    };
    let a = seq(&narrow);
    let b = seq(&wide);

    for (i, x) in a.iter().enumerate() {
        for y in a.iter().skip(i + 1) {
            if x.0 != y.0 {
                continue;
            }
            let (px, py) = (
                b.iter().position(|z| z.1 == x.1 && z.2 == x.2),
                b.iter().position(|z| z.1 == y.1 && z.2 == y.2),
            );
            if let (Some(px), Some(py)) = (px, py) {
                assert!(
                    px < py,
                    "tied pairs {}..{} and {}..{} came back in the other order",
                    x.1,
                    x.2,
                    y.1,
                    y.2
                );
            }
        }
    }

    // And the tie-break really is the structural key: among equal penalties the
    // forward start ascends.
    for w in b.windows(2) {
        if w[0].0 == w[1].0 {
            assert!(
                w[0].1 <= w[1].1,
                "equal penalties must order by forward start: {:?} then {:?}",
                w[0],
                w[1]
            );
        }
    }
}

/// Nothing in the answer depends on when it was asked.
///
/// The structural half of the same claim: no clock, no environment, no
/// randomness, no hash iteration. `tests/purity.rs` checks the source; this
/// checks the behaviour, by running the identical search in a different process
/// state (a warmed allocator, a different thread) and comparing.
#[test]
fn the_answer_does_not_depend_on_the_process_it_was_computed_in() {
    let template = tandem(300, 3_000, 41);
    let region = Region::new(401, 1_400);
    let c = constraints(200);
    let want = design(&template, false, region, &c)
        .unwrap()
        .json("fixture");

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let t = template.clone();
            let c = c.clone();
            std::thread::spawn(move || design(&t, false, region, &c).unwrap().json("fixture"))
        })
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap(), want);
    }
}
