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
    angle, commas, esc, isotonic, n, nice_step, place_column, polar, ranges, safe_color, LabelBox,
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

#[test]
fn coordinate_rounding_agrees() {
    let doc = load();
    for c in doc.get("round").arr() {
        let v = c.get("v").num();
        assert_eq!(n(v), c.get("out").str(), "n({v})");
    }
}
