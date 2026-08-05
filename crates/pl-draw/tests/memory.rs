//! What a PNG export asks the allocator for.
//!
//! `pl_draw::PNG_BYTES_PER_PIXEL` is quoted back to the user in every refusal
//! `pl_draw::MAX_PIXELS` produces, and `deflate.rs`'s `BLOCK_SYMS` doc states
//! the same arithmetic per input byte. Both are prose, and prose about
//! memory is exactly the kind that rots silently: nothing in `cargo test`
//! notices when an encoder stops holding what its comment says it holds.
//!
//! So the number is measured here rather than asserted about. A counting
//! `GlobalAlloc` sums live bytes and keeps the high-water mark across a real
//! `png_at` on a real map.
//!
//! **One test function on purpose.** `cargo test` runs a file's tests on
//! several threads and this allocator is process-wide, so a second test would
//! be measuring the first one's buffers.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use pl_core::{Feature, Molecule, Segment, Strand, Topology};

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// `System`, with a running total and a high-water mark.
///
/// Sizes come from the `Layout`, so this counts what the program asked for and
/// not what the allocator rounded it up to. That is the right side of the
/// question here: the abort this guards against is `handle_alloc_error` on a
/// request, and the request is what the caller controls.
struct Track;

unsafe impl GlobalAlloc for Track {
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
            LIVE.fetch_sub(l.size(), Ordering::Relaxed);
            let now = LIVE.fetch_add(new, Ordering::Relaxed) + new;
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        q
    }
}

#[global_allocator]
static ALLOC: Track = Track;

/// The map from `tests/render.rs`: features on both strands, labels long
/// enough to need leaders, a centre title. A blank circle would compress to
/// almost nothing and understate every buffer downstream of the pixels.
fn molecule() -> Molecule {
    let mut seq = Vec::new();
    for i in 0..2400u32 {
        seq.push(b"ACGT"[(i as usize * 7 + i as usize / 13) % 4]);
    }
    let feat = |name: &str, start: u64, end: u64, rev: bool| {
        let mut f = Feature::new(name, "CDS");
        f.strand = if rev {
            Strand::Reverse
        } else {
            Strand::Forward
        };
        f.segments = vec![Segment::new(start, end)];
        f
    };
    Molecule {
        name: "pRASTER".into(),
        seq,
        topology: Topology::Circular,
        features: vec![
            feat("AmpR", 100, 900, false),
            feat("ori", 1000, 1500, true),
            feat("lacZ-alpha", 1600, 2000, false),
            feat("T7 promoter", 2100, 2160, true),
        ],
        ..Default::default()
    }
}

/// Run `f`, and report the high-water mark of live heap over it.
fn peak_over<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let out = f();
    (out, PEAK.load(Ordering::Relaxed).saturating_sub(base))
}

/// The documented per-pixel cost is the one an export actually pays, and a
/// refused export pays nothing at all.
///
/// PROVEN TO FAIL two ways. Against the working tree of 2026-08-04, before
/// `png_budget` existed, the third block below has nothing to call: `png_at`
/// returned `(Vec<u8>, Report)`, allocated 10,987,919,938 bytes for the
/// documented `--journal nature --column double --dpi 2400`, and returned a
/// 2.2 MB file after five seconds on a 128 GB machine — or aborted through
/// `handle_alloc_error`, with no diagnostic and no partial file, on anything
/// smaller. Against the fixed tree with the budget check in `png_at` replaced
/// by `let _ = png_budget(..);`, the third block fails on
/// `2400 dpi at a double column was not refused`.
///
/// The bounds on the first two blocks are two-sided on purpose. An upper bound
/// alone passes if the encoder gets *cheaper*, which would leave
/// `PNG_BYTES_PER_PIXEL` — and the GB figure in every refusal message — quoting
/// a cost nothing pays.
#[test]
fn a_png_export_costs_the_bytes_per_pixel_its_constant_claims() {
    let mol = molecule();
    let (sc, _) = pl_draw::scene(&mol, pl_draw::Options::default());
    let mm = 89.0;

    // A fixed 256 KB hash head, plus the scene and the finished file, sit on
    // top of the per-pixel cost and do not scale with it. Two sizes an order of
    // magnitude apart so that a fixed cost cannot be mistaken for a per-pixel
    // one.
    for dpi in [300.0f64, 600.0] {
        let (_, peak) =
            peak_over(|| pl_draw::png_at(&sc, Some(mm), dpi, [255, 255, 255]).expect("in budget"));
        let (w, h) = pl_draw::png_budget(&sc, Some(mm), dpi).expect("in budget");
        let n = u64::from(w) * u64::from(h);
        let per_px = peak as f64 / n as f64;
        // 512 KB of slack over the fixed costs; at 1.1 megapixels that is 0.5
        // bytes a pixel, and the measured overshoot is 0.25.
        let ceiling = n * pl_draw::PNG_BYTES_PER_PIXEL + 512 * 1024;
        assert!(
            (peak as u64) <= ceiling,
            "{w} x {h} at {dpi} dpi peaked at {peak} B ({per_px:.2} B/px), past the \
             {} B/px PNG_BYTES_PER_PIXEL documents",
            pl_draw::PNG_BYTES_PER_PIXEL
        );
        // The floor: `prev` at 24 B/px and the scanlines at 3 are the two that
        // dominate, and either one going away would make the documented figure
        // — and every GB in every refusal — an overstatement.
        assert!(
            per_px >= 30.0,
            "{w} x {h} at {dpi} dpi peaked at only {per_px:.2} B/px; \
             PNG_BYTES_PER_PIXEL claims {}",
            pl_draw::PNG_BYTES_PER_PIXEL
        );
    }

    // And the refusal costs nothing. This is the whole point of checking the
    // budget rather than the allocator's return: 183 mm at 2400 dpi is
    // 17,291 px square, 298,978,681 pixels, a measured 10,987,919,938 bytes.
    let (refused, peak) = peak_over(|| pl_draw::png_at(&sc, Some(183.0), 2400.0, [255, 255, 255]));
    // Matched rather than `expect_err`, which on the failing path would
    // `Debug`-print the 2.2 MB of PNG it is complaining about the existence of.
    let e = match refused {
        Ok((bytes, _)) => panic!(
            "2400 dpi at a double column was not refused: {} bytes of PNG, {peak} B of heap",
            bytes.len()
        ),
        Err(e) => e,
    };
    assert_eq!((e.w, e.h), (17291, 17291), "{e}");
    assert_eq!(e.pixels(), 298_978_681);
    assert!(
        peak < 1024 * 1024,
        "the refusal itself allocated {peak} B; it is supposed to be arithmetic"
    );
    // The advice has to be advice: the dpi it names must itself pass.
    let d = e.fits_at_dpi.expect("some resolution fits a 183 mm figure");
    assert!(d < 2400.0, "{e}");
    pl_draw::png_budget(&sc, Some(183.0), d)
        .expect("the refusal suggested a dpi the same guard refuses");
    assert!(
        pl_draw::png_budget(&sc, Some(183.0), d + 1.0).is_err(),
        "the refusal suggested {d} dpi when {} dpi also fits",
        d + 1.0
    );
}
