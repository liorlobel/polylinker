//! Dump every printable ASCII and Latin-1 outline, for fontTools to judge.
//!
//! The unit tests beside `font.rs` check the face's identity and its metrics.
//! They do not check the **outline decoder**, because there is nothing in this
//! repository to check it against: a test that walks `glyf` the way `font.rs`
//! walks `glyf` agrees with itself by construction.
//!
//! fontTools decodes the same table by a different implementation, in a
//! different language, maintained by other people. That makes it the oracle,
//! and it is the reason this stage was worth doing early.
//!
//! # Why the range is not just ASCII
//!
//! Printable ASCII alone leaves [`Face::outline`]'s composite branch entirely
//! unjudged. Measured by walking `glyf` directly (2026-08-04): **no** ASCII
//! codepoint in either committed face is a composite glyph, while 59 Latin-1
//! codepoints are in Regular and 58 in Bold — every accented letter, plus
//! U+00A0, U+00AD, and the fractions. `pdf::encode` passes Latin-1 through
//! precisely because such characters turn up in feature names, so `pTet-α`'s
//! neighbours reach the composite decoder in a real PNG export.
//!
//! The composite path assembles a glyph from other glyphs at signed offsets —
//! byte or word arguments, one component or several — and getting any of that
//! wrong shifts an accent by a few font units, which is invisible in a rendered
//! glyph and exactly what an oracle is for.
//!
//! `reference/python/tests/xcheck_glyphs.py` reads what this writes.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use pl_draw::font::{Curve, Face, BOLD, REGULAR};

#[test]
fn write_outlines_for_the_fonttools_cross_check() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("glyphs");
    fs::create_dir_all(&dir).expect("a place to write");

    for (name, bytes) in [("regular", REGULAR), ("bold", BOLD)] {
        let face = Face::parse(bytes).expect("the committed face parses");
        let mut out = String::new();
        // Printable ASCII, then printable Latin-1. The gap 0x7F..0xA0 is the C1
        // controls, which no face spells and `pdf::encode` never emits.
        for cp in (0x20u32..=0x7E).chain(0xA0..=0xFF) {
            let c = char::from_u32(cp).expect("Latin-1 is valid Unicode");
            let gid = face.glyph(c).expect("a glyph id");
            writeln!(out, "# {} {}", cp, gid).expect("write");
            for cur in face.outline(gid) {
                // Font units, y up, exactly as decoded -- no scaling here, so
                // a disagreement cannot be blamed on a transform.
                match cur {
                    Curve::Move(x, y) => writeln!(out, "M {x:.4} {y:.4}"),
                    Curve::Line(x, y) => writeln!(out, "L {x:.4} {y:.4}"),
                    Curve::Quad(cx, cy, x, y) => {
                        writeln!(out, "Q {cx:.4} {cy:.4} {x:.4} {y:.4}")
                    }
                    Curve::Close => writeln!(out, "Z"),
                }
                .expect("write");
            }
        }
        fs::write(dir.join(format!("{name}.outlines")), out).expect("the dump");
    }
    assert!(dir.join("regular.outlines").exists());
}
