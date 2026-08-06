//! `docs/PLAN.md` §v1.0 item 5: auto-annotation "in under 200 ms for a 10 kb
//! plasmid". This is the file that MEASURES it.
//!
//! The claim sat in the plan unmeasured. A number nobody recomputes is how
//! `rust-version` sat at a wrong `1.82` in this repository for months, and a
//! performance budget is worse than a version string: it is checked by nobody
//! and cited by everybody, and the day it stops holding is the day it silently
//! becomes marketing.
//!
//! # What is timed, and what is not
//!
//! [`Annotator::annotate`] alone, over a molecule already in memory. Not
//! parsing the file, not the k-mer index build, not process startup.
//!
//! The index build is excluded because it does not depend on the molecule and
//! both callers pay it once: `bins/pl-gui/src/featuredb.rs` holds it in a
//! `OnceLock` for the process, and `pl annotate` runs one molecule per process.
//! It is measured separately below anyway, because "excluded" is a claim about
//! its size as much as about where it lands, and
//! [`the_index_build_is_paid_once_and_is_small`] is what keeps that honest.
//!
//! # Why the assertion is loose and the report is exact
//!
//! Wall-clock on a shared CI runner is not a controlled measurement, so an
//! assertion tight enough to be interesting is an assertion that fails for
//! reasons that are not defects. The budget is therefore asserted with a
//! generous multiple, and the actual figure is printed on every run
//! (`cargo test -p pl-features --test budget -- --nocapture`) so that a real
//! regression shows up as a number a human reads rather than as a green tick.
//!
//! A debug build is NOT held to the budget at all, and that is not a dodge:
//! `docs/PLAN.md` is describing what a user's machine does, and no user runs an
//! unoptimised build. The debug figure is printed for scale — see
//! `docs/PLAN.md` where both are now written down.

use pl_core::{Molecule, Topology};
use pl_features::annotate::{Annotator, Config};
use pl_features::Db;

/// The plan's budget, in milliseconds.
const BUDGET_MS: f64 = 200.0;

/// How far over the budget a run may go before this fails.
///
/// 4× in release. The measured figure on the development machine is far below
/// the budget (see the module doc and `docs/PLAN.md`), so this is not a
/// weakened claim about the software — it is the allowance for a runner that is
/// swapping, throttled, or sharing a core with fifteen other jobs. What it
/// still catches is the failure that matters: an algorithmic change that makes
/// annotation an order of magnitude slower, which is the only kind of
/// regression a wall-clock test on unowned hardware can honestly detect.
#[cfg(not(debug_assertions))]
const SLACK: f64 = 4.0;

/// A deterministic filler generator.
///
/// xorshift64*, written out because `crates/` take no dependencies. It only has
/// to produce bases that do not accidentally reproduce a database feature, and
/// to produce the same ones on every machine so that two runs are comparable.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn dna(&mut self, n: usize) -> Vec<u8> {
        (0..n)
            .map(|_| b"ACGT"[(self.next() >> 33) as usize % 4])
            .collect()
    }
}

/// How the 10 kb is filled with real database features.
///
/// **Both, and the budget is asserted against the worse**, because which one is
/// worse was not guessable and the first guess was wrong. Measured on the
/// development machine, release build: `Dense` is 11 ms and `Large` is 103 ms.
/// Nine times the work from a molecule with a TENTH the number of hits in it —
/// because the cost is in the aligner, [`pl_features::align`] is a plain
/// dynamic program whose cost is the product of the two lengths, and four
/// multi-kilobase genes are a far larger product than thirty-odd tags and
/// terminators.
///
/// Had this file measured only the dense plasmid, which is the one that looks
/// like the harder case, it would have reported a tenth of the real worst
/// figure and called the budget met with room to spare.
#[derive(Clone, Copy, Debug)]
enum Packing {
    /// Shortest records first, so the most whole features fit: about 37 hits.
    /// What an ordinary expression plasmid looks like — tags, a terminator, a
    /// couple of promoters, a marker.
    Dense,
    /// Longest records first: about 4 hits, each of them kilobases long. A
    /// plasmid carrying two or three big CDSs, and the expensive case.
    Large,
}

/// A 10 kb circular plasmid that really does carry database features.
///
/// **Not 10 kb of random bases**, which would be the easy and useless
/// measurement: seeding rejects random text almost immediately, so a molecule
/// with no hits in it measures the k-mer scan and never reaches the aligner —
/// and the aligner is the expensive half. This one is built the way a plasmid
/// is: real reference sequences out of the shipped table, laid end to end with
/// filler between them, until the total is 10 kb.
///
/// Circular, because that is what a plasmid is and because
/// [`Annotator::annotate`] doubles a circular sequence before scanning it. The
/// text actually searched is therefore 20 kb, which is the case the budget has
/// to cover.
fn ten_kb_plasmid(db: &Db, packing: Packing) -> Molecule {
    const TARGET: usize = 10_000;
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut seq: Vec<u8> = Vec::with_capacity(TARGET);
    let mut refs: Vec<&[u8]> = db
        .records
        .iter()
        .map(|r| r.reference_nt.as_slice())
        .filter(|s| !s.is_empty())
        .collect();
    match packing {
        Packing::Dense => refs.sort_by_key(|s| s.len()),
        Packing::Large => refs.sort_by_key(|s| std::cmp::Reverse(s.len())),
    }
    for r in refs {
        if seq.len() + r.len() + 120 > TARGET {
            continue;
        }
        seq.extend_from_slice(&rng.dna(120));
        seq.extend_from_slice(r);
    }
    assert!(
        seq.len() > TARGET / 2,
        "{packing:?}: the shipped table no longer fills half a 10 kb plasmid with real \
         features; this measurement would be timing an empty scan"
    );
    seq.extend_from_slice(&rng.dna(TARGET - seq.len()));
    assert_eq!(seq.len(), TARGET);
    Molecule {
        seq,
        topology: Topology::Circular,
        ..Molecule::default()
    }
}

/// Median of `n` runs, in milliseconds, with the answer returned so that the
/// optimiser cannot delete the work.
fn time_ms(runs: usize, mut f: impl FnMut() -> usize) -> (f64, usize) {
    let mut times = Vec::with_capacity(runs);
    let mut found = 0;
    for _ in 0..runs {
        let t = std::time::Instant::now();
        found = f();
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.partial_cmp(b).expect("no NaN from a duration"));
    (times[times.len() / 2], found)
}

/// The number in `docs/PLAN.md`, measured.
///
/// PROVEN TO FAIL by wrapping the annotate call in a `for _ in 0..12` loop —
/// the cheapest available stand-in for an algorithmic regression:
///
/// ```text
/// Large: annotating a 10 kb plasmid took 1254.2 ms, which is over the 200 ms
/// budget docs/PLAN.md claims (allowance: 800.0 ms)
/// ```
///
/// It was also run with `SLACK` at 1.0, which passes: the real figure is inside
/// the budget itself and not merely inside the allowance.
#[test]
fn a_ten_kb_plasmid_is_annotated_inside_the_plan_s_budget() {
    let (db, errors) = Db::builtin();
    assert!(
        errors.is_empty(),
        "the shipped tables did not load: {errors:?}"
    );
    let db = db.reviewed();
    let annotator = Annotator::new(&db, Config::default());
    let build = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };

    for packing in [Packing::Dense, Packing::Large] {
        let mol = ten_kb_plasmid(&db, packing);
        let (ms, found) = time_ms(5, || annotator.annotate(&mol).len());
        assert!(
            found > 0,
            "{packing:?}: nothing was found in a molecule built out of database features, \
             so this measured a scan that never reached the aligner"
        );
        println!(
            "annotate {packing:?}: {ms:.1} ms for a 10 kb circular plasmid \
             ({found} hits, {} records, {build} build)",
            db.records.len()
        );

        // Debug is not held to the budget: `docs/PLAN.md` describes what a
        // user's machine does, and nobody ships an unoptimised build. The
        // figure is printed above either way, and both are in the plan.
        #[cfg(not(debug_assertions))]
        assert!(
            ms < BUDGET_MS * SLACK,
            "{packing:?}: annotating a 10 kb plasmid took {ms:.1} ms, which is over the \
             {BUDGET_MS:.0} ms budget docs/PLAN.md claims (allowance: {:.1} ms)",
            BUDGET_MS * SLACK
        );
    }
    let _ = BUDGET_MS;
}

/// The index build, which the budget above excludes, is small and is paid once.
///
/// The exclusion is only honest if the excluded thing is small. Both shipping
/// callers pay it exactly once — `bins/pl-gui/src/featuredb.rs` holds it in a
/// process `OnceLock`, `pl annotate` runs one molecule per process — so it is
/// not part of a per-document budget; but a build that grew to seconds would
/// make the first annotation of a session miss the budget in the only sense a
/// user experiences it, and nothing else would notice.
///
/// PROVEN TO FAIL by asserting `< 1.0` ms, which reports the real figure:
///
/// ```text
/// building the annotation index took 6.8 ms; it is paid once per process, but
/// at this size the first annotation of a session misses the plan's budget
/// ```
#[test]
fn the_index_build_is_paid_once_and_is_small() {
    let (db, _) = Db::builtin();
    let db = db.reviewed();
    let t = std::time::Instant::now();
    let annotator = Annotator::new(&db, Config::default());
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    // Used, so the build cannot be optimised away.
    assert!(annotator.db().records.len() == db.records.len());
    println!(
        "index build: {ms:.1} ms for {} records ({} build)",
        db.records.len(),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    #[cfg(not(debug_assertions))]
    assert!(
        ms < 500.0,
        "building the annotation index took {ms:.1} ms; it is paid once per process, but \
         at this size the first annotation of a session misses the plan's budget"
    );
}
