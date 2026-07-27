//! Does the library stay within its budgets at the size it is built for?
//!
//! `tools/ci.ps1` has one numeric floor in it — the benchmark score — and
//! nothing that would notice an index costing 800 MB or a query allocating 92
//! MB. That is the gap this file closes, and it is not hypothetical: the first
//! index of a real lab drive came out at 5.9 GB, and every functional test
//! passed.
//!
//! Memory is measured with a **counting global allocator** rather than by
//! shelling out for RSS: zero dependencies, identical on all three CI operating
//! systems, and it measures what this crate allocated rather than what the
//! process happens to be holding.
//!
//! The floors are deliberately loose — they are tripwires for an order-of-
//! magnitude regression, not benchmarks. Raise one only with a reason in the
//! commit message, the same rule as `$BenchFloor`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let now = LIVE.fetch_add(l.size(), Ordering::Relaxed) + l.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            let now = if new >= l.size() {
                LIVE.fetch_add(new - l.size(), Ordering::Relaxed) + (new - l.size())
            } else {
                LIVE.fetch_sub(l.size() - new, Ordering::Relaxed) - (l.size() - new)
            };
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

fn reset_peak() {
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}
fn peak_since() -> usize {
    PEAK.load(Ordering::Relaxed)
        .saturating_sub(LIVE.load(Ordering::Relaxed))
}

use pl_index::codec::{self, Library};
use pl_index::query::{run, Query};
use pl_index::scan::Motif;
use pl_index::{nibble, Row, State, Topology};

/// 3,000 plasmids of 8 kb: the size `docs/PLAN.md` names, a little larger than
/// the measured plasmid subset of a real drive.
const RECORDS: usize = 3_000;
const BASES_EACH: usize = 8_000;

fn rng(state: &mut u64) -> u64 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    state.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

fn build() -> Library {
    let mut st = 0x5123_4567_89ab_cdefu64;
    let alphabet = b"ACGT";
    let mut all = Vec::with_capacity(RECORDS * BASES_EACH);
    let mut rows = Vec::with_capacity(RECORDS);
    for i in 0..RECORDS {
        let seq: Vec<u8> = (0..BASES_EACH)
            .map(|_| alphabet[(rng(&mut st) % 4) as usize])
            .collect();
        rows.push(Row {
            path: format!("dir{}/plasmid-{i}.gb", i % 40),
            record: 0,
            size: 20_000,
            mtime_ns: 1_700_000_000_000_000_000,
            content: "0".repeat(40),
            state: State::Ok,
            name: format!("pTEST-{i}"),
            topology: Topology::Circular,
            length: BASES_EACH as u64,
            n_features: 12,
            text: "AmpR\nori\nlacZ-alpha\nT7 promoter\nM13 fwd".into(),
            seq_off: (i * BASES_EACH) as u64,
            seq_bases: BASES_EACH as u64,
            ..Default::default()
        });
        all.extend_from_slice(&seq);
    }
    Library {
        root: "C:/lab/plasmids".into(),
        built_ns: 1,
        complete: true,
        packed: nibble::pack(&all),
        packed_bases: all.len() as u64,
        rows,
    }
}

#[test]
fn a_three_thousand_plasmid_library_stays_within_its_budgets() {
    let lib = build();
    assert_eq!(lib.packed_bases, (RECORDS * BASES_EACH) as u64);

    // --- the file ---------------------------------------------------------
    let t = std::time::Instant::now();
    let bytes = codec::to_bytes(&lib);
    let write_ms = t.elapsed().as_millis();
    let mb = bytes.len() as f64 / 1e6;
    println!("index: {:.1} MB, written in {write_ms} ms", mb);
    // 24 Mbase packs to 12 MB; the table adds a few. 40 MB is a tripwire for
    // an order-of-magnitude regression, not a target.
    assert!(mb < 40.0, "index is {mb:.1} MB");

    // --- opening it -------------------------------------------------------
    let t = std::time::Instant::now();
    let back = codec::parse(&bytes).expect("parse");
    let open_ms = t.elapsed().as_millis();
    println!("open (incl. SHA-1 over every byte): {open_ms} ms");
    assert_eq!(back.rows.len(), RECORDS);
    assert!(open_ms < 3_000, "opening took {open_ms} ms");

    // --- searching it -----------------------------------------------------
    for pattern in ["GAATTC", "GGWCC", "CCANNNNNTGG"] {
        reset_peak();
        let q = Query {
            motif: Some(Motif::new(pattern).unwrap()),
            ..Default::default()
        };
        let t = std::time::Instant::now();
        let r = run(&back.rows, &back.packed, &q);
        let ms = t.elapsed().as_millis();
        let peak_mb = peak_since() as f64 / 1e6;
        println!(
            "search {pattern:>12}: {ms:>5} ms, {:>7} hits, peak +{peak_mb:.1} MB",
            r.total_hits
        );
        assert_eq!(r.coverage.searched, RECORDS);
        // Measured throughput is ~335 Mbase/s; 24 Mbase is ~72 ms. A second
        // is an order of magnitude of headroom.
        assert!(ms < 1_000, "{pattern} took {ms} ms");
        // The scan must not materialise a reverse-complemented corpus, which
        // would cost another 12 MB per query.
        assert!(peak_mb < 40.0, "{pattern} peaked at +{peak_mb:.1} MB");
    }

    // --- text search ------------------------------------------------------
    reset_peak();
    let t = std::time::Instant::now();
    let r = run(
        &back.rows,
        &back.packed,
        &Query {
            text: Some("lacZ".into()),
            ..Default::default()
        },
    );
    let ms = t.elapsed().as_millis();
    println!("search by text: {ms} ms, {} records", r.matches.len());
    assert_eq!(r.matches.len(), RECORDS);
    assert!(ms < 500, "text search took {ms} ms");
}

#[test]
fn the_whole_library_fits_in_memory_many_times_over() {
    // The claim the design rests on: hold everything resident, never page.
    reset_peak();
    let lib = build();
    let live = lib.packed.len()
        + lib
            .rows
            .iter()
            .map(|r| r.text.len() + r.path.len())
            .sum::<usize>();
    let mb = live as f64 / 1e6;
    println!("resident: ~{mb:.1} MB for {RECORDS} records");
    assert!(mb < 100.0, "resident set is {mb:.1} MB");
}
