//! Does the Rust renderer agree with the TypeScript one?
//!
//! Two implementations of one specification. `tools/gen-agreement.mjs` runs
//! `@polylinker/circular-map` over a fixed set of inputs and writes down its
//! answers; this replays them through `pl-draw` and asserts the two agree.
//!
//! Regenerate with:
//!
//! ```text
//! node --experimental-strip-types tools/gen-agreement.mjs
//! ```
//!
//! `tools/ci.ps1` regenerates and diffs it, so the fixture cannot drift away
//! from the TypeScript without the gate noticing. A fixture nobody regenerates
//! is a record of what the reference used to say, which agrees with nothing.
//!
//! There is no skip-if-missing path. A cross-check that quietly does nothing
//! when its oracle is absent reports success for having run zero comparisons —
//! the failure mode this project has now hit twice.

use pl_draw::{
    angle, commas, esc, isotonic, n, nice_step, place_column, polar, ranges, safe_color, svg_of,
    LabelBox, Scene,
};

mod json;
use json::Json;

/// Agreement is to 1e-6, not to the bit.
///
/// Both languages use IEEE-754 doubles and the same operations in the same
/// order, so they mostly *are* bit-identical — but `sin`/`cos` are not
/// specified to the last bit by either standard, and pinning to exact equality
/// would make the test a report on libm rather than on the renderer.
const EPS: f64 = 1e-6;

fn load() -> Json {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/agreement.json");
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("{path}: {e}\nregenerate: node --experimental-strip-types tools/gen-agreement.mjs")
    });
    Json::parse(&text).expect("agreement.json is not valid JSON")
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS * (1.0 + a.abs().max(b.abs()))
}

/// One attribute of an opening tag, or `None` if the tag does not carry it.
///
/// The match is anchored on a preceding space, because `space="` occurs inside
/// `xml:space="` and an unanchored search would read the two as one attribute.
fn attr(tag: &str, name: &str) -> Option<String> {
    let pat = format!(" {name}=\"");
    let at = tag.find(&pat)? + pat.len();
    Some(tag[at..].split('"').next()?.to_string())
}

#[test]
fn isotonic_regression_agrees() {
    let doc = load();
    let cases = doc.get("isotonic").arr();
    assert!(cases.len() >= 50, "thin fixture: {} cases", cases.len());
    for (i, c) in cases.iter().enumerate() {
        let targets = c.get("targets").nums();
        let weights = c.get("weights").nums();
        let want = c.get("out").nums();
        let got = isotonic(&targets, &weights);
        assert_eq!(got.len(), want.len(), "case {i}");
        for (k, (g, w)) in got.iter().zip(&want).enumerate() {
            assert!(close(*g, *w), "case {i} element {k}: rust {g} vs ts {w}");
        }
    }
}

#[test]
fn column_placement_agrees() {
    let doc = load();
    let cases = doc.get("columns").arr();
    assert!(cases.len() >= 100, "thin fixture: {} cases", cases.len());
    let mut with_drops = 0;
    for (i, c) in cases.iter().enumerate() {
        let boxes: Vec<LabelBox> = c
            .get("boxes")
            .arr()
            .iter()
            .map(|b| LabelBox {
                ideal: b.get("ideal").num(),
                height: b.get("height").num(),
                weight: b.get("weight").num(),
            })
            .collect();
        let got = place_column(&boxes, c.get("lo").num(), c.get("hi").num());

        // Dropped indices, in removal order — lightest first. Comparing them as
        // sets would let the two disagree about *which* label yields while the
        // test still passed.
        let want_dropped: Vec<usize> = c
            .get("dropped")
            .nums()
            .iter()
            .map(|v| *v as usize)
            .collect();
        assert_eq!(got.dropped, want_dropped, "case {i}: dropped");
        if !want_dropped.is_empty() {
            with_drops += 1;
        }

        // `null` in the fixture is TypeScript's `NaN`, which is how it spells
        // "dropped"; Rust spells it `None`. Same claim, different vocabulary.
        for (k, want) in c.get("positions").arr().iter().enumerate() {
            match (want.opt_num(), got.positions[k]) {
                (None, None) => {}
                (Some(w), Some(g)) => {
                    assert!(close(g, w), "case {i} label {k}: rust {g} vs ts {w}")
                }
                (w, g) => panic!("case {i} label {k}: ts {w:?} vs rust {g:?}"),
            }
        }
    }
    assert!(
        with_drops >= 5,
        "only {with_drops} cases exercised dropping"
    );
}

#[test]
fn base_to_angle_agrees() {
    let doc = load();
    for (i, c) in doc.get("angles").arr().iter().enumerate() {
        let (base, length) = (c.get("base").num() as u64, c.get("length").num() as u64);
        let (got, want) = (angle(base, length), c.get("out").num());
        assert!(
            close(got, want),
            "case {i} base {base}/{length}: {got} vs {want}"
        );
    }
}

#[test]
fn polar_projection_agrees() {
    let doc = load();
    for (i, c) in doc.get("polar").arr().iter().enumerate() {
        let (x, y) = polar(
            c.get("cx").num(),
            c.get("cy").num(),
            c.get("r").num(),
            c.get("a").num(),
        );
        assert!(close(x, c.get("x").num()), "case {i} x");
        assert!(close(y, c.get("y").num()), "case {i} y");
    }
}

#[test]
fn colour_sanitising_agrees() {
    let doc = load();
    let cases = doc.get("colors").arr();
    let mut rejected = 0;
    for c in cases.iter() {
        let value = c.get("value").opt_str();
        let want = c.get("out").str();
        let got = safe_color(value.as_deref(), "#7f8a95");
        assert_eq!(got, want, "colour {value:?}");
        if got == "#7f8a95" && value.as_deref() != Some("#7f8a95") {
            rejected += 1;
        }
    }
    // The injection strings are the reason this check exists; if the fixture
    // ever stops containing any, the agreement is on the easy half only.
    assert!(rejected >= 5, "only {rejected} of {} refused", cases.len());
}

#[test]
fn segment_ranges_agree() {
    let doc = load();
    let cases = doc.get("ranges").arr();
    let mut empties = 0;
    for (i, c) in cases.iter().enumerate() {
        let got = ranges(
            c.get("start").num() as u64,
            c.get("end").num() as u64,
            c.get("length").num() as u64,
            c.get("circular").boolean(),
        );
        let want: Vec<(u64, u64)> = c
            .get("out")
            .arr()
            .iter()
            .map(|p| {
                let v = p.nums();
                (v[0] as u64, v[1] as u64)
            })
            .collect();
        assert_eq!(
            got,
            want,
            "case {i}: {}..{} of {} (circular {})",
            c.get("start").num(),
            c.get("end").num(),
            c.get("length").num(),
            c.get("circular").boolean()
        );
        if want.is_empty() {
            empties += 1;
        }
    }
    assert!(empties >= 2, "no malformed spans in the fixture");
}

#[test]
fn ruler_steps_agree() {
    let doc = load();
    for c in doc.get("steps").arr() {
        let raw = c.get("raw").num();
        assert_eq!(
            nice_step(raw),
            c.get("out").num() as u64,
            "nice_step({raw})"
        );
    }
}

#[test]
fn thousands_separators_agree() {
    let doc = load();
    for c in doc.get("commas").arr() {
        let v = c.get("v").num() as u64;
        assert_eq!(commas(v), c.get("out").str(), "commas({v})");
    }
}

#[test]
fn xml_escaping_agrees() {
    let doc = load();
    for c in doc.get("esc").arr() {
        let s = c.get("s").str();
        assert_eq!(esc(s), c.get("out").str(), "esc({s:?})");
    }
}

/// The two renderers must PRESENT their documents alike, not only compute alike.
///
/// PROVEN TO FAIL against 7bf5aad, on three of the four attributes: the
/// TypeScript root asked for `system-ui, -apple-system, 'Segoe UI', Helvetica,
/// …` where this crate asks for `Helvetica, 'Nimbus Sans', Arial, sans-serif`,
/// and carried neither `stroke-linecap` nor `stroke-linejoin` where this crate
/// carries both. Every other test in this file passed throughout, which is the
/// point of adding this one: they each compare a single pure function, the root
/// element belongs to no function, and so the harness whose whole purpose is
/// catching drift could not see the drift 7bf5aad opened. It survived four
/// commits.
///
/// None of the four is decoration.
///
/// * `font-family` decides which advances the reserved margin is actually spent
///   in. Both renderers reserve with the same 0.55 em/character estimate — see
///   `label_width` — which is what keeps their radii identical, and an estimate
///   names no face, so the root has to. With `system-ui` first, one renderer
///   drew in Segoe UI on Windows and San Francisco on macOS while the other drew
///   in Helvetica.
/// * `stroke-linecap` and `stroke-linejoin` decide where every leader-line elbow
///   and every arrowhead point lands. SVG's initial values are `butt` and
///   `miter`; this crate states `round` because its PDF back end emits `1 J 1 j`.
/// * `xml:space` decides whether a run of spaces inside a label is drawn at the
///   width it was measured at — and in a cut-site label the run is a delimiter
///   `fit_label` splits on, not spacing. See
///   `pdf::file_tests::a_cut_site_labels_two_spaces_reach_the_page_in_both_formats`.
///
/// Presence is asserted as well as equality, so this cannot pass by both sides
/// being equally silent — which is the state all four attributes were in for
/// `xml:space` when this test was written, and equality alone would have called
/// that agreement.
#[test]
fn svg_root_presentation_attributes_agree() {
    let doc = load();
    let want = doc.get("root");
    // The root is a constant template, so any scene reaches it; an empty one
    // keeps this about the document element and nothing inside it.
    let svg = svg_of(&Scene {
        width: 620.0,
        height: 620.0,
        title: "pTEST".into(),
        items: Vec::new(),
    });
    let root = &svg[..svg.find('>').expect("a root element")];
    for key in [
        "font-family",
        "stroke-linecap",
        "stroke-linejoin",
        "xml:space",
    ] {
        let ts = want.get(key).opt_str();
        assert!(
            ts.is_some(),
            "the TypeScript root carries no {key}, so both renderers leave it to \
             whatever opens the file — regenerate the fixture once it is fixed"
        );
        assert_eq!(attr(root, key), ts, "root {key}: rust vs ts");
    }
}

#[test]
fn coordinate_rounding_agrees() {
    let doc = load();
    for c in doc.get("round").arr() {
        let v = c.get("v").num();
        assert_eq!(n(v), c.get("out").str(), "n({v})");
    }
}
