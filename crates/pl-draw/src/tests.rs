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

#[test]
fn a_short_feature_degrades_to_a_triangle_not_a_bow_tie() {
    // The arrowhead is clamped to half the arc; unclamped it would start before
    // the arc did and the path would cross itself.
    let d = arc_path(100.0, 100.0, 80.0, 98.0, 0.0, 0.01, Arrow::End);
    assert!(!d.is_empty());
    assert!(!d.contains("NaN"), "{d}");
    for tok in d.split(['M', 'L', 'A', 'Z', ' ', ',']) {
        if tok.is_empty() {
            continue;
        }
        assert!(
            tok.parse::<f64>().map(|v| v.is_finite()).unwrap_or(true),
            "non-finite coordinate {tok} in {d}"
        );
    }
}
