//! What the cross-implementation fixture cannot cover.
//!
//! `tests/agreement.rs` checks that the two renderers compute the same numbers.
//! These check the things only this one does: emitting a document that parses,
//! reporting what it could not draw, and refusing hostile input from a file.
//!
//! And, since 2026-08-14, where the figures put a feature. That belongs here
//! and not in the fixture because `tests/agreement.rs` imports scalar helpers
//! only — it never builds a `Molecule` and never calls `scene`, so it cannot
//! see a band drawn at the wrong angle. See the "reading a ring band back off
//! the scene" block below, and [`span_x`] for the track's older equivalent.

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

/// Cheap well-formedness: every character is one XML 1.0 allows, every tag
/// opened is closed, in order, and no tag contains an odd number of quotes. Not
/// a validator — enough to fail loudly on the mistakes a string-building emitter
/// actually makes.
///
/// The character check is the half that was missing until 2026-08-13, and its
/// absence is why `a_hostile_colour_cannot_inject_an_attribute` — the test
/// written for exactly that surface, closing with exactly this call — stayed
/// green while `safe_color` was letting a U+000B through into
/// `fill="rgb(79,127,208\x0b)"`. The scanner below reads *structure*: brackets,
/// quotes, tag names. It has no opinion about the bytes inside them, so it
/// called that document well formed and every conformant parser refuses it
/// outright — the whole figure, not one wrong colour. `docs/AUDIT-2026-07.md`
/// had named the byte class in July; the fix went into the escapers, and nothing
/// in the tree could see the sanitiser that had missed it.
///
/// So this now checks the whole `Char` production and not merely the two
/// codepoints of the bug that prompted it. That is the difference between
/// closing a hole and closing the class: the next sanitiser to admit a byte XML
/// forbids fails here, in hostile-input tests that already exist and already
/// build documents out of file-supplied strings, rather than in a figure a
/// reader cannot open.
fn well_formed(svg: &str) -> Result<(), String> {
    // The characters before the structure. An illegal one is fatal to the whole
    // document wherever it sits — in a tag, in an attribute value, in text — so
    // it is looked for across the whole string rather than only where the
    // scanner below happens to look.
    if let Some((at, c)) = svg.char_indices().find(|(_, c)| !is_xml_char(*c)) {
        return Err(format!(
            "U+{:04X} at byte {at} is outside the XML 1.0 Char production, so no \
             conformant parser will open this document",
            c as u32
        ));
    }
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

/// The XML 1.0 `Char` production, which is the whole specification of what may
/// appear in a document at all:
///
/// ```text
/// Char ::= #x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF]
/// ```
///
/// Written out rather than approximated as "not a control character", because
/// the approximation is wrong in both directions and both mistakes have been
/// made here: the TypeScript `esc` used to delete U+007F, which XML allows, and
/// `safe_color` used to admit U+000B and U+000C, which it does not. Tab, LF and
/// CR are in the production and no other C0 control is, in any form: `&#1;` is
/// exactly as illegal as a raw U+0001, which is why `esc` *drops* control
/// characters instead of escaping them. U+007F (DEL) **is** in it, inside
/// `[#x20-#xD7FF]`, and both escapers keep it deliberately, so a checker that
/// refused DEL would be reporting them broken for obeying the specification.
/// U+FFFE and U+FFFF are out, by the upper bound of `[#xE000-#xFFFD]`. The
/// surrogate range needs no arm here: a Rust `char` cannot hold one, so the gap
/// between `#xD7FF` and `#xE000` is unreachable rather than unchecked.
fn is_xml_char(c: char) -> bool {
    let u = c as u32;
    c == '\t'
        || c == '\n'
        || c == '\r'
        || (0x20..=0xd7ff).contains(&u)
        || (0xe000..=0xfffd).contains(&u)
        || (0x10000..=0x10ffff).contains(&u)
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

/// PROVEN TO FAIL at f0e4a6f: `safe_color`'s functional-notation arm listed
/// `\x0b` and `\x0c` among the bytes it would admit between the parentheses of
/// `rgb(`/`rgba(`/`hsl(`/`hsla(`, so `rgb(79,127,208\x0b)` came back unchanged
/// and `svg_at` interpolated it into `fill="…"` untouched — `esc` is applied to
/// `<title>` and `<text>` content and to nothing else. Neither byte is in the
/// XML 1.0 `Char` production, so what `pl export --svg` wrote was a document no
/// conformant reader will open: the entire figure lost, not one feature
/// miscoloured, from a value that arrived in a downloaded file.
///
/// The `str::trim` at the top of `safe_color` is no defence and never was. Both
/// bytes are `White_Space`, so it strips a leading or trailing one and cannot
/// reach an interior one — every byte below is interior, which is the only
/// position that matters.
///
/// One case per parenthesised form that admits the class, because the arm tries
/// four prefixes and a fix reaching only `rgb(` would still ship the bug in the
/// other three. Each case is asserted twice: at the sanitiser, which is where
/// the defect is and where a failure should point, and then on a whole rendered
/// document through `well_formed`, which is what a reader actually opens. The
/// second assertion is the one that would still catch a future sanitiser that
/// refuses these two bytes and admits the next.
///
/// Mutation that re-breaks it: in `crates/pl-draw/src/lib.rs`, restore the two
/// bytes to `safe_color`'s allowed list, `b"eE+-.,%/ \t\n\r"` back to
/// `b"eE+-.,%/ \t\n\x0b\x0c\r"`.
#[test]
fn a_colour_carrying_a_byte_xml_forbids_is_refused() {
    for bad in [
        "rgb(79,127,208\u{b})",
        "rgba(79,127,\u{c}208,.5)",
        "hsl(120 50%\u{b} 40%)",
        "hsla(1,2%,3%,\u{c}4)",
    ] {
        // The unit-level claim first, so a failure names the sanitiser rather
        // than the renderer that trusted it.
        assert_eq!(
            safe_color(Some(bad), "#7f8a95"),
            "#7f8a95",
            "{bad:?} was passed through"
        );

        let mut m = plasmid(1000, true);
        let mut f = feat("evil", "CDS", 10, 500);
        f.segments[0].color = Some(bad.into());
        m.features.push(f);
        let (svg, _) = circular_svg(&m, Options::default());
        well_formed(&svg).unwrap_or_else(|e| panic!("{bad:?}: {e}"));
        assert!(
            !svg.contains('\u{b}') && !svg.contains('\u{c}'),
            "{bad:?}: the byte reached the file"
        );
        // Refused, not merely escaped: the feature is drawn in the CDS colour.
        assert!(
            svg.contains(colour_for("CDS")),
            "{bad:?}: the CDS fallback colour is not in the figure"
        );
    }
}

/// PROVEN TO FAIL at f0e4a6f: `well_formed` was a bracket-and-quote-parity
/// scanner — it said so of itself, "Not a validator" — and every document below
/// is perfectly balanced, so it answered `Ok` for files no XML parser will open.
///
/// This function is the oracle nearly every hostile-input test in this file
/// closes with, and an oracle that cannot fail is worth nothing, so it gets a
/// test of its own rather than being trusted. That is not a hypothetical here:
/// `a_hostile_colour_cannot_inject_an_attribute` and the `safe_color` hole it
/// was meant to catch both arrived in `442496c`, and it ended in this very call
/// and stayed green until 2026-08-13.
///
/// The legal half is asserted too. `esc` keeps tab, LF, CR and U+007F on purpose
/// and argues the case at length; a checker that rejected any of the four would
/// turn the hostile-input tests red for the wrong reason, and be believed — the
/// repair would then go into the escaper that was already right.
///
/// Mutation that re-breaks it — in this file rather than in the crate, because
/// what is under test is the harness: delete the `svg.char_indices().find(…)`
/// guard at the top of `well_formed`.
#[test]
fn the_well_formedness_check_can_see_a_byte_xml_forbids() {
    // The exact shape `svg_at` emits, with one illegal byte in an attribute
    // value — the position `safe_color` is responsible for.
    let attr = "<svg><path d=\"M0,0\" fill=\"rgb(1,\u{b}2,3)\"/></svg>";
    assert!(
        well_formed(attr).is_err(),
        "a raw U+000B in an attribute value was called well formed"
    );

    // Both ends of the production, not just the C0 controls of the bug that
    // prompted the check: U+FFFE and U+FFFF are excluded by `[#xE000-#xFFFD]`
    // and are perfectly possible in a feature name out of a binary payload.
    for bad in [
        '\u{0}', '\u{1}', '\u{b}', '\u{c}', '\u{1f}', '\u{fffe}', '\u{ffff}',
    ] {
        let doc = format!("<svg><title>a{bad}b</title></svg>");
        assert!(
            well_formed(&doc).is_err(),
            "U+{:04X} in text was called well formed",
            bad as u32
        );
    }

    // …and everything XML allows must still pass, including DEL and the two
    // codepoints either side of the surrogate gap.
    let legal = "<svg><title>a\tb\nc\rd\u{7f}e\u{d7ff}f\u{e000}g\u{fffd}h\u{10ffff}</title></svg>";
    well_formed(legal).expect("a legal document was refused");
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

/// The `Arrow::Start` arm of [`arc_segs`], which nothing but the ring asks for.
///
/// **This passes at c0b60b2 and is meant to: the arm is correct and always
/// was.** What did not exist was any execution of it. `Arrow::Start` was never
/// once passed to `arc_segs` by a test in this crate — the two calls above both
/// pass `Arrow::End`, and the only `Arrow::Start` anywhere in `pl-draw`'s tests
/// was `linear.rs:1025`, which is `box_segs`, a different function drawing a
/// different shape. Half of `arc_segs` — the half every reverse-strand feature
/// on every plasmid map goes through — was reached by no assertion at all. See
/// F4 in `docs/AUDIT-2026-08-14-r3.md`.
///
/// The arm has to be the mirror of `Arrow::End`, because a reverse gene reads
/// towards its own low coordinate: the outline is walked from the arc's FAR
/// end, the arrowhead's base sits `head` past `a0` rather than `head` short of
/// `a1`, and the point itself is at `a0` on the band's mid radius. All three
/// are asserted, so a fix to one of them cannot be had by breaking another.
///
/// The point is compared to `polar(cx, cy, mid, a0)` for bit equality rather
/// than within a tolerance, because `arc_segs` reaches it through that same
/// function with those same arguments: anything but an identical `f64` pair
/// means it went somewhere else, and a tolerance would only decide how far
/// somewhere else is allowed to be.
///
/// Mutation that re-breaks it: in `crates/pl-draw/src/lib.rs`, in the
/// `Arrow::Start` arm of `arc_segs`, change `segs.push(arc(ro, a1, base));`
/// (line 2191 at c0b60b2) to `segs.push(arc(ro, a0, base));`.
#[test]
fn the_reverse_arm_of_arc_segs_walks_from_the_far_end_and_points_at_the_near_one() {
    let (cx, cy, ri, ro, a0, a1) = (100.0, 100.0, 80.0, 98.0, 0.0, 1.0);
    let segs = arc_segs(cx, cy, ri, ro, a0, a1, Arrow::Start);
    let mid = (ri + ro) / 2.0;

    // The outer arc leaves a1 and runs BACKWARDS to the arrowhead's base.
    // `Arrow::End`'s first arc leaves a0 instead, so this one number is what
    // separates the two arms of the match.
    let (r, from, to) = segs
        .iter()
        .find_map(|s| match *s {
            Seg::Arc { r, from, to, .. } => Some((r, from, to)),
            _ => None,
        })
        .expect("an arc");
    assert!((r - ro).abs() < 1e-12, "the outer arc is at r={r}");
    assert!(
        (from - a1).abs() < 1e-12,
        "the outline starts at {from}, not a1"
    );
    assert!(
        (to - (a0 + 8.0 / mid)).abs() < 1e-12,
        "the arrowhead's base is at {to}"
    );
    assert!(to > a0 && to < a1, "the head is not inside the arc: {to}");

    // And the point is at a0, on the mid radius, exactly once.
    let on_band: Vec<(f64, f64)> = segs
        .iter()
        .filter_map(|s| match *s {
            Seg::Move(x, y) | Seg::Line(x, y) if ((x - cx).hypot(cy - y) - mid).abs() < 1e-9 => {
                Some((x, y))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        on_band.len(),
        1,
        "{} vertices on the band's mid radius",
        on_band.len()
    );
    assert_eq!(on_band[0], polar(cx, cy, mid, a0), "the point is not at a0");
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
/// line cannot change what it is counting, and on the ring it cannot: `note`
/// reaches `centre_room` -> `keep_clear` -> the ruler's radius and nothing there
/// feeds back into the reserve, the geometry or the packing. Asserted rather
/// than left as a claim in a comment, because a comment is where that claim was.
///
/// **The track is the reason there is a second test below this one.** This one
/// covers the ring only, and its wording — "`note` reaches `centre_room`" — is
/// about the ring's plumbing, so it kept passing while the same claim in the
/// same words was false on the new figure.
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
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_be_bytes(*w))
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

// ---------------------------------------------------------------------------
// the linear figure
// ---------------------------------------------------------------------------

/// A strand-bearing feature, since `feat` leaves the strand at its default.
fn feat_on(name: &str, kind: &str, start: u64, end: u64, strand: Strand) -> Feature {
    let mut f = feat(name, kind, start, end);
    f.strand = strand;
    f
}

/// Every `x` a scene item touches, and every `y`.
fn extents(sc: &Scene) -> (f64, f64, f64, f64) {
    let (mut lo_x, mut hi_x, mut lo_y, mut hi_y) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    let mut mark = |x: f64, y: f64| {
        lo_x = lo_x.min(x);
        hi_x = hi_x.max(x);
        lo_y = lo_y.min(y);
        hi_y = hi_y.max(y);
    };
    for item in &sc.items {
        match item {
            Item::Path { segs, .. } => {
                for s in segs {
                    match *s {
                        Seg::Move(x, y) | Seg::Line(x, y) => mark(x, y),
                        Seg::Arc { cx, cy, r, .. } => {
                            mark(cx - r, cy - r);
                            mark(cx + r, cy + r);
                        }
                        Seg::Close => {}
                    }
                }
            }
            Item::Circle { cx, cy, r, .. } => {
                mark(cx - r, cy - r);
                mark(cx + r, cy + r);
            }
            // The drawn box, not the anchor: an `Anchor::Middle` label reaches
            // half its width either side, and half its size above and below the
            // baseline, which is the middle of the glyphs.
            Item::Text {
                x,
                y,
                size,
                anchor,
                text,
                bold,
                ..
            } => {
                let w = crate::pdf::text_width_in(text, *size, *bold);
                let (a, b) = match anchor {
                    Anchor::Start => (*x, *x + w),
                    Anchor::Middle => (*x - w * 0.5, *x + w * 0.5),
                    Anchor::End => (*x - w, *x),
                };
                mark(a, *y - size * 0.5);
                mark(b, *y + size * 0.5);
            }
        }
    }
    (lo_x, hi_x, lo_y, hi_y)
}

/// The `Path` items carrying this `<title>`, which is how a feature is named in
/// the scene.
fn titled<'a>(sc: &'a Scene, name: &str) -> Vec<&'a Vec<Seg>> {
    sc.items
        .iter()
        .filter_map(|i| match i {
            Item::Path { segs, title, .. } if title.as_deref() == Some(name) => Some(segs),
            _ => None,
        })
        .collect()
}

/// The horizontal extent of a set of segments.
fn span_x(segs: &[Seg]) -> (f64, f64) {
    let xs: Vec<f64> = segs
        .iter()
        .filter_map(|s| match *s {
            Seg::Move(x, _) | Seg::Line(x, _) => Some(x),
            _ => None,
        })
        .collect();
    (
        xs.iter().copied().fold(f64::INFINITY, f64::min),
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    )
}

// ---------------------------------------------------------------------------
// reading a ring band back off the scene
//
// [`span_x`] above is what the track's geometry is asserted through, and until
// 2026-08-14 the ring had no equivalent: every test that drew features on a
// circular molecule asserted names, counts, `Report` fields or well-formedness
// and never where an arc landed. Rotating every band by a thousand bases, or
// reflecting the whole figure across the vertical axis, passed all 208 tests in
// this crate and all 18 integration tests besides. F4 and F5 in
// `docs/AUDIT-2026-08-14-r3.md` are that hole; these three functions are the
// instrument the tests below close it with.
//
// They read the SCENE and nothing else. Nothing here calls `angle`, `frac`,
// `angle_past` or `ring::radius`, so a renderer cannot satisfy them by making
// the same mistake twice — the only thing taken from the production side is
// where the circle's centre is, and that is read off the path's own arcs.
// ---------------------------------------------------------------------------

/// Every drawn vertex of one ring path, as `(radius, angle)` about the ring's
/// own centre.
///
/// The inverse of [`polar`], which is `(cx + r·sin a, cy − r·cos a)`: `r` is
/// the hypotenuse and `a` is `atan2(x − cx, cy − y)`, normalised into
/// `0.0..TAU` because `atan2` answers in `−π..π` and this crate's convention is
/// clockwise from twelve o'clock.
///
/// The centre comes off the path's own `Seg::Arc`s rather than out of
/// `Options`, because the figure's radius is a function of how many labels
/// landed in a side column and rebuilding it here would be a second
/// implementation of the thing under test.
fn ring_vertices(segs: &[Seg]) -> Vec<(f64, f64)> {
    let (cx, cy) = segs
        .iter()
        .find_map(|s| match *s {
            Seg::Arc { cx, cy, .. } => Some((cx, cy)),
            _ => None,
        })
        .expect("a feature band on the ring is drawn with arcs");
    segs.iter()
        .filter_map(|s| match *s {
            Seg::Move(x, y) | Seg::Line(x, y) => Some((
                (x - cx).hypot(cy - y),
                (x - cx).atan2(cy - y).rem_euclid(TAU),
            )),
            _ => None,
        })
        .collect()
}

/// The two angles one ring band runs BETWEEN, in radians clockwise from twelve
/// o'clock, as `(start, end)` with `end` always the larger.
///
/// `end` may exceed a full turn, which is what a band closing on the origin
/// means and exactly what `angle_past(len, len)` returns for it.
///
/// The band is found as the complement of the WIDEST gap between consecutive
/// vertex angles rather than as plain `min..max`, because `atan2`'s branch cut
/// falls somewhere on every circle: a band ending on the last base of the
/// molecule has one end at 359.99 degrees and the other at 0.0, and `min..max`
/// would report it as the 359.99 degrees it does *not* cover. Taking the
/// complement of the gap gets that case and the ordinary one with the same
/// arithmetic, which is why a feature on the last base can be a fixture at all.
///
/// **It cannot read a band longer than half a turn**, where the shaft is itself
/// the widest gap and the answer would be the outside of the arc. It says so
/// rather than returning that quietly, because a silently inverted reading is
/// how a test stops meaning anything.
fn arc_span(segs: &[Seg]) -> (f64, f64) {
    let mut a: Vec<f64> = ring_vertices(segs).iter().map(|v| v.1).collect();
    assert!(a.len() >= 2, "a band drawn with {} vertices", a.len());
    a.sort_by(|p, q| p.partial_cmp(q).expect("no NaN in a drawn angle"));
    // Seeded with the gap that wraps past twelve o'clock, so index 0 is a
    // candidate start on the same terms as every other index.
    let (mut at, mut gap) = (0usize, a[0] + TAU - a[a.len() - 1]);
    for (i, pair) in a.windows(2).enumerate() {
        if pair[1] - pair[0] > gap {
            (at, gap) = (i + 1, pair[1] - pair[0]);
        }
    }
    let sweep = TAU - gap;
    assert!(
        sweep < TAU * 0.5,
        "a band of {sweep} radians cannot be told from its own complement"
    );
    (a[at], a[at] + sweep)
}

/// The 1-based bases a ring band runs between, inverted out of where it was
/// drawn.
///
/// `angle(a, len)` puts base `a` at `(a − 1)/len` of a turn and
/// `angle_past(b, len)` closes the band at `b/len` of one, so this is those two
/// read backwards — and read backwards out of coordinates, not recomputed from
/// them.
///
/// Fractional, and deliberately not rounded to a base: the whole point of the
/// assertions downstream is the SIZE of the error, and rounding would hide a
/// half-base one behind an exact-looking integer.
fn arc_bases(segs: &[Seg], len: u64) -> (f64, f64) {
    let (start, end) = arc_span(segs);
    (start / TAU * len as f64 + 1.0, end / TAU * len as f64)
}

/// The angle of the arrowhead's point on a ring band, or `None` if it has none.
///
/// The point is the one vertex at the band's MID radius. The shaft's corners
/// sit at `ri` and `ro` and the two barbs overshoot to `ri − barb` and
/// `ro + barb`, so `(ri + ro)/2` belongs to the point alone — and, because the
/// barbs are equal and opposite, that midpoint is the mean of the extreme radii
/// whether the band carries a head or not. So neither `ri` nor `ro` has to be
/// known here, which is what keeps this independent of `ring::radius`.
///
/// `None` is the honest answer for `Strand::Unoriented`, whose band is a plain
/// wedge, and for every part of a joined feature except the one `arrow_on`
/// names. A point is a directional claim, and a file that declines to make one
/// must not have it made on its behalf.
fn arrow_tip_angle(segs: &[Seg]) -> Option<f64> {
    let vs = ring_vertices(segs);
    let lo = vs.iter().map(|v| v.0).fold(f64::INFINITY, f64::min);
    let hi = vs.iter().map(|v| v.0).fold(f64::NEG_INFINITY, f64::max);
    let mid = (lo + hi) * 0.5;
    // 1e-6, not 1e-9. What it has to absorb is the sin/cos/hypot round trip,
    // about 1e-12 on the 270 unit radius these figures use; what it has to stay
    // clear of is the next vertex in, which is half the ring's width away --
    // nine units at the default. There is no width of doubt between the two.
    let mut on_band = vs.iter().filter(|v| (v.0 - mid).abs() < 1e-6);
    let tip = on_band.next()?;
    assert!(
        on_band.next().is_none(),
        "two vertices on the band's mid radius: a second arrowhead"
    );
    Some(tip.1)
}

#[test]
fn a_linear_molecule_now_gets_a_linear_figure() {
    // The whole complaint, as an assertion. Every FASTA, every assembly, every
    // PCR product and every gBlock opened linear and exported as a C-shaped ring
    // with a notch in it, because `scene` read the topology only to decide
    // whether to close the backbone.
    let mut m = plasmid(1000, false);
    m.features.push(feat("insert", "CDS", 100, 600));
    let (sc, _) = scene(&m, Options::default());

    assert!(
        !sc.items.iter().any(|i| matches!(i, Item::Circle { .. })),
        "a line drawn as a circle"
    );
    assert!(
        !sc.items.iter().any(|i| match i {
            Item::Path { segs, .. } => segs.iter().any(|s| matches!(s, Seg::Arc { .. })),
            _ => false,
        }),
        "a line drawn with arcs -- the ring's geometry is still in this figure"
    );
    // And the backbone is one horizontal line the width of the drawing.
    let backbone = sc
        .items
        .iter()
        .find_map(|i| match i {
            Item::Path {
                segs, fill: None, ..
            } if segs.len() == 2 => match (segs[0], segs[1]) {
                (Seg::Move(x0, y0), Seg::Line(x1, y1)) if y0 == y1 && x1 - x0 > sc.width * 0.5 => {
                    Some((x0, x1, y0))
                }
                _ => None,
            },
            _ => None,
        })
        .expect("no horizontal backbone in a linear figure");
    assert!(backbone.2 > 0.0 && backbone.2 < sc.height);

    // The circular molecule is untouched: still a ring, still a `<circle>`.
    let (round, _) = scene(&plasmid(1000, true), Options::default());
    assert!(round.items.iter().any(|i| matches!(i, Item::Circle { .. })));

    let (svg, _) = map_svg(&m, Options::default());
    well_formed(&svg).expect("malformed svg");
    assert!(svg.contains("insert"));
    assert!(!svg.contains("<circle"), "{svg}");
}

#[test]
fn a_circular_molecule_still_exports_exactly_the_ring_it_always_did() {
    // The other half of the mode's contract: adding a knob must not move a
    // single byte of any figure that already existed. `Shape::Auto` on a
    // circular molecule and `Shape::Circular` are the same picture, and so is
    // `Shape::Circular` on a linear one -- the gapped ring this crate has drawn
    // for a line since the beginning.
    for circular in [true, false] {
        let mut m = plasmid(4000, circular);
        for i in 0..6u64 {
            m.features
                .push(feat(&format!("f{i}"), "CDS", i * 500 + 1, i * 500 + 300));
        }
        let opts = Options {
            sites: vec![("EcoRI".into(), 402), ("BamHI".into(), 2_205)],
            ..Default::default()
        };
        let (pinned, pr) = circular_svg(&m, opts.clone());
        if circular {
            let (auto, ar) = map_svg(&m, opts.clone());
            assert_eq!(auto, pinned, "Auto moved a circular figure");
            assert_eq!(ar, pr);
        }
        // Pinning a ring on a line keeps the gap, and keeps it a ring.
        let (forced, _) = linear_svg(&m, opts);
        assert_ne!(forced, pinned);
        assert_eq!(pinned.contains("<circle"), circular);
    }
}

#[test]
fn a_feature_at_the_start_at_the_end_and_across_the_whole_molecule_is_drawn_where_it_is() {
    // The three cases a track gets wrong when the coordinate map is off by one
    // wrap: the first base, the last base, and everything.
    //
    // `head` must start at the very left of the axis, `tail` must finish at the
    // very right, and `most`, which covers 99.8% of the molecule, must reach
    // within a base of both ends -- rather than, as the ring's `angle_past`
    // would have it, wrapping to zero width at the origin.
    let len = 10_000u64;
    let mut m = plasmid(len as usize, false);
    m.features
        .push(feat_on("head", "CDS", 1, 400, Strand::Forward));
    m.features
        .push(feat_on("tail", "CDS", len - 399, len, Strand::Reverse));
    m.features
        .push(feat_on("most", "gene", 10, len - 10, Strand::Forward));
    let (sc, report) = scene(&m, Options::default());
    assert!(report.malformed.is_empty() && report.partly_drawn.is_empty());

    let one_base = sc.width / len as f64;
    let (head_lo, head_hi) = span_x(titled(&sc, "head")[0]);
    let (tail_lo, tail_hi) = span_x(titled(&sc, "tail")[0]);
    let (most_lo, most_hi) = span_x(titled(&sc, "most")[0]);

    // The axis's own ends, taken off the widest thing on it.
    let (x0, x1) = (head_lo, tail_hi);
    assert!(x0 > 0.0 && x1 < sc.width, "the track is off the canvas");

    assert!(
        (head_hi - x0 - 400.0 / len as f64 * (x1 - x0)).abs() < 1.0,
        "a 400 bp feature at base 1 is {} wide on a {} pt axis",
        head_hi - head_lo,
        x1 - x0
    );
    assert!(
        (tail_lo - (x1 - 400.0 / len as f64 * (x1 - x0))).abs() < 1.0,
        "the last 400 bases do not finish at the end of the track"
    );
    assert!(
        most_lo - x0 < 12.0 * one_base && x1 - most_hi < 12.0 * one_base,
        "a feature over 99.8% of the molecule spans {}..{} of {x0}..{x1}",
        most_lo,
        most_hi
    );
    assert!(
        most_hi - most_lo > (x1 - x0) * 0.99,
        "a near-whole-molecule feature collapsed"
    );
    // Strand is in the shape: forward points right, reverse points left. A
    // pentagon has five distinct x, a rectangle four, and the arrowhead's tip is
    // the extreme one on the side it points at.
    let axis_y = |segs: &[Seg]| {
        segs.iter()
            .filter_map(|s| match *s {
                Seg::Line(x, y) if x == head_hi || x == tail_lo => Some(y),
                _ => None,
            })
            .next()
    };
    assert!(axis_y(titled(&sc, "head")[0]).is_some(), "no forward tip");
    assert!(axis_y(titled(&sc, "tail")[0]).is_some(), "no reverse tip");
}

#[test]
fn nothing_a_linear_figure_draws_leaves_its_own_canvas() {
    // The failure this crate spends its comments on: a name that was measured to
    // fit and then cropped by the viewBox, the /MediaBox, the %%BoundingBox and
    // the raster canvas alike, in silence. Swept over widths, because the label
    // band's capacity, the ruler's inset and the caption's room all change with
    // it and only some of them bind at once.
    let sites: Vec<(String, u64)> = [
        ("EcoRI", 402u64),
        ("BamHI", 1_205),
        ("HindIII", 2_530),
        ("NotI", 3_100),
        ("KpnI", 3_260),
        ("SpeI", 3_300),
        ("NcoI", 3_340),
    ]
    .iter()
    .map(|(n, p)| ((*n).to_string(), *p))
    .collect();
    let mut m = plasmid(4000, false);
    m.name = "pLONGISH-NAME-FOR-A-CONSTRUCT".into();
    for (i, n) in ["bla", "ori", "lacZ-alpha", "T7 promoter", "AmpR-promoter"]
        .iter()
        .enumerate()
    {
        m.features
            .push(feat(n, "CDS", i as u64 * 700 + 1, i as u64 * 700 + 500));
    }
    for w in [200.0, 260.0, 300.0, 420.0, 720.0, 1000.0, 1600.0] {
        for h in [180.0, 300.0, 720.0] {
            let opts = Options {
                width: w,
                height: h,
                sites: sites.clone(),
                note: Some(ring::Disclosure {
                    cutters: 9,
                    labelled: 7,
                    dual: 2,
                    ..Default::default()
                }),
                ..Default::default()
            };
            let (sc, _) = scene(&m, opts);
            let (lo_x, hi_x, lo_y, hi_y) = extents(&sc);
            assert!(lo_x >= -0.01, "{w}x{h}: reaches x={lo_x}");
            assert!(
                hi_x <= sc.width + 0.01,
                "{w}x{h}: reaches x={hi_x} of {}",
                sc.width
            );
            assert!(lo_y >= -0.01, "{w}x{h}: reaches y={lo_y}");
            assert!(
                hi_y <= sc.height + 0.01,
                "{w}x{h}: reaches y={hi_y} of {}",
                sc.height
            );
        }
    }
}

#[test]
fn the_linear_figure_is_byte_identical_for_identical_input() {
    // The promise the whole crate is built on, on the new path: no HashMap
    // order, no float formatting that varies, no clock. Asserted through all
    // four writers, because the scene being stable is necessary and each writer
    // has its own float formatting.
    let mut m = plasmid(6000, false);
    m.name = "pDETERMINISM".into();
    for i in 0..30u64 {
        m.features
            .push(feat(&format!("f{i}"), "CDS", i * 190 + 1, i * 190 + 150));
    }
    let opts = Options {
        sites: (0..14u64)
            .map(|i| (format!("Enz{i}"), i * 401 + 7))
            .collect(),
        ..Default::default()
    };
    let (first_svg, r1) = map_svg_at(&m, opts.clone(), Some(89.0));
    let (first_pdf, _, _) = map_pdf_at(&m, opts.clone(), Some(89.0));
    let (sc, _) = scene(&m, opts.clone());
    let (first_eps, _) = eps::to_eps(&sc, page::Fit::to_width_mm(&sc, 89.0).scale);
    let (first_png, _, _) =
        map_png_at(&m, opts.clone(), Some(89.0), 300.0, [255, 255, 255]).expect("within budget");
    for _ in 0..8 {
        let (again, r2) = map_svg_at(&m, opts.clone(), Some(89.0));
        assert_eq!(again, first_svg);
        assert_eq!(r1, r2);
        let (pdf, _, _) = map_pdf_at(&m, opts.clone(), Some(89.0));
        assert_eq!(pdf, first_pdf, "the pdf moved");
        let (sc, _) = scene(&m, opts.clone());
        let (e, _) = eps::to_eps(&sc, page::Fit::to_width_mm(&sc, 89.0).scale);
        assert_eq!(e, first_eps, "the eps moved");
        let (png, _, _) =
            map_png_at(&m, opts.clone(), Some(89.0), 300.0, [255, 255, 255]).expect("budget");
        assert_eq!(png, first_png, "the png moved");
    }
    assert!(r1.labels_placed > 0);

    // And again on a canvas too small to hold it, because the drop path is
    // where an unordered container shows: `labels_hidden`, `sites_hidden` and
    // the `dropped` list `place_rows` returns are all sequences a reader sees,
    // and a set iterated in hash order would put them in a different order on
    // every run without changing a single coordinate.
    let cramped = Options {
        width: 260.0,
        height: 150.0,
        ..opts.clone()
    };
    let (small, sr1) = map_svg(&m, cramped.clone());
    assert!(
        !sr1.labels_hidden.is_empty(),
        "nothing was dropped to compare"
    );
    assert!(
        !sr1.sites_hidden.is_empty(),
        "no enzyme was dropped to compare"
    );
    for _ in 0..8 {
        let (again, sr2) = map_svg(&m, cramped.clone());
        assert_eq!(again, small);
        assert_eq!(sr2.labels_hidden, sr1.labels_hidden);
        assert_eq!(sr2.sites_hidden, sr1.sites_hidden);
        assert_eq!(sr2, sr1);
    }
}

#[test]
fn a_circular_molecule_drawn_on_a_track_says_it_was_cut_open() {
    // A track and a track are the same picture: nothing in the geometry
    // distinguishes a linearised plasmid from a molecule that really is a line,
    // and "this is not a closed molecule" is the most consequential thing a map
    // can get wrong. So the figure says it, and `Report` carries it.
    let m = plasmid(3000, true);
    let (svg, report) = linear_svg(&m, Options::default());
    assert!(report.cut_open, "the report did not record the cut");
    assert!(
        svg.contains("circular, shown cut open at base 1"),
        "the figure does not say it was cut: {svg}"
    );
    // The wording narrows rather than being cut through with an ellipsis, and
    // the claim survives to the narrowest form that still fits.
    let narrow = Options {
        width: 220.0,
        ..Default::default()
    };
    let (small, small_report) = linear_svg(&m, narrow);
    assert!(small_report.cut_open);
    assert!(small.contains("cut circle"), "{small}");
    assert!(!small.contains("cut ci..."), "a disclosure was cut short");

    // And a molecule that really is linear claims nothing.
    let (plain, plain_report) = map_svg(&plasmid(3000, false), Options::default());
    assert!(!plain_report.cut_open);
    assert!(!plain.contains("cut"), "{plain}");
    // Nor does a circular molecule drawn as the ring it is.
    let (round, round_report) = map_svg(&m, Options::default());
    assert!(!round_report.cut_open);
    assert!(!round.contains("cut"));
}

#[test]
fn the_mode_overrides_the_topology_in_both_directions() {
    let line = plasmid(2000, false);
    let circle = plasmid(2000, true);
    // An arc, not a `<circle>`: a LINEAR molecule pinned to a ring gets the
    // GAPPED one, which is an `Item::Path` full of `Seg::Arc` and no circle at
    // all. Asking only for the circle would pass this line while drawing a
    // track, which is the whole thing being asserted.
    let ring = |sc: &Scene| {
        sc.items.iter().any(|i| match i {
            Item::Circle { .. } => true,
            Item::Path { segs, .. } => segs.iter().any(|s| matches!(s, Seg::Arc { .. })),
            Item::Text { .. } => false,
        })
    };

    // Auto asks the molecule.
    assert!(ring(&scene(&circle, Options::default()).0));
    assert!(!ring(&scene(&line, Options::default()).0));
    // And each pin is obeyed against the topology.
    let pin = |shape| Options {
        shape,
        ..Default::default()
    };
    assert!(ring(&scene(&line, pin(Shape::Circular)).0));
    assert!(!ring(&scene(&circle, pin(Shape::Linear)).0));
    // `Topology::default()` is Linear, so Auto must never mean "circular unless
    // told otherwise" -- a record whose file did not say would become a ring.
    assert!(!ring(&scene(&Molecule::default(), Options::default()).0));
}

#[test]
fn a_linear_figure_names_every_label_it_could_not_draw() {
    // The same accounting the ring keeps: a map missing three labels looks
    // exactly like a molecule with three fewer features.
    let mut m = plasmid(3000, false);
    for i in 0..120u64 {
        m.features.push(feat(
            &format!("a-rather-long-feature-name-{i}"),
            "CDS",
            i * 20 + 1,
            i * 20 + 15,
        ));
    }
    let (_, report) = map_svg(
        &m,
        Options {
            width: 300.0,
            height: 220.0,
            ..Default::default()
        },
    );
    assert!(!report.labels_hidden.is_empty(), "nothing reported hidden");
    assert_eq!(report.labels_placed + report.labels_hidden.len(), 120);
}

#[test]
fn a_linear_figure_survives_every_molecule_the_ring_does() {
    // The hostile sweep `degenerate_molecules_do_not_panic` runs through the
    // ring, run through the track: an empty record, a 1 bp molecule, coordinates
    // at zero and past the end, and a length off a LOCUS line at the top of the
    // u64 range, where the ruler's `base += step` overflows and `frac`'s modulo
    // is the only thing between a hostile file and a wrong figure.
    let mut zero_coords = plasmid(500, false);
    zero_coords.features.push(feat("zero", "CDS", 0, 0));
    zero_coords.features.push(feat("past", "CDS", 900, 1200));
    zero_coords.features.push(feat("straddle", "CDS", 0, 250));

    let mut declared_max = Molecule {
        declared_len: Some(u64::MAX),
        topology: Topology::Linear,
        ..Default::default()
    };
    declared_max.features.push(feat("w", "CDS", 1, u64::MAX));

    let mut joined = plasmid(1000, false);
    let mut j = Feature::new("split", "CDS");
    j.segments.push(Segment::new(100, 200));
    j.segments.push(Segment::new(5000, 6000));
    joined.features.push(j);

    let cases = [
        Molecule::default(),
        plasmid(0, false),
        plasmid(1, false),
        plasmid(2, false),
        zero_coords,
        declared_max,
        joined,
    ];
    for (i, m) in cases.iter().enumerate() {
        for w in [120.0, 300.0, 720.0] {
            let (svg, _) = map_svg_at(
                m,
                Options {
                    width: w,
                    sites: vec![("EcoRI".into(), 1), ("BsaI".into(), u64::MAX)],
                    ..Default::default()
                },
                Some(89.0),
            );
            well_formed(&svg).unwrap_or_else(|e| panic!("case {i} at {w}: {e}"));
        }
    }
    // The joined feature loses a segment and is named for it, on this path too.
    let (_, report) = map_svg(&cases[6], Options::default());
    assert_eq!(report.partly_drawn, vec!["split".to_string()]);
}

#[test]
fn a_feature_too_short_to_carry_an_arrowhead_is_a_mark_across_the_band() {
    // The ring's rule and the ring's threshold: below `min_feature_degrees` an
    // arrowhead is smaller than the outline around it and reads as dirt on the
    // figure. `degrees` is a share of the molecule times 360, so the two figures
    // agree about which features are marks.
    let mut m = plasmid(100_000, false);
    m.features.push(feat("dot", "CDS", 50_000, 50_010));
    m.features.push(feat("real", "CDS", 10_000, 30_000));
    let (sc, _) = scene(&m, Options::default());
    let dot = titled(&sc, "dot");
    assert_eq!(dot.len(), 1);
    assert_eq!(dot[0].len(), 2, "a mark is a Move and a Line, not a box");
    let (lo, hi) = span_x(dot[0]);
    assert_eq!(lo, hi, "the mark is not vertical");
    let real = titled(&sc, "real");
    assert!(real[0].len() > 4, "a 20 kb feature drawn as a mark");
}

#[test]
fn a_track_is_only_as_tall_as_the_figure_it_holds() {
    // `Options::height` is the label band's budget, not the canvas. Padding out
    // to it puts a 190 pt drawing in 530 pt of white at the 720 x 720 default,
    // which `page::Fit` then prints as an 89 x 89 mm block and a raster export
    // pays for in pixels.
    let mut m = plasmid(3000, false);
    m.features.push(feat("insert", "CDS", 100, 900));
    let (sc, _) = scene(&m, Options::default());
    assert_eq!(sc.width, 720.0);
    assert!(sc.height < 200.0, "the figure came back {} tall", sc.height);
    // More room means more rows for the labels that need them, never more white.
    let tall = Options {
        height: 2000.0,
        ..Default::default()
    };
    let (roomy, _) = scene(&m, tall);
    assert_eq!(roomy.height, sc.height, "empty height reached the figure");
    // And the height that is used is a function of what is drawn.
    let mut crowded = m.clone();
    for i in 0..40u64 {
        crowded
            .features
            .push(feat(&format!("g{i}"), "CDS", i * 70 + 1, i * 70 + 40));
    }
    let (deep, _) = scene(&crowded, Options::default());
    assert!(deep.height > sc.height, "40 more labels cost no height");
}

#[test]
fn a_name_the_figure_keeps_whole_is_a_name_the_figure_can_draw() {
    // The invariant that makes `place_rows` terminate, and the reason
    // `fit_label` is given `row - ROW_GAP` rather than the whole band: a label
    // admitted at exactly the band's width costs the band its width PLUS the
    // gutter, so every row drops it and it lands in `labels_hidden` -- measured
    // to fit, and then not drawn, which is the silent half of the
    // decide-in-one-unit-draw-in-another defect this crate keeps finding.
    //
    // Swept over lengths and widths because the window is one gutter wide, so a
    // single name misses it.
    for w in [220.0_f64, 260.0, 300.0, 420.0, 720.0] {
        for n in 1..=80usize {
            let mut m = plasmid(2000, false);
            m.features.push(feat(&"M".repeat(n), "CDS", 100, 900));
            let (_, report) = map_svg(
                &m,
                Options {
                    width: w,
                    ..Default::default()
                },
            );
            assert_eq!(
                report.labels_placed, 1,
                "{w} pt wide, {n} characters: the only label was not drawn"
            );
            assert!(
                report.labels_hidden.is_empty(),
                "{w} pt wide, {n} characters: hidden {:?} with nothing shortened={}",
                report.labels_hidden,
                report.labels_truncated.is_empty()
            );
        }
    }
}

#[test]
fn the_arrowhead_is_at_the_end_the_feature_reads_towards() {
    // The tip is the one vertex ON the axis, and which end it is on is the only
    // thing in the picture that says which way the feature reads. An unoriented
    // feature has no such vertex, because a point is a directional claim and
    // `Strand::Unoriented` is the file declining to make it.
    let mut m = plasmid(6000, false);
    m.features
        .push(feat_on("fwd", "CDS", 1_000, 2_000, Strand::Forward));
    m.features
        .push(feat_on("rev", "CDS", 3_000, 4_000, Strand::Reverse));
    m.features
        .push(feat_on("flat", "CDS", 4_500, 5_500, Strand::Unoriented));
    let (sc, _) = scene(&m, Options::default());

    for (name, want) in [("fwd", Some(true)), ("rev", Some(false)), ("flat", None)] {
        let segs = titled(&sc, name)[0];
        let ys: Vec<f64> = segs
            .iter()
            .filter_map(|s| match *s {
                Seg::Move(_, y) | Seg::Line(_, y) => Some(y),
                _ => None,
            })
            .collect();
        let mid = (ys.iter().copied().fold(f64::INFINITY, f64::min)
            + ys.iter().copied().fold(f64::NEG_INFINITY, f64::max))
            * 0.5;
        let (lo, hi) = span_x(segs);
        let on_axis: Vec<f64> = segs
            .iter()
            .filter_map(|s| match *s {
                Seg::Move(x, y) | Seg::Line(x, y) if (y - mid).abs() < 1e-9 => Some(x),
                _ => None,
            })
            .collect();
        match want {
            None => assert!(
                on_axis.is_empty(),
                "{name}: an unoriented feature was given an arrowhead at {on_axis:?}"
            ),
            Some(forward) => {
                assert_eq!(on_axis.len(), 1, "{name}: {on_axis:?} vertices on the axis");
                let want_x = if forward { hi } else { lo };
                assert!(
                    (on_axis[0] - want_x).abs() < 1e-9,
                    "{name}: the tip is at {} and the feature runs {lo}..{hi}",
                    on_axis[0]
                );
            }
        }
    }
}

/// The ring twin of [`the_arrowhead_is_at_the_end_the_feature_reads_towards`],
/// which draws a LINEAR molecule and so tests `box_segs` and never `arc_segs`.
///
/// **This passes at c0b60b2 and is meant to: the ring draws this correctly and
/// always has.** The finding is that nothing noticed. Until this existed, the
/// test above was the only one in `pl-draw` asserting which end of a feature
/// carries the point, its fixture is `plasmid(6000, false)`, and `plasmid`'s
/// second parameter is `circular` — so the ring's own copy of the rule, the
/// `match d.strand` in `circular_scene`'s feature loop, was read back by
/// nothing. Inverting it wholesale left all 208 tests in this crate green while
/// moving a reverse gene's point 90 degrees to the wrong end of its arc. F4 in
/// `docs/AUDIT-2026-08-14-r3.md` measured exactly that.
///
/// Which end carries the point is not decoration. It is the only thing in a
/// plasmid map that says which way a gene is transcribed, and a reader who
/// clones off a figure with it reversed builds the construct backwards.
///
/// **Relative, on purpose.** The assertion is where the point sits within the
/// band's own two ends, never at an absolute angle or an absolute base, so this
/// test answers for direction alone and
/// [`a_feature_on_the_ring_is_drawn_between_the_bases_it_names`] answers for
/// position. Rotating the whole ring leaves this green, which is what keeps the
/// two findings' checks from collapsing into one check that fails for two
/// reasons and pins neither. The second assertion in each oriented arm — that
/// the point is nowhere near the band's other end — is what stops a degenerate
/// band, whose two ends coincide, satisfying the first arm vacuously.
///
/// The fixture is 3,600 bp so a base is a tenth of a degree, and every feature
/// is 600 bp: a 60 degree band, far above `min_feature_degrees`, so each is
/// drawn as an arrowed arc rather than degrading to the tick branch.
///
/// Mutation that re-breaks it: in `crates/pl-draw/src/lib.rs`, in the feature
/// loop of `circular_scene`, change `Strand::Reverse => Arrow::Start,`
/// (line 1346 at c0b60b2) to `Strand::Reverse => Arrow::End,`.
#[test]
fn on_the_ring_too_the_arrowhead_is_at_the_end_the_feature_reads_towards() {
    let mut m = plasmid(3_600, true);
    m.features
        .push(feat_on("fwd", "CDS", 201, 800, Strand::Forward));
    m.features
        .push(feat_on("rev", "CDS", 1_001, 1_600, Strand::Reverse));
    m.features
        .push(feat_on("flat", "CDS", 2_001, 2_600, Strand::Unoriented));
    let (sc, report) = circular_scene(&m, Options::default());
    assert!(report.malformed.is_empty() && report.partly_drawn.is_empty());

    for (name, want) in [("fwd", Some(true)), ("rev", Some(false)), ("flat", None)] {
        let bands = titled(&sc, name);
        assert_eq!(bands.len(), 1, "{name}: {} bands drawn", bands.len());
        let (start, end) = arc_span(bands[0]);
        // The premise every assertion below rests on: the band has two ENDS,
        // told apart by more than float noise. 600 of 3,600 bases is 60
        // degrees, so this is a long way from binding.
        assert!(
            end - start > 0.5,
            "{name}: a band of {} radians has no two ends to speak of",
            end - start
        );
        match (want, arrow_tip_angle(bands[0])) {
            (None, tip) => assert!(
                tip.is_none(),
                "{name}: an unoriented feature was given an arrowhead at {tip:?}"
            ),
            (Some(_), None) => panic!("{name}: an oriented feature has no arrowhead"),
            (Some(forward), Some(tip)) => {
                // Clockwise from twelve o'clock, so a forward feature's own
                // reading direction is towards the LARGER angle.
                let (point, tail) = if forward { (end, start) } else { (start, end) };
                assert!(
                    (tip - point).abs() < 1e-9,
                    "{name}: the point is at {tip} and the band runs {start}..{end}"
                );
                assert!(
                    (tip - tail).abs() > 0.5,
                    "{name}: the point is at the band's other end, {tip} of {start}..{end}"
                );
            }
        }
    }
}

/// Where a ring band lands, in the molecule's own coordinates.
///
/// **This passes at c0b60b2 and is meant to.** Nothing in this crate tied a
/// base number to an angle on the ring: the audit rotated every band by 10, 50,
/// 200 and 1,000 bases, shortened each by one, swapped its two endpoints and
/// finally reflected the entire figure across the vertical axis, and all 208
/// tests here plus all 18 integration tests stayed green through every one of
/// them. A figure with every feature mirrored passed the whole `pl-draw` test
/// surface. That is not a loose bound; it is no bound. F5 in
/// `docs/AUDIT-2026-08-14-r3.md`.
///
/// The track has had this since it was written —
/// [`a_feature_at_the_start_at_the_end_and_across_the_whole_molecule_is_drawn_where_it_is`]
/// — and this is its ring twin, with the three cases a coordinate map gets
/// wrong when it is off by one wrap: a feature on the FIRST base, one ending on
/// the LAST, and one crossing the origin, whose two parts have to come back at
/// their own coordinates and not at the whole feature's.
///
/// **Read backwards out of the drawing, not compared against `angle`.**
/// [`arc_bases`] inverts `atan2` over the vertices the back ends will actually
/// write, so this would still fail if `angle` and `frac` were themselves wrong
/// — which asserting `drawn == angle(start, len)` would not, both sides moving
/// together.
///
/// **The second half is what makes the first unfakeable.** Absolute positions
/// alone can be satisfied by a renderer told the answer; two identical features
/// exactly 1,000 bases apart having to land exactly 1,000 bases apart cannot
/// be, and the pair together pin the mapping rather than one point on it.
///
/// Mutation that re-breaks it: in `crates/pl-draw/src/lib.rs`, in the one line
/// of `circular_scene` that turns bases into an arc —
/// `segs: arc_segs(cx, cy, ri, ro, angle(a, len), angle_past(b, len), arrow),`,
/// line 1353 at c0b60b2 — rotate every band by a thousand bases by replacing
/// `angle(a, len), angle_past(b, len)` with
/// `angle(a.wrapping_add(1000), len), angle_past(b.wrapping_add(1000), len)`.
///
/// `wrapping_add` and not `+`, because `len` comes off a LOCUS line and two
/// unrelated tests here hand this function a molecule near the top of the `u64`
/// range on purpose: a plain `+ 1000` overflows and panics in those instead,
/// and a mutation that fails a test for a second reason proves nothing about
/// the first. A ONE-base rotation reddens this test too, and so does dropping
/// `angle_past` for `angle`, negating both angles, or exchanging them; all four
/// were run.
#[test]
fn a_feature_on_the_ring_is_drawn_between_the_bases_it_names() {
    let len = 3_600u64;
    let mut m = plasmid(len as usize, true);
    m.features
        .push(feat_on("first", "CDS", 1, 300, Strand::Forward));
    m.features
        .push(feat_on("plus1000", "CDS", 1_001, 1_300, Strand::Forward));
    m.features
        .push(feat_on("last", "CDS", 3_301, len, Strand::Forward));
    m.features
        .push(feat_on("wrap", "CDS", 3_500, 200, Strand::Forward));
    let (sc, report) = circular_scene(&m, Options::default());
    assert!(report.malformed.is_empty() && report.partly_drawn.is_empty());

    // A quarter of a base. The round trip through `polar` and `atan2` is good
    // to about 1e-12 bases at this radius and the smallest mutation this exists
    // to catch moves a band by a whole one, so there is no width of error
    // between exact and caught, and the number is stated in BASES because that
    // is the unit a reader of the figure works in.
    let tol = 0.25;
    for (name, want) in [
        ("first", vec![(1.0, 300.0)]),
        ("plus1000", vec![(1_001.0, 1_300.0)]),
        ("last", vec![(3_301.0, 3_600.0)]),
        // `ranges` splits this one at the origin, and the parts are the
        // feature's real coordinates rather than 3,500..3,700 or 1..200 twice.
        ("wrap", vec![(3_500.0, 3_600.0), (1.0, 200.0)]),
    ] {
        let bands = titled(&sc, name);
        assert_eq!(
            bands.len(),
            want.len(),
            "{name}: {} bands drawn for {} parts",
            bands.len(),
            want.len()
        );
        for (i, &(a, b)) in want.iter().enumerate() {
            let (from, to) = arc_bases(bands[i], len);
            assert!(
                (from - a).abs() < tol,
                "{name} part {i} begins at base {from}, not {a}"
            );
            assert!(
                (to - b).abs() < tol,
                "{name} part {i} ends at base {to}, not {b}"
            );
        }
    }

    let (first_from, _) = arc_bases(titled(&sc, "first")[0], len);
    let (later_from, _) = arc_bases(titled(&sc, "plus1000")[0], len);
    assert!(
        (later_from - first_from - 1_000.0).abs() < tol,
        "two features 1,000 bases apart were drawn {} bases apart",
        later_from - first_from
    );
}

#[test]
fn the_rulers_own_numbers_never_overprint_each_other() {
    // The ring divides by a flat twelve and can: twelve numbers around `2πr` are
    // spread over the whole circumference. A track has only its width, and at
    // 300 pt twelve numbers of `5,386` come 26 pt apart with 21 pt of glyphs.
    // Overprinted digits on a scale are a cut coordinate by another route: what
    // the reader takes off the figure is not a number the molecule has.
    for len in [900u64, 5_386, 48_502, 4_641_652] {
        let m = plasmid(len as usize, false);
        for w in [200.0_f64, 260.0, 300.0, 420.0, 720.0, 1600.0] {
            let (sc, _) = scene(
                &m,
                Options {
                    width: w,
                    ..Default::default()
                },
            );
            // The ruler is the run of same-size, same-y text below the band.
            let size = 12.0 * 0.72;
            let mut row: Vec<(f64, f64)> = sc
                .items
                .iter()
                .filter_map(|i| match i {
                    Item::Text {
                        x, size: s, text, ..
                    } if (*s - size).abs() < 1e-9 => {
                        Some((*x, crate::pdf::text_width_in(text, *s, false)))
                    }
                    _ => None,
                })
                .collect();
            assert!(!row.is_empty(), "{len} bp at {w} pt has no ruler at all");
            row.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("no NaN"));
            for pair in row.windows(2) {
                let gap = (pair[1].0 - pair[1].1 * 0.5) - (pair[0].0 + pair[0].1 * 0.5);
                assert!(
                    gap >= 0.0,
                    "{len} bp at {w} pt: two ruler numbers overlap by {}",
                    -gap
                );
            }
        }
    }
}

#[test]
fn what_folds_onto_one_tick_does_not_depend_on_the_type_size() {
    // `bases_per_unit` is handed the TICK'S OWN STROKE, never a label height.
    // Two cuts closer together than the mark that draws them are one mark, which
    // is a fact about the picture; two cuts whose names collide are a fact about
    // the type, and the packer already has an answer for that -- move them a row
    // apart. With a label height the threshold grows as the type does, so
    // enlarging the font would change what the figure claims about the molecule.
    let sites: Vec<(String, u64)> = [
        ("XmaI", 2_917u64),
        ("SmaI", 2_919),
        ("SphI", 4_000),
        ("NsiI", 4_060),
        ("EcoRI", 402),
    ]
    .iter()
    .map(|(n, p)| ((*n).to_string(), *p))
    .collect();
    let m = plasmid(5386, false);
    // A site label is the only `<title>` with two spaces in it.
    let claims = |font: f64| -> Vec<String> {
        let (sc, _) = scene(
            &m,
            Options {
                width: 1400.0,
                font_size: font,
                sites: sites.clone(),
                ..Default::default()
            },
        );
        let mut v: Vec<String> = sc
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Path { title: Some(t), .. } if t.contains("  ") => Some(t.clone()),
                _ => None,
            })
            .collect();
        v.sort();
        v
    };
    let at_twelve = claims(12.0);
    assert!(
        at_twelve
            .iter()
            .any(|t| t.contains("XmaI") && t.contains("SmaI")),
        "two cuts 2 bp apart are one mark and should share a label: {at_twelve:?}"
    );
    assert!(
        !at_twelve
            .iter()
            .any(|t| t.contains("SphI") && t.contains("NsiI")),
        "two cuts 60 bp apart are two marks: {at_twelve:?}"
    );
    for font in [7.0, 9.0, 16.0, 24.0] {
        assert_eq!(
            claims(font),
            at_twelve,
            "{font} pt type changed what the figure says is at one tick"
        );
    }
}

/// The two-pass note is exact **on a track too**, which it was not.
///
/// PROVEN TO FAIL before `caption_for_capacity`: on a 6 kb track with 40 cut
/// sites at 720 x 180, pass one reported 33 enzymes named and 7 hidden, and the
/// figure that then went out with that line printed on it named 24 and hid 16.
///
/// The ring's version of this test could not see it and never will: on a ring
/// the note reaches `centre_room` and stops, while on a track it landed in
/// `caption_bottom`, one of the four terms deciding how many rows of labels
/// there is room for, so drawing the note cost the band a row. Both callers say
/// in a comment that this cannot happen, `debug_assert!(Disclosure::closes)`
/// agrees with them — 24 + 16 and 33 + 7 both reach 40 — and the reader gets a
/// count taken off a different picture from the one in front of them.
///
/// Swept over heights, because the row capacity only moves where `height` binds
/// and one size cannot find that.
#[test]
fn the_note_does_not_change_what_it_counts_on_a_track_either() {
    let mut m = plasmid(6000, false);
    m.name = "pTRACK".into();
    for i in 0..20u64 {
        m.features.push(feat(
            &format!("feature-{i}"),
            "CDS",
            i * 90 + 1,
            i * 90 + 70,
        ));
    }
    let sites: Vec<(String, u64)> = (0..40u64)
        .map(|i| (format!("Enz{i}"), i * 140 + 11))
        .collect();
    let mut bound = 0;
    for w in [420.0_f64, 720.0] {
        for h in [
            140.0_f64, 150.0, 160.0, 180.0, 200.0, 220.0, 240.0, 300.0, 720.0,
        ] {
            let base = Options {
                width: w,
                height: h,
                sites: sites.clone(),
                ..Default::default()
            };
            let (_, first) = scene(&m, base.clone());
            let told = ring::Disclosure {
                cutters: 40,
                single: 0,
                labelled: first.sites_named,
                dual: 0,
                multi: 0,
                hidden: first.sites_dropped,
                shortened: first.sites_shortened,
            };
            assert!(told.closes(), "{told:?}");
            let (_, second) = scene(
                &m,
                Options {
                    note: Some(told),
                    ..base
                },
            );
            assert_eq!(
                first.sites_named, second.sites_named,
                "{w}x{h}: the line says {} enzymes are named and the figure it is \
                 printed on names {}",
                first.sites_named, second.sites_named
            );
            assert_eq!(first.sites_dropped, second.sites_dropped, "{w}x{h}");
            assert_eq!(first.sites_hidden, second.sites_hidden, "{w}x{h}");
            assert_eq!(first.labels_placed, second.labels_placed, "{w}x{h}");
            assert_eq!(first.labels_hidden, second.labels_hidden, "{w}x{h}");
            if !first.labels_hidden.is_empty() {
                bound += 1;
            }
        }
    }
    // A sweep where nothing was ever dropped would assert nothing at all: the
    // note can only steal a row from a figure that is already short of them.
    assert!(
        bound >= 4,
        "only {bound} of the sizes were actually crowded"
    );
}

/// A feature across the origin, on a molecule cut open to be drawn flat.
///
/// **The answer is SPLIT, not refuse, and it is the same split `ranges` has
/// always made for the ring.** A cut circle genuinely has that feature at both
/// ends — those are the bases a reader would find there — so drawing one box per
/// part states what is true, and refusing would delete a real feature from the
/// picture on a technicality about where base 1 happens to fall. The reading
/// that IS wrong is one box from 1,900 to 300, which on a track runs backwards.
///
/// What makes the split safe is the caption: the figure says the molecule is a
/// circle shown cut open at base 1, so two boxes under one name read as a wrap
/// and not as two copies. Without that line this test would be pinning a lie.
#[test]
fn a_feature_across_the_origin_is_split_and_the_figure_says_where_it_was_cut() {
    let len = 2000u64;
    let mut m = plasmid(len as usize, true);
    m.name = "pORIGIN".into();
    let mut wraps = Feature::new("wraps", "CDS");
    wraps.segments.push(Segment::new(1_900, 300));
    m.features.push(wraps);
    m.features.push(feat("inside", "CDS", 600, 900));

    let (sc, report) = scene(
        &m,
        Options {
            shape: Shape::Linear,
            ..Default::default()
        },
    );

    // Two boxes, one name, nothing reported lost: a wrap is not a malformed
    // feature and must not be counted as one.
    let parts = titled(&sc, "wraps");
    assert_eq!(parts.len(), 2, "an origin-spanning feature was not split");
    assert!(report.malformed.is_empty(), "{:?}", report.malformed);
    assert!(report.partly_drawn.is_empty(), "{:?}", report.partly_drawn);
    assert_eq!(titled(&sc, "inside").len(), 1);

    // One part ends at the right-hand end of the track and the other starts at
    // the left-hand end — which is what "cut between the last base and the
    // first" looks like. Bracketed against a feature that does NOT wrap, so this
    // cannot pass by both parts landing in the middle.
    let (a_lo, a_hi) = span_x(parts[0]);
    let (b_lo, b_hi) = span_x(parts[1]);
    let (whole_lo, whole_hi) = span_x(titled(&sc, "inside")[0]);
    let (left, right) = if a_lo < b_lo {
        ((a_lo, a_hi), (b_lo, b_hi))
    } else {
        ((b_lo, b_hi), (a_lo, a_hi))
    };
    assert!(
        left.0 < whole_lo && right.1 > whole_hi,
        "the two parts {left:?} {right:?} are not at the two ends"
    );
    // And they cover the right number of bases: 300 at the start, 101 at the
    // end, of 2,000.
    let axis = right.1 - left.0;
    assert!(
        ((left.1 - left.0) - 300.0 / len as f64 * axis).abs() < 1.0,
        "the leading part covers {} of a {axis} pt axis",
        left.1 - left.0
    );
    assert!(
        ((right.1 - right.0) - 101.0 / len as f64 * axis).abs() < 1.0,
        "the trailing part covers {}",
        right.1 - right.0
    );

    // The disclosure that makes the split readable, in the figure and in the
    // report. Without it, two boxes under one name is a molecule with two copies
    // of that feature.
    assert!(report.cut_open);
    let (svg, _) = linear_svg(&m, Options::default());
    assert!(
        svg.contains("circular, shown cut open at base 1"),
        "a split feature with no statement of where the cut is"
    );

    // And the label points at a base the feature actually covers: `mid_base`
    // accumulates across the parts, so the anchor is base 100, inside the
    // leading box, and not base 1,000, which the feature does not touch.
    let anchored_inside = sc.items.iter().any(|i| match i {
        Item::Path {
            segs,
            fill: None,
            title: None,
            ..
        } if segs.len() == 3 => matches!(
            segs[0],
            Seg::Move(x, _) if (left.0..=left.1).contains(&x) || (right.0..=right.1).contains(&x)
        ),
        _ => false,
    });
    assert!(anchored_inside, "no leader inside the feature's own extent");
}

/// Segments the file lists out of order, which GenBank permits and `join()`
/// uses to say which way a feature reads.
///
/// Both parts are drawn where their coordinates put them, and the arrowhead goes
/// on the segment the FILE lists last rather than the one furthest right — which
/// for `join(1500..1700,200..400)` on a forward feature is the left-hand box.
/// That is deliberate, and it is the ring's answer too, from the same `arrow_on`
/// in the same shared `resolve_features`: the order is how the feature reads,
/// and re-sorting by coordinate would silently rewrite that.
#[test]
fn segments_out_of_order_are_drawn_where_they_are_and_read_the_way_the_file_says() {
    let mut m = plasmid(2000, false);
    let mut f = Feature::new("descending", "CDS");
    f.strand = Strand::Forward;
    f.segments.push(Segment::new(1_500, 1_700));
    f.segments.push(Segment::new(200, 400));
    m.features.push(f);
    let (sc, report) = scene(&m, Options::default());
    assert!(report.malformed.is_empty() && report.partly_drawn.is_empty());

    let parts = titled(&sc, "descending");
    assert_eq!(parts.len(), 2);
    // In file order: part 0 is the high one.
    assert!(
        span_x(parts[0]).0 > span_x(parts[1]).0,
        "the parts were re-sorted, which rewrites which one the feature ends on"
    );
    // The tip is the vertex on the axis, and it is on the LAST-LISTED part.
    let tips = |segs: &[Seg]| {
        let ys: Vec<f64> = segs
            .iter()
            .filter_map(|s| match *s {
                Seg::Move(_, y) | Seg::Line(_, y) => Some(y),
                _ => None,
            })
            .collect();
        let mid = (ys.iter().copied().fold(f64::INFINITY, f64::min)
            + ys.iter().copied().fold(f64::NEG_INFINITY, f64::max))
            * 0.5;
        segs.iter()
            .filter(|s| matches!(**s, Seg::Move(_, y) | Seg::Line(_, y) if (y - mid).abs() < 1e-9))
            .count()
    };
    assert_eq!(
        tips(parts[1]),
        1,
        "the last-listed segment has no arrowhead"
    );
    assert_eq!(tips(parts[0]), 0, "a second arrowhead on a joined feature");

    // The ring makes the same choice from the same field, so the two figures
    // cannot disagree about which PART of a joined feature carries the point --
    // and that is now asserted here rather than merely stated. Until
    // 2026-08-14 this block ended at the part count below, and the sentence
    // above it claimed a compensating control that did not exist: the ring's
    // arrowhead could go on the wrong part, or on the wrong end of the right
    // one, with nothing in the crate the wiser. The false claim had already
    // cost something, having talked round 2's finding 7 into calling the
    // shipped desktop figures guarded. Which END the point is on is
    // `on_the_ring_too_the_arrowhead_is_at_the_end_the_feature_reads_towards`;
    // which PART it is on is here, because only this fixture has two.
    let mut round = plasmid(2000, true);
    round.features = m.features.clone();
    let (ring_scene, _) = circular_scene(&round, Options::default());
    let ring_parts = titled(&ring_scene, "descending");
    assert_eq!(
        ring_parts.len(),
        2,
        "the ring drew a different number of parts for the same feature"
    );
    assert!(
        arrow_tip_angle(ring_parts[1]).is_some(),
        "the ring left the last-listed segment without an arrowhead"
    );
    assert!(
        arrow_tip_angle(ring_parts[0]).is_none(),
        "a second arrowhead on a joined feature, on the ring this time"
    );
}

/// Five hundred features on top of each other, all wanting the same label spot.
///
/// The pathological input for [`crate::place_rows`]: every label's ideal `x` is
/// within a few points of every other's, so row after row fills, spills and
/// fills again. It has to terminate, account for every label it did not draw,
/// and stay inside the height it was budgeted.
///
/// Bounded in TIME as well, loosely — one second is roughly four hundred times
/// what this measures, so it cannot flake on a slow machine, and it still fails
/// outright on anything quadratic in the labels. Without the no-progress guard
/// in `place_rows`, the sibling unit test runs past ninety seconds.
#[test]
fn five_hundred_overlapping_features_terminate_and_are_all_accounted_for() {
    let mut m = plasmid(5000, false);
    for i in 0..500u64 {
        m.features.push(feat(
            &format!("ov{i}"),
            "CDS",
            2_000 + i % 7,
            2_400 + i % 11,
        ));
    }
    for height in [120.0_f64, 200.0, 400.0, 720.0, 4000.0] {
        let started = std::time::Instant::now();
        let (sc, report) = scene(
            &m,
            Options {
                height,
                ..Default::default()
            },
        );
        let took = started.elapsed();
        assert!(
            took.as_secs_f64() < 1.0,
            "500 overlapping features took {took:?} at height {height}"
        );
        assert_eq!(
            report.labels_placed + report.labels_hidden.len(),
            500,
            "at height {height}, {} labels went missing without being named",
            500 - report.labels_placed - report.labels_hidden.len()
        );
        // Every feature is DRAWN whatever happens to its label. Overprinting in
        // one band is the ring's answer too, and losing a box would be a feature
        // deleted from the picture rather than a name deferred to the Features
        // tab.
        let boxes = sc
            .items
            .iter()
            .filter(|i| {
                matches!(i, Item::Path { title: Some(t), fill: Some(_), .. } if t.starts_with("ov"))
            })
            .count();
        assert_eq!(
            boxes, 500,
            "at height {height}: a feature box was dropped, not just its label"
        );
        // And what was drawn fits the scene that claims to hold it.
        let (_, _, _, hi_y) = extents(&sc);
        assert!(
            hi_y <= sc.height + 0.01,
            "at height {height}: reaches {hi_y}"
        );
    }
}

/// A canvas too short for the figure at all: it comes back TALLER than the
/// budget rather than losing its ruler.
///
/// [`crate::linear::scene`]'s doc says this in words, and it is the one place
/// the height is allowed to be exceeded. The other reading — crop to `height` —
/// takes the scale off the bottom of the drawing in silence, which is the one
/// thing this crate refuses to do anywhere else. `Options::height` is a budget
/// on the label rows and on nothing else; the caption, the band and the scale
/// are not negotiable.
#[test]
fn a_height_too_small_for_the_figure_loses_labels_and_never_the_scale() {
    let mut m = plasmid(3000, false);
    for i in 0..12u64 {
        m.features
            .push(feat(&format!("f{i}"), "CDS", i * 240 + 1, i * 240 + 180));
    }
    let ruler_size = 12.0 * 0.72;
    // The irreducible figure — caption, band, ruler, not one row of labels —
    // measured rather than written down, so this does not have to be re-edited
    // every time a type size moves.
    let (floor, floor_report) = scene(
        &m,
        Options {
            height: 0.0,
            ..Default::default()
        },
    );
    assert_eq!(floor_report.labels_placed, 0);
    assert!(floor.height > 0.0, "a zero budget produced a zero figure");
    for height in [1.0_f64, 20.0, 40.0, 80.0, 120.0] {
        let (sc, report) = scene(
            &m,
            Options {
                height,
                ..Default::default()
            },
        );
        assert_eq!(
            sc.height, floor.height,
            "at a {height} pt budget the figure is not the irreducible one"
        );
        if height < floor.height {
            assert!(
                sc.height > height,
                "a {height} pt budget cropped the figure to {}",
                sc.height
            );
        }
        // The scale survived: the ruler's numbers are the run of text at 0.72 em.
        let ticks = sc
            .items
            .iter()
            .filter(|i| matches!(i, Item::Text { size, .. } if (*size - ruler_size).abs() < 1e-9))
            .count();
        assert!(ticks > 0, "at {height} pt the ruler was cropped off");
        // What did go is named, and all of it went: there is no room for a row.
        assert_eq!(report.labels_placed + report.labels_hidden.len(), 12);
        assert_eq!(
            report.labels_placed, 0,
            "there was room for a label at {height} pt after all"
        );
        let (_, _, _, hi_y) = extents(&sc);
        assert!(hi_y <= sc.height + 0.01, "at {height}: reaches {hi_y}");
    }
}

/// A 200-character feature name, at every width the app and the command line
/// offer.
///
/// The long tail of `a_name_the_figure_keeps_whole_is_a_name_the_figure_can_draw`,
/// which sweeps 1 to 80 characters: a name this long is shortened at every width,
/// so what is checked here is the other half of the contract — a shortened name
/// is REPORTED as shortened, is still drawn, keeps its full text somewhere the
/// reader can reach, and pushes no glyph past the canvas edge on the way.
#[test]
fn a_two_hundred_character_name_is_shortened_reported_and_still_fits() {
    let name = "N".repeat(200);
    for w in [120.0_f64, 200.0, 300.0, 720.0, 1600.0] {
        let mut m = plasmid(3000, false);
        m.features.push(feat(&name, "CDS", 100, 900));
        m.features.push(feat("short", "CDS", 1_500, 1_600));
        let opts = Options {
            width: w,
            ..Default::default()
        };
        let (sc, report) = scene(&m, opts.clone());
        assert_eq!(report.labels_placed, 2, "at {w} pt a label was not drawn");
        assert!(report.labels_hidden.is_empty(), "at {w} pt");
        assert_eq!(
            report.labels_truncated,
            vec![name.clone()],
            "at {w} pt a 200-character name was drawn whole, or cut without saying so"
        );
        let (lo_x, hi_x, lo_y, hi_y) = extents(&sc);
        assert!(
            lo_x >= -0.01 && hi_x <= sc.width + 0.01,
            "at {w} pt: {lo_x}..{hi_x}"
        );
        assert!(
            lo_y >= -0.01 && hi_y <= sc.height + 0.01,
            "at {w} pt: {lo_y}..{hi_y}"
        );
        // The whole name survives where a reader can still reach it, which is
        // what makes shortening the drawn one acceptable.
        let (svg, _) = map_svg(&m, opts);
        assert!(
            svg.contains(&name),
            "at {w} pt the full name is nowhere in the svg"
        );
        well_formed(&svg).expect("malformed svg");
    }
}

/// Where a shortened feature name actually survives, writer by writer.
///
/// The asymmetry `widest_of` argues for — shorten a feature name, never a cut
/// coordinate — rests on the full name surviving somewhere a reader can reach.
/// That premise was overstated in two comments, in the same words: "in the SVG
/// `<title>`, in the PDF annotation and in the app's Features tab". There is no
/// PDF annotation. `pdf.rs`'s own module doc has always said so — an annotation
/// "would be furniture in a figure" — and the writer emits no `/Annots` at all.
///
/// So this measures it rather than restating it, on the figure that needs the
/// argument most. The conclusion survives on the true premise: the loss is
/// still REPORTED, in `labels_truncated`, which is what makes it different in
/// kind from a shortened `EcoRI  402`.
#[test]
fn a_shortened_name_survives_in_the_writers_that_can_carry_it_and_no_others() {
    let name = "a-feature-with-a-name-far-too-long-for-any-of-this";
    let mut m = plasmid(2000, false);
    m.features.push(feat(name, "CDS", 100, 900));
    let opts = Options {
        width: 260.0,
        ..Default::default()
    };
    let (sc, report) = scene(&m, opts.clone());
    assert_eq!(
        report.labels_truncated,
        vec![name.to_string()],
        "the name was not shortened, so there is nothing to trace"
    );

    // SVG: a real `<title>`, which a browser shows on hover.
    let (svg, _) = map_svg(&m, opts.clone());
    assert!(
        svg.contains(&format!("<title>{name}</title>")),
        "the whole name is not in the SVG"
    );
    // EPS: a PostScript comment. Nothing renders it and the text is there, so
    // `eps.rs`'s "the information survives in the file for a human reading it"
    // is exact.
    let (eps, _) = eps::to_eps(&sc, 1.0);
    assert!(
        eps.contains(&format!("% {name}")),
        "the whole name is not in the EPS"
    );
    // PDF: nowhere. No annotation array, and the drawn string is the shortened
    // one. Searched over the raw bytes, since a PDF string is not UTF-8 text.
    let (pdf, _, _) = map_pdf(&m, opts.clone());
    assert!(
        !pdf.windows(name.len()).any(|w| w == name.as_bytes()),
        "the PDF carries the whole name after all -- the comments that said so \
         were right and this test is the thing that is wrong"
    );
    assert!(
        !pdf.windows(7).any(|w| w == b"/Annots"),
        "the PDF grew an annotation array"
    );
    // And the shortened form IS drawn, so the name was not simply dropped.
    let drawn: Vec<&String> = sc
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Text { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    // Three ASCII dots, not U+2026: `fit_label` uses "..." so the mark survives
    // the WinAnsi round trip every one of these writers puts text through, and
    // an ellipsis that encoded as a missing glyph would be reported as
    // unencodable rather than drawn.
    assert!(
        drawn
            .iter()
            .any(|t| t.ends_with("...") && name.starts_with(t.trim_end_matches('.'))),
        "no shortened form of the name is on the figure: {drawn:?}"
    );
}

/// The crate header may not guarantee for the whole crate what [`raster`]
/// declines for itself.
///
/// PROVEN TO FAIL at c44757b, on both halves at once. `lib.rs`'s
/// "# What is guaranteed" section read "**Byte-identical output for identical
/// input**, on every platform" with no carve-out and no mention of the raster,
/// and `Cargo.toml` repeated the unqualified form in the description a package
/// index would publish:
///
/// ```text
/// the guarantee section never mentions the raster, whose own header declines
/// the cross-platform claim this section makes for the whole crate
/// ```
///
/// `include_str!` rather than a file read: the paths resolve at compile time
/// relative to this file, so the test cannot pass by failing to find a file.
/// The same reason `pl-features`' README check gives.
#[test]
fn the_crate_header_does_not_promise_what_the_raster_declines() {
    const LIB: &str = include_str!("lib.rs");
    const RASTER: &str = include_str!("raster.rs");
    const MANIFEST: &str = include_str!("../Cargo.toml");
    // The one cross-process check this project has. Included, not merely cited:
    // a citation nobody can follow is how the paragraph this test guards went
    // wrong in the first place, so renaming that test has to turn this red.
    const CLI: &str = include_str!("../../../bins/pl/tests/cli.rs");

    // The premise, read out of `raster.rs` rather than written down here. If
    // the raster ever buys cross-platform identity — a deterministic trig
    // module is the open decision in its own header — this line is what says
    // the guarantee section may be widened again.
    assert!(
        RASTER.contains("does not claim byte-identical output across platforms"),
        "raster.rs no longer declines the cross-platform claim; the paragraph \
         in lib.rs that this test constrains was written around that sentence"
    );

    let heading = "//! # What is guaranteed";
    let at = LIB
        .find(heading)
        .expect("lib.rs still has a guarantee section");
    let after = &LIB[at + heading.len()..];
    let section = &after[..after.find("\n//! # ").unwrap_or(after.len())];
    assert!(
        section.contains("raster.rs"),
        "the guarantee section says nothing about raster.rs, whose own header \
         declines the cross-platform claim this section makes for the whole \
         crate: {section}"
    );
    for name in [
        "the_linear_figure_is_byte_identical_for_identical_input",
        "two_processes_render_the_same_molecule_to_the_same_bytes",
    ] {
        assert!(
            section.contains(name),
            "the guarantee section does not say where the reader can check it: \
             {name} is missing"
        );
    }
    // And the two named checks exist where the section says they do.
    assert!(
        THIS_FILE.contains("fn the_linear_figure_is_byte_identical_for_identical_input"),
        "the header cites a test this file no longer has"
    );
    assert!(
        CLI.contains("fn two_processes_render_the_same_molecule_to_the_same_bytes"),
        "the header cites a test bins/pl/tests/cli.rs no longer has"
    );

    assert!(
        !MANIFEST.contains("byte-identical across platforms"),
        "the package description repeats, to anyone reading the manifest, the \
         unqualified claim raster.rs declines"
    );
}

/// This file, for the citation checks in
/// [`the_crate_header_does_not_promise_what_the_raster_declines`]. A named
/// constant because `include_str!("tests.rs")` written inside `tests.rs` reads
/// the file from disk at compile time rather than recursing, which is easy to
/// misread as the latter.
const THIS_FILE: &str = include_str!("tests.rs");
