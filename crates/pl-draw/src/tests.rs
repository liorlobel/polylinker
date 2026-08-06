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
        if !inner.matches('"').count().is_multiple_of(2) {
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

/// Every text item's horizontal extent, measured the way it will be typeset.
///
/// `pdf::text_width_in` in the item's own weight, which is what `pdf::to_pdf`
/// and `eps::to_eps` position with and therefore what the `/MediaBox` and the
/// `%%BoundingBox` crop against. Measuring with `label_width` here instead --
/// the same estimate `fit_label` used to decide with -- is why the test below
/// could not see a label 5.93 pt off the page: the check and the decision
/// shared an assumption, so the check could only ever agree with it.
fn text_extents(sc: &Scene) -> Vec<(String, f64, f64)> {
    sc.items
        .iter()
        .filter_map(|i| match i {
            Item::Text {
                x,
                size,
                anchor,
                text,
                bold,
                ..
            } => {
                let w = pdf::text_width_in(text, *size, *bold);
                let (l, r) = match anchor {
                    Anchor::Start => (*x, x + w),
                    Anchor::Middle => (x - w / 2.0, x + w / 2.0),
                    Anchor::End => (x - w, *x),
                };
                Some((text.clone(), l, r))
            }
            _ => None,
        })
        .collect()
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
    for (text, l, r) in text_extents(&sc) {
        assert!(
            l >= 0.0 && r <= sc.width,
            "{text:?} runs from {l} to {r} on a canvas 0..{}",
            sc.width
        );
    }
    assert_eq!(report.labels_truncated, vec![long.to_string()]);
    // Shortened, not dropped: the reader can still tell which feature it is.
    assert!(sc.items.iter().any(|i| matches!(
        i,
        Item::Text { text, .. } if text.starts_with("TetR-P2A") && text.ends_with("...")
    )));
}

#[test]
fn a_capital_heavy_name_inside_the_reservation_is_still_measured_in_helvetica() {
    // The uncapped case, which the cap-binding test above cannot reach. On a
    // square canvas below the 30% cap the reservation closes with exactly 8
    // units to spare -- but in `label_width`'s 0.55 em/character units, and the
    // figure is cropped in Helvetica's. `pCMV-WPRE` is 9 characters: 59.4
    // estimate units against 67.4 of room, so `fit_label` kept it whole and
    // reported nothing, and then the PDF wrote `652.6 555.59 Td (pCMV-WPRE) Tj`
    // with `/MediaBox [0 0 720 720]` against a real width of 73.33 pt -- ending
    // at 725.93, most of the final E cropped off the printed figure. The EPS is
    // the same numbers under `%%BoundingBox: 0 0 720 720`.
    //
    // EGFP, WPRE, CMV, BGH: capital-heavy is the dominant plasmid naming style,
    // and it is exactly where the 0.55 estimate is furthest out.
    let name = "pCMV-WPRE";
    for (start, end, want_right_column) in [(100u64, 900u64, true), (2100, 2900, false)] {
        let mut m = plasmid(4000, true);
        m.features.push(feat(name, "CDS", start, end));
        m.features.push(feat("ori", "rep_origin", 2000, 2400));
        let (sc, report) = scene(&m, Options::default());

        // The label really is in the column this case is about, or the mirror
        // half of the defect would go untested.
        let (x, anchor) = sc
            .items
            .iter()
            .find_map(|i| match i {
                Item::Text {
                    x, anchor, text, ..
                } if text.starts_with("pCMV") => Some((*x, *anchor)),
                _ => None,
            })
            .expect("the label");
        let right = anchor == Anchor::Start;
        assert_eq!(right, want_right_column, "label at x={x}");

        for (text, l, r) in text_extents(&sc) {
            assert!(
                l >= 0.0 && r <= sc.width,
                "{text:?} runs from {l} to {r} on a canvas 0..{} ({start}..{end})",
                sc.width
            );
        }
        // Shortened rather than cropped, and named -- a label the reader can
        // see has been cut is worth more than one that silently lost a letter.
        assert_eq!(report.labels_truncated, vec![name.to_string()]);
    }
}

#[test]
fn a_name_that_really_fits_is_not_shortened_by_the_stricter_measure() {
    // The control for the change of unit: measuring in Helvetica must shorten
    // only what would otherwise have been cropped. `room` is the distance from
    // the label's own origin to the canvas edge, so the two conditions are the
    // same inequality -- a name whose real glyphs fit goes out whole.
    let mut m = plasmid(5386, true);
    m.features.push(feat("bla", "CDS", 100, 960));
    m.features.push(feat("lacZalpha", "gene", 1200, 1800));
    m.features.push(feat("ori", "rep_origin", 3000, 3200));
    let (sc, report) = scene(&m, Options::default());
    assert!(
        report.labels_truncated.is_empty(),
        "{:?}",
        report.labels_truncated
    );
    for name in ["bla", "lacZalpha", "ori"] {
        assert!(
            sc.items
                .iter()
                .any(|i| matches!(i, Item::Text { text, .. } if text == name)),
            "{name} was shortened or dropped"
        );
    }
    for (text, l, r) in text_extents(&sc) {
        assert!(l >= 0.0 && r <= sc.width, "{text:?}: {l}..{r}");
    }
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

    // A hostile *length*, not just a hostile coordinate. This list is what the
    // name of this test promises to cover and the longest molecule in it was
    // 3000 bp, so the ruler's `base += step` and `angle`'s i64 modulo -- both of
    // which only overflow near the top of the range -- were never reached.
    let mut declared_max = Molecule {
        declared_len: Some(u64::MAX),
        topology: Topology::Circular,
        ..Default::default()
    };
    declared_max.features.push(feat("w", "CDS", 1, u64::MAX));
    let mut past_i64 = Molecule {
        declared_len: Some(5_000_000_000_000_000_000),
        topology: Topology::Linear,
        ..Default::default()
    };
    past_i64.features.push(feat(
        "v",
        "CDS",
        4_999_999_999_999_999_000,
        5_000_000_000_000_000_000,
    ));

    let cases = [
        Molecule::default(),
        plasmid(0, true),
        plasmid(1, true),
        plasmid(1, false),
        zero_coords,
        huge,
        track,
        declared_max,
        past_i64,
    ];
    for (i, m) in cases.iter().enumerate() {
        let (svg, _) = circular_svg(m, Options::default());
        well_formed(&svg).unwrap_or_else(|e| panic!("case {i}: {e}"));
    }
}

#[test]
fn a_declared_length_at_the_top_of_the_u64_range_neither_panics_nor_runs_away() {
    // `LOCUS HOSTILE 18446744073709551615 bp DNA circular SYN` with `ORIGIN`
    // immediately followed by `//`. The GenBank reader parses that length with a
    // bare `parse::<u64>()` and `Molecule::validate` compares a declared length
    // against the sequence only when there *is* one -- annotation-only records
    // are a supported class -- so nothing between the file and here bounds it,
    // and `pl info` prints it back cheerfully.
    //
    // Two sites overflowed. The ruler's `base += step` walked 2e18 at a time and
    // passed u64::MAX on the tenth tick: debug panicked, and the shipped release
    // build wrapped to 1553255926290448384, which is still `<= len`, so the loop
    // never ended and pushed two `Item`s a turn until the process was killed
    // with no file and no error. `angle` did its modulo through i64, which
    // panicked above about 4.6e18.
    let mut m = Molecule {
        declared_len: Some(u64::MAX),
        topology: Topology::Circular,
        ..Default::default()
    };
    // Ends on the last base, so the closing angle of its arc is asked for at
    // `len` -- where `angle(b + 1, len)` overflowed before `angle` was entered.
    m.features.push(feat("everything", "CDS", 1, u64::MAX));

    let (svg, report) = circular_svg(&m, Options::default());
    well_formed(&svg).expect("malformed svg");
    assert!(report.malformed.is_empty(), "{:?}", report.malformed);
    assert_eq!(report.labels_placed, 1);
    // The ruler stopped at the last tick that fits instead of wrapping round.
    assert!(
        svg.contains("18,000,000,000,000,000,000"),
        "the last in-range tick is missing"
    );
    assert!(
        !svg.contains("1,553,255,926,290,448,384"),
        "the ruler wrapped past u64::MAX and started again"
    );
    assert!(svg.contains("18,446,744,073,709,551,615 bp"));
}

#[test]
fn the_angle_of_a_base_is_right_at_lengths_no_i64_can_hold() {
    // `((base as i64 - 1) % l + l) % l` overflowed the `+ l` once `len` passed
    // about 4.6e18 with a base large enough to reach it -- and the ruler walks
    // bases all the way up to `len`, so it did. Above i64::MAX the same
    // expression stopped panicking and started lying instead: `l` went negative
    // and it returned 0 for every base, i.e. every feature at twelve o'clock,
    // with nothing in any `Report`.
    let huge = 5_000_000_000_000_000_000u64;
    assert!(angle(huge, huge).is_finite());
    assert!(
        angle(huge, huge) > TAU * 0.99,
        "the last base is nearly a full turn round, got {}",
        angle(huge, huge)
    );
    let half = angle(u64::MAX / 2, u64::MAX);
    assert!((half - TAU / 2.0).abs() < 1e-6, "half a turn is {half}");
    assert!(
        angle(1, u64::MAX) == 0.0 && angle(2, u64::MAX) > 0.0,
        "every base collapsed onto the origin"
    );
    // Base 0 still lands one step *before* the origin, at every length.
    assert!((angle(0, 1000) - TAU * 999.0 / 1000.0).abs() < 1e-12);
    assert!((angle(0, u64::MAX) - TAU).abs() < 1e-9);
    // And `angle_past` is `angle(base + 1)` wherever that addition is safe.
    for (b, len) in [(0u64, 1000u64), (1, 1000), (999, 1000), (500, 4000), (7, 7)] {
        assert!(
            (angle_past(b, len) - angle(b + 1, len)).abs() < 1e-12,
            "{b}/{len}"
        );
    }
    // ... and it is defined where that addition is not.
    assert_eq!(angle_past(u64::MAX, u64::MAX), 0.0);
    assert_eq!(angle_past(5, 0), 0.0);
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

// ---------------------------------------------------------------------------
// the label ring
// ---------------------------------------------------------------------------

/// The user's own plasmid, features only: the table its Features tab lists.
///
/// `mol.name` is left empty on purpose. That is what every SnapGene file gives
/// this function, and it is the input that captioned the exported figure
/// `unnamed`.
/// pKoV's 22 unique cutters, including the three co-located pairs that decide
/// whether folding is right: SalI/XbaI 6 bp apart, SphI/NsiI and XmaI/SmaI 2 bp.
fn pkov_sites() -> Vec<(String, u64)> {
    [
        ("AflII", 271u64),
        ("SpeI", 562),
        ("NdeI", 1_682),
        ("HindIII", 2_059),
        ("SnaBI", 2_648),
        ("BsrGI", 2_713),
        ("SalI", 4_413),
        ("XbaI", 4_419),
        ("SphI", 4_758),
        ("NsiI", 4_760),
        ("BglII", 4_886),
        ("SacI", 5_171),
        ("PmeI", 5_345),
        ("PstI", 5_464),
        ("BamHI", 5_588),
        ("MluI", 5_932),
        ("BclI", 6_561),
        ("XmaI", 6_917),
        ("SmaI", 6_919),
        ("ScaI", 7_117),
        ("EcoRI", 7_530),
        ("BbsI", 7_963),
    ]
    .iter()
    .map(|(n, p)| (n.to_string(), *p))
    .collect()
}

/// pKoV's 12 **dual** cutters, both positions each — 24 pairs, 12 enzymes.
///
/// `pl_enzymes::digest_all` on the user's own file, so `--sites dual` in a test
/// and `--sites dual` on the command line are the same site list. Read as a
/// literal table because `pl-draw` has no reference to an enzyme anywhere: it is
/// handed `(name, position)` and treats the name as an opaque identity token,
/// which is exactly the ignorance a `BTreeSet` of those tokens preserves.
fn pkov_dual_sites() -> Vec<(String, u64)> {
    [
        ("AatII", 5_824u64),
        ("AatII", 6_306),
        ("AgeI", 5_181),
        ("AgeI", 6_272),
        ("BsmBI", 7_306),
        ("BsmBI", 7_859),
        ("BspEI", 5_591),
        ("BspEI", 7_534),
        ("BstBI", 7_001),
        ("BstBI", 7_954),
        ("Esp3I", 7_306),
        ("Esp3I", 7_859),
        ("HpaI", 1_985),
        ("HpaI", 2_807),
        ("KpnI", 2_340),
        ("KpnI", 6_925),
        ("NcoI", 6_120),
        ("NcoI", 7_229),
        ("PvuI", 5_100),
        ("PvuI", 6_310),
        ("SacII", 2_320),
        ("SacII", 6_609),
        ("StuI", 3_156),
        ("StuI", 5_605),
    ]
    .iter()
    .map(|(n, p)| (n.to_string(), *p))
    .collect()
}

/// pKoV's 6 **multi** cutters, every position — 25 pairs, 6 enzymes.
///
/// The fixture the mention-counting bug is only visible on. DraI alone accounts
/// for nine of these pairs at spacings from 44 to 612 bases, which at every
/// canvas size is far above the fold threshold (about 2 bases at the default
/// radius), so it is nine ticks, nine labels and — summed as a tally — nine
/// enzymes. Two coincidences here are chosen and not incidental: DraI 5,345 sits
/// exactly on [`pkov_sites`]' PmeI 5,345, and BsmBI/Esp3I in
/// [`pkov_dual_sites`] are isoschizomers coinciding at both of their positions.
/// So the concatenated list exercises genuine folds of DIFFERENT enzymes as well
/// as repeats of ONE, which are the two cases a tally cannot tell apart.
fn pkov_multi_sites() -> Vec<(String, u64)> {
    [
        ("ClaI", 2_701u64),
        ("ClaI", 3_038),
        ("ClaI", 4_850),
        ("DraI", 1_182),
        ("DraI", 1_226),
        ("DraI", 1_750),
        ("DraI", 2_357),
        ("DraI", 2_969),
        ("DraI", 5_345),
        ("DraI", 5_381),
        ("DraI", 7_275),
        ("DraI", 7_614),
        ("MfeI", 591),
        ("MfeI", 622),
        ("MfeI", 5_294),
        ("NruI", 5_102),
        ("NruI", 5_838),
        ("NruI", 6_020),
        ("NruI", 6_686),
        ("PvuII", 2_994),
        ("PvuII", 3_138),
        ("PvuII", 7_634),
        ("SspI", 236),
        ("SspI", 3_962),
        ("SspI", 7_222),
    ]
    .iter()
    .map(|(n, p)| (n.to_string(), *p))
    .collect()
}

/// `Sites::of`'s ordering: by position, then by name, so a test's figure is
/// `pl export`'s figure.
fn sorted_sites(parts: &[Vec<(String, u64)>]) -> Vec<(String, u64)> {
    let mut out: Vec<(String, u64)> = parts.iter().flatten().cloned().collect();
    out.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    out
}

fn pkov() -> Molecule {
    let mut mol = plasmid(8_117, true);
    mol.name.clear();
    for &(name, start, end, rev) in &[
        ("cat promoter", 7_748u64, 7_850u64, true),
        ("CmR", 7_088, 7_747, true),
        ("sacB promoter", 3_398, 3_843, true),
        ("SacB", 1_976, 3_397, true),
        ("f1 ori", 3_945, 4_399, true),
        ("pSC101 ori", 363, 585, false),
        ("Rep101(Ts)", 633, 1_583, false),
        ("decR", 5_423, 5_878, false),
        ("decR his", 5_423, 5_905, false),
    ] {
        let mut f = feat(name, "misc_feature", start, end);
        f.strand = if rev {
            pl_core::Strand::Reverse
        } else {
            pl_core::Strand::Forward
        };
        mol.features.push(f);
    }
    mol
}

/// Every leader in a scene, as `(first point, last point)`.
///
/// A leader is the only thing drawn with `ink::LEADER_STROKE`, which is what
/// makes it findable without reaching into the layout.
fn leaders(scene: &Scene) -> Vec<((f64, f64), (f64, f64))> {
    scene
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Path { segs, stroke, .. } if stroke.as_deref() == Some(ink::LEADER_STROKE) => {
                let pts: Vec<(f64, f64)> = segs
                    .iter()
                    .map(|s| match *s {
                        Seg::Move(x, y) | Seg::Line(x, y) => (x, y),
                        Seg::Arc { cx, cy, r, to, .. } => scene::on_circle(cx, cy, r, to),
                        Seg::Close => (f64::NAN, f64::NAN),
                    })
                    .collect();
                Some((*pts.first()?, *pts.last()?))
            }
            _ => None,
        })
        .collect()
}

/// PROVEN TO FAIL at e087e27, on the leader length.
///
/// One column per side pins a label's `x` to `cx ± (ro + 26)` whatever its
/// angle, so the leader has to run `26 + ro(1 - |sin a|)` horizontally to reach
/// it: `f1 ori`, at 185 degrees on this plasmid, got a 241 pt leader across a
/// 242 pt radius — 0.995 of it, at a degree and a half off horizontal. That is
/// the "long, near-horizontal leader lines that are hard to trace back to their
/// tick" the user reported, and it is the same defect on the screen and in the
/// figure because both had a fixed two-column layout.
///
/// Written against `Options::default()` and features alone so it compiles at
/// e087e27, where `Options` has no `sites` and no `title`.
#[test]
fn no_leader_runs_most_of_the_ring_radius() {
    let (scene, report) = scene(&pkov(), Options::default());
    assert!(report.labels_placed >= 8, "{report:?}");
    let ls = leaders(&scene);
    assert_eq!(ls.len(), report.labels_placed, "one leader per label");

    // The ring, taken from the scene rather than recomputed: the backbone is the
    // one full-turn arc drawn in the backbone's own ink.
    let cx = 720.0 / 2.0;
    let cy = 720.0 / 2.0;
    let ro = ls
        .iter()
        .map(|(tip, _)| ((tip.0 - cx).powi(2) + (tip.1 - cy).powi(2)).sqrt())
        .fold(0.0_f64, f64::max);
    assert!(ro > 100.0, "the ring came out at {ro}");

    for (tip, end) in &ls {
        let run = ((end.0 - tip.0).powi(2) + (end.1 - tip.1).powi(2)).sqrt();
        assert!(
            run <= 0.6 * ro,
            "a leader runs {run:.1} pt across a {ro:.1} pt radius; \
             a reader cannot follow that back to its tick"
        );
    }
}

/// The leader bound as a property over shapes, not one fixture.
///
/// `no_leader_runs_most_of_the_ring_radius` above runs one molecule at
/// `Options::default()`, where `sites` is empty — so it exercises feature labels
/// on pKoV and nothing else, and the headline "-43% on the longest leader" it
/// guards was measured on that same file. It did not generalise: with a
/// full-canvas twelve-o'clock row, pGhost9ISS1's worst leader was 290 pt and
/// NC_017320's 243, against the 241 pt on pKoV that started this. Both are files
/// where a cluster of sites sits near the origin, which is the shape this covers.
///
/// The bound here is on the ROW runs specifically, because that is what
/// [`ring::row_span`] fixed and what a fixture-of-one could not see: a row member's
/// tick is inside `tick_r * sin(30 deg)` of centre by construction, so a row that
/// reaches the canvas edge reproduces exactly the near-horizontal run the two
/// columns produced.
#[test]
fn a_row_leader_is_bounded_whatever_the_molecule() {
    let named = |list: &[(&str, u64)]| -> Vec<(String, u64)> {
        list.iter().map(|(n, p)| (n.to_string(), *p)).collect()
    };
    // Three shapes, all real: sites spread evenly; a polylinker straddling the
    // origin (pGhost9ISS1's eight sites within 30 bp of base 1); and a dense
    // cluster away from the origin (pET28a's twelve MCS cutters).
    let corpus: Vec<(&str, Vec<(String, u64)>)> = vec![
        (
            "spread",
            named(&[
                ("AflII", 271),
                ("NdeI", 1_682),
                ("SnaBI", 2_648),
                ("SalI", 4_413),
                ("BglII", 4_886),
                ("BamHI", 5_588),
                ("BclI", 6_561),
                ("EcoRI", 7_530),
            ]),
        ),
        (
            "polylinker across the origin",
            named(&[
                ("EcoRI", 4_573),
                ("PstI", 4_583),
                ("XmaI", 4_585),
                ("SmaI", 4_587),
                ("SpeI", 4_597),
                ("EagI", 8),
                ("NotI", 8),
                ("SacII", 20),
            ]),
        ),
        (
            "a dense cluster off the origin",
            named(&[
                ("NcoI", 2_000),
                ("EcoRI", 2_010),
                ("SacI", 2_020),
                ("KpnI", 2_030),
                ("XmaI", 2_040),
                ("SmaI", 2_050),
                ("BamHI", 2_060),
                ("XbaI", 2_070),
                ("SalI", 2_080),
                ("PstI", 2_090),
                ("SbfI", 2_100),
                ("HindIII", 2_110),
            ]),
        ),
    ];
    for (why, sites) in corpus {
        let opts = Options {
            sites,
            ..Default::default()
        };
        let (sc, report) = scene(&pkov(), opts);
        assert!(report.labels_placed >= 12, "{why}: {report:?}");
        let (cx, cy) = (360.0, 360.0);
        let ls = leaders(&sc);
        let ro = ls
            .iter()
            .map(|(tip, _)| ((tip.0 - cx).powi(2) + (tip.1 - cy).powi(2)).sqrt())
            .fold(0.0_f64, f64::max);
        for (tip, end) in &ls {
            // A row leader is the one whose last leg is vertical-ish; a column's
            // is horizontal-ish. Bound the horizontal run of a row label, which
            // is what the unbounded row gave away.
            let dx = (end.0 - tip.0).abs();
            let dy = (end.1 - tip.1).abs();
            if dy > dx {
                assert!(
                    dx <= ro,
                    "{why}: a row leader runs {dx:.1} pt sideways across a {ro:.1} pt radius"
                );
            }
            let run = (dx * dx + dy * dy).sqrt();
            assert!(
                run <= 1.35 * ro,
                "{why}: a leader runs {run:.1} pt across a {ro:.1} pt radius"
            );
        }
    }
}

/// PROVEN TO FAIL at 0ebaa41 — measured `sites_named = 5`.
///
/// `Report::sites_named` counts enzymes, and `Label::names` was a TALLY of the
/// enzymes one label names. DraI's five cuts here are 44 to 612 bases apart and
/// the fold threshold at this radius is about 2 bases, so they are five ticks,
/// five labels and — summed — five enzymes. On the user's own plasmid that
/// arithmetic put "71 of 40 cutters labelled" into an exported figure. No
/// accumulation over labels can return 1; only de-duplicating by identity can.
#[test]
fn one_enzyme_cutting_five_times_is_one_enzyme_on_the_map() {
    let sites: Vec<(String, u64)> = [1_182u64, 1_226, 1_750, 2_357, 2_969]
        .iter()
        .map(|p| ("DraI".to_string(), *p))
        .collect();
    let (_, r) = scene(
        &pkov(),
        Options {
            sites,
            ..Default::default()
        },
    );
    assert_eq!(
        r.sites_named, 1,
        "five ticks of one enzyme name one enzyme: {r:?}"
    );
    assert_eq!(r.sites_dropped, 0, "{r:?}");
    assert!(r.sites_hidden.is_empty(), "{:?}", r.sites_hidden);
}

/// PROVEN TO FAIL at 0ebaa41 in 8 of these 12 cells — every `dual` and every
/// `all` row, at every width.
///
/// `ring::Disclosure::closes` is the guard against the map lying by omission, and
/// the only data it was ever asserted on was 22 unique cutters at one position
/// each — where a mention, a label and an enzyme are the same integer and the
/// guard cannot fail whatever the implementation. That is a check that cannot
/// fail, and the wrong number in the figure was its symptom.
///
/// The `unique` row is kept as the control: it is the row that passes at 0ebaa41,
/// and the reason the other two are here. Nothing is pinned at 520/400/300 pt
/// except the invariants, because what fits is a packing question and the
/// conservation law is not: once `hidden` is `admitted \ named`, `closes()`
/// reduces to `distinct(admitted) + single + dual + multi == cutters`, which has
/// no canvas in it. Measured at 0ebaa41, `sites_named + sites_dropped` is 46 in
/// every `dual` cell and 71 in every `all` cell against the 34 and 40 enzymes
/// asked for.
///
/// The four bucket counts are DERIVED from the three fixture tables and not
/// written down, so the row cannot drift from the data it describes if the
/// 58-enzyme set ever gains or loses a pKoV cutter — the previous form pinned
/// `cutters: 40` and `(12, 6)` by hand, and a literal beside a table is a literal
/// that goes stale in silence. `none` is here because `pl`'s bucket arithmetic
/// got that mode wrong while `closes()` passed: this row can only check that a
/// non-zero `single` is a term of the sum, which it now is. Whether an enzyme
/// lands in the RIGHT bucket is a question about a digest and belongs where a
/// digest exists — `bins/pl/tests/cli.rs`, which pins the exact sentence for all
/// four modes; `pl-draw` cannot depend on `pl-enzymes` and has no way to know
/// that XhoI cuts once.
#[test]
fn the_disclosure_closes_on_every_sites_filter_not_only_the_one_with_no_folds() {
    let distinct = |s: &[(String, u64)]| -> usize {
        s.iter()
            .map(|(n, _)| n.as_str())
            .collect::<std::collections::BTreeSet<&str>>()
            .len()
    };
    let (n_single, n_dual, n_multi) = (
        distinct(&pkov_sites()),
        distinct(&pkov_dual_sites()),
        distinct(&pkov_multi_sites()),
    );
    let cutters = n_single + n_dual + n_multi;
    let unique = sorted_sites(&[pkov_sites()]);
    let dual = sorted_sites(&[pkov_sites(), pkov_dual_sites()]);
    let all = sorted_sites(&[pkov_sites(), pkov_dual_sites(), pkov_multi_sites()]);
    for (why, sites, s_bucket, d_bucket, m_bucket) in [
        ("unique", unique, 0, n_dual, n_multi),
        ("dual", dual, 0, 0, n_multi),
        ("all", all, 0, 0, 0),
        ("none", Vec::new(), n_single, n_dual, n_multi),
    ] {
        let enzymes: std::collections::BTreeSet<&str> =
            sites.iter().map(|(n, _)| n.as_str()).collect();
        for w in [720.0, 520.0, 400.0, 300.0] {
            let (_, r) = scene(
                &pkov(),
                Options {
                    width: w,
                    height: w,
                    sites: sites.clone(),
                    ..Default::default()
                },
            );
            // pl-draw's own conservation law, in pl-draw's own units and
            // independent of anything the caller believes about the molecule.
            assert_eq!(
                r.sites_named + r.sites_dropped,
                enzymes.len(),
                "--sites {why} at {w}: {} named + {} dropped of {} enzymes asked for",
                r.sites_named,
                r.sites_dropped,
                enzymes.len()
            );
            assert_eq!(
                r.sites_dropped,
                r.sites_hidden.len(),
                "--sites {why} at {w}"
            );
            assert!(
                r.sites_hidden.iter().all(|n| enzymes.contains(n.as_str())),
                "--sites {why} at {w}: hidden names something never asked for: {:?}",
                r.sites_hidden
            );
            // And the sentence a reader actually sees.
            let told = ring::Disclosure {
                cutters,
                single: s_bucket,
                dual: d_bucket,
                multi: m_bucket,
                labelled: r.sites_named,
                hidden: r.sites_dropped,
                shortened: r.sites_shortened,
            };
            assert!(told.closes(), "--sites {why} at {w}: {}", told.long());
            assert!(
                told.labelled <= told.cutters,
                "--sites {why} at {w}: {}",
                told.long()
            );
        }
    }
    // The anchor: a 720 pt figure of this plasmid fits every cutter it is given.
    let (_, r) = scene(
        &pkov(),
        Options {
            sites: sorted_sites(&[pkov_sites(), pkov_dual_sites(), pkov_multi_sites()]),
            ..Default::default()
        },
    );
    assert_eq!(r.sites_named, cutters, "{r:?}");
    assert!(r.sites_hidden.is_empty(), "{:?}", r.sites_hidden);
}

/// Two properties, and the second is what the first one used to conceal.
///
/// **`labelled` counts enzymes a reader can READ.** Not "a label was placed for
/// this enzyme" — the paint loop counted that, and a figure whose only enzyme text
/// was `Ec...` reported `1 of 1 cutters labelled · 1 shortened`. At 300 pt the note
/// collapses to `tiny()` — `1/1` — which drops the shortening clause altogether: a
/// publication figure asserting every cutter is labelled, naming no enzyme anywhere
/// on it, disclosing nothing. Proven to fail against the tree as handed over, which
/// measured `sites_named = 1` there.
///
/// **And the ring is never sized so that an admitted enzyme CANNOT be read.** This
/// test used to pin the opposite — `Ec...` at 300, 500, 720 and 1400 pt, on the
/// argument that being size-invariant made it "not a canvas-too-small case". It was
/// not a canvas-too-small case; it was a RESERVE case, and the size-invariance was
/// the evidence. `widest_of` reserved radius only for `Side::Left | Side::Right`,
/// so a lone six-o'clock label reserved nothing, the ring grew to a 305 pt radius
/// on a 720 pt pane, and 27 pt of room was left for a 59 pt name — while
/// [`ring::label_room`] charges a row label the COLUMN's allowance regardless,
/// because [`ring::place_ring`] may spill it there. So the figure was honest about
/// a loss it had no reason to take, and a test asserted the loss.
///
/// The rows below are now the real boundary, measured rather than assumed: at 200 pt
/// the pane genuinely cannot hold `EcoRI  121` even with the reserve capped, and the
/// name does not survive; at 240 the name survives and the coordinate goes; from
/// 300 pt up the label is whole. All three cases matter — without the 240 pt row the
/// test would pass equally well if shortening stopped counting anything, and without
/// the 300 pt row it would pass with the reserve bug back.
#[test]
fn an_enzyme_whose_label_was_cut_to_an_ellipsis_is_not_a_labelled_cutter() {
    let text_items = |sc: &Scene| -> Vec<String> {
        sc.items
            .iter()
            .filter_map(|i| match i {
                Item::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    };
    let mut tiny = plasmid(244, true);
    tiny.name = "pTiny".into();
    // Six o'clock on a 244 bp circle: the BOTTOM row, the run whose labels used to
    // reserve nothing at all.
    let at = |w: f64, pos: u64| -> (Vec<String>, Report) {
        let (sc, r) = scene(
            &tiny,
            Options {
                width: w,
                height: w,
                sites: vec![("EcoRI".to_string(), pos)],
                ..Default::default()
            },
        );
        (text_items(&sc), r)
    };

    // 200 pt: `Eco...` — the name itself did not survive, so no enzyme is named.
    // This is the assertion the test is called after, and it now sits on a canvas
    // that really is too small rather than on a reserve that gave up.
    let (texts, r) = at(200.0, 121);
    assert!(
        texts.iter().any(|t| t == "Eco..."),
        "at 200 pt the fixture no longer reproduces a name-destroying cut: {texts:?}"
    );
    assert_eq!(
        r.sites_named, 0,
        "`Eco...` names no enzyme, yet {} is claimed: {texts:?}",
        r.sites_named
    );
    assert_eq!(r.sites_hidden, vec!["EcoRI".to_string()]);
    assert_eq!(r.sites_shortened, 1, "it IS shortened, and said so");

    // 240 pt: `EcoRI` — the coordinate went and the NAME survived, so it counts.
    // Without this row the whole test would pass just as well if shortening stopped
    // counting anything at all.
    let (texts, r) = at(240.0, 121);
    assert_eq!(
        texts.iter().filter(|t| *t == "EcoRI").count(),
        1,
        "{texts:?}"
    );
    assert_eq!(r.sites_named, 1, "{r:?} — {texts:?}");
    assert!(r.sites_hidden.is_empty(), "{:?}", r.sites_hidden);
    assert_eq!(r.sites_shortened, 1, "the coordinate was cut: {texts:?}");

    // From 300 pt up the label is WHOLE, in the row exactly as in a column. Four
    // positions, one per run: twelve o'clock, the right column, six o'clock, the
    // left column. Before the reserve counted site labels wherever they land, the
    // two rows drew `Ec...` at every one of these sizes.
    for w in [300.0, 500.0, 720.0, 1400.0] {
        for pos in [1u64, 61, 121, 183] {
            let (texts, r) = at(w, pos);
            let want = format!("EcoRI  {pos}");
            assert!(
                texts.contains(&want),
                "at {w} pt base {pos}: no {want:?} on the figure, only {texts:?}"
            );
            assert_eq!(r.sites_named, 1, "at {w} pt base {pos}: {r:?}");
            assert!(r.sites_hidden.is_empty(), "at {w} pt base {pos}: {r:?}");
            assert_eq!(
                r.sites_shortened, 0,
                "at {w} pt base {pos} nothing needed shortening: {texts:?}"
            );
        }
    }

    // Control two, the general form: on the real plasmid at every width, every
    // enzyme the report calls named appears LITERALLY in the figure's text. This
    // is the property a reader checks, and no count can stand in for it.
    let all = sorted_sites(&[pkov_sites(), pkov_dual_sites(), pkov_multi_sites()]);
    for w in [720.0, 520.0, 420.0, 360.0, 300.0] {
        let (sc, r) = scene(
            &pkov(),
            Options {
                width: w,
                height: w,
                sites: all.clone(),
                ..Default::default()
            },
        );
        let texts = text_items(&sc);
        let readable = all
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<std::collections::BTreeSet<&str>>()
            .into_iter()
            .filter(|n| texts.iter().any(|t| t.contains(n)))
            .count();
        assert_eq!(
            r.sites_named, readable,
            "at {w} pt the figure claims {} named enzymes and carries {readable}",
            r.sites_named
        );
    }
}

/// The two-pass note is exact, not approximate.
///
/// `bins/pl` and `bins/pl-gui` both build the disclosure line by rendering once
/// to get the counts and again to draw them. That is only honest if adding the
/// line cannot change what it is counting, and it cannot: `note` reaches
/// `centre_room` -> `keep_clear` -> the ruler's radius and nothing there feeds
/// back into the reserve, the geometry or the packing. Asserted rather than left
/// as a claim in a comment, because a comment is where that claim was.
#[test]
fn the_note_does_not_change_what_it_counts() {
    let sites: Vec<(String, u64)> = pkov_sites();
    let base = Options {
        sites: sites.clone(),
        title: Some("pKoV with His decR".into()),
        ..Default::default()
    };
    let (_, first) = scene(&pkov(), base.clone());
    let told = ring::Disclosure {
        cutters: 40,
        single: 0,
        labelled: first.sites_named,
        dual: 12,
        multi: 6,
        hidden: first.sites_dropped,
        shortened: first.sites_shortened,
    };
    assert!(told.closes(), "{told:?}");
    let (sc, second) = scene(
        &pkov(),
        Options {
            note: Some(told),
            ..base
        },
    );
    assert_eq!(first.sites_named, second.sites_named);
    assert_eq!(first.sites_dropped, second.sites_dropped);
    assert_eq!(first.sites_shortened, second.sites_shortened);
    assert_eq!(first.labels_hidden, second.labels_hidden);
    assert_eq!(first.labels_truncated, second.labels_truncated);
    // And the line is actually in the figure, which is the point of it.
    assert!(
        sc.items.iter().any(
            |i| matches!(i, Item::Text { text, .. } if text.contains("cutters") && text.contains("dual"))
        ),
        "the figure does not say what it is not showing"
    );
}

/// The exported figure states its own filter, or says nothing at all — never a
/// count with an ellipsis through it.
#[test]
fn the_figure_narrows_its_disclosure_by_choosing_a_form_not_by_cutting_one() {
    let told = ring::Disclosure {
        cutters: 40,
        single: 0,
        labelled: 22,
        dual: 12,
        multi: 6,
        hidden: 0,
        shortened: 16,
    };
    let mut seen = 0;
    for w in [720.0, 520.0, 420.0, 360.0, 300.0, 260.0] {
        let (sc, _) = scene(
            &pkov(),
            Options {
                width: w,
                height: w,
                note: Some(told),
                sites: pkov_sites(),
                ..Default::default()
            },
        );
        let lines: Vec<&String> = sc
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Text { text, .. } if text.contains('/') || text.contains("cutters") => {
                    Some(text)
                }
                _ => None,
            })
            .collect();
        for l in &lines {
            assert!(
                !l.contains("..."),
                "at {w}: the disclosure line was cut to {l:?}, which puts an ellipsis \
                 through a count"
            );
        }
        if lines.iter().any(|l| l.contains("40")) {
            seen += 1;
        }
    }
    assert!(
        seen >= 4,
        "the line was dropped at {} of six sizes",
        6 - seen
    );
}

/// COMPILE-ONLY at e087e27: `Options::sites` does not exist there, which is the
/// defect — `pl-draw` held no reference to an enzyme anywhere, so every exported
/// figure of this plasmid had no restriction sites on it at all. The behaviour is
/// proven at e087e27 by the `pl export` test in `bins/pl/tests/cli.rs`, which
/// runs the shipped binary and finds no enzyme in the SVG.
#[test]
fn a_folded_site_label_never_costs_the_ring_its_radius() {
    let named = |list: &[(&str, u64)]| -> Vec<(String, u64)> {
        list.iter().map(|(n, p)| (n.to_string(), *p)).collect()
    };
    // The same list twice, once with a co-located pair in it and once without.
    // XmaI and SmaI cut two bases apart, so they fold; `HindIII  2,059` is the
    // widest single label in both. The radius must therefore be the same, and
    // under the rule this replaces it was not: the folded label
    // `XmaI/SmaI  6,917-6,919` is eight characters wider than HindIII and took
    // 53 pt of radius off the ring to fit itself in.
    let sites = named(&[("XmaI", 6_917), ("SmaI", 6_919), ("HindIII", 2_059)]);
    let bare = Options {
        sites: named(&[("XmaI", 6_917), ("HindIII", 2_059)]),
        ..Default::default()
    };
    let with = Options {
        sites: sites.clone(),
        ..Default::default()
    };
    let radius_of = |o: Options| -> f64 {
        let (s, _) = scene(&pkov(), o);
        s.items
            .iter()
            .find_map(|it| match *it {
                Item::Circle { r, ref stroke, .. } if stroke == ink::BACKBONE_STROKE => Some(r),
                _ => None,
            })
            .expect("the backbone was drawn")
    };
    let (r0, r1) = (radius_of(bare), radius_of(with));
    assert!(
        (r0 - r1).abs() < 1.0,
        "folding a co-located pair cost the ring {:.1} pt of radius",
        r0 - r1
    );

    // And both enzymes are still named, at both coordinates.
    let (s, _) = scene(
        &pkov(),
        Options {
            sites,
            ..Default::default()
        },
    );
    let texts: Vec<&str> = s
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let joined = texts.join(" | ");
    assert!(joined.contains("XmaI"), "{joined}");
    assert!(joined.contains("SmaI"), "{joined}");
    assert!(joined.contains("6,917"), "{joined}");
    assert!(joined.contains("6,919"), "{joined}");
}

/// COMPILE-ONLY at e087e27: `Options::title` does not exist there. The behaviour
/// is proven at e087e27 by `bins/pl/tests/cli.rs`, where the shipped `pl export`
/// writes `<title>unnamed</title>` for this molecule.
#[test]
fn a_real_molecule_name_beats_the_filename_and_the_filename_beats_unnamed() {
    let bare = pkov();
    assert!(bare.name.is_empty(), "the premise: SnapGene has no name");

    let title_of = |mol: &Molecule, given: Option<&str>| -> String {
        let (s, _) = scene(
            mol,
            Options {
                title: given.map(str::to_string),
                ..Default::default()
            },
        );
        s.title.clone()
    };

    assert_eq!(title_of(&bare, None), "unnamed", "nothing to go on");
    assert_eq!(
        title_of(&bare, Some("pKoV with His decR")),
        "pKoV with His decR"
    );
    // Blank is nothing to go on either, not a name made of spaces.
    assert_eq!(title_of(&bare, Some("   ")), "unnamed");

    let mut named = pkov();
    named.name = "SYNPUC19CV".into();
    assert_eq!(
        title_of(&named, Some("pKoV with His decR")),
        "SYNPUC19CV",
        "a LOCUS name is a real name and a filename is a guess"
    );
}

/// `PROVENANCE.md` has to record the spec constants this crate actually writes.
///
/// The rule at `PROVENANCE.md:7` — every piece of format knowledge gets a row,
/// with its source, in the same commit as the code — is prose, and prose about
/// numbers goes stale in the direction of looking fine. The PNG stack is three
/// formats read out of four published specifications, and the only thing that
/// can tell whether the row still describes the encoder is the encoder.
///
/// So every needle below is BUILT FROM A MEASUREMENT, never written here as a
/// literal: the gamma and the eight chromaticities come out of the `gAMA` and
/// `cHRM` chunks of a PNG this crate encodes during the test, the header pair
/// comes off a real zlib stream, the units-per-em comes from parsing the
/// committed face. Change a constant in the code and this fails; change the
/// number in the row and this fails; delete the section and this fails.
///
/// WHAT IT CANNOT DO is judge whether the specification says those are the
/// right values. Nothing in this repository can — that is why `tools/ci.ps1`
/// runs PIL, `zlib` and fontTools over the same artifacts, and why the row
/// itself cites the documents and the dates they were read.
///
/// PROVEN TO FAIL three ways on 2026-08-04, because a documentation test that
/// cannot fail is the failure mode this repository has caught twice already:
///
/// 1. Against the working tree before the section was written — `PROVENANCE.md
///    has no section for the published specifications the PNG stack
///    implements`.
/// 2. Section present, one digit changed in the row (`gAMA 45455` → `45454`) —
///    `the provenance section does not record "gAMA 45455", which is what this
///    crate writes today`.
/// 3. Row restored, the constant changed in `png.rs` instead
///    (`45455u32` → `45454u32`) — `... does not record "gAMA 45454" ...`.
///
/// The second and third are the ones worth the lines. Either alone would be
/// satisfied by a test that only checked the section exists.
#[test]
fn the_provenance_rows_record_the_constants_the_code_actually_writes() {
    const PROV: &str = include_str!("../../../PROVENANCE.md");

    // The section, not the file: a number that happens to appear in the .dna
    // sections must not satisfy a claim about PNG.
    let at = PROV.find("Open published specifications").expect(
        "PROVENANCE.md has no section for the published specifications the PNG \
         stack implements, and CONTRIBUTING.md:29 requires the row in the same \
         commit as the code",
    );
    let sec = &PROV[at..];
    let sec = sec.split_once("\n## ").map_or(sec, |(head, _)| head);

    // One real file, walked as chunks rather than searched, so a needle cannot
    // be satisfied by a byte run inside the compressed data.
    let file = png::encode(&png::Image::filled(3, 2, [255, 255, 255]), Some(300.0));
    let payload = |want: &[u8; 4]| -> Vec<u8> {
        let mut i = 8;
        while i + 12 <= file.len() {
            let n = u32::from_be_bytes([file[i], file[i + 1], file[i + 2], file[i + 3]]) as usize;
            if &file[i + 4..i + 8] == want {
                return file[i + 8..i + 8 + n].to_vec();
            }
            i += 12 + n;
        }
        panic!(
            "no {} chunk in the encoder's own output",
            String::from_utf8_lossy(want)
        )
    };

    let ihdr = payload(b"IHDR");
    let gama = u32::from_be_bytes(payload(b"gAMA")[..4].try_into().unwrap());
    let chrm: Vec<u32> = payload(b"cHRM")
        .chunks_exact(4)
        .map(|w| u32::from_be_bytes(w.try_into().unwrap()))
        .collect();
    // Without this, an empty or short cHRM would join to the empty string, and
    // `contains("")` is true of every file — the needle would stop being able to
    // fail while still looking like an assertion.
    assert_eq!(
        chrm.len(),
        8,
        "cHRM carries {} values, not the white point and three primaries",
        chrm.len()
    );
    let srgb = payload(b"sRGB")[0];
    let phys = payload(b"pHYs")[8];
    let zhdr = deflate::zlib(b"provenance");
    let upem = font::Face::parse(font::REGULAR)
        .expect("the committed regular face parses")
        .units_per_em;

    let needles = [
        format!("colour type {}, bit depth {}", ihdr[9], ihdr[8]),
        format!("gAMA {gama}"),
        chrm.iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        format!("rendering intent {srgb}"),
        format!("unit specifier {phys}"),
        format!("0x{:02X} 0x{:02X}", zhdr[0], zhdr[1]),
        format!("{} units per em", upem as u64),
    ];
    for needle in &needles {
        assert!(
            sec.contains(needle.as_str()),
            "the provenance section does not record {needle:?}, which is what \
             this crate writes today"
        );
    }
}

/// The pixel budget refuses the canvas nothing else bounds, and refuses
/// nothing else.
///
/// PROVEN TO FAIL against the working tree of 2026-08-04 — where `png_at`
/// returned `(Vec<u8>, raster::Report)`, `png_budget` did not exist, and the
/// invocation asserted below allocated 10,987,919,938 bytes — and again
/// against the fixed tree with `png_at`'s `png_budget(..)?` replaced by
/// `let _ = png_budget(..);`, which fails on `183 mm at 2400 dpi is
/// 298978681 px and was not refused`.
///
/// `png_budget` and `png_at` are asserted to agree because they are two
/// callers of `png_scale` and `raster::size`, and a guard measuring a
/// different canvas from the one that gets allocated is not a guard.
#[test]
fn the_pixel_budget_refuses_a_canvas_no_flag_band_bounds() {
    let mol = plasmid(3000, true);
    let (sc, _) = scene(&mol, Options::default());

    // Nature's double column at the dpi ceiling `bins/pl` accepts: inside the
    // `--mm` band, inside the `--dpi` band, and 3x the ceiling on their
    // product.
    let e = png_budget(&sc, Some(183.0), 2400.0)
        .expect_err("183 mm at 2400 dpi is 298978681 px and was not refused");
    assert_eq!((e.w, e.h), (17291, 17291));
    assert!(
        e.pixels() > MAX_PIXELS,
        "{} px is not past the {MAX_PIXELS} px ceiling",
        e.pixels()
    );
    assert!(
        png_at(&sc, Some(183.0), 2400.0, [255, 255, 255]).is_err(),
        "png_budget refuses this and png_at renders it anyway"
    );

    // The message is the whole of what the user gets, so it carries all four
    // numbers they need: what they asked for, what it came to, the ceiling,
    // and a resolution that works.
    let said = e.to_string();
    for want in [
        "17291 x 17291",
        "299 megapixels",
        "100 megapixel",
        "2400 dpi",
    ] {
        assert!(
            said.contains(want),
            "the refusal does not say {want:?}: {said}"
        );
    }
    assert!(
        said.contains("11.1 GB"),
        "the refusal does not price the allocation: {said}"
    );

    // The controls. Every journal preset at every resolution the GUI offers is
    // an ordinary export and has to stay one — a guard that refuses a
    // publication figure is a worse defect than the abort it prevents. Asked
    // of `png_budget`, which costs no pixels, so all 48 are affordable.
    for p in page::PRESETS {
        for dpi in [150.0f64, 300.0, 600.0, 1200.0] {
            for mm in [p.single_mm, p.double_mm] {
                png_budget(&sc, Some(mm), dpi)
                    .unwrap_or_else(|e| panic!("{} at {mm} mm, {dpi} dpi: {e}", p.name));
            }
        }
    }

    // ...and the dimensions it reports are the ones `IHDR` ends up holding.
    // Rendered, so this is the real file rather than a second copy of the same
    // arithmetic. One size: the claim is the agreement, not the coverage.
    let (w, h) = png_budget(&sc, Some(89.0), 300.0).expect("89 mm at 300 dpi");
    let (bytes, _) = png_at(&sc, Some(89.0), 300.0, [255, 255, 255]).expect("just checked");
    let ihdr = |i: usize| u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap());
    assert_eq!(
        (w, h),
        (ihdr(16), ihdr(20)),
        "the budget measured a different canvas from the one IHDR describes"
    );

    // The three figures `MAX_PIXELS`'s own doc quotes for where the ceiling
    // sits, measured here rather than worked out on paper — that doc is what a
    // maintainer reads before moving the constant.
    let elsevier = page::preset("elsevier")
        .expect("a shipped preset")
        .double_mm;
    let wide = png_budget(&sc, Some(elsevier), 1200.0)
        .expect("the widest preset column at 1200 dpi has to stay an ordinary export");
    assert_eq!(
        wide,
        (8976, 8976),
        "{elsevier} mm at 1200 dpi is not the 8,976 px MAX_PIXELS's doc names"
    );
    for (mm, top) in [(elsevier, 1336.0), (183.0, 1388.0)] {
        png_budget(&sc, Some(mm), top)
            .unwrap_or_else(|e| panic!("{mm} mm at {top} dpi is refused: {e}"));
        assert!(
            png_budget(&sc, Some(mm), top + 1.0).is_err(),
            "{mm} mm still fits at {} dpi, so {top} is not where the ceiling bites",
            top + 1.0
        );
    }

    // The suggested dpi is the largest one that fits, not a safe guess.
    let d = e
        .fits_at_dpi
        .expect("a 183 mm figure fits at some resolution");
    png_budget(&sc, Some(183.0), d).expect("the refusal named a dpi it would refuse");
    assert!(
        png_budget(&sc, Some(183.0), d + 1.0).is_err(),
        "{} dpi also fits, so {d} was not the largest",
        d + 1.0
    );

    // And the branch with no printed width, where `--mm` is never read at all
    // and the scene's units are points: `--width 720 --dpi 2400` is 24,000 px
    // square, 576 megapixels, and was reachable the same way.
    let e = png_budget(&sc, None, 2400.0).expect_err("720 pt at 2400 dpi was not refused");
    assert_eq!((e.w, e.h), (24000, 24000), "{e}");
    png_budget(&sc, None, 300.0).expect("the same figure at 300 dpi is 3000 px");
}
