//! Tests for the TrueType reader.
//!
//! The contour-level oracle is **fontTools**, in `tools/ci.ps1` via
//! `crates/pl-draw/tests/glyphs.rs`: it decodes the same `glyf` table by a
//! different implementation and reports every point and flag, which is the one
//! genuinely independent check on the outline decoder. What is here is what can
//! be settled without Python — the face's identity, its metrics, the structural
//! properties a wrong parse would break, and the two things fontTools cannot
//! judge: what glyph 0 is, and the component transforms no committed face uses.

use super::*;

fn regular() -> Face<'static> {
    Face::parse(REGULAR).expect("the committed regular face parses")
}

fn bold() -> Face<'static> {
    Face::parse(BOLD).expect("the committed bold face parses")
}

/// The committed faces are the files NOTICE records.
///
/// Modelled on `the_vendored_faces_are_the_files_notice_records` in
/// `bins/pl-gui`, and for the reason that test's own comment gives: a hash in a
/// text file that nothing compares goes stale the first time somebody
/// re-downloads a face, and then the provenance chain is broken silently, in
/// the direction of looking fine.
///
/// **It reads NOTICE with `include_str!`.** A hash living in two places with
/// nothing joining them could fail if somebody swapped a font and could NOT
/// fail if somebody mistyped the record of one. The `include_str!` is the join.
///
/// The licence text is in here too. It is not linked into any binary — it
/// travels beside it — so nothing else in the build would notice if it were
/// truncated or re-wrapped by an editor, and a licence that has quietly become
/// the wrong bytes is worse than a missing one because the package still looks
/// complete.
#[test]
fn the_vendored_faces_are_the_files_notice_records() {
    let notice: &str = include_str!("../../../../NOTICE");
    for (what, bytes, len, want) in [
        (
            "Liberation Sans 2.1.5 Regular",
            REGULAR,
            410_712usize,
            "76d04c18ea243f426b7de1f3ad208e927008f961dc5945e5aad352d0dfde8ee8",
        ),
        (
            "Liberation Sans 2.1.5 Bold",
            BOLD,
            414_456,
            "788abee4c806d660e8aee46689dd8540cd4bb98da03dcc9d171ce3efd99a9173",
        ),
        (
            "the Liberation OFL text",
            include_bytes!("../../fonts/Liberation-OFL.txt"),
            4_414,
            "93fed46019c38bbe566b479d22148e2e8a1e85ada614accb0211c37b2c61c19b",
        ),
    ] {
        assert_eq!(bytes.len(), len, "{what}: byte count");
        let got = pl_core::sha256::sha256_hex(bytes);
        assert_eq!(got, want, "{what}: sha256 of the committed bytes");
        assert!(
            notice.contains(want),
            "{what}: the digest of the committed file does not appear in NOTICE, \
             so NOTICE is recording a file that is not the one shipped"
        );
        assert!(
            notice.contains(&format!("{}", CommaSep(len))),
            "{what}: NOTICE does not record the byte count {len}"
        );
    }
    // The copyright holders OFL clause 4 requires to be named. NOTICE is the
    // only place this notice can travel for a recipient of `dist/`.
    for who in ["Google Corporation", "Red Hat, Inc.", "Steve Matteson"] {
        assert!(notice.contains(who), "NOTICE does not name {who}");
    }
    // And the Reserved Font Name, which is why the face is not subsetted.
    assert!(
        notice.contains("Reserved Font Name") && notice.contains("Liberation"),
        "NOTICE does not record the Reserved Font Name"
    );
}

/// Format a byte count the way NOTICE writes it, so the two can be compared.
struct CommaSep(usize);

impl std::fmt::Display for CommaSep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.0.to_string();
        let b = s.as_bytes();
        for (i, c) in b.iter().enumerate() {
            if i > 0 && (b.len() - i) % 3 == 0 {
                write!(f, ",")?;
            }
            write!(f, "{}", *c as char)?;
        }
        Ok(())
    }
}

/// The face has the advances the layout measures.
///
/// THE REASON THIS FACE IS HERE, and it is checked rather than trusted.
/// `pdf::text_width_in` prices every label from `HELVETICA`/`HELVETICA_BOLD`,
/// and the truncation, the `viewBox` crop and every `Anchor::End` placement
/// come out of those numbers. Drawing the glyphs in a face with different
/// advances reintroduces, in raster, the defect commit 7bf5aad fixed in the
/// SVG: a figure measured in one typeface and drawn in another.
///
/// Liberation is metrically compatible with Arial, which is metrically
/// compatible with Helvetica by design, and which is what those tables were
/// cross-checked against. That is the argument; this is the measurement.
///
/// Would fail immediately for either Plex face — neither is metrically
/// compatible with anything here — which is what makes it a real gate on the
/// choice rather than a restatement of it.
#[test]
fn the_vendored_face_has_the_advances_the_layout_measures() {
    for (face, bold_weight, what) in [(regular(), false, "regular"), (bold(), true, "bold")] {
        let upem = face.units_per_em;
        assert_eq!(upem, 2048.0, "{what}: unexpected units per em");
        let mut worst = 0.0f64;
        for cp in 0x20u8..=0x7E {
            let c = cp as char;
            let gid = face.glyph(c).unwrap_or(0);
            assert_ne!(gid, 0, "{what}: no glyph for {c:?}");
            let adv = face.advance(gid).expect("an advance") * 1000.0 / upem;
            // What the layout will actually use for this character.
            let want = crate::pdf::text_width_in(&c.to_string(), 1000.0, bold_weight);
            worst = worst.max((adv - want).abs());
            assert!(
                (adv.round() - want).abs() < 1e-9,
                "{what}: {c:?} advances {adv} in the face and {want} in the table \
                 the layout measures with"
            );
        }
        // The raw disagreement, before rounding, is what would accumulate if
        // the pen ever advanced by `hmtx` instead. Recorded so the number in
        // the module comment cannot go stale.
        assert!(
            worst < 0.25,
            "{what}: worst raw deviation {worst}/1000 em is larger than recorded"
        );
    }
}

/// A glyph with a counter produces more than one contour, wound oppositely.
///
/// This is the property the rasterizer's nonzero fill depends on to cut the
/// hole in an `o`, and it is a property of the FONT, not of our code — so if a
/// future face were dropped in whose contours were all wound alike, every
/// counter would fill in solid and this says so before the pixels do.
///
/// Measured while prototyping: normalising these windings produced 1.44x the
/// correct ink.
#[test]
fn a_counter_is_a_contour_wound_against_its_outer() {
    let f = regular();
    let gid = f.glyph('o').expect("a glyph for o");
    let cs = f.outline(gid);
    let mut areas = Vec::new();
    let mut area = 0.0f64;
    let mut cur = (0.0f64, 0.0f64);
    let mut start = cur;
    for c in &cs {
        match *c {
            Curve::Move(x, y) => {
                cur = (x, y);
                start = cur;
                area = 0.0;
            }
            Curve::Line(x, y) | Curve::Quad(_, _, x, y) => {
                area += cur.0 * y - x * cur.1;
                cur = (x, y);
            }
            Curve::Close => {
                area += cur.0 * start.1 - start.0 * cur.1;
                areas.push(area);
                cur = start;
            }
        }
    }
    assert_eq!(areas.len(), 2, "an 'o' should have two contours: {areas:?}");
    assert!(
        areas[0] * areas[1] < 0.0,
        "the two contours of an 'o' are wound the same way, so a nonzero fill \
         has no counter to cut: {areas:?}"
    );
}

/// Every printable ASCII glyph decodes to a closed, non-empty outline.
///
/// Except the space, which is legitimately empty — and asserting that it IS
/// empty matters, because an outline reader that returns nothing on error
/// would otherwise look correct for all 95 characters.
#[test]
fn every_printable_ascii_glyph_decodes() {
    for (face, what) in [(regular(), "regular"), (bold(), "bold")] {
        for cp in 0x20u8..=0x7E {
            let c = cp as char;
            let gid = face.glyph(c).expect("a glyph id");
            let cs = face.outline(gid);
            if c == ' ' {
                assert!(cs.is_empty(), "{what}: the space has an outline");
                continue;
            }
            assert!(!cs.is_empty(), "{what}: {c:?} decoded to nothing");
            assert!(
                matches!(cs[0], Curve::Move(..)),
                "{what}: {c:?} does not begin with a move"
            );
            assert!(
                matches!(cs[cs.len() - 1], Curve::Close),
                "{what}: {c:?} does not end closed"
            );
            // Every contour opens with a move and shuts with a close.
            let moves = cs.iter().filter(|c| matches!(c, Curve::Move(..))).count();
            let closes = cs.iter().filter(|c| matches!(c, Curve::Close)).count();
            assert_eq!(moves, closes, "{what}: {c:?} has unbalanced contours");
        }
    }
}

/// A Latin-1 composite is exactly its components, translated.
///
/// **The composite branch of `Face::outline` had no test at all.** Measured by
/// walking `glyf` directly on 2026-08-04: NO printable-ASCII codepoint in
/// either committed face is a composite glyph, while 59 Latin-1 codepoints are
/// in Regular and 58 in Bold — every accented letter, the fractions, U+00A0 and
/// U+00AD. Everything that judged this decoder stopped at U+007E, so the branch
/// that assembles `é` was executed by nothing.
///
/// It is not a branch that can stay untested: `pdf::encode` passes Latin-1
/// through because accented characters turn up in feature names, so a name like
/// `pTet-α` reaches it in a real PNG export.
///
/// **The offsets below came out of fontTools, not out of this code** — the same
/// oracle `tests/glyphs.rs` uses, read on 2026-08-04. The component SHAPES here
/// are our own simple-glyph decoder, which that cross-check judges separately;
/// what this test pins is the assembly: which components, in what order, at
/// which signed offsets. Bold's `à` sets its base at x = -8, which is a
/// negative BYTE argument, so reading `a1`/`a2` as unsigned fails here; `½` has
/// three components and a negative y, so a dropped `MORE_COMPONENTS` fails here.
#[test]
fn a_latin1_composite_is_its_components_translated() {
    /// A composite character and its components, each with the offset the
    /// `glyf` table places it at, in font units, as fontTools reads them.
    type Case = (char, &'static [(char, f64, f64)]);

    let regular_cases: &[Case] = &[
        ('é', &[('e', 0.0, 0.0), ('\u{B4}', 368.0, 0.0)]),
        ('à', &[('a', 0.0, 0.0), ('\u{60}', 188.0, 0.0)]),
        (
            '½',
            &[
                ('\u{B9}', -24.0, 0.0),
                ('\u{2044}', 761.0, 0.0),
                ('\u{B2}', 1010.0, -561.0),
            ],
        ),
        // A composite of the space: a real glyph, legitimately no contours.
        ('\u{A0}', &[(' ', 0.0, 0.0)]),
    ];
    let bold_cases: &[Case] = &[
        ('é', &[('e', 0.0, 0.0), ('\u{B4}', 331.0, 0.0)]),
        ('à', &[('a', -8.0, 0.0), ('\u{60}', 174.0, 0.0)]),
        (
            '½',
            &[
                ('\u{B9}', 12.0, 0.0),
                ('\u{2044}', 700.0, 0.0),
                ('\u{B2}', 973.0, -695.0),
            ],
        ),
        ('\u{A0}', &[(' ', 0.0, 0.0)]),
    ];

    let translate = |cs: &[Curve], dx: f64, dy: f64| -> Vec<Curve> {
        cs.iter()
            .map(|c| match *c {
                Curve::Move(x, y) => Curve::Move(x + dx, y + dy),
                Curve::Line(x, y) => Curve::Line(x + dx, y + dy),
                Curve::Quad(cx, cy, x, y) => Curve::Quad(cx + dx, cy + dy, x + dx, y + dy),
                Curve::Close => Curve::Close,
            })
            .collect()
    };

    let mut composites = 0;
    for (face, cases, what) in [
        (regular(), regular_cases, "regular"),
        (bold(), bold_cases, "bold"),
    ] {
        for (whole, parts) in cases {
            let gid = face.glyph(*whole).expect("a glyph id");
            assert_ne!(gid, 0, "{what}: no glyph for {whole:?}");
            let mut want = Vec::new();
            for (part, dx, dy) in *parts {
                let pgid = face.glyph(*part).expect("a glyph id");
                assert_ne!(pgid, 0, "{what}: no glyph for the component {part:?}");
                want.extend(translate(&face.outline(pgid), *dx, *dy));
            }
            assert_eq!(
                face.outline(gid),
                want,
                "{what}: {whole:?} is not its components at the offsets the file gives"
            );
            composites += 1;
        }
    }
    assert_eq!(composites, 8, "every case must actually have been compared");
}

/// The component transforms, on a face built here byte by byte.
///
/// `Face::composite` decodes three optional transforms — `WE_HAVE_A_SCALE`
/// (0x0008), `X_AND_Y_SCALE` (0x0040) and `WE_HAVE_A_TWO_BY_TWO` (0x0080) — and
/// **no committed face reaches any of them.** Measured by walking `glyf`
/// directly on 2026-08-04: of 2,131 components across 1,076 composite glyphs in
/// Regular, and 2,149 across 1,092 in Bold, exactly zero set any of those three
/// bits. Every one of them sets `ARGS_ARE_XY_VALUES` and
/// `UNSCALED_COMPONENT_OFFSET` and nothing else that moves a point. So neither
/// the real faces nor fontTools can judge this code, and a font swap is exactly
/// when it would first run.
///
/// Hence a face assembled here: five glyphs, the tables `Face::parse` needs, and
/// component flags chosen to walk every branch. It pins
///
/// * the F2Dot14 divisor — 16384, not 16 and not 65536. Every transformed
///   coordinate below moves if it changes;
/// * that only the diagonal of a 2x2 is honoured. Glyph 2's off-diagonal terms
///   are 0.25 and -0.75, so a reader that applied them would put the third
///   point's x at 300 or at -500 depending on which convention it chose, rather
///   than at 100;
/// * that a component's offset is scaled by the PARENT's accumulated scale and
///   not by the component's own — `UNSCALED_COMPONENT_OFFSET` semantics, which
///   is what all 4,280 real components ask for. Glyph 3 nests glyph 2 under a
///   0.5 scale, so `dx + ox` instead of `dx + ox * sx` puts it at 300, not 250;
/// * the signed byte argument and the `MORE_COMPONENTS` loop, in glyph 4.
#[test]
fn the_component_transforms_decode() {
    let data = synthetic_face();
    let f = Face::parse(&data).expect("the face assembled here parses");
    assert_eq!(f.units_per_em, 1000.0, "units per em");
    assert_eq!(f.glyph('A'), Some(1), "the cmap this test built");
    assert_eq!(f.glyph('D'), Some(4), "the cmap this test built");

    // Glyph 1, the only real outline in the file: a triangle, three on-curve
    // points, written as coordinate deltas. If this one is wrong the rest of
    // the test is measuring the builder rather than the decoder.
    assert_eq!(
        f.outline(1),
        vec![
            Curve::Move(0.0, 0.0),
            Curve::Line(400.0, 0.0),
            Curve::Line(0.0, 800.0),
            Curve::Close,
        ],
        "the simple glyph this test wrote"
    );

    // Glyph 2: the triangle under a full 2x2 [0.5 0.25 / -0.75 -1.0] at
    // (100, -50). Diagonal only: x scales by 0.5, y flips.
    assert_eq!(
        f.outline(2),
        vec![
            Curve::Move(100.0, -50.0),
            Curve::Line(300.0, -50.0),
            Curve::Line(100.0, -850.0),
            Curve::Close,
        ],
        "WE_HAVE_A_TWO_BY_TWO"
    );

    // Glyph 3: glyph 2 again, under a single scale of 0.5 at (200, 0). The
    // inner offset (100, -50) is itself scaled by that 0.5.
    assert_eq!(
        f.outline(3),
        vec![
            Curve::Move(250.0, -25.0),
            Curve::Line(350.0, -25.0),
            Curve::Line(250.0, -425.0),
            Curve::Close,
        ],
        "WE_HAVE_A_SCALE, nested"
    );

    // Glyph 4: two components. The first takes signed BYTE arguments (-8, 3)
    // and no transform; the second word arguments (1000, 0) with separate x and
    // y scales of 0.25 and 1.5.
    assert_eq!(
        f.outline(4),
        vec![
            Curve::Move(-8.0, 3.0),
            Curve::Line(392.0, 3.0),
            Curve::Line(-8.0, 803.0),
            Curve::Close,
            Curve::Move(1000.0, 0.0),
            Curve::Line(1100.0, 0.0),
            Curve::Line(1000.0, 1200.0),
            Curve::Close,
        ],
        "MORE_COMPONENTS with a signed byte argument and X_AND_Y_SCALE"
    );
}

/// The bytes of the face `the_component_transforms_decode` reads.
///
/// Only what [`Face::parse`] looks at: `head`, `maxp`, `loca` (long form),
/// `glyf`, `cmap` format 4 at (3, 1), `hhea` and `hmtx`. Everything else in a
/// real file — `name`, `OS/2`, `post`, checksums, the binary search fields — is
/// absent or zero, because this reader never reads it, and a builder that wrote
/// more would be asserting more than the test knows.
fn synthetic_face() -> Vec<u8> {
    fn u16b(v: &mut Vec<u8>, x: u16) {
        v.extend_from_slice(&x.to_be_bytes());
    }
    fn i16b(v: &mut Vec<u8>, x: i16) {
        v.extend_from_slice(&x.to_be_bytes());
    }
    fn u32b(v: &mut Vec<u8>, x: u32) {
        v.extend_from_slice(&x.to_be_bytes());
    }
    /// F2Dot14: two integer bits, fourteen fractional.
    fn f2dot14(x: f64) -> i16 {
        (x * 16384.0) as i16
    }
    /// One component of a composite: flags, glyph index, two arguments (word if
    /// `ARG_1_AND_2_ARE_WORDS` is set, else signed bytes), then the transform.
    fn component(v: &mut Vec<u8>, flags: u16, gid: u16, a1: i16, a2: i16, xform: &[i16]) {
        u16b(v, flags);
        u16b(v, gid);
        if flags & 0x0001 != 0 {
            i16b(v, a1);
            i16b(v, a2);
        } else {
            v.push(a1 as i8 as u8);
            v.push(a2 as i8 as u8);
        }
        for &t in xform {
            i16b(v, t);
        }
    }

    // ---- glyf and loca ----------------------------------------------------
    let mut glyf: Vec<u8> = Vec::new();
    // Glyph 0 is empty: `loca[0] == loca[1]`, which is how a font says so.
    let mut loca: Vec<u32> = vec![0, 0];
    let add = |glyf: &mut Vec<u8>, loca: &mut Vec<u32>, g: &[u8]| {
        glyf.extend_from_slice(g);
        while glyf.len() % 4 != 0 {
            glyf.push(0);
        }
        loca.push(glyf.len() as u32);
    };

    // Glyph 1: one contour, three on-curve points at (0,0), (400,0), (0,800).
    let mut tri = Vec::new();
    i16b(&mut tri, 1); // one contour
    for v in [0i16, 0, 400, 800] {
        i16b(&mut tri, v); // xMin, yMin, xMax, yMax
    }
    u16b(&mut tri, 2); // the contour ends at point 2
    u16b(&mut tri, 0); // no instructions
    tri.extend_from_slice(&[0x01, 0x01, 0x01]); // on-curve, both deltas 16-bit
    for d in [0i16, 400, -400] {
        i16b(&mut tri, d); // x deltas
    }
    for d in [0i16, 0, 800] {
        i16b(&mut tri, d); // y deltas
    }
    add(&mut glyf, &mut loca, &tri);

    // Glyph 2: glyph 1 under a 2x2, word arguments.
    let mut two_by_two = Vec::new();
    i16b(&mut two_by_two, -1); // composite
    for v in [-500i16, -900, 500, 100] {
        i16b(&mut two_by_two, v);
    }
    component(
        &mut two_by_two,
        0x0001 | 0x0002 | 0x0080, // WORDS | XY | TWO_BY_TWO
        1,
        100,
        -50,
        &[f2dot14(0.5), f2dot14(0.25), f2dot14(-0.75), f2dot14(-1.0)],
    );
    add(&mut glyf, &mut loca, &two_by_two);

    // Glyph 3: glyph 2 under a single scale — a composite of a composite.
    let mut nested = Vec::new();
    i16b(&mut nested, -1);
    for v in [0i16, -500, 500, 100] {
        i16b(&mut nested, v);
    }
    component(
        &mut nested,
        0x0001 | 0x0002 | 0x0008, // WORDS | XY | SCALE
        2,
        200,
        0,
        &[f2dot14(0.5)],
    );
    add(&mut glyf, &mut loca, &nested);

    // Glyph 4: two components — byte arguments then word arguments.
    let mut pair = Vec::new();
    i16b(&mut pair, -1);
    for v in [-8i16, 0, 1100, 1200] {
        i16b(&mut pair, v);
    }
    component(
        &mut pair,
        0x0002 | 0x0020, // XY | MORE_COMPONENTS, byte arguments
        1,
        -8,
        3,
        &[],
    );
    component(
        &mut pair,
        0x0001 | 0x0002 | 0x0040, // WORDS | XY | X_AND_Y_SCALE
        1,
        1000,
        0,
        &[f2dot14(0.25), f2dot14(1.5)],
    );
    add(&mut glyf, &mut loca, &pair);

    let num_glyphs = (loca.len() - 1) as u16;
    let mut loca_bytes = Vec::new();
    for off in &loca {
        u32b(&mut loca_bytes, *off);
    }

    // ---- the small tables -------------------------------------------------
    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // indexToLocFormat: long

    let mut maxp = vec![0u8; 32];
    maxp[4..6].copy_from_slice(&num_glyphs.to_be_bytes());

    let mut hhea = vec![0u8; 36];
    hhea[34..36].copy_from_slice(&num_glyphs.to_be_bytes()); // numberOfHMetrics

    let mut hmtx = Vec::new();
    for _ in 0..num_glyphs {
        u16b(&mut hmtx, 500); // advance
        i16b(&mut hmtx, 0); // left side bearing
    }

    // cmap: one (3, 1) format 4 subtable mapping 'A'..'D' to glyphs 1..4.
    let mut cmap = Vec::new();
    u16b(&mut cmap, 0); // version
    u16b(&mut cmap, 1); // one subtable
    u16b(&mut cmap, 3); // platform: Windows
    u16b(&mut cmap, 1); // encoding: BMP
    u32b(&mut cmap, 12); // and it starts here
    u16b(&mut cmap, 4); // format
    u16b(&mut cmap, 32); // length: 16 + 8 * segCount
    u16b(&mut cmap, 0); // language
    u16b(&mut cmap, 4); // segCountX2 — two segments
    for _ in 0..3 {
        u16b(&mut cmap, 0); // searchRange, entrySelector, rangeShift
    }
    for e in [0x0044u16, 0xFFFF] {
        u16b(&mut cmap, e); // endCode
    }
    u16b(&mut cmap, 0); // reservedPad
    for s in [0x0041u16, 0xFFFF] {
        u16b(&mut cmap, s); // startCode
    }
    for delta in [(1i32 - 0x41) as i16, 1] {
        i16b(&mut cmap, delta); // idDelta
    }
    for _ in 0..2 {
        u16b(&mut cmap, 0); // idRangeOffset: none, so idDelta decides
    }

    // ---- the sfnt wrapper -------------------------------------------------
    let tables: [(&[u8; 4], Vec<u8>); 7] = [
        (b"cmap", cmap),
        (b"glyf", glyf),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca_bytes),
        (b"maxp", maxp),
    ];
    let mut out = Vec::new();
    u32b(&mut out, 0x0001_0000); // sfntVersion
    u16b(&mut out, tables.len() as u16);
    for _ in 0..3 {
        u16b(&mut out, 0); // searchRange, entrySelector, rangeShift
    }
    let mut at = 12 + 16 * tables.len();
    for (tag, data) in &tables {
        out.extend_from_slice(*tag);
        u32b(&mut out, 0); // checksum, which this reader does not verify
        u32b(&mut out, at as u32);
        u32b(&mut out, data.len() as u32);
        at += data.len().next_multiple_of(4);
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    out
}

/// A character the face cannot spell resolves to glyph 0, and glyph 0 decodes.
///
/// The last assertion used to be `is_empty() || !is_empty()`, which is true of
/// every possible value and so proved only that the call returned. What glyph 0
/// SHOULD be is a fact about the committed files, and this is it: `.notdef` in
/// both Liberation faces is the hollow rectangle — two contours, ten commands,
/// four straight lines each, the inner one wound against the outer. The
/// coordinates below were read out of the two `.ttf` files with fontTools on
/// 2026-08-04 and are byte-identical between Regular and Bold, which is why one
/// table serves both.
///
/// **Nothing else in this file pins the `.notdef` path.**
/// `every_printable_ascii_glyph_decodes` asserts `gid != 0` throughout, and the
/// fontTools cross-check walks the `cmap`, which never yields 0. So a reader
/// that returned nothing for glyph 0 — the exact failure mode that test's own
/// comment names, an error path that looks like the space — would ship green.
///
/// This is a claim about the DECODER, not about the picture: `raster.rs:751`
/// draws a glyph only when `gid != 0`, and pushes the string onto
/// `Report::unencodable` instead, so no PNG has ever shown this rectangle.
#[test]
fn an_unmappable_character_is_glyph_zero() {
    let f = regular();
    // Outside the BMP, so `cmap` format 4 cannot express it at all.
    assert_eq!(f.glyph('\u{1F600}'), None, "an astral codepoint");
    // Inside the BMP and genuinely absent from this face.
    assert_eq!(f.glyph('\u{4E2D}'), Some(0), "a CJK ideograph");

    let notdef = vec![
        Curve::Move(205.0, 1409.0),
        Curve::Line(1330.0, 1409.0),
        Curve::Line(1330.0, 0.0),
        Curve::Line(205.0, 0.0),
        Curve::Close,
        Curve::Move(281.0, 1333.0),
        Curve::Line(281.0, 76.0),
        Curve::Line(1254.0, 76.0),
        Curve::Line(1254.0, 1333.0),
        Curve::Close,
    ];
    for (face, what) in [(regular(), "regular"), (bold(), "bold")] {
        assert_eq!(
            face.outline(0),
            notdef,
            "{what}: glyph 0 is not the .notdef box the file holds"
        );
        // 1536/2048 em, the same in both faces.
        assert_eq!(face.advance(0), Some(1536.0), "{what}: .notdef advance");
    }
}
