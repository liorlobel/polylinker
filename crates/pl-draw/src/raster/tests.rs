//! Tests for the rasterizer.
//!
//! The picture-level oracle is **resvg**, in `tools/ci.ps1` via
//! `crates/pl-draw/tests/render.rs`: it renders the SVG this crate emits, from
//! the same font binary this crate fills outlines from, and the two images are
//! compared pixel by pixel. That is the only check that covers flattening,
//! winding, stroking, antialiasing, glyph placement and colour at once, and it
//! is an independent implementation.
//!
//! What is here is what can be settled without it — the properties whose
//! failure would be a *silent* wrong picture rather than a visibly wrong one.

use super::*;
use crate::scene::{Anchor, Item, Scene, Seg};

fn px(img: &Image, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * img.width() + x) * 3) as usize;
    let p = img.pixels();
    [p[i], p[i + 1], p[i + 2]]
}

fn blank(w: f64, h: f64, items: Vec<Item>) -> Scene {
    Scene {
        width: w,
        height: h,
        title: "t".into(),
        items,
    }
}

/// A filled square lands exactly where it was asked to, with clean edges.
///
/// The coordinate check first, because every other test here would pass just as
/// happily on a picture drawn one pixel over.
#[test]
fn a_square_covers_the_pixels_it_says_it_does() {
    let sc = blank(
        10.0,
        10.0,
        vec![Item::Path {
            segs: vec![
                Seg::Move(2.0, 3.0),
                Seg::Line(6.0, 3.0),
                Seg::Line(6.0, 8.0),
                Seg::Line(2.0, 8.0),
                Seg::Close,
            ],
            fill: Some("#000000".into()),
            stroke: None,
            stroke_width: 0.0,
            title: None,
        }],
    );
    let (img, _) = draw(&sc, 1.0, [255, 255, 255]);
    assert_eq!(px(&img, 2, 3), [0, 0, 0], "the first covered pixel");
    assert_eq!(px(&img, 5, 7), [0, 0, 0], "the last covered pixel");
    assert_eq!(px(&img, 1, 3), [255, 255, 255], "one to the left");
    assert_eq!(px(&img, 6, 3), [255, 255, 255], "one past the right edge");
    assert_eq!(px(&img, 2, 2), [255, 255, 255], "one above");
    assert_eq!(px(&img, 2, 8), [255, 255, 255], "one below the bottom edge");
}

/// Coverage is the fraction of the pixel the shape actually occupies.
///
/// The whole reason for an analytic accumulator rather than a hit test: a
/// half-covered pixel must come out half dark, or every edge in the figure
/// aliases. Checked against the exact areas, not against a golden.
#[test]
fn a_partly_covered_pixel_is_shaded_by_its_exact_area() {
    for (frac, what) in [
        (0.25f64, "a quarter"),
        (0.5, "half"),
        (0.75, "three quarters"),
    ] {
        let sc = blank(
            4.0,
            4.0,
            vec![Item::Path {
                segs: vec![
                    Seg::Move(1.0, 1.0),
                    Seg::Line(1.0 + frac, 1.0),
                    Seg::Line(1.0 + frac, 2.0),
                    Seg::Line(1.0, 2.0),
                    Seg::Close,
                ],
                fill: Some("#000000".into()),
                stroke: None,
                stroke_width: 0.0,
                title: None,
            }],
        );
        let (img, _) = draw(&sc, 1.0, [255, 255, 255]);
        let got = f64::from(px(&img, 1, 1)[0]);
        let want = 255.0 * (1.0 - frac);
        assert!(
            (got - want).abs() <= 1.0,
            "{what} of a pixel came out {got}, expected about {want}"
        );
    }
}

/// Headroom over the sagitta bound, for the quantisation residue measured in
/// `a_ring_of_any_size_inks_the_same_fraction_of_its_area`.
///
/// It is squeezed from both sides and 0.006 is inside the window, not at
/// either end of it. From below, the doubled sagitta term does not quite cover
/// the measured deficit on its own: at r=8 the ink is 0.400% under the annulus
/// against a doubled term of 0.391%, and at r=800 it is 0.0113% under against
/// 0.0042%, so SLACK must exceed 0.0092%. From above, a 12-gon's 1.138%
/// deficit has to stay outside the bound at every radius, and the doubled term
/// is largest at r=8 (0.391%), so SLACK must stay under 0.747%.
///
/// Where it sits inside that window is what sets the detection floor
/// `the_ring_bound_would_reject_a_coarse_polygon` states: an n-gon is short by
/// about (pi/n)^2/6, so 0.006 puts the floor at n = 16.
const SLACK: f64 = 0.006;

/// A stroked ring's ink does not drift as the ring grows.
///
/// THE TEST AN EARLIER DESIGN GOT WRONG, twice. Its flattening check was
/// stated at one radius, 200, and one radius cannot separate a sagitta-driven
/// count from a hardcoded one at all: an n-gon's ink deficit is
/// radius-INVARIANT, so a constant count that matches at 200 matches
/// everywhere. This comment said instead that a 720-gon "passes at 200 and the
/// same check fails at 800" until 2026-08-04, and the file already contradicted
/// it — `the_ring_bound_would_reject_a_coarse_polygon` passes
/// `(720.0, /*must_reject*/ false)` over radii including 800, asserting that a
/// 720-gon must be **admitted** there. It must: its sagitta at r=800 is
/// 0.0076 px, 6.6× finer than `FLATNESS`, as `raster.rs`'s flattening section
/// now states and `the_flattening_example_is_true_of_arc_points` now pins.
///
/// The radius is swept for a different reason: the bound the sagitta rule
/// predicts tightens as the radius grows, so the sweep is what puts the
/// loosest and the tightest case both under assertion. The second version
/// swept the radius but then asserted only that the relative error stayed
/// **flat** across it,
/// which a hardcoded segment count satisfies exactly; see
/// `the_ring_bound_would_reject_a_coarse_polygon`, the control that caught it.
/// What is asserted now is not a trend but the bound the sagitta rule itself
/// predicts at each radius, and that bound *tightens* as the radius grows —
/// 0.991% at r=8 down to 0.604% at r=800 — where a single absolute tolerance
/// would have loosened with the area and hidden exactly what this is for.
///
/// A ring rather than a disc, and the annulus area is exact:
/// `π[(r + w/2)² − (r − w/2)²] = 2πrw`, with no small-`w` approximation. It is
/// the sharper probe too, because the perimeter is all the ink there is.
///
/// THE ERROR IS A DEFICIT, AND MOST OF IT IS THE FLATTENING. Ink comes in
/// *under* the annulus at every radius — measured here, at scale 1 and
/// W = 1.25: **-0.400%** at r=8, **-0.078%** at r=40, **-0.0098%** at r=200,
/// **-0.0113%** at r=800. There is no surplus to account for. An earlier
/// version of this comment justified the tolerance by a positive bias, from
/// partial coverages that "add before the clamp" where two stroke pieces share
/// a pixel — but that is the signed-area accumulator `raster.rs` records as
/// TRIED AND **REJECTED**. What ships runs the winding along sorted crossings,
/// which is a true union, so `cov.min(1.0)` has nothing left to clamp and
/// overlapping quads and discs cannot inflate anything.
///
/// Most of the deficit is geometry, and exactly the geometry the bound below
/// models. The union of the round-stroke quads and discs around a convex k-gon
/// is the Minkowski annulus, area `2·P·h + h²(π − k·tan(π/k))`, which predicts
/// -0.190%, -0.0405%, -0.0082% and -0.0021% at the four radii. The remainder
/// is the rasterizer's own quantisation — 8-bit coverage and `SUB` = 16
/// sub-scanlines, on a ring 1.25 px wide that is nearly all antialiased edge
/// with no interior to average over. That it is quantisation and not geometry
/// is settled by resolution: the r=8 remainder falls -0.210% → -0.056% →
/// -0.009% as `scale` goes 1 → 2 → 4, while the geometric term does not move
/// with it.
///
/// The `abs` still earns its place even though the measured error is one-sided,
/// because the defect that flips the sign is exactly the double-count above and
/// this bound already catches it. Re-broken by sweeping each stroke subpath onto
/// the image separately instead of unioning them: ink came out **+7.92%** at
/// r=8, and **+0.81%** at r=800 — the least favourable radius, where the bound
/// is tightest at 0.604% — and it failed at both. A separate assertion on the
/// sign was written and deleted: it could not be made to fail on its own.
///
/// resvg, which builds a real stroke outline, is the judge of whether any of
/// this matters on a figure — on a real map at 1.25 px stroke it did not, at
/// 99.3% of pixels identical.
#[test]
fn a_ring_of_any_size_inks_the_same_fraction_of_its_area() {
    const W: f64 = 1.25; // what `pl-draw` actually strokes the backbone with
                         // Out to 800 because that is where the bound is TIGHTEST -- 0.604%
                         // against 0.991% at r=8 -- not because a coarse flattening becomes
                         // visible there. It does not: see the doc above.
    for r in [8.0f64, 40.0, 200.0, 800.0] {
        let n = (r * 2.0 + W * 2.0 + 6.0).ceil();
        let sc = blank(
            n,
            n,
            vec![Item::Circle {
                cx: n / 2.0,
                cy: n / 2.0,
                r,
                stroke: "#000000".into(),
                stroke_width: W,
            }],
        );
        let (img, _) = draw(&sc, 1.0, [255, 255, 255]);
        let ink: f64 = img
            .pixels()
            .chunks(3)
            .map(|p| (255.0 - f64::from(p[0])) / 255.0)
            .sum();
        let want = 2.0 * std::f64::consts::PI * r * W;
        let err = (ink - want) / want;

        // THE BOUND THE SAGITTA RULE ITSELF PREDICTS, not a trend.
        //
        // `arc_points` picks n so the chord sags at most FLATNESS from the arc,
        // which fixes the half-angle at theta/2 = acos(1 - FLATNESS/r). A
        // regular n-gon's perimeter is short of the circle's by
        // `1 - sin(pi/n)/(pi/n)`, and the ink is a perimeter measure, so that
        // ratio bounds how much ink a correct flattening may lose. Doubled, and
        // SLACK on top, for the quantisation remainder described above: at r=8
        // the measured deficit is 0.400% against a single term of 0.196%, so
        // the doubling alone is still 0.009% short.
        let theta = 2.0 * (1.0 - FLATNESS / r).clamp(-1.0, 1.0).acos();
        let segs = (std::f64::consts::TAU / theta).ceil().max(3.0);
        let x = std::f64::consts::PI / segs;
        let allowed = 2.0 * (1.0 - x.sin() / x) + SLACK;
        assert!(
            err.abs() < allowed,
            "radius {r}: ink is {:.3}% off the annulus, against {:.3}% that the              sagitta rule allows at {segs} segments -- the flattening is coarser              than FLATNESS claims",
            err * 100.0,
            allowed * 100.0
        );
    }
}

/// The bound above is tight enough to reject a visibly polygonal circle.
///
/// THE POSITIVE CONTROL, and it exists because the first version of the test
/// above could not fail. That version asserted the error was *flat* across
/// radii, on the reasoning that "a fixed segment count gets steadily coarser as
/// the circle grows". It does not: for a regular n-gon the perimeter ratio
/// `sin(pi/n)/(pi/n)` is independent of r, and ink is a perimeter measure — so
/// a hardcoded segment count produces a radius-INVARIANT error, which is
/// exactly the flat trend the assertion accepted. Measured: a 12-gon — visibly
/// a dodecagon — passed both arms.
///
/// So the bound is checked against the thing it must reject, rather than only
/// against the code that satisfies it.
#[test]
fn the_ring_bound_would_reject_a_coarse_polygon() {
    // 12 must be rejected; 720 must be admitted. NOTHING BETWEEN IS CLAIMED,
    // and the gap is stated rather than hidden: the deficit of an n-gon is
    // about (pi/n)^2/6, so SLACK sets the detection floor at roughly n = 16.
    // A 48-gon is 0.07% short — under SLACK itself — and this measure
    // cannot see it. resvg's picture-level comparison is what covers that
    // range, which is why both checks exist.
    for (segs, must_reject) in [(12.0f64, true), (720.0, false)] {
        // The relative ink deficit a regular `segs`-gon produces, from the same
        // perimeter identity the bound is derived from.
        let x = std::f64::consts::PI / segs;
        let deficit = 1.0 - x.sin() / x;
        for r in [8.0f64, 200.0, 800.0] {
            let theta = 2.0 * (1.0 - FLATNESS / r).clamp(-1.0, 1.0).acos();
            let honest = (std::f64::consts::TAU / theta).ceil().max(3.0);
            let hx = std::f64::consts::PI / honest;
            let allowed = 2.0 * (1.0 - hx.sin() / hx) + SLACK;
            if must_reject {
                assert!(
                    deficit > allowed,
                    "a {segs}-gon is {:.3}% short at r={r} and the bound allows                      {:.3}% — it would sail through",
                    deficit * 100.0,
                    allowed * 100.0
                );
            } else {
                assert!(
                    deficit < allowed,
                    "a {segs}-gon at r={r} is rejected by a bound meant to admit                      any flattening at least as fine as the shipped one"
                );
            }
        }
    }
}

/// A stroke has no hole where its own pieces overlap.
///
/// The load-bearing claim of the whole stroking approach: round joins let a
/// stroke be a nonzero fill of overlapping quads and discs, and that is true
/// ONLY while every one of them is wound the same way. Two wound opposite
/// cancel, and the hole appears exactly where the stroke is thickest, which is
/// at a sharp corner — the least likely place anyone looks.
///
/// Re-broken by returning the quads unwound: the corner pixel came out white.
#[test]
fn a_sharp_corner_in_a_stroke_has_no_hole_in_it() {
    let sc = blank(
        20.0,
        20.0,
        vec![Item::Path {
            // A tight zig-zag, so consecutive segments overlap heavily.
            segs: vec![
                Seg::Move(3.0, 10.0),
                Seg::Line(10.0, 3.0),
                Seg::Line(10.0, 17.0),
                Seg::Line(17.0, 10.0),
            ],
            fill: None,
            stroke: Some("#000000".into()),
            stroke_width: 4.0,
            title: None,
        }],
    );
    let (img, _) = draw(&sc, 1.0, [255, 255, 255]);
    for (x, y) in [(10u32, 4u32), (10, 16), (9, 10), (10, 10)] {
        let v = px(&img, x, y)[0];
        assert!(
            v < 40,
            "({x}, {y}) is {v}, so the stroke has a hole where two of its \
             pieces overlap"
        );
    }
}

/// A path's own winding cuts its holes, and is never normalised away.
///
/// A donut: an outer square and an inner square wound the other way. Under
/// nonzero they cancel and the middle stays background. If fill geometry were
/// pushed through `wound` — as stroke geometry must be — the hole would fill in
/// solid, which is what it did while prototyping, at 1.44x the correct ink.
#[test]
fn an_inner_contour_wound_the_other_way_cuts_a_hole() {
    let sc = blank(
        12.0,
        12.0,
        vec![Item::Path {
            segs: vec![
                // Outer, clockwise.
                Seg::Move(1.0, 1.0),
                Seg::Line(11.0, 1.0),
                Seg::Line(11.0, 11.0),
                Seg::Line(1.0, 11.0),
                Seg::Close,
                // Inner, anticlockwise.
                Seg::Move(4.0, 4.0),
                Seg::Line(4.0, 8.0),
                Seg::Line(8.0, 8.0),
                Seg::Line(8.0, 4.0),
                Seg::Close,
            ],
            fill: Some("#000000".into()),
            stroke: None,
            stroke_width: 0.0,
            title: None,
        }],
    );
    let (img, _) = draw(&sc, 1.0, [255, 255, 255]);
    assert_eq!(px(&img, 2, 2), [0, 0, 0], "the ring should be filled");
    assert_eq!(
        px(&img, 6, 6),
        [255, 255, 255],
        "the counter filled in, so the winding was normalised away"
    );
}

/// A label's ink lands where the layout said it would.
///
/// `Anchor::Middle` centres on the width `pdf::text_width_in` reports, so the
/// ink must straddle the anchor. A pen advancing by the face's own `hmtx`, or a
/// baseline resolved differently from the PDF's, moves this.
#[test]
fn a_centred_label_straddles_its_anchor() {
    let sc = blank(
        200.0,
        60.0,
        vec![Item::Text {
            x: 100.0,
            y: 30.0,
            size: 24.0,
            anchor: Anchor::Middle,
            color: "#000000".into(),
            bold: false,
            text: "HHHHH".into(),
        }],
    );
    let (img, rep) = draw(&sc, 1.0, [255, 255, 255]);
    assert!(rep.unencodable.is_empty(), "{rep:?}");
    let (mut lo, mut hi) = (u32::MAX, 0u32);
    for y in 0..img.height() {
        for x in 0..img.width() {
            if px(&img, x, y)[0] < 200 {
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
    }
    assert!(lo < hi, "nothing was drawn at all");
    let centre = f64::from(lo + hi) / 2.0;
    assert!(
        (centre - 100.0).abs() < 1.5,
        "the ink is centred on {centre}, not on the anchor at 100"
    );
    // ...and its width is the measured width, less the side bearings.
    let measured = crate::pdf::text_width_in("HHHHH", 24.0, false);
    let ink = f64::from(hi - lo + 1);
    assert!(
        ink < measured && ink > measured * 0.8,
        "ink spans {ink} px against a measured advance of {measured}"
    );
}

/// Bold text is heavier than regular at the same size.
///
/// Cheap, and it is the check that catches selecting one face for both — which
/// would leave every map's centre title in the wrong weight while every
/// coordinate still landed correctly, because the advances come from a table
/// either way.
#[test]
fn bold_selects_the_other_face() {
    let ink = |bold: bool| {
        let sc = blank(
            200.0,
            60.0,
            vec![Item::Text {
                x: 10.0,
                y: 30.0,
                size: 24.0,
                anchor: Anchor::Start,
                color: "#000000".into(),
                bold,
                text: "plasmid".into(),
            }],
        );
        let (img, _) = draw(&sc, 1.0, [255, 255, 255]);
        img.pixels()
            .chunks(3)
            .map(|p| 255.0 - f64::from(p[0]))
            .sum::<f64>()
    };
    let (r, b) = (ink(false), ink(true));
    assert!(
        b > r * 1.1,
        "bold inked {b:.0} against regular's {r:.0}, so both are drawing the \
         same face"
    );
}

/// A colour the parser cannot read is reported, not guessed at.
///
/// `safe_color` passes any alphabetic word of 1..=32 characters, so a name that
/// is not a colour reaches here. Drawing it black would put a claim on the map
/// that nothing in the file supports.
#[test]
fn an_unreadable_colour_is_reported_rather_than_drawn() {
    let sc = blank(
        8.0,
        8.0,
        vec![Item::Path {
            segs: vec![
                Seg::Move(1.0, 1.0),
                Seg::Line(7.0, 1.0),
                Seg::Line(7.0, 7.0),
                Seg::Close,
            ],
            fill: Some("chartreusey".into()),
            stroke: None,
            stroke_width: 0.0,
            title: None,
        }],
    );
    let (img, rep) = draw(&sc, 1.0, [255, 255, 255]);
    assert_eq!(rep.unparsed_colours, vec!["chartreusey".to_string()]);
    assert_eq!(px(&img, 3, 3), [255, 255, 255], "it was drawn anyway");
    // ...and `none` is not a failure to parse, it is an instruction.
    let mut sc2 = sc;
    if let Item::Path { fill, .. } = &mut sc2.items[0] {
        *fill = Some("none".into());
    }
    let (_, rep2) = draw(&sc2, 1.0, [255, 255, 255]);
    assert!(rep2.unparsed_colours.is_empty(), "{rep2:?}");
}

/// The colour parser reads every form `safe_color` can emit.
#[test]
fn the_colour_parser_covers_what_safe_color_admits() {
    for (input, want) in [
        ("#000", Some([0, 0, 0])),
        ("#fff", Some([255, 255, 255])),
        ("#4a7ebb", Some([0x4A, 0x7E, 0xBB])),
        ("#4a7ebbff", Some([0x4A, 0x7E, 0xBB])),
        ("rgb(10, 20, 30)", Some([10, 20, 30])),
        ("rgba(10, 20, 30, 0.5)", Some([10, 20, 30])),
        // Percentage channels. `safe_color` admits `%` inside `rgb(`/`rgba(`
        // and the table had no such case at all, which is how the percent arm
        // of `numbers` came to ignore the sign it had just stepped past.
        // 50% of 255 is 127.5 and `f64::round` breaks the tie away from zero.
        ("rgb(100%,0%,0%)", Some([255, 0, 0])),
        ("rgb(0%, 100%, 0%)", Some([0, 255, 0])),
        ("rgb(50%, 50%, 50%)", Some([128, 128, 128])),
        ("rgb(100%, 100%, 100%)", Some([255, 255, 255])),
        ("rgb(0%, 0%, 0%)", Some([0, 0, 0])),
        ("rgba(100%, 0%, 0%, 0.5)", Some([255, 0, 0])),
        // The `hsl` percentages must NOT take the same scaling: `colour`
        // divides them by 100 itself. These four are the guard against fixing
        // `rgb()` by double-counting `hsl()`.
        ("hsl(0, 100%, 50%)", Some([255, 0, 0])),
        ("hsl(120, 100%, 50%)", Some([0, 255, 0])),
        ("hsl(0, 0%, 100%)", Some([255, 255, 255])),
        ("hsla(240, 100%, 50%, 0.5)", Some([0, 0, 255])),
        ("black", Some([0, 0, 0])),
        ("none", None),
        ("NONE", None),
        ("notacolour", None),
    ] {
        assert_eq!(colour(input), want, "{input}");
    }
}

/// A percentage `rgb()` reaches the page as the colour the SVG means by it.
///
/// The table above is a claim about a private function; this is the claim that
/// matters, made through `raster::draw` on real pixels. `safe_color` passes a
/// percentage `rgb()` through unaltered and the SVG back end interpolates that
/// string into the `fill` attribute verbatim, so whatever CSS says the string
/// means is what a reader of the SVG sees — and the PNG of the same figure has
/// to show the same colour or one plasmid map has two colours depending on
/// which button was pressed.
///
/// Each pair is two spellings of one colour, asserted equal to each other *and*
/// to the expected triple. Equality alone would pass if both spellings were
/// broken the same way, and the literal pins which of the two is right.
///
/// PROVEN TO FAIL before the `numbers` fix, measured through this same public
/// entry point: `rgb(100%,0%,0%)` rasterised to [100, 0, 0] against
/// `rgb(255,0,0)`'s [255, 0, 0], with `Report::unparsed_colours` empty.
#[test]
fn a_percentage_rgb_rasters_to_the_same_pixel_as_its_absolute_spelling() {
    // Black, so a white fill is not confused with an unpainted pixel. None of
    // the four colours below is black, so nothing here can pass by omission.
    let bg = [0, 0, 0];
    // Every percentage here resolves to a whole number of 255ths — 20% is
    // exactly 51, 40% exactly 102 — so each pair is two spellings of one
    // colour with no rounding in between, and the pairing claims nothing about
    // tie-breaking. The 127.5 tie is pinned in the table test above instead.
    for (pct, abs, want) in [
        ("rgb(100%,0%,0%)", "rgb(255,0,0)", [255u8, 0, 0]),
        ("rgb(0%, 100%, 0%)", "rgb(0,255,0)", [0, 255, 0]),
        ("rgb(100%, 100%, 100%)", "rgb(255,255,255)", [255, 255, 255]),
        ("rgba(20%, 40%, 20%, 1)", "rgb(51,102,51)", [51, 102, 51]),
    ] {
        let paint = |fill: &str| {
            let sc = blank(
                8.0,
                8.0,
                vec![Item::Path {
                    segs: vec![
                        Seg::Move(1.0, 1.0),
                        Seg::Line(7.0, 1.0),
                        Seg::Line(7.0, 7.0),
                        Seg::Line(1.0, 7.0),
                        Seg::Close,
                    ],
                    fill: Some(fill.into()),
                    stroke: None,
                    stroke_width: 0.0,
                    title: None,
                }],
            );
            let (img, rep) = draw(&sc, 1.0, bg);
            // A colour this crate cannot read is reported rather than guessed
            // at. Both spellings must be *read*, not merely agreed on.
            assert!(rep.unparsed_colours.is_empty(), "{fill}: {rep:?}");
            px(&img, 3, 3)
        };
        let (a, b) = (paint(pct), paint(abs));
        assert_eq!(a, want, "{pct} rasterised wrong");
        assert_eq!(b, want, "{abs} rasterised wrong");
        assert_eq!(a, b, "{pct} and {abs} are the same colour");
    }
}

/// The same scene twice is the same pixels twice.
///
/// This crate's promise is deterministic output. It cannot see across
/// platforms — see the module comment on why that stronger claim is not made
/// here — but it does catch output depending on anything but its input.
#[test]
fn the_same_scene_rasters_identically() {
    let sc = blank(
        60.0,
        40.0,
        vec![
            Item::Circle {
                cx: 30.0,
                cy: 20.0,
                r: 15.0,
                stroke: "#33383d".into(),
                stroke_width: 1.25,
            },
            Item::Text {
                x: 30.0,
                y: 20.0,
                size: 9.0,
                anchor: Anchor::Middle,
                color: "#16191c".into(),
                bold: true,
                text: "pUC19".into(),
            },
        ],
    );
    let a = draw(&sc, 2.0, [255, 255, 255]).0;
    let b = draw(&sc, 2.0, [255, 255, 255]).0;
    assert!(
        a.pixels() == b.pixels(),
        "two runs produced different pixels"
    );
}

/// `raster.rs`'s own prose, with the comment markers and the line wrapping
/// taken out, so a claim that spans two comment lines can be searched for as
/// one sentence.
///
/// The same join `deflate/tests.rs` makes, for the same reason. Every number in
/// this module's header and in its two resolution constants is a measurement,
/// and a measurement recorded in prose beside code that contradicts it stays
/// green forever. That is what happened to the flattening example the tests
/// below now pin: the header called a 720-gon "visibly polygonal at 800" while
/// `the_ring_bound_would_reject_a_coarse_polygon`, at that very radius,
/// asserted that a 720-gon must be admitted.
fn module_prose() -> String {
    const SRC: &str = include_str!("../raster.rs");
    let mut out = String::new();
    for line in SRC.lines() {
        let t = line.trim_start();
        let Some(body) = t.strip_prefix("//!").or_else(|| t.strip_prefix("///")) else {
            continue;
        };
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(body.trim());
    }
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out
}

/// The number that follows `anchor` in the module's prose.
///
/// Panics when the anchor is absent, and **also when it occurs more than
/// once**: a second occurrence would silently pin whichever came first, which
/// is one of the ways a prose test stops being able to fail.
/// `deflate/tests.rs`'s version takes the first match; this one refuses to
/// guess, so an anchor that stops being distinctive is a failure rather than a
/// coincidence.
fn number_after(prose: &str, anchor: &str) -> f64 {
    let n = prose.matches(anchor).count();
    assert_eq!(
        n, 1,
        "raster.rs has {n} occurrences of the anchor {anchor:?}, and a pin needs exactly one"
    );
    let (_, rest) = prose.split_once(anchor).expect("the single occurrence");
    let digits: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.')
        .filter(|c| *c != ',')
        .collect();
    let digits = digits.trim_end_matches('.');
    assert!(
        !digits.is_empty(),
        "{anchor:?} in raster.rs is not followed by a number"
    );
    digits.parse().expect("a decimal number")
}

/// A full circle's worth of points, as [`arc_points`] really flattens it.
fn full_circle(r: f64) -> usize {
    let mut p: Poly = Vec::new();
    arc_points(0.0, 0.0, r, 0.0, std::f64::consts::TAU, 1.0, &mut p);
    p.len()
}

/// A regular `n`-gon's sagitta at radius `r` — the identity the header quotes.
fn sagitta(r: f64, n: f64) -> f64 {
    r * (1.0 - (std::f64::consts::PI / n).cos())
}

/// The flattening section's fixed-count example has to be true of `arc_points`.
///
/// PROVEN TO FAIL against the prose in the working tree before 2026-08-04,
/// which read "A fixed 720-gon is the trap: it is invisible at 200 px radius
/// and visibly polygonal at 800." Observed failure, before the paragraph was
/// rewritten: `raster.rs has 0 occurrences of the anchor "emits exactly "`.
/// The arithmetic arms then reject any future sentence that puts the crossover
/// at the wrong radius.
///
/// The old sentence was wrong by a factor of about 65 and the file already
/// said so: `the_ring_bound_would_reject_a_coarse_polygon` passes
/// `(720.0, /*must_reject*/ false)` over radii including 800, asserting that a
/// 720-gon **must be admitted** there. Nothing joined the two, because one was
/// prose and the other was code.
#[test]
fn the_flattening_example_is_true_of_arc_points() {
    let prose = module_prose();
    let segs = number_after(&prose, "emits exactly ");
    let cross = number_after(&prose, "720 segments at a radius of ");

    // The one radius a fixed count is right at: the rule asks for exactly it
    // here, and for more one pixel further out, which is what "first" means.
    assert_eq!(
        full_circle(cross),
        segs as usize,
        "arc_points emits {} segments at r={cross}, not the {segs} the header says",
        full_circle(cross)
    );
    assert!(
        full_circle(cross + 1.0) > segs as usize,
        "r={cross} is not the last radius the rule is content with {segs} segments at"
    );
    assert!(
        sagitta(cross, segs) < FLATNESS && sagitta(cross + 1.0, segs) >= FLATNESS,
        "r={cross} is not the last radius a {segs}-gon stays under FLATNESS at: \
         its sagitta is {} there and {} one pixel out",
        sagitta(cross, segs),
        sagitta(cross + 1.0, segs)
    );
}

/// ...and the correction of the claim it replaced, at the radius that claim
/// named.
///
/// PROVEN TO FAIL by the same edit and for the same reason: none of these
/// anchors exists in the old sentence. Kept separate from the test above
/// because this is the arm that would have caught the original defect — it
/// asserts, in arithmetic, that a 720-gon at the radius the header names is
/// FINER than `FLATNESS` rather than visibly polygonal.
#[test]
fn a_fixed_720_gon_is_finer_than_flatness_at_the_radius_the_header_names() {
    let prose = module_prose();
    let segs = number_after(&prose, "emits exactly ");
    let r = number_after(&prose, "At a radius of ");

    let sag = number_after(&prose, "a 720-gon sags ");
    assert!(
        (sagitta(r, segs) - sag).abs() < 5e-5,
        "a {segs}-gon sags {} px at r={r}, and the header says {sag}",
        sagitta(r, segs)
    );
    // FINER than FLATNESS, and by how much. This is the sign the old sentence
    // had backwards.
    assert!(
        sagitta(r, segs) < FLATNESS,
        "a {segs}-gon is not finer than FLATNESS at r={r} after all"
    );
    let factor = number_after(&prose, "by a factor of ");
    assert!(
        (FLATNESS / sagitta(r, segs) - factor).abs() < 0.05,
        "the sagitta at r={r} is finer than FLATNESS by {:.3}, not {factor}",
        FLATNESS / sagitta(r, segs)
    );
    let emitted = number_after(&prose, "the rule itself emits only ");
    assert_eq!(
        full_circle(r),
        emitted as usize,
        "arc_points emits {} at r={r}, not {emitted}",
        full_circle(r)
    );
    // Where it does go coarse, which is the honest version of "visibly
    // polygonal": half a pixel is the first sag a reader could point at.
    let half = number_after(&prose, "half a pixel until a radius of ");
    assert!(
        sagitta(half, segs) >= 0.5 && sagitta(half - 1.0, segs) < 0.5,
        "a {segs}-gon does not first sag half a pixel at r={half}: it is {} there",
        sagitta(half, segs)
    );
}

/// The work the small end costs, which is the real reason for the sagitta rule.
///
/// PROVEN TO FAIL by the same missing anchors. The point counts come out of
/// `disc` and `stroke_of` rather than out of the comment, so a change to
/// either that moves them fails here with the line to edit in the message.
#[test]
fn the_disc_at_every_stroke_joint_is_the_polygon_the_header_says() {
    let prose = module_prose();
    let half = number_after(&prose, "half-width — ");
    let width = number_after(&prose, "px for the ");
    assert!(
        (width - 2.0 * half).abs() < 1e-9,
        "{half} px is not half of the {width} px backbone stroke"
    );
    let pts = number_after(&prose, "which the rule flattens to ");
    assert_eq!(
        disc(0.0, 0.0, half, 1.0).len(),
        pts as usize,
        "a disc of radius {half} flattens to {} points, not {pts}",
        disc(0.0, 0.0, half, 1.0).len()
    );

    // The ring those discs are strung on, and one disc per vertex of it.
    let r = number_after(&prose, "backbone ring at a radius of ");
    let ring_pts = number_after(&prose, "800 px is ");
    let discs = number_after(&prose, "so it carries ");
    let ring = disc(0.0, 0.0, r, 1.0);
    assert_eq!(
        ring.len(),
        ring_pts as usize,
        "the ring at r={r} is {} points, not {ring_pts}",
        ring.len()
    );
    // `stroke_of` closes the ring, so it emits one quad per vertex of the open
    // line and one disc per vertex of the closed one: the header's two counts,
    // summed.
    assert_eq!(
        stroke_of(&ring, true, width, 1.0).len(),
        (ring_pts + discs) as usize,
        "stroke_of emits {} polygons for that ring, and the header accounts for {}",
        stroke_of(&ring, true, width, 1.0).len(),
        ring_pts + discs
    );
    assert_eq!(
        discs,
        ring_pts + 1.0,
        "closing the ring adds one disc, so {ring_pts} points carry {}, not {discs}",
        ring_pts + 1.0
    );

    // The fixed-count column closes it the same way, and its ring is the
    // crossover count from the paragraph above.
    let fixed_ring = number_after(&prose, "same picture out of ");
    let fixed_discs = number_after(&prose, "ring points and ");
    assert_eq!(
        fixed_ring,
        number_after(&prose, "emits exactly "),
        "the fixed-count column is not the 720 the crossover paragraph is about"
    );
    assert_eq!(
        fixed_discs,
        fixed_ring + 1.0,
        "a closed {fixed_ring}-gon carries {} discs, not {fixed_discs}",
        fixed_ring + 1.0
    );
}

/// Both resolution constants have to say which side of 8-bit they are on.
///
/// PROVEN TO FAIL against the prose in the working tree before 2026-08-04,
/// where `SUB`'s doc read "Sixteen puts the vertical quantisation at 1/16 of a
/// pixel, below what an 8-bit coverage value can express on a near-horizontal
/// edge" and `FLATNESS`'s read "below what an 8-bit coverage value can
/// express". Both are above it — by 8 levels and by 12.75 respectively — and
/// the observed failure was `raster.rs has 0 occurrences of the anchor
/// "coverage lands on multiples of 1/"`.
///
/// The inversion mattered because
/// `a_ring_of_any_size_inks_the_same_fraction_of_its_area` charges a residue to
/// exactly these two constants. A maintainer chasing edge banding would read
/// both comments, conclude the sampling is already past what the output can
/// show, and go looking at compositing instead.
#[test]
fn the_resolution_constants_are_above_8_bit_and_say_so() {
    let prose = module_prose();

    // SUB: y is sampled and x is not, so this is the vertical quantisation.
    let steps = number_after(&prose, "coverage lands on multiples of 1/");
    assert_eq!(
        steps as usize, SUB,
        "SUB is {SUB}, and the doc says 1/{steps}"
    );
    let worst = number_after(&prose, "the worst error is half a step, 1/");
    assert_eq!(
        worst as usize,
        2 * SUB,
        "half of 1/{SUB} is 1/{}, not 1/{worst}",
        2 * SUB
    );
    let levels = number_after(&prose, "of a pixel — ");
    assert_eq!(
        levels,
        (255.0 / (2.0 * SUB as f64)).round(),
        "1/{worst} of a pixel is {} of 255 levels, not {levels}",
        (255.0 / (2.0 * SUB as f64)).round()
    );
    assert!(
        levels > 1.0,
        "the doc no longer puts SUB above what an 8-bit channel resolves"
    );

    // ...and the SUB that would actually reach the 8-bit floor.
    let floor = number_after(&prose, "would take SUB = ");
    assert!(
        1.0 / (2.0 * floor) <= 1.0 / 255.0 && 1.0 / floor > 1.0 / 255.0,
        "SUB = {floor} is not the smallest doubling whose half-step reaches 1/255"
    );
    let floor_worst = number_after(&prose, "a worst error of 1/");
    assert_eq!(
        floor_worst,
        2.0 * floor,
        "SUB = {floor} has a worst error of 1/{}, not 1/{floor_worst}",
        2.0 * floor
    );

    // FLATNESS: x IS analytic, so the comparison is against 1/255 of a pixel
    // of edge displacement rather than against a sub-scanline.
    let flat_levels = number_after(&prose, "FLATNESS is ");
    assert!(
        (flat_levels - FLATNESS * 255.0).abs() < 0.01,
        "FLATNESS is {} coverage steps wide, not {flat_levels}",
        FLATNESS * 255.0
    );
    let fine_r = number_after(&prose, "the flattening at radius ");
    let shipped = number_after(&prose, "px from ");
    let fine_segs = number_after(&prose, " segments to ");
    let cost = number_after(&prose, "— a factor of ");
    assert_eq!(
        full_circle(fine_r),
        shipped as usize,
        "arc_points emits {} segments at r={fine_r}, not {shipped}",
        full_circle(fine_r)
    );
    let at_8bit = {
        let theta = 2.0 * (1.0 - (1.0 / 255.0) / fine_r).acos();
        (std::f64::consts::TAU / theta).ceil()
    };
    assert_eq!(
        fine_segs, at_8bit,
        "a FLATNESS of 1/255 needs {at_8bit} segments at r={fine_r}, not {fine_segs}"
    );
    assert!(
        (fine_segs / shipped - cost).abs() < 0.05,
        "1/255 costs {:.3} times the segments, not {cost}",
        fine_segs / shipped
    );
}
