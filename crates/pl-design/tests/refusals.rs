//! What the tool says when it will not design, and what it counts while doing
//! it.
//!
//! `report.rs`'s module doc says an empty result is a first-class result, and
//! `Tally::terminal`'s doc says the machinery exists to stop advice that cannot
//! work and then loops. Every test here is a case where that promise was broken
//! in the shipped output: a sentence with a number in it that the same run
//! disproves, or a remedy that provably cannot move the thing it names.
//!
//! **Each was run against HEAD (dfd6ac9) in a clean clone and failed there at
//! runtime, not merely to compile** — every assertion below is written against
//! API that existed at HEAD for exactly that reason. The failure is recorded in
//! each test's comment.

use pl_design::{design, Constraints, DesignError, Mode, Region};

/// A deterministic pseudo-random template, the same LCG `design.rs` uses.
fn seq(n: usize, seed: u64) -> Vec<u8> {
    let mut s = seed;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        out.push(b"ACGT"[((s >> 24) & 3) as usize]);
    }
    out
}

fn ecori() -> &'static pl_enzymes::Enzyme {
    pl_enzymes::by_name("EcoRI").unwrap()
}

// ---------------------------------------------------------------------------
// Mode::Contain needs no template outside the region
// ---------------------------------------------------------------------------

/// "Amplify this whole fragment" is a design, not an error.
///
/// PROVEN TO FAIL at HEAD: the guard measured the bases lying *outside* the
/// region and refused when both sides had fewer than `len_min` of them, so this
/// call returns
/// `designing primers around this region needs template on both sides of it on
/// a 600 bp linear sequence; there is not enough. 0 nt available, 18 nt
/// minimum. Use --mode within...`. But `flank` bounds the primer's OUTER end:
/// the forward may start at the region's first base and the reverse may end at
/// its last, which is what `Mode::Contain`'s own doc calls the seamless-cloning
/// case. The remedy it offered — `--mode within` — produces a product missing
/// both ends of the selection, the exact failure the default exists to prevent.
#[test]
fn a_whole_linear_fragment_can_be_amplified_end_to_end_in_contain_mode() {
    let template = seq(600, 7);
    let c = Constraints {
        // Widened because a 600 bp fragment offers exactly one legal 5' start
        // per side; the criterion under test is the guard, not the chemistry.
        len_min: 18,
        len_max: 30,
        tm_min: 40.0,
        tm_max: 80.0,
        tm_diff_max: 20.0,
        ..Default::default()
    };

    let r = design(&template, false, Region::new(1, 600), &c)
        .expect("contain needs no template outside the region");
    let p = &r.pairs[0];
    assert_eq!(p.forward.start, 1, "the forward footprint starts at base 1");
    assert_eq!(p.reverse.end, 600, "the reverse footprint ends at base 600");
    assert_eq!(p.product_start, 1);
    assert_eq!(p.product_end, 600);
    assert_eq!(p.product_bp, 600, "the whole fragment");
    // The independent check: pl_clone::pcr makes the same molecule.
    assert_eq!(p.pcr_check, Ok(600));

    // The control, which is what stops this asserting that the guard was simply
    // deleted: a region whose start leaves no room for a forward footprint IS
    // refused, and the refusal names the end that has no room rather than
    // blaming a flank that is not the problem.
    let tight = Constraints { flank: 0, ..c };
    let err = design(&template, false, Region::new(590, 600), &tight)
        .expect_err("11 bases cannot hold an 18 nt footprint");
    match &err {
        DesignError::NoFlank {
            which,
            available,
            needed,
            ..
        } => {
            assert_eq!(*which, "at the start of the region");
            assert_eq!(*available, 11, "600 - 589");
            assert_eq!(*needed, 18);
        }
        other => panic!("{other}"),
    }
    let msg = err.to_string();
    assert!(msg.contains("no room for a primer"), "{msg}");
    assert!(
        msg.contains("needs no template OUTSIDE the region"),
        "the refusal must not repeat the premise that was wrong: {msg}"
    );
}

// ---------------------------------------------------------------------------
// The search bound is positional, and the refusal has to say so
// ---------------------------------------------------------------------------

/// The per-side cut must not delete the pair space it was bounding.
///
/// `cap` orders by the per-oligo penalty with the coordinate only as a
/// tie-break, so in `Mode::Within` over a long region the retained candidates
/// spread out and the expected pair count is
/// `max_per_side^2 * window / region_bp`. Here that is `5 * 5 * 31 / 30000` =
/// 0.026 — so the two sides, cut independently, have essentially no chance of
/// containing a single pair, while the region contains thousands.
///
/// PROVEN TO FAIL at HEAD, at runtime: `expect` fires on
/// `NoPair { survivors: 10, built: 0, .. }` — ten oligos reported as having
/// passed out of the 1,600-odd that did, no pair ever formed, and the advice
/// blaming Tm. With the reverse cut conditioned on the retained forwards, every
/// retained reverse has a partner by construction.
#[test]
fn a_within_search_over_a_long_region_still_finds_the_pairs_that_are_there() {
    let template = seq(31_000, 3);
    let c = Constraints {
        mode: Mode::Within,
        // Fixed length and a narrow window, so the arithmetic above is exact.
        len_min: 20,
        len_max: 20,
        product_min: 400,
        product_max: 430,
        max_per_side: 5,
        max_pairs: 5,
        // The template is unrepetitive by construction and the scan over 31 kb
        // is the slowest thing in this file; the criterion under test is the
        // cut, not the scan.
        specificity: false,
        ..Default::default()
    };

    let r = design(&template, false, Region::new(501, 30_500), &c)
        .expect("a 30 kb region holds thousands of 400-430 bp pairs");
    assert!(!r.pairs.is_empty());
    for p in &r.pairs {
        assert!(
            (400..=430).contains(&p.product_bp),
            "product {} bp is outside the window that was asked for",
            p.product_bp
        );
        assert!(
            p.product_start >= 501 && p.product_end <= 30_500,
            "both primers must lie inside the region: {}..{}",
            p.product_start,
            p.product_end
        );
    }
    // Both sides really were cut, so the test is about the cut and not about a
    // search small enough to avoid it.
    assert!(
        r.survivors_forward > c.max_per_side && r.survivors_reverse > c.max_per_side,
        "the bound has to bite or this proves nothing: {} / {}",
        r.survivors_forward,
        r.survivors_reverse
    );
}

/// A refusal must not report the number the cut left behind as the number that
/// passed, and must say that a cut happened.
///
/// PROVEN TO FAIL at HEAD, at runtime, on both counts: `survivors` was read off
/// `forward`/`reverse` *after* `cap` truncated them in place, so the message
/// opened "400 oligos passed on their own" — exactly `2 * max_per_side`,
/// whatever the truth — one line above a table from which the reader subtracts
/// and gets 1,291. And the `bound` sentence was pushed onto the success
/// `Report`'s warnings, which do not exist on this path, so the run where the
/// cut decided the answer was the one run that never mentioned it.
#[test]
fn a_refusal_reports_the_survivors_that_passed_and_discloses_the_cut() {
    let mut template = seq(3_000, 31);
    template[1_200..1_206].copy_from_slice(b"GAATTC");
    let c = Constraints {
        tail_five: Some(pl_design::params::Tailspec {
            enzyme: ecori(),
            spacer: b"TTAAGG".to_vec(),
        }),
        ..Default::default()
    };

    let err = design(&template, false, Region::new(1_001, 1_400), &c)
        .expect_err("every product carries an unusable EcoRI");
    let (survivors, enumerated, tally) = match &err {
        DesignError::NoPair {
            survivors,
            enumerated,
            tally,
            ..
        } => (*survivors, *enumerated, tally),
        other => panic!("{other}"),
    };
    // The premise: the bound really did bite.
    assert!(
        survivors > 2 * c.max_per_side,
        "survivors {survivors} is at most 2 x max_per_side, so this is the post-cap \
         number and the finding is unfixed"
    );
    // And it is the number the table's own arithmetic gives: a reader who
    // subtracts the candidate-stage rows from `enumerated` must land on it.
    let candidate_stage: u32 = pl_design::Reason::ALL
        .iter()
        .filter(|r| !r.pair_stage())
        .map(|r| tally.get(*r))
        .sum();
    assert_eq!(
        survivors as u32,
        enumerated as u32 - candidate_stage,
        "the sentence and the table have to agree"
    );

    let msg = err.to_string();
    assert!(
        msg.contains("only the best 200 per side were paired"),
        "the refusal never says a cut happened: {msg}"
    );
}

// ---------------------------------------------------------------------------
// The product window is a gate nobody counted
// ---------------------------------------------------------------------------

/// A window no pair of survivors can satisfy must be named, not blamed on Tm.
///
/// Every combination the window excludes is skipped by the pairing loop's
/// `partition_point`/`break`, before `built += 1` and before any `tally.bump`,
/// so the tally holds candidate-stage counts only and `advice` falls through to
/// the largest of them.
///
/// PROVEN TO FAIL at HEAD, at runtime: the message reads
/// `400 oligos passed on their own and all 0 pairs were rejected`, a table of
/// candidate-stage counts, and then `Tm is the binding constraint. Widen the
/// LENGTH range first -- --len 15..40 is the usual move`. No length turns an
/// 800 bp span into a 2,000 bp product; the enumeration bounds on `lo` and `hi`
/// are fixed by `--mode`, `--flank` and `--region`. The one number that explains
/// the refusal appeared nowhere in the output.
#[test]
fn a_product_window_no_pair_can_reach_is_named_rather_than_blamed_on_tm() {
    let template = seq(3_000, 31);
    let c = Constraints {
        product_min: 2_000,
        product_max: 3_000,
        ..Default::default()
    };
    let err = design(&template, false, Region::new(1_001, 1_400), &c)
        .expect_err("a 400 bp region at flank 200 spans at most 800 bp");
    match &err {
        DesignError::NoPair { built, .. } => assert_eq!(*built, 0, "the window skips them all"),
        other => panic!("{other}"),
    }
    let msg = err.to_string();
    assert!(
        msg.contains("inside --product 2000..3000 bp"),
        "the refusal must name the window that refused it: {msg}"
    );
    assert!(
        msg.contains("The survivors reach"),
        "and the span they can actually reach: {msg}"
    );
    assert!(
        !msg.contains("Widen the LENGTH range first"),
        "a remedy that cannot move a span bounded by --mode, --flank and --region: {msg}"
    );
    assert!(
        !msg.contains("all 0 pairs were rejected"),
        "a sentence about a rejection that never happened: {msg}"
    );
}

// ---------------------------------------------------------------------------
// One funnel's size must not silence the other funnel's diagnosis
// ---------------------------------------------------------------------------

/// A pair-stage gate that killed 100% of the pairs outranks a candidate-stage
/// count a hundred times larger.
///
/// PROVEN TO FAIL at HEAD, at runtime: `Tally::terminal`'s floor was a share of
/// `total()`, which spans both funnels, so with ~41,000 candidate-stage
/// rejections the floor is ~413 and a gate that rejected all 250 pairs built is
/// below it. `terminal()` returns `None`, `binding()` returns Tm, and the report
/// prints "Widen the LENGTH range first" directly beneath a table row reading
/// "the added restriction site already occurs in the product <- all N that
/// reached it". The correct branch's own words are "No threshold moves this",
/// and it is the only code path that ever renders the clash coordinates that
/// `note_clash` computed — so the enzyme name and the template positions were
/// dropped too.
#[test]
fn a_pair_gate_that_killed_every_pair_is_not_silenced_by_a_larger_candidate_funnel() {
    // GAATTC every 100 bp, so no ~200 bp amplicon inside the region can avoid
    // one and the internal-site gate is 100% fatal.
    let mut template = seq(4_000, 11);
    for k in 0..28 {
        let at = 600 + k * 100;
        template[at..at + 6].copy_from_slice(b"GAATTC");
    }
    let c = Constraints {
        mode: Mode::Within,
        product_min: 200,
        product_max: 202,
        specificity: false,
        tail_five: Some(pl_design::params::Tailspec {
            enzyme: ecori(),
            spacer: Vec::new(),
        }),
        ..Default::default()
    };
    let err = design(&template, false, Region::new(501, 3_500), &c)
        .expect_err("no 200 bp amplicon here avoids an EcoRI site");
    let (built, enumerated, tally) = match &err {
        DesignError::NoPair {
            built,
            enumerated,
            tally,
            ..
        } => (*built, *enumerated, tally),
        other => panic!("{other}"),
    };
    // The premise, checked rather than hoped for: the gate saw a whole
    // population and killed all of it, and the other funnel dwarfs it.
    assert!(built > 0, "pairs have to be built for a pair gate to bind");
    let n = tally.get(pl_design::Reason::InternalSite);
    assert_eq!(
        n,
        tally.reached(pl_design::Reason::InternalSite, enumerated, built),
        "the gate has to be 100% fatal or this is a different test"
    );
    // And the premise that makes the fix necessary: a 1% floor taken over the
    // WHOLE tally is above this gate's count, so the old rule silenced it.
    assert!(
        n < (tally.total() / 100).max(1),
        "a floor over total() has to silence this gate or the test proves nothing: \
         n={n}, total={}",
        tally.total()
    );

    let msg = err.to_string();
    assert!(
        msg.contains("No threshold moves this"),
        "the diagnosis was silenced by the other funnel's size: {msg}"
    );
    assert!(
        msg.contains("EcoRI also reads at"),
        "the clash coordinates are computed and then dropped: {msg}"
    );
    assert!(
        !msg.contains("Widen the LENGTH range first"),
        "no length moves a site that is already in the template: {msg}"
    );
}

// ---------------------------------------------------------------------------
// The off-the-end remedy has to know which side is stuck
// ---------------------------------------------------------------------------

/// `--flank` cannot widen a window the molecule's end has already clipped.
///
/// PROVEN TO FAIL at HEAD, at runtime: the remedy was the unconditional
/// "There is not enough template outside the region. Raise --flank past 200, or
/// use --mode within." Following it gives "past 400", then "past 800", then
/// "past 1600" — forward survivors 0 at every step, because the only legal 5'
/// start is still base 1 and every base `--flank` adds is off the end. And for
/// a region too short to hold a pair the second remedy refuses too, which the
/// sentence never checked.
#[test]
fn the_off_the_end_remedy_names_the_side_the_molecule_clipped() {
    // The first 40 bases are A/T only, so no forward candidate at the one legal
    // 5' start survives the Tm window and the empty side is the clipped one.
    let mut template = seq(3_000, 5);
    for b in template[..40].iter_mut() {
        *b = b'A';
    }

    let err = design(
        &template,
        false,
        Region::new(1, 400),
        &Constraints::default(),
    )
    .expect_err("no forward candidate survives here");
    let tally = match &err {
        DesignError::NoCandidate { tally, .. } => tally,
        other => panic!("{other}"),
    };
    // The premise: off-the-end really is what the tally blames.
    assert_eq!(
        tally.binding(),
        Some(pl_design::Reason::OffTheEnd),
        "this fixture has to make OffTheEnd the reported reason"
    );

    let msg = err.to_string();
    assert!(
        msg.contains("adds no forward candidate at all"),
        "the remedy still points at the side the end has clipped: {msg}"
    );
    assert!(
        msg.contains("--mode within puts both primers inside"),
        "and the remedy that does work has to be the one offered: {msg}"
    );

    // The case where BOTH offered remedies used to refuse: a region too short
    // to hold a pair, at a linear end.
    let short = design(
        &template,
        false,
        Region::new(1, 30),
        &Constraints::default(),
    )
    .expect_err("30 bases at a linear end")
    .to_string();
    assert!(
        short.contains("--mode within is not a way out either"),
        "sending the user to a mode that refuses on arrival: {short}"
    );
    assert!(short.contains("two 18 nt footprints need 36"), "{short}");
}

// ---------------------------------------------------------------------------
// A counter that means one thing must not be labelled as another
// ---------------------------------------------------------------------------

/// "Product outside 100-3000 bp" printed beneath five products of ~2,600 bp.
///
/// The pairing loop bounds `r.hi` to the product window before anything is
/// counted, so every pair reaching `span_bases` already satisfies it. The only
/// thing that counter can mean is the one-turn cap on a circle.
///
/// PROVEN TO FAIL at HEAD, at runtime: this successful run's attrition table
/// reads `4955  product outside 100-3000 bp` while every reported amplicon is
/// inside 100-3000 — a false statement about the user's own numbers in a report
/// the tool is otherwise returning as correct. If it ever becomes the terminal
/// gate the remedy is "Widen --product past 100..3000", which admits strictly
/// more of them.
#[test]
fn the_over_one_turn_counter_is_labelled_as_what_it_counts() {
    let template = seq(2_686, 7);
    let r = design(
        &template,
        true,
        Region::new(100, 2_500),
        &Constraints::default(),
    )
    .expect("a 2,686 bp circle designs here");
    assert!(!r.pairs.is_empty());
    let text = r.text("fixture");

    // The premise: the counter fired on this run.
    assert!(
        text.contains("longer than the molecule itself"),
        "the fixture has to reach the counter under test: {text}"
    );
    // Every reported product really is inside the window the old label blamed.
    for p in &r.pairs {
        assert!(
            (100..=3_000).contains(&p.product_bp),
            "product {} bp",
            p.product_bp
        );
    }
    assert!(
        !text.contains("product outside"),
        "a row asserting the opposite of the products printed above it: {text}"
    );
    assert!(
        !r.json("fixture").contains("product_length"),
        "and the machine-readable key said it too: {}",
        r.json("fixture")
    );
}

// ---------------------------------------------------------------------------
// A criterion the user asked for must not be dropped in silence
// ---------------------------------------------------------------------------

/// `--product-opt` at the top of the window is a request, not a no-op.
///
/// PROVEN TO FAIL at HEAD, at runtime: the guard was
/// `Some(target) if target > 0 && c.product_max > target`, so a target equal to
/// `product_max` fell to the `_ => 0.0` arm and every reported pair came back
/// with `"product": 0.000000` — the criterion switched off, with no error, no
/// warning, and no echo of the value anywhere in the report, while the weights
/// line went on printing `product 1.0`. Rank 1 was a 428 bp amplicon for a user
/// who asked for 500.
#[test]
fn a_product_target_at_the_top_of_the_window_still_ranks() {
    let template = seq(600, 7);
    let c = Constraints {
        product_min: 100,
        product_max: 500,
        product_target: Some(500),
        ..Default::default()
    };
    let r = design(&template, false, Region::new(250, 350), &c).expect("pairs exist here");
    assert!(r.pairs.len() > 1, "need two sizes to compare");

    let term = |p: &pl_design::Pair| {
        p.terms
            .iter()
            .find(|(k, _)| *k == "product")
            .map(|(_, v)| *v)
            .expect("the product term is always reported")
    };
    assert!(
        r.pairs.iter().any(|p| term(p) > 0.0),
        "every pair scored 0 on a criterion the user asked for: {:?}",
        r.pairs.iter().map(term).collect::<Vec<_>>()
    );
    // And it ranks in the right direction: the pair nearest 500 bp scores least.
    let mut by_size = r.pairs.clone();
    by_size.sort_by_key(|p| (500i64 - p.product_bp as i64).abs());
    assert!(
        term(&by_size[0]) <= term(&by_size[by_size.len() - 1]),
        "the nearest amplicon must not be penalised more than the farthest"
    );

    // The value is echoed where the window is read from, so a dropped or
    // clamped target could be seen at all.
    assert!(
        r.constraints.contains("target 500 bp"),
        "the requested size reached no surface in the report: {}",
        r.constraints
    );
}

// ---------------------------------------------------------------------------
// The oligo that is ordered is the one that folds
// ---------------------------------------------------------------------------

/// A tail is out of the Tm and IS in the fold, and the report has to say both.
///
/// PROVEN TO FAIL at HEAD, at runtime: `oligo::evaluate` screens the footprint,
/// which is all it has when a candidate is enumerated, and nothing ever folded
/// `Primer::oligo()`. So a report could print a hairpin of `none found` for an
/// oligo whose own screen value is past the gate the same run applied to
/// footprints — here the spacer alone folds at about −9.5 kcal/mol against a
/// −5.0 gate, and the assertion on "whole oligo" fires because that number
/// reached no surface at all.
#[test]
fn the_whole_ordered_oligo_has_its_structure_reported_beside_the_gated_one() {
    let template = seq(3_000, 61);
    // A spacer that folds on its own: the fixture `fold.rs` uses for a 6 bp
    // stem, so the whole-oligo number is a property of the tail and not of the
    // template.
    let c = Constraints {
        tail_five: Some(pl_design::params::Tailspec {
            enzyme: ecori(),
            spacer: b"GGGGCCAAAAGGCCCC".to_vec(),
        }),
        ..Default::default()
    };
    let r = design(&template, false, Region::new(1_001, 1_400), &c).expect("pairs exist here");
    let p = &r.pairs[0];

    // The premise: the gated number passed and the ordered oligo's does not.
    let whole = pl_design::fold::hairpin(&p.forward.oligo(), &Constraints::DG_TABLE);
    assert!(
        p.forward.hairpin.dg > c.dg_hairpin,
        "the footprint passed the gate: {:?}",
        p.forward.hairpin
    );
    assert!(
        whole.dg <= c.dg_hairpin,
        "the fixture has to cross the gate on the whole oligo: {whole:?}"
    );

    let text = r.text("fixture");
    assert!(
        text.contains("whole oligo, tail included, NOT gated"),
        "the ordered oligo's structure reached no surface: {text}"
    );
    assert!(
        text.contains(&format!("{:.1}", whole.dg)),
        "and the number printed has to be that oligo's: {text}"
    );
    assert!(
        r.json("fixture").contains("hairpin_dg37_whole_oligo"),
        "a --json consumer must not lose it"
    );
    assert!(
        r.warnings
            .iter()
            .any(|w| w.contains("GATED and RANKED") && w.contains("FOOTPRINTS'")),
        "and the report has to say which number was screened: {:?}",
        r.warnings
    );
}

// ---------------------------------------------------------------------------
// A seed longer than the primer is a configuration, not a molecule
// ---------------------------------------------------------------------------

/// `--off-seed` above the shortest `--len` is refused rather than crashed on.
///
/// PROVEN TO FAIL at HEAD, at runtime: `specificity::scan`'s index path computed
/// `&primer[primer.len() - seed_len..]` with no length guard, so this call
/// aborts the process — debug "attempt to subtract with overflow", release
/// "range start index 18446744073709551615 out of range for slice of length 19".
/// Nothing enforced the relation: `--off-seed` is validated against 8..32 and
/// `--len` against 8..60, independently, and `Constraints` had no validator.
#[test]
fn a_seed_longer_than_the_shortest_primer_is_refused_and_does_not_panic() {
    let template = seq(3_000, 7);
    let c = Constraints {
        off_seed: 20,
        ..Default::default()
    };
    assert!(c.off_seed > c.len_min, "the premise");
    let err = design(&template, false, Region::new(1_000, 1_600), &c)
        .expect_err("a seed longer than the primer cannot anchor");
    let msg = err.to_string();
    assert!(msg.contains("--off-seed 20"), "{msg}");
    assert!(
        msg.contains("18"),
        "the shortest --len has to be named: {msg}"
    );

    // The control: at or below `len_min` the same search runs.
    let ok = Constraints {
        off_seed: 18,
        ..c.clone()
    };
    assert!(design(&template, false, Region::new(1_000, 1_600), &ok).is_ok());

    // And turning the scan off removes the relation along with the scan.
    let unchecked = Constraints {
        specificity: false,
        ..c
    };
    assert!(design(&template, false, Region::new(1_000, 1_600), &unchecked).is_ok());
}

// ---------------------------------------------------------------------------
// The qualifier on a screened dG has to be one the real structure satisfies
// ---------------------------------------------------------------------------

/// A screen result must not be printed with the inequality pointing the wrong
/// way.
///
/// The companion of `fold.rs`'s own unit test, written against API that exists
/// at HEAD — `Structure::render` took no arguments there and takes none now, so
/// unlike the in-module version this one compiles against unfixed code and
/// fails at runtime rather than at the type checker.
///
/// PROVEN TO FAIL at HEAD, at runtime: `render` printed `>= -9.5 (6 bp helix)`,
/// and `SCREEN_NOTE` — carried in every report's warnings — ended "That is why
/// each is printed as >=". `>=` is honest for a hairpin, whose omitted
/// loop-initiation term is positive, and dishonest for a dimer, whose omissions
/// all remove stabilisation; the only thing `render` is ever called on in a
/// report is the 3' cross-dimer. The measured counterexample is below: the
/// screen sees one helix of a two-helix structure, and the value it excludes
/// with `>=` is exactly the one the qualifier was introduced to warn about.
#[test]
fn a_screened_helix_is_qualified_in_a_direction_the_real_structure_satisfies() {
    let screened = pl_design::fold::dimer(
        b"ATTATTATTATTATTGCGAGCG",
        b"ATTATTATTATTATTCGCACGC",
        &Constraints::DG_TABLE,
    )
    .1;
    // The premise: the screen found a helix, so there is a number to qualify,
    // and it passes the shipped gate.
    assert!(screened.pairs > 0, "{screened:?}");
    assert!(
        screened.dg > Constraints::default().dg_dimer_three_prime,
        "the premise: this pair passes the gate ({screened:?})"
    );
    // And the premise that decides the direction: the real structure is MORE
    // stable than the screen's number, so `>= {number}` excludes it.
    let two_helices = 2.0 * pl_thermo::dg37_stacks(b"GCG", &Constraints::DG_TABLE).unwrap();
    assert!(two_helices < screened.dg, "{two_helices} vs {screened:?}");

    let r = screened.render();
    assert!(
        !r.contains(">="),
        "the printed relation excludes the value it exists to warn about: {r}"
    );
    assert!(r.contains("or more stable"), "{r}");

    // The sentence the report carries beside it must not still be justifying
    // the operator that was removed.
    assert!(
        !pl_design::fold::SCREEN_NOTE.contains("printed as >="),
        "the note endorsed the direction that was wrong: {}",
        pl_design::fold::SCREEN_NOTE
    );

    // And it reaches a real report rather than only the unit under test. The
    // check is scoped to the rendered structure: `Constraints::describe_dg`
    // prints the ACCEPTANCE CONDITIONS as ">= -6.0 kcal/mol" a few lines above,
    // and those are `>=` correctly — a gate is a threshold, not a screen
    // result. Flipping every `>=` in the report would have replaced one wrong
    // claim with another.
    let template = seq(3_000, 61);
    let r = design(
        &template,
        false,
        Region::new(1_001, 1_400),
        &Constraints::default(),
    )
    .expect("pairs exist here");
    let text = r.text("fixture");
    let rendered: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("cross-dimer dG37") && !l.contains("none found"))
        .collect();
    assert!(
        !rendered.is_empty(),
        "the fixture has to print at least one screened structure: {text}"
    );
    for l in &rendered {
        assert!(
            !l.contains(">="),
            "a report line still reads as the inequality the wrong way round: {l}"
        );
        assert!(l.contains("or more stable"), "{l}");
    }
    // The gate thresholds keep their `>=`, which is what makes the line above a
    // scoped check rather than a blanket ban on the character.
    assert!(
        text.contains("3'-end dimer >= -6.0 kcal/mol"),
        "the acceptance conditions are a threshold and read correctly: {text}"
    );
}

// ---------------------------------------------------------------------------
// The frame note has to reason about the length the digest leaves behind
// ---------------------------------------------------------------------------

/// The spacer is 5' of the cut, so it is not in the number that moves the frame.
///
/// The companion of `tail.rs`'s own unit test, routed through `design` so it
/// compiles against HEAD — `Tail::frame_note` gained a `which` argument in the
/// fix, so the in-module version cannot be run against unfixed code at all.
///
/// PROVEN TO FAIL at HEAD, at runtime: both tails printed "this tail adds 8 nt
/// to the 5' end of the product. If you are cloning in frame, this shifts the
/// reading frame." The reverse tail's 8 nt land on the 3' end of the product's
/// top strand, and the frameshift claim is false: `pl digest` on the amplicons
/// from spacers of 0, 1 and 2 nt yields a byte-identical insert, because what
/// separates the vector from the first template base after ligation is the
/// regenerated 6 nt site and never the spacer.
#[test]
fn the_frame_note_reasons_about_the_regenerated_site_and_names_the_right_end() {
    let template = seq(3_000, 61);
    let c = Constraints {
        tail_five: Some(pl_design::params::Tailspec {
            enzyme: ecori(),
            spacer: b"GG".to_vec(),
        }),
        tail_three: Some(pl_design::params::Tailspec {
            enzyme: pl_enzymes::by_name("HindIII").expect("HindIII ships"),
            spacer: b"GG".to_vec(),
        }),
        ..Default::default()
    };
    let r = design(&template, false, Region::new(1_001, 1_400), &c).expect("pairs exist here");
    let text = r.text("fixture");

    // The premise: both tails are 8 nt, so the old arithmetic had something to
    // be wrong about.
    assert!(text.contains("adds 8 nt"), "{text}");
    assert!(
        !text.contains("this shifts the reading frame"),
        "an affirmative frame claim about a construct the digest leaves frame-neutral: {text}"
    );
    assert!(
        text.contains("6, not 8"),
        "the regenerated site is the length that moves the frame: {text}"
    );
    // And the two tails land at opposite ends of the product's top strand.
    assert!(
        text.contains("5' end of the product's top strand"),
        "{text}"
    );
    assert!(
        text.contains("3' end of the product's top strand"),
        "{text}"
    );
}
