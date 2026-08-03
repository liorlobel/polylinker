//! A [`Scene`] as a one-page PDF.
//!
//! # Why this is hand-written
//!
//! `docs/PLAN.md` names `resvg`/`svg2pdf`. Those convert *someone else's* SVG,
//! and we do not have someone else's SVG — we have the geometry that produced
//! ours. Rendering the same [`Scene`] to PDF is a few hundred lines and no
//! dependencies; going through SVG would mean serialising a picture, parsing it
//! back, and pulling in a font-shaping stack to do it. It also keeps the two
//! outputs from drifting, because there is only one geometry.
//!
//! A plasmid map needs exactly three things from PDF: filled and stroked paths,
//! cubic Béziers, and text in two weights of one face. It needs no images, no
//! transparency, no shading and no layers.
//!
//! # The font
//!
//! **Helvetica, one of the fourteen fonts every PDF viewer must provide**, so
//! nothing is embedded and no font licence is involved. The cost is
//! `WinAnsiEncoding`: a feature name in Greek or Chinese has no glyph. Rather
//! than emit a mangled byte, [`encode`] replaces what it cannot represent and
//! [`Report::unencodable`] names every string it had to touch — the same rule
//! as everywhere else here, that loss is reported rather than silent.
//!
//! Widths are needed because PDF has no `text-anchor` and a centred title must
//! be centred by measurement. They were **derived, not recalled**: taken from
//! PyMuPDF's base-14 Helvetica and independently from Arial's `hmtx` table via
//! fontTools, which agreed on all 95 printable ASCII characters.
//!
//! **There are two tables, because there are two fonts.** The centre title is
//! drawn in Helvetica-Bold, and measuring it with the regular widths is not a
//! near-enough approximation: "pcDNA3.1(+)-mCherry-WPRE" is 13306/1000 em
//! regular and 13751/1000 em bold, so a title centred on the regular
//! measurement sat 3.34 pt right of centre at 15 pt while the SVG — which has a
//! real font and a real `text-anchor` — centred it properly. With
//! `Anchor::End` the whole 6.7 pt difference lands in the label's position.
//!
//! # What is not carried across
//!
//! SVG `<title>` tooltips. PDF has no equivalent short of annotations, which
//! would be furniture in a figure. Stated here rather than quietly dropped.

use crate::scene::{arc_to_beziers, Anchor, Item, Scene, Seg};

/// Advance widths for Helvetica, U+0020..U+007E, in 1/1000 em.
///
/// Derived from two independent sources that agreed exactly: PyMuPDF's
/// built-in base-14 metrics, and Arial's `hmtx` scaled from 2048 units/em.
/// Arial is metrically compatible with Helvetica by design, which is why the
/// agreement is meaningful rather than circular.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, //  !"#$%&'()
    389, 584, 278, 333, 278, 278, 556, 556, 556, 556, // *+,-./0123
    556, 556, 556, 556, 556, 556, 278, 278, 584, 584, // 456789:;<=
    584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, // >?@ABCDEFG
    722, 278, 500, 667, 556, 833, 722, 778, 667, 778, // HIJKLMNOPQ
    722, 667, 611, 722, 667, 944, 667, 667, 611, 278, // RSTUVWXYZ[
    278, 278, 469, 556, 333, 556, 556, 500, 556, 556, // \]^_`abcde
    278, 556, 556, 222, 222, 500, 222, 833, 556, 556, // fghijklmno
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, // pqrstuvwxy
    500, 334, 260, 334, 584, // z{|}~
];

/// Advance widths for Helvetica-Bold, U+0020..U+007E, in 1/1000 em.
///
/// The Adobe base-14 `Helvetica-Bold.afm` values, cross-checked against Arial
/// Bold's `hmtx` the same way the regular table was. Bold is not the regular
/// table scaled: `c` is 500 regular and 556 bold, `r` is 333 and 389, `y` is
/// 500 and 556, while every digit is 556 in both — so no single factor
/// reproduces it and the second table has to be a table.
const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, //  !"#$%&'()
    389, 584, 278, 333, 278, 278, 556, 556, 556, 556, // *+,-./0123
    556, 556, 556, 556, 556, 556, 333, 333, 584, 584, // 456789:;<=
    584, 611, 975, 722, 722, 722, 722, 667, 611, 778, // >?@ABCDEFG
    722, 278, 556, 722, 611, 833, 722, 778, 667, 778, // HIJKLMNOPQ
    722, 667, 611, 722, 667, 944, 667, 667, 611, 333, // RSTUVWXYZ[
    278, 333, 584, 556, 333, 556, 611, 556, 611, 556, // \]^_`abcde
    333, 611, 611, 278, 278, 556, 278, 889, 611, 611, // fghijklmno
    611, 611, 389, 556, 333, 611, 556, 778, 556, 556, // pqrstuvwxy
    500, 389, 280, 389, 584, // z{|}~
];

/// The width of one WinAnsi byte, in 1/1000 em.
///
/// Outside printable ASCII this is an estimate, and deliberately a middling one
/// rather than zero: a wrong width nudges a label, a zero width stacks glyphs.
fn width_of(b: u8, bold: bool) -> f64 {
    let table = if bold { &HELVETICA_BOLD } else { &HELVETICA };
    match b {
        0x20..=0x7E => table[(b - 0x20) as usize] as f64,
        _ => 556.0,
    }
}

/// How wide a string will be, in points, in the regular weight.
pub fn text_width(s: &str, size: f64) -> f64 {
    text_width_in(s, size, false)
}

/// How wide a string will be, in points, in the weight it is actually drawn in.
///
/// Measuring bold text with the regular table and then drawing it in
/// Helvetica-Bold is how the centre title came to sit off centre; every
/// anchored string must be measured in the font that will render it.
pub fn text_width_in(s: &str, size: f64, bold: bool) -> f64 {
    encode(s).0.iter().map(|&b| width_of(b, bold)).sum::<f64>() * size / 1000.0
}

/// How far below the visual middle of the glyphs the alphabetic baseline sits,
/// as a fraction of the type size.
///
/// The scene's `y` for a string is the middle of its glyphs, matching SVG's
/// `dominant-baseline: middle`; PDF and PostScript both position the alphabetic
/// baseline instead. Helvetica's x-height is 0.523 em and half of it is the
/// conventional centre, so this is 0.2615.
///
/// **One constant, because three formats that disagree about it are three
/// different figures.** The EPS writer used to carry its own 0.36 next to a
/// comment claiming it was "the same 0.36 the PDF writer uses" — it was not,
/// and every label in an EPS export sat 0.0985 em (1.18 pt at size 12) below
/// where the PDF and the SVG put it.
pub const BASELINE_DROP_EM: f64 = 0.523 / 2.0;

/// A string as WinAnsi bytes, and whether anything had to be replaced.
///
/// Latin-1 passes through, since WinAnsi agrees with it over that range. The
/// handful of WinAnsi characters that live in Latin-1's control block —
/// typographic quotes, the dash family, the bullet — are mapped, because they
/// turn up constantly in feature names copied out of papers. Everything else
/// becomes `?` and is reported.
pub fn encode(s: &str) -> (Vec<u8>, bool) {
    let mut out = Vec::with_capacity(s.len());
    let mut lost = false;
    for c in s.chars() {
        let b = match c {
            '\u{20AC}' => 0x80,
            '\u{201A}' => 0x82,
            '\u{0192}' => 0x83,
            '\u{201E}' => 0x84,
            '\u{2026}' => 0x85,
            '\u{2020}' => 0x86,
            '\u{2021}' => 0x87,
            '\u{02C6}' => 0x88,
            '\u{2030}' => 0x89,
            '\u{0160}' => 0x8A,
            '\u{2039}' => 0x8B,
            '\u{0152}' => 0x8C,
            '\u{017D}' => 0x8E,
            '\u{2018}' => 0x91,
            '\u{2019}' => 0x92,
            '\u{201C}' => 0x93,
            '\u{201D}' => 0x94,
            '\u{2022}' => 0x95,
            '\u{2013}' => 0x96,
            '\u{2014}' => 0x97,
            '\u{02DC}' => 0x98,
            '\u{2122}' => 0x99,
            '\u{0161}' => 0x9A,
            '\u{203A}' => 0x9B,
            '\u{0153}' => 0x9C,
            '\u{017E}' => 0x9E,
            '\u{0178}' => 0x9F,
            // Control characters have no glyph and would be a malformed
            // string; drop them exactly as the SVG writer does.
            c if (c as u32) < 0x20 => continue,
            c if (c as u32) == 0x7F => continue,
            c if (c as u32) < 0x100 => c as u8,
            _ => {
                lost = true;
                b'?'
            }
        };
        out.push(b);
    }
    (out, lost)
}

/// What the PDF could not carry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Strings holding characters Helvetica's WinAnsi encoding cannot express.
    ///
    /// Reported rather than silently mangled: a feature named in Greek would
    /// otherwise appear as a row of question marks with nothing to say why.
    pub unencodable: Vec<String>,
}

/// Round for the content stream. Two decimals, as everywhere else here.
fn n(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    let r = if r == 0.0 { 0.0 } else { r };
    format!("{r}")
}

/// `#rrggbb` and friends as 0..1 components, for PDF and for EPS alike.
///
/// Falls back to black rather than failing, because a black feature is better
/// than no figure. That fallback is not as narrow as it looks: `safe_color`
/// admits `rgb()`, `rgba()`, `hsl()` and `hsla()` on purpose, and none of them
/// is parsed here, so a map coloured by another tool in functional notation
/// comes out black in both vector formats while the SVG shows it correctly.
/// That is a known gap, written down rather than implied by a comment claiming
/// "anything else is a named CSS colour" — which is what both writers used to
/// say, and it was false.
///
/// **Shared, because two copies drifted.** The EPS writer kept its own version
/// that matched only 3 and 6 hex digits, so a feature carrying
/// `#4f7fd0ff` — 8-digit hex with alpha, exactly what a SnapGene `.dna`
/// segment can hold and what `safe_color` passes through unnormalised — drew
/// blue in the SVG, blue in the PDF and solid black in the EPS the author sent
/// to the journal.
///
/// The alpha nibble is measured and discarded: PDF and PostScript both need a
/// graphics state or a transparency group to honour it, and a figure is opaque.
pub(crate) fn rgb(colour: &str) -> (f64, f64, f64) {
    let hex = colour.strip_prefix('#').unwrap_or("");
    // `get`, not `&hex[a..b]`: the length is a *byte* count, and a two-byte
    // character would put the slice boundary inside it and panic.
    let v = |a: usize, b: usize| -> f64 {
        u8::from_str_radix(hex.get(a..b).unwrap_or("0"), 16).unwrap_or(0) as f64 / 255.0
    };
    match hex.len() {
        6 | 8 => (v(0, 2), v(2, 4), v(4, 6)),
        3 | 4 => {
            let d = |i: usize| {
                let x = u8::from_str_radix(hex.get(i..i + 1).unwrap_or("0"), 16).unwrap_or(0);
                (x * 17) as f64 / 255.0
            };
            (d(0), d(1), d(2))
        }
        _ => match colour {
            "white" => (1.0, 1.0, 1.0),
            "red" => (1.0, 0.0, 0.0),
            _ => (0.0, 0.0, 0.0),
        },
    }
}

/// Escape a byte string for a PDF literal string.
fn pdf_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('(');
    for &b in bytes {
        match b {
            b'(' => out.push_str("\\("),
            b')' => out.push_str("\\)"),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7E => out.push(b as char),
            // Anything outside printable ASCII goes as a three-digit octal
            // escape, which is always safe and never depends on the reader's
            // handling of raw high bytes.
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out.push(')');
    out
}

/// Render a scene as a complete PDF document.
///
/// One page, one font, no compression — the content stream stays readable, so
/// `strings map.pdf` shows the drawing commands and a reviewer can check the
/// file without a library. A plasmid map is tens of kilobytes either way.
/// The scene as PDF, at a physical width.
///
/// `Some(mm)` sets the MediaBox to that size in points and prefixes the content
/// stream with a `cm` scale matrix, so every coordinate below is written exactly
/// as it always was and the PAGE is what changes. Rewriting the numbers instead
/// would round a thousand of them, and this file's whole geometry is checked
/// against the SVG's by `tests/agreement.rs` — two renderers that round
/// differently stop agreeing.
///
/// `None` is what [`to_pdf`] passes, and is byte-identical to what this crate
/// has always emitted: every existing test passes untouched, which is the proof
/// that adding the option moved nothing.
pub fn pdf_at(scene: &Scene, width_mm: Option<f64>) -> (Vec<u8>, Report) {
    let fit = width_mm.map(|mm| crate::page::Fit::to_width_mm(scene, mm));
    let scale = fit.as_ref().map(|f| f.scale).unwrap_or(1.0);
    let mut report = Report::default();
    let h = scene.height;
    // PDF's origin is bottom-left with y up; the scene is top-left with y down.
    let fy = |y: f64| h - y;

    let mut c = String::new();
    c.push_str("q\n");
    if let Some(f) = &fit {
        // The whole of the physical sizing, in one matrix.
        c.push_str(&format!("{} 0 0 {} 0 0 cm\n", n(f.scale), n(f.scale)));
    }
    // Round caps and joins. The comment here used to claim this matched "the
    // SVG default look" and it did not: SVG's initial values are `butt` caps
    // and `miter` joins with a limit of 4, so every stroked join in the PDF was
    // rounded and the same join in the SVG was mitred — two renderings of one
    // scene, differing at every corner. The SVG root now states round
    // explicitly, and this line is what it is matching.
    c.push_str("1 J 1 j\n");

    for item in &scene.items {
        match item {
            Item::Circle {
                cx,
                cy,
                r,
                stroke,
                stroke_width,
            } => {
                let (rr, gg, bb) = rgb(stroke);
                c.push_str(&format!(
                    "{} {} {} RG\n{} w\n",
                    n(rr),
                    n(gg),
                    n(bb),
                    n(*stroke_width)
                ));
                let (sx, sy) = crate::scene::on_circle(*cx, *cy, *r, 0.0);
                c.push_str(&format!("{} {} m\n", n(sx), n(fy(sy))));
                for s in arc_to_beziers(*cx, *cy, *r, 0.0, std::f64::consts::TAU) {
                    c.push_str(&format!(
                        "{} {} {} {} {} {} c\n",
                        n(s[0]),
                        n(fy(s[1])),
                        n(s[2]),
                        n(fy(s[3])),
                        n(s[4]),
                        n(fy(s[5]))
                    ));
                }
                c.push_str("h S\n");
            }
            Item::Path {
                segs,
                fill,
                stroke,
                stroke_width,
                ..
            } => {
                if let Some(f) = fill {
                    let (rr, gg, bb) = rgb(f);
                    c.push_str(&format!("{} {} {} rg\n", n(rr), n(gg), n(bb)));
                }
                if let Some(s) = stroke {
                    let (rr, gg, bb) = rgb(s);
                    c.push_str(&format!(
                        "{} {} {} RG\n{} w\n",
                        n(rr),
                        n(gg),
                        n(bb),
                        n(*stroke_width)
                    ));
                }
                let mut cursor = (0.0, 0.0);
                let mut closed = false;
                for seg in segs {
                    match *seg {
                        Seg::Move(x, y) => {
                            c.push_str(&format!("{} {} m\n", n(x), n(fy(y))));
                            cursor = (x, y);
                        }
                        Seg::Line(x, y) => {
                            c.push_str(&format!("{} {} l\n", n(x), n(fy(y))));
                            cursor = (x, y);
                        }
                        Seg::Arc {
                            cx,
                            cy,
                            r,
                            from,
                            to,
                        } => {
                            // The arc's own start may differ from the cursor by
                            // rounding; PDF has no arc, so simply continue from
                            // where we are.
                            let _ = cursor;
                            for s in arc_to_beziers(cx, cy, r, from, to) {
                                c.push_str(&format!(
                                    "{} {} {} {} {} {} c\n",
                                    n(s[0]),
                                    n(fy(s[1])),
                                    n(s[2]),
                                    n(fy(s[3])),
                                    n(s[4]),
                                    n(fy(s[5]))
                                ));
                            }
                            cursor = crate::scene::on_circle(cx, cy, r, to);
                        }
                        Seg::Close => {
                            c.push_str("h\n");
                            closed = true;
                        }
                    }
                }
                let _ = closed;
                c.push_str(match (fill.is_some(), stroke.is_some()) {
                    (true, true) => "B\n",
                    (true, false) => "f\n",
                    (false, true) => "S\n",
                    (false, false) => "n\n",
                });
            }
            Item::Text {
                x,
                y,
                size,
                anchor,
                color,
                bold,
                text,
            } => {
                let (bytes, lost) = encode(text);
                if lost {
                    report.unencodable.push(text.clone());
                }
                if bytes.is_empty() {
                    continue;
                }
                // Measured in the weight it is drawn in, three lines down.
                let w = bytes.iter().map(|&b| width_of(b, *bold)).sum::<f64>() * size / 1000.0;
                let tx = match anchor {
                    Anchor::Start => *x,
                    Anchor::Middle => x - w / 2.0,
                    Anchor::End => x - w,
                };
                // The scene's `y` is the visual middle of the glyphs, matching
                // SVG's `dominant-baseline: middle`. PDF positions the
                // baseline, so drop by half the x-height — see
                // [`BASELINE_DROP_EM`], which the EPS writer shares.
                let baseline = fy(*y) - size * BASELINE_DROP_EM;
                let (rr, gg, bb) = rgb(color);
                c.push_str(&format!(
                    "BT\n/{} {} Tf\n{} {} {} rg\n{} {} Td\n{} Tj\nET\n",
                    if *bold { "F2" } else { "F1" },
                    n(*size),
                    n(rr),
                    n(gg),
                    n(bb),
                    n(tx),
                    n(baseline),
                    pdf_string(&bytes)
                ));
            }
        }
    }
    c.push_str("Q\n");

    let content = c.into_bytes();
    let (title_bytes, title_lost) = encode(&scene.title);
    if title_lost {
        report.unencodable.push(scene.title.clone());
    }

    // --- assemble the file ------------------------------------------------
    //
    // Seven objects, written in order, with a cross-reference table of their
    // byte offsets. The offsets must be exact: a viewer seeks by them, and one
    // wrong number is a file that will not open.
    let mut out: Vec<u8> = Vec::with_capacity(content.len() + 2048);
    out.extend_from_slice(b"%PDF-1.7\n");
    // A comment of high bytes, so tools that sniff text-vs-binary get it right.
    out.extend_from_slice(b"%\xE2\xE3\xCF\xD3\n");

    let mut offsets: Vec<usize> = Vec::new();
    let obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(out.len());
        let i = offsets.len();
        out.extend_from_slice(format!("{i} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };

    obj(&mut out, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(
        &mut out,
        &mut offsets,
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    );
    obj(
        &mut out,
        &mut offsets,
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] \
             /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> /Contents 4 0 R >>",
            n(scene.width * scale),
            n(scene.height * scale)
        )
        .as_bytes(),
    );
    {
        offsets.push(out.len());
        out.extend_from_slice(b"4 0 obj\n");
        out.extend_from_slice(format!("<< /Length {} >>\nstream\n", content.len()).as_bytes());
        out.extend_from_slice(&content);
        out.extend_from_slice(b"endstream\nendobj\n");
    }
    obj(
        &mut out,
        &mut offsets,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    );
    obj(
        &mut out,
        &mut offsets,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>",
    );
    obj(
        &mut out,
        &mut offsets,
        format!(
            "<< /Title {} /Producer (Polylinker) >>",
            pdf_string(&title_bytes)
        )
        .as_bytes(),
    );

    let startxref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", offsets.len() + 1).as_bytes());
    // Entry zero is the head of the free list, and its form is fixed.
    out.extend_from_slice(b"0000000000 65535 f \n");
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 7 0 R >>\nstartxref\n{startxref}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    (out, report)
}

/// The scene as PDF at its own scene units. See [`pdf_at`].
pub fn to_pdf(scene: &Scene) -> (Vec<u8>, Report) {
    pdf_at(scene, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_width_table_matches_helveticas_published_metrics() {
        // Spot checks against values that are the same in every Helvetica and
        // Arial metric file. If the table were shifted by one, these would be
        // the first casualties.
        assert_eq!(width_of(b' ', false), 278.0);
        assert_eq!(width_of(b'A', false), 667.0);
        assert_eq!(width_of(b'W', false), 944.0);
        assert_eq!(width_of(b'i', false), 222.0);
        assert_eq!(width_of(b'l', false), 222.0);
        assert_eq!(width_of(b'1', false), 556.0);
        assert_eq!(width_of(b'.', false), 278.0);
        assert_eq!(width_of(b'@', false), 1015.0);
        assert_eq!(width_of(b'~', false), 584.0);
        assert_eq!(HELVETICA.len(), 95, "U+0020..U+007E inclusive");
    }

    #[test]
    fn the_bold_width_table_matches_helvetica_bolds_published_metrics() {
        // The same spot checks against `Helvetica-Bold.afm`. A table shifted by
        // one, or the regular table copied, shows up here first.
        assert_eq!(width_of(b' ', true), 278.0);
        assert_eq!(width_of(b'A', true), 722.0);
        assert_eq!(width_of(b'W', true), 944.0);
        assert_eq!(width_of(b'i', true), 278.0);
        assert_eq!(width_of(b'l', true), 278.0);
        assert_eq!(width_of(b'1', true), 556.0);
        assert_eq!(width_of(b'.', true), 278.0);
        assert_eq!(width_of(b'@', true), 975.0);
        assert_eq!(width_of(b'~', true), 584.0);
        assert_eq!(HELVETICA_BOLD.len(), 95, "U+0020..U+007E inclusive");
        // Bold is wider than regular for the letters and the same for the
        // digits, which is what makes it a second table rather than a factor.
        for c in b'a'..=b'z' {
            assert!(
                width_of(c, true) >= width_of(c, false),
                "bold {} is narrower than regular",
                c as char
            );
        }
        for c in b'0'..=b'9' {
            assert_eq!(width_of(c, true), width_of(c, false), "{}", c as char);
        }
    }

    #[test]
    fn a_bold_string_is_measured_in_bold_not_in_the_regular_weight() {
        // The map's centre title is bold and `Anchor::Middle`, so half of any
        // measurement error moves it off centre. This is the title of a real
        // plasmid: 13306/1000 em regular, 13751/1000 em bold, which is 3.34 pt
        // of offset at the 15 pt the title is drawn at.
        let name = "pcDNA3.1(+)-mCherry-WPRE";
        let regular = text_width_in(name, 1000.0, false);
        let bold = text_width_in(name, 1000.0, true);
        assert!((regular - 13306.0).abs() < 1e-9, "{regular}");
        assert!((bold - 13751.0).abs() < 1e-9, "{bold}");
        // The default entry point stays the regular one.
        assert_eq!(text_width(name, 15.0), text_width_in(name, 15.0, false));
        let half = (bold - regular) / 2000.0 * 15.0;
        assert!((half - 3.3375).abs() < 1e-4, "{half}");
    }

    #[test]
    fn a_centred_bold_title_is_centred_on_the_glyphs_that_are_drawn() {
        // The whole point, at the emitter: the `Td` x for a bold Middle-anchored
        // string must be `x - bold_width/2`, not `x - regular_width/2`. It was
        // the latter, and the title sat 3.34 pt right of centre in every PDF
        // while the SVG -- which has a real font and a real text-anchor --
        // centred it correctly.
        let name = "pcDNA3.1(+)-mCherry-WPRE";
        let sc = Scene {
            width: 720.0,
            height: 720.0,
            title: name.into(),
            items: vec![Item::Text {
                x: 360.0,
                y: 356.0,
                size: 15.0,
                anchor: Anchor::Middle,
                color: "#16191c".into(),
                bold: true,
                text: name.into(),
            }],
        };
        let (bytes, _) = to_pdf(&sc);
        let text = String::from_utf8_lossy(&bytes);
        let td = text.find(" Td").expect("Td");
        let line: &str = text[..td].rsplit('\n').next().unwrap();
        let x: f64 = line.split_whitespace().next().unwrap().parse().unwrap();
        let want = 360.0 - text_width_in(name, 15.0, true) / 2.0;
        assert!(
            (x - want).abs() < 0.01,
            "centred at {x}, should be {want} (the regular table would say {})",
            360.0 - text_width_in(name, 15.0, false) / 2.0
        );
        // And the glyphs really do come from the bold font, or the measurement
        // would be the wrong one in the other direction.
        assert!(text.contains("/F2 15 Tf"), "the title is drawn in F2");
        assert!(text.contains("/BaseFont /Helvetica-Bold"));
    }

    #[test]
    fn measured_width_grows_with_the_string_and_scales_with_the_size() {
        assert!(text_width("iii", 12.0) < text_width("WWW", 12.0));
        let a = text_width("Polylinker", 12.0);
        let b = text_width("Polylinker", 24.0);
        assert!((b - 2.0 * a).abs() < 1e-9, "width must scale linearly");
        assert_eq!(text_width("", 12.0), 0.0);
    }

    #[test]
    fn characters_helvetica_cannot_encode_are_replaced_and_reported() {
        let (b, lost) = encode("AmpR");
        assert_eq!(b, b"AmpR");
        assert!(!lost);

        // Latin-1 passes through; the typographic set is mapped.
        let (b, lost) = encode("caf\u{e9} \u{2013} \u{201c}x\u{201d}");
        assert!(!lost, "Latin-1 and WinAnsi punctuation are representable");
        assert!(b.contains(&0xE9) && b.contains(&0x96) && b.contains(&0x93));

        // Greek is not, and must say so rather than emit nonsense.
        let (b, lost) = encode("\u{3b2}-galactosidase");
        assert!(lost);
        assert!(b.starts_with(b"?-gal"));

        // Control characters are dropped, as in the SVG writer.
        let (b, _) = encode("a\u{0}b");
        assert_eq!(b, b"ab");
    }

    #[test]
    fn a_literal_string_is_escaped_so_it_cannot_break_the_syntax() {
        // An unescaped bracket in a feature name would terminate the string
        // early and corrupt every object after it.
        assert_eq!(pdf_string(b"plain"), "(plain)");
        assert_eq!(pdf_string(b"a(b)c"), "(a\\(b\\)c)");
        assert_eq!(pdf_string(b"back\\slash"), "(back\\\\slash)");
        assert_eq!(pdf_string(&[0xE9]), "(\\351)");
    }

    #[test]
    fn colours_become_pdf_components() {
        assert_eq!(rgb("#000000"), (0.0, 0.0, 0.0));
        assert_eq!(rgb("#ffffff"), (1.0, 1.0, 1.0));
        let (r, g, b) = rgb("#ff8000");
        assert!((r - 1.0).abs() < 1e-9 && (g - 0.502).abs() < 0.01 && b == 0.0);
        // Short form expands the way CSS says.
        assert_eq!(rgb("#fff"), rgb("#ffffff"));
        assert_eq!(rgb("#000"), rgb("#000000"));
        // A name we do not know is black, never a panic.
        assert_eq!(rgb("rebeccapurple"), (0.0, 0.0, 0.0));
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;
    use crate::scene::{Anchor, Item, Scene, Seg};

    fn sample() -> Scene {
        Scene {
            width: 200.0,
            height: 100.0,
            title: "pTEST".into(),
            items: vec![
                Item::Circle {
                    cx: 50.0,
                    cy: 50.0,
                    r: 30.0,
                    stroke: "#33383d".into(),
                    stroke_width: 1.25,
                },
                Item::Path {
                    segs: vec![
                        Seg::Move(10.0, 10.0),
                        Seg::Arc {
                            cx: 50.0,
                            cy: 50.0,
                            r: 30.0,
                            from: 0.0,
                            to: 1.0,
                        },
                        Seg::Line(20.0, 20.0),
                        Seg::Close,
                    ],
                    fill: Some("#4f7fd0".into()),
                    stroke: Some("#2b2f34".into()),
                    stroke_width: 0.6,
                    title: Some("a feature".into()),
                },
                Item::Text {
                    x: 100.0,
                    y: 50.0,
                    size: 12.0,
                    anchor: Anchor::Middle,
                    color: "#16191c".into(),
                    bold: true,
                    text: "pTEST".into(),
                },
            ],
        }
    }

    /// Find a byte pattern, searching from the end.
    fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).rposition(|w| w == needle)
    }

    /// Every byte offset in the cross-reference table must land on its object.
    ///
    /// This is the difference between a file that opens and one that does not,
    /// and it is invisible to any check that only greps the bytes: a PDF with
    /// wrong offsets contains every string you would look for.
    ///
    /// Worked on **raw bytes**, never on `from_utf8_lossy`. The header carries
    /// a deliberate run of high bytes so tools treat the file as binary, and a
    /// lossy view turns each into a three-byte replacement character -- which
    /// shifts every offset after it and makes a correct file look broken. The
    /// first version of this test did exactly that and accused the writer.
    #[test]
    fn the_cross_reference_table_points_at_the_objects() {
        let (bytes, _) = to_pdf(&sample());

        let sx = rfind(&bytes, b"startxref").expect("startxref");
        let tail = std::str::from_utf8(&bytes[sx..]).expect("the trailer is ASCII");
        let start: usize = tail
            .lines()
            .nth(1)
            .expect("the offset is on the next line")
            .trim()
            .parse()
            .expect("startxref offset");
        assert!(
            bytes[start..].starts_with(
                b"xref
"
            ),
            "startxref is wrong"
        );

        let table = std::str::from_utf8(&bytes[start..]).expect("the table is ASCII");
        let mut lines = table.lines();
        assert_eq!(lines.next(), Some("xref"));
        let head = lines.next().expect("subsection header");
        let count: usize = head.split_whitespace().nth(1).unwrap().parse().unwrap();

        let free = lines.next().unwrap();
        assert!(free.starts_with("0000000000 65535 f"), "{free:?}");

        for i in 1..count {
            let entry = lines.next().unwrap_or_else(|| panic!("entry {i}"));
            let off: usize = entry[..10]
                .parse()
                .unwrap_or_else(|e| panic!("{entry:?}: {e}"));
            let want = format!("{i} 0 obj");
            assert!(
                bytes[off..].starts_with(want.as_bytes()),
                "object {i}: offset {off} lands on {:?}",
                String::from_utf8_lossy(&bytes[off..(off + 20).min(bytes.len())])
            );
        }
        assert!(rfind(&bytes, b"/Root 1 0 R").is_some());
        assert!(bytes.ends_with(
            b"%%EOF
"
        ));
    }

    #[test]
    fn the_declared_stream_length_is_the_real_one() {
        // A wrong /Length truncates the drawing in some readers and is accepted
        // by others, so the file appears to work until it does not.
        let (bytes, _) = to_pdf(&sample());
        let text = String::from_utf8_lossy(&bytes);
        let at = text.find("/Length ").expect("/Length");
        let declared: usize = text[at + 8..]
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let s = text.find("stream\n").unwrap() + 7;
        let e = text.find("\nendstream").unwrap();
        assert_eq!(
            declared,
            e - s + 1,
            "declared {declared}, actual {}",
            e - s + 1
        );
    }

    /// The SVG must ask for the typeface its own arithmetic measured.
    ///
    /// PROVEN TO FAIL against 759b272, and this is a real defect rather than a
    /// tidiness point. `drawn_width` measures every feature label with
    /// `pdf::text_width_in` — HELVETICA's advances — and the label that was
    /// shortened to fit, the `viewBox` it was cropped against and the
    /// `Anchor::End` placement all come from that number. The root element then
    /// asked for `system-ui, -apple-system, 'Segoe UI', Helvetica, …`, so a
    /// browser on Windows drew the whole figure in Segoe UI.
    ///
    /// The layout was therefore computed in one typeface and rendered in
    /// another, and the error runs the same direction as the `label_width`
    /// defect recorded beside it: a name that fitted when measured overflows
    /// when drawn. `pCMV-WPRE` at 12 pt is 73.33 pt in Helvetica, and the crate
    /// cropped the page to that.
    ///
    /// The three names are the metric-compatible chain — Nimbus Sans is the
    /// free clone on Linux and Arial is metrically compatible by design, which
    /// is what `pdf.rs` cross-checked its width tables against. Whichever a
    /// viewer resolves, the advances are the measured ones.
    #[test]
    fn the_svg_asks_for_the_typeface_its_own_measurements_describe() {
        let svg = crate::svg_of(&sample());
        let root = &svg[..svg.find('>').expect("a root element")];
        assert!(
            root.contains("font-family="),
            "the SVG names no typeface at all, so every viewer picks its own"
        );
        // The measured face first. Anything ahead of it is a face the layout
        // arithmetic knows nothing about.
        let fam = root
            .split("font-family=\"")
            .nth(1)
            .and_then(|r| r.split('"').next())
            .expect("a font-family value");
        assert!(
            fam.starts_with("Helvetica"),
            "the layout is measured in Helvetica and drawn in whatever comes first \
             here: {fam:?}"
        );
        for metric_compatible in ["Nimbus Sans", "Arial"] {
            assert!(
                fam.contains(metric_compatible),
                "{metric_compatible} is the fallback on a platform without Helvetica, and \
                 it is missing: {fam:?}"
            );
        }
        assert!(
            !fam.contains("system-ui") && !fam.contains("Segoe"),
            "a face whose advances nothing here measured is offered ahead of the \
             fallbacks: {fam:?}"
        );
    }

    /// The PDF and the SVG must join and cap their strokes the same way.
    ///
    /// PROVEN TO FAIL against 759b272: `pdf.rs` emitted `1 J 1 j` — round caps
    /// and joins — under a comment claiming it matched "the SVG default look".
    /// SVG's initial values are `butt` caps and `miter` joins with a limit of 4,
    /// so the comment was false and every stroked corner differed between two
    /// renderings of one scene. On a plasmid map that is every leader line's
    /// elbow and every arrowhead's point.
    ///
    /// Asserted on both outputs rather than on one, because the claim is that
    /// they agree; checking either alone would pass while they disagreed.
    #[test]
    fn the_two_vector_back_ends_stroke_their_corners_alike() {
        let sc = sample();
        let svg = crate::svg_of(&sc);
        let root = &svg[..svg.find('>').expect("a root element")];
        assert!(
            root.contains(r#"stroke-linecap="round""#)
                && root.contains(r#"stroke-linejoin="round""#),
            "the SVG leaves caps and joins at butt and miter while the PDF rounds them: \
             {root}"
        );
        let (bytes, _) = to_pdf(&sc);
        assert!(
            String::from_utf8_lossy(&bytes).contains("1 J 1 j"),
            "the PDF stopped rounding, so the two now differ the other way"
        );
    }

    /// A printed width must move ALL THREE formats, or the control lies.
    ///
    /// PROVEN TO FAIL against 7ba75bc: `to_eps` took a scale and `svg_of` and
    /// `to_pdf` did not, so a "printed width" control would have resized the
    /// EPS and left the SVG and the PDF at their scene units. A user setting
    /// "Nature, single column" and exporting a PDF would get a 720 pt square,
    /// and nothing on screen would say so — the defect class `docs/PLAN.md`
    /// item 33 is about, a control whose effect is invisible.
    ///
    /// 89 mm is Nature's single column. The assertions are on the PAGE — the
    /// SVG root's `width`, the PDF's MediaBox — because that is what a journal's
    /// submission system measures, and because the geometry inside deliberately
    /// does not move: the SVG keeps its `viewBox` and the PDF gets a `cm`
    /// matrix, so `tests/agreement.rs` still compares two renderers that round
    /// identically.
    #[test]
    fn a_printed_width_reaches_svg_and_pdf_and_not_only_eps() {
        let sc = sample();
        let mm = 89.0;
        let fit = crate::page::Fit::to_width_mm(&sc, mm);
        let want_pt = mm / crate::page::MM_PER_INCH * crate::page::PT_PER_INCH;
        assert!(
            (fit.width_pt - want_pt).abs() < 0.5,
            "the fixture's own arithmetic: {} vs {want_pt}",
            fit.width_pt
        );

        // SVG: a real size on the root, and the drawing untouched behind it.
        let svg = crate::svg_at(&sc, Some(mm));
        assert!(
            svg.contains(r#"width="89mm""#),
            "the SVG carries no physical width: {}",
            &svg[..svg.find('>').unwrap_or(200).min(svg.len())]
        );
        assert!(
            svg.contains(&format!(r#"viewBox="0 0 {} {}""#, sc.width, sc.height)),
            "the viewBox moved, so the geometry was rescaled rather than the page"
        );

        // PDF: the MediaBox in points, and a scale matrix rather than rewritten
        // coordinates.
        let (bytes, _) = pdf_at(&sc, Some(mm));
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("/MediaBox [0 0 252.28"),
            "the PDF page is not 89 mm wide"
        );
        assert!(
            text.contains(" 0 0 ") && text.contains(" cm"),
            "the PDF has no scale matrix, so its coordinates were rewritten"
        );

        // And the control: with no width asked for, both are exactly what this
        // crate has always emitted. That is what lets every other test in the
        // file stand unchanged as the proof.
        assert_eq!(crate::svg_at(&sc, None), crate::svg_of(&sc));
        assert_eq!(pdf_at(&sc, None).0, to_pdf(&sc).0);
    }

    #[test]
    fn every_arc_becomes_curves_and_the_page_is_the_scenes_size() {
        let (bytes, _) = to_pdf(&sample());
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/MediaBox [0 0 200 100]"), "page size");
        // PDF has no arc operator: an `A` surviving would mean the SVG form
        // leaked through.
        assert!(text.contains(" c\n"), "arcs must become Beziers");
        assert!(!text.contains(" re\n"), "no rectangles are emitted");
        assert!(text.contains("/Helvetica"), "the base-14 font");
        assert!(text.contains("Tj"), "text is drawn");
    }

    #[test]
    fn the_y_axis_is_flipped_exactly_once() {
        // The scene puts the title at y = 50 of a 100-high page, which is the
        // middle either way -- so the test uses an off-centre point, where a
        // missing or doubled flip is visible.
        let mut sc = sample();
        sc.items = vec![Item::Text {
            x: 0.0,
            y: 10.0,
            size: 10.0,
            anchor: Anchor::Start,
            color: "#000000".into(),
            bold: false,
            text: "T".into(),
        }];
        let (bytes, _) = to_pdf(&sc);
        let text = String::from_utf8_lossy(&bytes);
        let td = text.find(" Td").expect("Td");
        let line: &str = text[..td].rsplit('\n').next().unwrap();
        let y: f64 = line.split_whitespace().nth(1).unwrap().parse().unwrap();
        // 100 - 10 = 90, less about a quarter of the font size for the baseline.
        assert!(
            (85.0..=90.0).contains(&y),
            "a scene y of 10 on a 100-high page should be near 90 in PDF space, got {y}"
        );
    }

    #[test]
    fn a_name_that_helvetica_cannot_spell_is_reported() {
        let mut sc = sample();
        sc.items.push(Item::Text {
            x: 10.0,
            y: 10.0,
            size: 10.0,
            anchor: Anchor::Start,
            color: "#000000".into(),
            bold: false,
            text: "\u{3b2}-gal".into(),
        });
        let (_, report) = to_pdf(&sc);
        assert_eq!(report.unencodable, vec!["\u{3b2}-gal".to_string()]);
    }
}
