//! What the cross-implementation fixture cannot cover.
//!
//! `tests/agreement.rs` checks that the two renderers compute the same numbers.
//! These check the things only this one does: emitting a document that parses,
//! reporting what it could not draw, and refusing hostile input from a file.

use super::*;
use pl_core::{Feature, Segment, Topology};

fn plasmid(len: usize, circular: bool) -> Molecule {
    Molecule {
        name: "pTEST".into(),
        seq: b"ACGT".iter().cycle().take(len).copied().collect(),
        topology: if circular {
            Topology::Circular
        } else {
            Topology::Linear
        },
        ..Default::default()
    }
}

fn feat(name: &str, kind: &str, start: u64, end: u64) -> Feature {
    let mut f = Feature::new(name, kind);
    f.segments.push(Segment::new(start, end));
    f
}

/// Cheap well-formedness: every tag opened is closed, in order, and no tag
/// contains an odd number of quotes. Not a validator — enough to fail loudly on
/// the mistakes a string-building emitter actually makes.
fn well_formed(svg: &str) -> Result<(), String> {
    let mut stack: Vec<String> = Vec::new();
    let mut i = 0;
    let b = svg.as_bytes();
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let end = svg[i..].find('>').ok_or("unclosed tag")? + i;
        let inner = &svg[i + 1..end];
        if inner.matches('"').count() % 2 != 0 {
            return Err(format!("odd quote count in <{inner}>"));
        }
        if let Some(name) = inner.strip_prefix('/') {
            match stack.pop() {
                Some(open) if open == name => {}
                other => return Err(format!("</{name}> closes {other:?}")),
            }
        } else if !inner.ends_with('/') {
            stack.push(inner.split_whitespace().next().unwrap_or("").to_string());
        }
        i = end + 1;
    }
    if stack.is_empty() {
        Ok(())
    } else {
        Err(format!("unclosed: {stack:?}"))
    }
}

#[test]
fn a_plasmid_renders_to_well_formed_svg() {
    let mut m = plasmid(5386, true);
    m.features.push(feat("bla", "CDS", 100, 960));
    m.features.push(feat("ori", "rep_origin", 1200, 1800));
    m.features.push(feat("lacZ", "gene", 3000, 4200));
    let (svg, report) = circular_svg(&m, Options::default());
    well_formed(&svg).expect("malformed svg");
    assert!(svg.starts_with("<svg "));
    assert!(svg.ends_with("</svg>"));
    assert_eq!(report.labels_placed, 3);
    assert!(report.malformed.is_empty());
    for name in ["bla", "ori", "lacZ", "pTEST"] {
        assert!(svg.contains(name), "{name} missing from the map");
    }
    assert!(svg.contains("5,386 bp"));
}

#[test]
fn identical_input_renders_byte_identically() {
    let mut m = plasmid(4000, true);
    for i in 0..25u64 {
        m.features
            .push(feat(&format!("f{i}"), "CDS", i * 150 + 1, i * 150 + 120));
    }
    let (first, r1) = circular_svg(&m, Options::default());
    for _ in 0..8 {
        let (again, r2) = circular_svg(&m, Options::default());
        assert_eq!(again, first);
        assert_eq!(r1, r2);
    }
}

#[test]
fn a_hostile_colour_cannot_inject_an_attribute() {
    let mut m = plasmid(1000, true);
    let mut f = feat("evil", "CDS", 10, 500);
    f.segments[0].color = Some("#fff\" onload=\"alert(1)\" x=\"".into());
    m.features.push(f);
    let (svg, _) = circular_svg(&m, Options::default());
    assert!(!svg.contains("onload"), "injected: {svg}");
    // The fallback for a CDS is the CDS colour, not the generic grey.
    assert!(svg.contains(colour_for("CDS")), "did not fall back");
    well_formed(&svg).expect("malformed svg");
}

#[test]
fn a_hostile_name_cannot_inject_markup() {
    let mut m = plasmid(1000, true);
    m.name = "<script>alert(1)</script>".into();
    m.features.push(feat("a\u{0}b<c>&\"d", "CDS", 10, 500));
    let (svg, _) = circular_svg(&m, Options::default());
    assert!(!svg.contains("<script>"));
    assert!(svg.contains("&lt;script&gt;"));
    assert!(svg.contains("ab&lt;c&gt;&amp;&quot;d"), "{svg}");
    assert!(!svg.contains('\u{0}'));
    well_formed(&svg).expect("malformed svg");
}

#[test]
fn coordinates_that_describe_nothing_are_reported_not_drawn() {
    let mut m = plasmid(1000, true);
    m.features.push(feat("beyond", "CDS", 5000, 6000));
    m.features.push(feat("real", "CDS", 10, 500));
    let (svg, report) = circular_svg(&m, Options::default());
    assert_eq!(report.malformed, vec!["beyond".to_string()]);
    assert!(
        !svg.contains("beyond"),
        "a feature outside the molecule was drawn"
    );
    assert!(svg.contains("real"));
}

#[test]
fn a_label_that_does_not_fit_is_named_not_dropped_in_silence() {
    let mut m = plasmid(2000, true);
    for i in 0..90u64 {
        m.features
            .push(feat(&format!("g{i}"), "CDS", i * 20 + 1, i * 20 + 15));
    }
    let (_, report) = circular_svg(
        &m,
        Options {
            height: 300.0,
            width: 300.0,
            ..Default::default()
        },
    );
    assert!(!report.labels_hidden.is_empty(), "nothing reported hidden");
    assert_eq!(report.labels_placed + report.labels_hidden.len(), 90);
}

#[test]
fn a_linear_molecule_is_not_drawn_as_a_closed_ring() {
    let (linear, _) = circular_svg(&plasmid(1000, false), Options::default());
    let (circular, _) = circular_svg(&plasmid(1000, true), Options::default());
    assert!(!linear.contains("<circle"), "a line drawn as a circle");
    assert!(circular.contains("<circle"));
    well_formed(&linear).expect("malformed svg");
}

#[test]
fn an_origin_spanning_feature_is_two_arcs_and_one_label() {
    let mut m = plasmid(1000, true);
    m.features.push(feat("wrap", "CDS", 950, 50));
    let (svg, report) = circular_svg(&m, Options::default());
    assert_eq!(report.labels_placed, 1);
    assert_eq!(
        svg.matches("<title>wrap</title>").count(),
        2,
        "not two arcs"
    );
    well_formed(&svg).expect("malformed svg");
}

/// Where a named label ended up, as (x, anchor).
fn label_at(sc: &Scene, name: &str) -> (f64, Anchor) {
    sc.items
        .iter()
        .find_map(|i| match i {
            Item::Text {
                x, anchor, text, ..
            } if text == name => Some((*x, *anchor)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no label {name}"))
}

#[test]
fn a_wrapping_features_label_points_at_the_middle_of_the_whole_feature() {
    // 502 bp on a 1000 bp plasmid, in two parts: [(999, 1000), (1, 500)]. The
    // middle is base 250, a quarter turn clockwise, which is the right-hand
    // label column. Adding half the total span to the first part and clamping
    // it to that part's width pinned the anchor to base 1000 instead -- 359.6
    // degrees, and since the column is chosen by `sin >= 0` and sin is -0.006
    // there, the label went to the LEFT column with a leader across the figure
    // pointing at 2 bases of the 502.
    let mut wrapped = plasmid(1000, true);
    wrapped.features.push(feat("bla", "CDS", 999, 500));
    let (sc, _) = scene(&wrapped, Options::default());
    let (x, anchor) = label_at(&sc, "bla");

    // The same 502 bp feature that does not cross the origin, which was always
    // anchored correctly. Identical name, identical size, so identical ring
    // radius: the two must land in the same column, or the picture depends on
    // where the origin happens to sit.
    let mut straight = plasmid(1000, true);
    straight.features.push(feat("bla", "CDS", 100, 601));
    let (sc2, _) = scene(&straight, Options::default());
    let (x2, anchor2) = label_at(&sc2, "bla");

    assert_eq!(
        anchor,
        Anchor::Start,
        "the middle is at 89.6 degrees: right"
    );
    assert_eq!((x, anchor), (x2, anchor2), "the origin moved the label");
    assert!(x > sc.width / 2.0, "left of centre: {x}");
}

#[test]
fn a_single_part_features_anchor_is_where_it_always_was() {
    // The control for the accumulator: for a feature in one piece it must
    // compute exactly what the old clamp did, or every existing map moves.
    for (start, end) in [(100u64, 600u64), (1, 1000), (7, 7), (250, 251)] {
        let parts = ranges(start, end, 1000, true);
        let span: u64 = parts.iter().map(|(a, b)| b - a + 1).sum();
        let old = parts[0].0 + (span / 2).min(parts[0].1 - parts[0].0);
        assert_eq!(mid_base(&parts, span), old, "{start}..{end}");
    }
}

#[test]
fn a_segment_past_the_end_is_reported_even_when_a_sibling_segment_is_drawable() {
    // `CDS join(100..200,5000..6000)` on a 1000 bp plasmid, which is how a
    // feature copied out of a larger parent record arrives. The map can draw
    // only the first exon, and a 101 bp single-exon `orfX` is indistinguishable
    // from a real one -- so the loss has to be named. The all-or-nothing check
    // could not see it: `parts` was non-empty, so nothing was reported.
    let mut m = plasmid(1000, true);
    let mut f = Feature::new("orfX", "CDS");
    f.segments.push(Segment::new(100, 200));
    f.segments.push(Segment::new(5000, 6000));
    m.features.push(f);
    let (svg, report) = circular_svg(&m, Options::default());
    assert_eq!(report.partly_drawn, vec!["orfX".to_string()]);
    // It *was* drawn, in part, so it is not malformed -- the CLI says
    // "not drawn" about that list and that would be a second untruth.
    assert!(report.malformed.is_empty(), "{:?}", report.malformed);
    assert!(svg.contains("orfX"), "the surviving exon is still drawn");
}

#[test]
fn a_feature_wholly_inside_the_molecule_is_not_called_partly_drawn() {
    // The control: ordinary features, single- and multi-segment, report
    // nothing. Over-reporting a loss that did not happen would train the reader
    // to ignore the message.
    let mut m = plasmid(1000, true);
    m.features.push(feat("whole", "CDS", 100, 200));
    let mut j = Feature::new("spliced", "CDS");
    j.segments.push(Segment::new(100, 200));
    j.segments.push(Segment::new(400, 500));
    m.features.push(j);
    m.features.push(feat("wraps", "CDS", 950, 50));
    let (_, report) = circular_svg(&m, Options::default());
    assert!(report.partly_drawn.is_empty(), "{:?}", report.partly_drawn);
    assert!(report.malformed.is_empty());
}

#[test]
fn no_label_is_drawn_past_the_edge_of_the_canvas() {
    // The radius reserves room for the widest label, but that reservation is
    // capped at 30% of the canvas so one long name cannot collapse the ring --
    // and past the cap the name no longer fits in what was reserved. A 31-char
    // name at the defaults put the label's right edge at 734.6 against a
    // 720-wide canvas, where the viewBox, the /MediaBox and the %%BoundingBox
    // all crop it silently.
    let long = "TetR-P2A-EGFP-WPRE-polyA-signal";
    let mut m = plasmid(1000, true);
    m.features.push(feat(long, "CDS", 100, 500));
    m.features.push(feat("ori", "rep_origin", 600, 700));
    let (sc, report) = scene(&m, Options::default());
    for item in &sc.items {
        if let Item::Text {
            x,
            size,
            anchor,
            text,
            ..
        } = item
        {
            let w = label_width(text, *size);
            let (l, r) = match anchor {
                Anchor::Start => (*x, x + w),
                Anchor::Middle => (x - w / 2.0, x + w / 2.0),
                Anchor::End => (x - w, *x),
            };
            assert!(
                l >= 0.0 && r <= sc.width,
                "{text:?} runs from {l} to {r} on a canvas 0..{}",
                sc.width
            );
        }
    }
    assert_eq!(report.labels_truncated, vec![long.to_string()]);
    // Shortened, not dropped: the reader can still tell which feature it is.
    assert!(sc.items.iter().any(|i| matches!(
        i,
        Item::Text { text, .. } if text.starts_with("TetR-P2A") && text.ends_with("...")
    )));
}

#[test]
fn a_name_that_fits_is_left_exactly_as_it_is() {
    // The control for the shortening: it must fire only where the cap binds.
    // Every name on an ordinary map goes out whole and nothing is reported.
    let mut m = plasmid(5386, true);
    m.features.push(feat("bla", "CDS", 100, 960));
    m.features
        .push(feat("AmpR-promoter", "promoter", 1200, 1400));
    let (svg, report) = circular_svg(&m, Options::default());
    assert!(report.labels_truncated.is_empty());
    assert!(svg.contains(">AmpR-promoter<"), "{svg}");
    assert!(!svg.contains("..."));
}

#[test]
fn the_agreement_harness_is_described_as_the_check_it_actually_is() {
    // The crate doc used to say the harness "renders the same molecule through
    // both and asserts they describe the same picture". It does not: it replays
    // scalar fixtures through ten standalone helpers and never builds a
    // Molecule. Believing otherwise is what left `scene` with no oracle at all,
    // which is how the origin-spanning anchor above survived. If anyone
    // restores the picture-level claim, the harness has to grow a picture.
    const DOC: &str = include_str!("lib.rs");
    const HARNESS: &str = include_str!("../tests/agreement.rs");
    let doc = DOC.split("\npub ").next().unwrap_or(DOC);
    let claims_a_picture = doc.contains("describe the same picture")
        || doc.contains("renders the same molecule through both");
    let checks_a_picture = HARNESS.contains("scene(") || HARNESS.contains("circular_svg");
    assert!(
        !claims_a_picture || checks_a_picture,
        "the crate doc claims a picture-level cross-check that agreement.rs does not make"
    );
}

#[test]
fn degenerate_molecules_do_not_panic() {
    let mut track = Molecule {
        declared_len: Some(3000),
        ..Default::default()
    };
    track.features.push(feat("x", "CDS", 1, 3000));

    let mut zero_coords = plasmid(10, true);
    zero_coords.features.push(feat("", "", 0, 0));

    let mut huge = plasmid(10, true);
    huge.features.push(feat("z", "CDS", u64::MAX, u64::MAX));

    let cases = [
        Molecule::default(),
        plasmid(0, true),
        plasmid(1, true),
        plasmid(1, false),
        zero_coords,
        huge,
        track,
    ];
    for (i, m) in cases.iter().enumerate() {
        let (svg, _) = circular_svg(m, Options::default());
        well_formed(&svg).unwrap_or_else(|e| panic!("case {i}: {e}"));
    }
}

#[test]
fn the_ruler_steps_are_round_numbers() {
    // The ladder is 1, 2, 5, 10 — it rounds *up*, so a 2,686 bp plasmid asking
    // for twelve ticks (224 bp apart) gets five at 500. Sparser than asked for,
    // which is the price of never printing a tick at 224.
    for (raw, want) in [
        (0.0, 1),
        (0.4, 1),
        (1.0, 1),
        (1.7, 2),
        (3.0, 5),
        (7.0, 10),
        (224.0, 500),
        (2686.0 / 12.0, 500),
    ] {
        assert_eq!(nice_step(raw), want, "nice_step({raw})");
    }
    assert_eq!(nice_step(f64::NAN), 1);
    assert_eq!(nice_step(f64::INFINITY), 1);
}

#[test]
fn thousands_separators_match_the_typescript() {
    for (v, want) in [
        (0u64, "0"),
        (7, "7"),
        (999, "999"),
        (1000, "1,000"),
        (5386, "5,386"),
        (1234567, "1,234,567"),
    ] {
        assert_eq!(commas(v), want);
    }
}

#[test]
fn a_negative_zero_formats_as_zero() {
    // Two identical pictures must not differ because one coordinate landed a
    // hair below the axis.
    assert_eq!(n(-0.0), "0");
    assert_eq!(n(-0.001), "0");
    assert_eq!(n(0.001), "0");
    assert_eq!(n(12.3456), "12.35");
}

/// The `to` of the first arc in an `Arrow::End` path: the arrowhead's base.
///
/// The shape is `Move(ro, a0)`, `Arc(ro, a0 -> base)`, then the barbs, so the
/// first arc's far end *is* the base angle. Reading it back out of the emitted
/// segments is the only way to assert where the arrowhead starts without
/// re-implementing `arc_segs` in the test.
fn first_arc_to(segs: &[Seg]) -> f64 {
    segs.iter()
        .find_map(|s| match *s {
            Seg::Arc { to, .. } => Some(to),
            _ => None,
        })
        .expect("an arc")
}

#[test]
fn a_short_feature_degrades_to_a_triangle_not_a_bow_tie() {
    // The arrowhead is clamped to half the arc; unclamped it would start before
    // the arc did and the path would cross itself.
    //
    // Asserting that the coordinates are finite does not test this: with the
    // clamp deleted, head = 8/89 = 0.0899 against a sweep of 0.01, so the base
    // lands at -0.0799 -- before the arc starts, self-intersecting -- and every
    // coordinate is still a perfectly finite `polar` of a finite angle. The
    // assertion that catches it is where the base sits.
    let (a0, a1) = (0.0, 0.01);
    let segs = arc_segs(100.0, 100.0, 80.0, 98.0, a0, a1, Arrow::End);
    assert!(!segs.is_empty());
    let base = first_arc_to(&segs);
    assert!(
        base > a0,
        "the arrowhead starts at {base}, before the arc's own start {a0}: a bow tie"
    );
    assert!(
        base <= a1,
        "the arrowhead starts past the arc's end: {base}"
    );
    // Half the arc exactly, which is what "degrades to a triangle" means: the
    // head takes as much as it may and the shaft keeps the rest.
    assert!((base - (a0 + a1) / 2.0).abs() < 1e-12, "{base}");
    for s in &segs {
        let finite = match *s {
            Seg::Move(x, y) | Seg::Line(x, y) => x.is_finite() && y.is_finite(),
            Seg::Arc {
                cx,
                cy,
                r,
                from,
                to,
            } => {
                cx.is_finite()
                    && cy.is_finite()
                    && r.is_finite()
                    && from.is_finite()
                    && to.is_finite()
            }
            Seg::Close => true,
        };
        assert!(finite, "non-finite coordinate in {s:?}");
    }
    // And the same shape reaches both back ends.
    let d = svg_path(&segs);
    assert!(!d.contains("NaN"), "{d}");
}

#[test]
fn a_feature_long_enough_for_a_full_arrowhead_keeps_one() {
    // The control for the clamp: it must bind only where it has to. A 1 radian
    // arc is far longer than the 8/89 radians the arrowhead wants, so the head
    // is its full size and the base is a1 - 8/mid, not the midpoint.
    let (ri, ro, a0, a1) = (80.0, 98.0, 0.0, 1.0);
    let segs = arc_segs(100.0, 100.0, ri, ro, a0, a1, Arrow::End);
    let mid = (ri + ro) / 2.0;
    let base = first_arc_to(&segs);
    assert!((base - (a1 - 8.0 / mid)).abs() < 1e-12, "{base}");
    assert!(base > a0 && base < a1);
}
