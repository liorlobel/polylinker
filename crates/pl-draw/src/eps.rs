//! Encapsulated PostScript, from the same [`Scene`] as the SVG and the PDF.
//!
//! # Why EPS is still here in 2026
//!
//! Because journals ask for it. Vector line art at a specified physical width
//! is what a figure of a plasmid map should be, and EPS is the format several
//! publishers' submission systems still name first. It costs one emitter,
//! shares the [`Scene`](crate::Scene) and the font metrics with
//! [`crate::pdf`], and removes a reason for somebody to open the figure in
//! Illustrator and re-save it — which is where figures acquire errors.
//!
//! # The three things an EPS writer has to get right
//!
//! **The BoundingBox is the contract.** `%%BoundingBox` in integer points is
//! what every consumer uses to place the figure, and it must *contain* the
//! artwork: a box smaller than the drawing crops it, silently, in the
//! typesetter's hands rather than on screen. The integer box here is rounded
//! outwards and an exact `%%HiResBoundingBox` is emitted beside it.
//!
//! **The y axis points up.** PostScript's origin is bottom-left; the scene's is
//! top-left. Emitting scene coordinates directly gives a figure that is
//! upside-down but otherwise perfect, which is the kind of wrong that survives
//! a quick look at a thumbnail.
//!
//! **Text has no `text-anchor`.** SVG centres a string declaratively.
//! PostScript must measure it, which is what [`crate::pdf::text_width_in`] is
//! for — the same metrics *and the same weight*, so a centred label lands in
//! the same place in all three formats. Both halves of that are load-bearing
//! and both were once wrong here: the baseline offset was a private 0.36
//! against the PDF's 0.2615, and bold strings were measured with the regular
//! widths. Colours go through [`crate::pdf::rgb`] for the same reason — the
//! copy that lived here understood fewer hex forms than the original.
//!
//! # What EPS cannot carry
//!
//! Tooltips. The scene's `title` fields become PostScript comments so the
//! information survives in the file for a human reading it, but nothing renders
//! them. That is stated rather than silently true.

use crate::pdf::{encode, rgb, text_width_in, Report, BASELINE_DROP_EM};
use crate::scene::{arc_to_beziers, Anchor, Item, Scene, Seg};

/// Render a scene as EPS at a physical size.
///
/// `scale` maps scene units to PostScript points; see [`crate::page::Fit`].
pub fn to_eps(scene: &Scene, scale: f64) -> (String, Report) {
    let mut rep = Report::default();
    let w = scene.width * scale;
    let h = scene.height * scale;
    let mut s = String::with_capacity(4096);

    s.push_str("%!PS-Adobe-3.0 EPSF-3.0\n");
    // Rounded outwards: a BoundingBox that does not contain the artwork crops
    // it in the typesetter's hands rather than on screen.
    s.push_str(&format!(
        "%%BoundingBox: 0 0 {} {}\n",
        w.ceil() as i64,
        h.ceil() as i64
    ));
    s.push_str(&format!("%%HiResBoundingBox: 0 0 {} {}\n", n(w), n(h)));
    s.push_str("%%Creator: Polylinker\n");
    s.push_str(&format!("%%Title: {}\n", ps_comment(&scene.title)));
    s.push_str("%%LanguageLevel: 2\n");
    s.push_str("%%EndComments\n");
    // A white background: an EPS with no paint is transparent, and a plasmid
    // map dropped on a coloured slide would otherwise show through.
    s.push_str(&format!(
        "gsave 1 1 1 setrgbcolor 0 0 {} {} rectfill grestore\n",
        n(w),
        n(h)
    ));
    s.push_str("1 setlinejoin 1 setlinecap\n");

    // PostScript's y axis points up and the scene's points down. Flipping once,
    // here, is the only place the two conventions meet.
    let ty = |y: f64| h - y * scale;
    let tx = |x: f64| x * scale;

    for item in &scene.items {
        match item {
            Item::Path {
                segs,
                fill,
                stroke,
                stroke_width,
                title,
            } => {
                if let Some(t) = title {
                    s.push_str(&format!("% {}\n", ps_comment(t)));
                }
                s.push_str("newpath\n");
                emit_path(&mut s, segs, scale, h);
                match (fill, stroke) {
                    (Some(f), Some(st)) => {
                        let (r, g, b) = rgb(f);
                        s.push_str(&format!(
                            "gsave {} {} {} setrgbcolor fill grestore\n",
                            n(r),
                            n(g),
                            n(b)
                        ));
                        let (r, g, b) = rgb(st);
                        s.push_str(&format!(
                            "{} setlinewidth {} {} {} setrgbcolor stroke\n",
                            n(stroke_width * scale),
                            n(r),
                            n(g),
                            n(b)
                        ));
                    }
                    (Some(f), None) => {
                        let (r, g, b) = rgb(f);
                        s.push_str(&format!("{} {} {} setrgbcolor fill\n", n(r), n(g), n(b)));
                    }
                    (None, Some(st)) => {
                        let (r, g, b) = rgb(st);
                        s.push_str(&format!(
                            "{} setlinewidth {} {} {} setrgbcolor stroke\n",
                            n(stroke_width * scale),
                            n(r),
                            n(g),
                            n(b)
                        ));
                    }
                    (None, None) => s.push_str("newpath\n"),
                }
            }
            Item::Circle {
                cx,
                cy,
                r,
                stroke,
                stroke_width,
            } => {
                // PostScript has an `arc` operator and this deliberately does
                // not use it. A native arc is a *better* circle than four
                // Béziers — but PDF has no such operator, so the PDF writer
                // must approximate, and two formats that approximate
                // differently are two slightly different figures. Building the
                // circle the same way in both is what lets the gate compare
                // their coordinate streams point for point, which is worth more
                // than 0.03% of a radius.
                let (rr, g, b) = rgb(stroke);
                let (sx, sy) = crate::scene::on_circle(*cx, *cy, *r, 0.0);
                s.push_str(&format!("newpath {} {} moveto\n", n(tx(sx)), n(ty(sy))));
                for c in arc_to_beziers(*cx, *cy, *r, 0.0, std::f64::consts::TAU) {
                    s.push_str(&format!(
                        "{} {} {} {} {} {} curveto\n",
                        n(tx(c[0])),
                        n(ty(c[1])),
                        n(tx(c[2])),
                        n(ty(c[3])),
                        n(tx(c[4])),
                        n(ty(c[5]))
                    ));
                }
                s.push_str(&format!(
                    "closepath {} setlinewidth {} {} {} setrgbcolor stroke\n",
                    n(stroke_width * scale),
                    n(rr),
                    n(g),
                    n(b)
                ));
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
                    rep.unencodable.push(text.clone());
                }
                if bytes.is_empty() {
                    continue;
                }
                let pts = size * scale;
                // Measured in the weight `font` selects two lines down.
                // Measuring bold text with the regular widths put every
                // anchored bold string — the centre title of every map — off
                // its anchor by half the difference, 3.34 pt at 15 pt for
                // "pcDNA3.1(+)-mCherry-WPRE".
                let width = text_width_in(text, pts, *bold);
                let x0 = match anchor {
                    Anchor::Start => tx(*x),
                    Anchor::Middle => tx(*x) - width / 2.0,
                    Anchor::End => tx(*x) - width,
                };
                // The scene's baseline is the middle of the glyphs, matching
                // SVG's `dominant-baseline: middle`. PostScript's is the
                // alphabetic baseline, so it moves down by half the x-height.
                // The constant is the PDF writer's, shared rather than copied:
                // this line held its own 0.36 under a comment saying it was the
                // PDF's number, and it put every EPS label 0.0985 em — 1.18 pt
                // at size 12 — below the same label in the PDF and the SVG.
                let y0 = ty(*y) - pts * BASELINE_DROP_EM;
                let font = if *bold { "Helvetica-Bold" } else { "Helvetica" };
                let (r, g, b) = rgb(color);
                s.push_str(&format!(
                    "/{} findfont {} scalefont setfont {} {} {} setrgbcolor {} {} moveto {} show\n",
                    font,
                    n(pts),
                    n(r),
                    n(g),
                    n(b),
                    n(x0),
                    n(y0),
                    ps_string(&bytes)
                ));
            }
        }
    }

    s.push_str("showpage\n%%EOF\n");
    (s, rep)
}

fn emit_path(s: &mut String, segs: &[Seg], scale: f64, h: f64) {
    let ty = |y: f64| h - y * scale;
    let tx = |x: f64| x * scale;
    let mut cur = (0.0f64, 0.0f64);
    for seg in segs {
        match seg {
            Seg::Move(x, y) => {
                cur = (*x, *y);
                s.push_str(&format!("{} {} moveto\n", n(tx(*x)), n(ty(*y))));
            }
            Seg::Line(x, y) => {
                cur = (*x, *y);
                s.push_str(&format!("{} {} lineto\n", n(tx(*x)), n(ty(*y))));
            }
            Seg::Arc {
                cx,
                cy,
                r,
                from,
                to,
            } => {
                // Held in centre form in the scene and converted per backend,
                // exactly as the SVG and PDF writers do, so all three describe
                // the same curve rather than three roundings of it.
                for c in arc_to_beziers(*cx, *cy, *r, *from, *to) {
                    s.push_str(&format!(
                        "{} {} {} {} {} {} curveto\n",
                        n(tx(c[0])),
                        n(ty(c[1])),
                        n(tx(c[2])),
                        n(ty(c[3])),
                        n(tx(c[4])),
                        n(ty(c[5]))
                    ));
                    cur = (c[4], c[5]);
                }
            }
            Seg::Close => s.push_str("closepath\n"),
        }
    }
    let _ = cur;
}

/// Two decimals, matching the SVG and PDF writers so the three agree.
fn n(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    let r = if r == 0.0 { 0.0 } else { r };
    format!("{r}")
}

/// A PostScript string literal.
///
/// `(`, `)` and `\` must be escaped or the string runs on and takes the rest of
/// the program with it. Bytes outside printable ASCII go out as octal, which is
/// what keeps a Latin-1 feature name intact in a 7-bit-safe file.
fn ps_string(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() + 2);
    s.push('(');
    for &b in bytes {
        match b {
            b'(' => s.push_str("\\("),
            b')' => s.push_str("\\)"),
            b'\\' => s.push_str("\\\\"),
            0x20..=0x7E => s.push(b as char),
            _ => s.push_str(&format!("\\{b:03o}")),
        }
    }
    s.push(')');
    s
}

/// Text safe to put in a `%` comment: no newline may escape it.
fn ps_comment(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Anchor, Item, Scene, Seg};

    fn scene(items: Vec<Item>) -> Scene {
        Scene {
            width: 200.0,
            height: 100.0,
            title: "map".into(),
            items,
        }
    }

    #[test]
    fn the_bounding_box_contains_the_artwork() {
        // A box smaller than the drawing crops it silently, in the
        // typesetter's hands rather than on screen. Rounded outwards for that
        // reason, with the exact one beside it.
        let s = scene(vec![]);
        let (eps, _) = to_eps(&s, 1.204724); // 85 mm across a 200-unit scene
        let bb = eps
            .lines()
            .find(|l| l.starts_with("%%BoundingBox:"))
            .expect("every EPS has one");
        let v: Vec<f64> = bb
            .split_whitespace()
            .skip(1)
            .map(|x| x.parse().unwrap())
            .collect();
        let exact_w = 200.0 * 1.204724;
        let exact_h = 100.0 * 1.204724;
        assert!(v[2] >= exact_w, "{} must contain {exact_w}", v[2]);
        assert!(v[3] >= exact_h, "{} must contain {exact_h}", v[3]);
        assert!(
            v[2] < exact_w + 1.0 && v[3] < exact_h + 1.0,
            "and not by much"
        );
        assert!(
            eps.contains("%%HiResBoundingBox: 0 0 240.94 120.47"),
            "{eps}"
        );
    }

    #[test]
    fn the_y_axis_is_flipped_exactly_once() {
        // PostScript's origin is bottom-left and the scene's is top-left.
        // Getting this wrong gives a figure that is upside-down but otherwise
        // perfect -- which survives a glance at a thumbnail.
        let s = scene(vec![Item::Path {
            segs: vec![Seg::Move(0.0, 0.0), Seg::Line(200.0, 100.0)],
            fill: None,
            stroke: Some("#000000".into()),
            stroke_width: 1.0,
            title: None,
        }]);
        let (eps, _) = to_eps(&s, 1.0);
        // Scene (0,0) is the top-left, so in PostScript it is (0, height).
        assert!(eps.contains("0 100 moveto"), "{eps}");
        // Scene (200,100) is the bottom-right: (200, 0).
        assert!(eps.contains("200 0 lineto"), "{eps}");
    }

    #[test]
    fn a_centred_label_is_centred_by_measurement() {
        // PostScript has no text-anchor, so the string must be measured. Using
        // the same metrics as the PDF writer is what makes a centred label land
        // in the same place in all three formats.
        let text = "AmpR";
        let items: Vec<Item> = [Anchor::Start, Anchor::Middle, Anchor::End]
            .iter()
            .map(|a| Item::Text {
                x: 100.0,
                y: 50.0,
                size: 10.0,
                anchor: *a,
                color: "#000000".into(),
                bold: false,
                text: text.into(),
            })
            .collect();
        let (eps, _) = to_eps(&scene(items), 1.0);
        let xs: Vec<f64> = eps
            .lines()
            .filter(|l| l.contains("moveto") && l.contains("show"))
            .map(|l| {
                let t: Vec<&str> = l.split_whitespace().collect();
                let i = t.iter().position(|x| *x == "moveto").unwrap();
                t[i - 2].parse().unwrap()
            })
            .collect();
        assert_eq!(xs.len(), 3);
        let w = text_width_in(text, 10.0, false);
        assert!((xs[0] - 100.0).abs() < 0.01, "start at x");
        assert!((xs[1] - (100.0 - w / 2.0)).abs() < 0.01, "middle: {:?}", xs);
        assert!((xs[2] - (100.0 - w)).abs() < 0.01, "end: {:?}", xs);
        assert!(w > 0.0);
    }

    /// The y of the `moveto` before a `show`, in PostScript points.
    fn eps_text_y(eps: &str) -> f64 {
        let line = eps
            .lines()
            .find(|l| l.contains("moveto") && l.contains("show"))
            .expect("the label");
        let t: Vec<&str> = line.split_whitespace().collect();
        let i = t.iter().position(|x| *x == "moveto").unwrap();
        t[i - 1].parse().unwrap()
    }

    #[test]
    fn a_label_sits_on_the_same_baseline_in_the_eps_as_in_the_pdf() {
        // At scale 1.0 the two writers work in the same units from the same
        // scene, so the baseline they compute must be the same number. This
        // file carried its own 0.36 against the PDF's 0.523/2 = 0.2615, under a
        // comment claiming it was the PDF's constant: every EPS label sat
        // 0.0985 em lower than the same label in the PDF and the SVG, which is
        // 1.18 pt at size 12 and 1.48 pt on the 15 pt title.
        let s = Scene {
            width: 720.0,
            height: 720.0,
            title: "map".into(),
            items: vec![Item::Text {
                x: 100.0,
                y: 300.0,
                size: 12.0,
                anchor: Anchor::Start,
                color: "#000000".into(),
                bold: false,
                text: "AmpR".into(),
            }],
        };
        let (eps, _) = to_eps(&s, 1.0);
        let (pdf_bytes, _) = crate::pdf::to_pdf(&s);
        let pdf = String::from_utf8_lossy(&pdf_bytes);
        let td = pdf.find(" Td").expect("Td");
        let line: &str = pdf[..td].rsplit('\n').next().unwrap();
        let pdf_y: f64 = line.split_whitespace().nth(1).unwrap().parse().unwrap();
        let eps_y = eps_text_y(&eps);
        assert!(
            (eps_y - pdf_y).abs() < 0.01,
            "EPS baseline {eps_y}, PDF baseline {pdf_y}"
        );
        // And it is the x-height rule, not some third number: 720 - 300 - 3.138.
        assert!((eps_y - 416.862).abs() < 0.01, "{eps_y}");
    }

    #[test]
    fn a_bold_label_is_measured_in_the_font_it_is_drawn_in() {
        // `Helvetica-Bold` is selected on the same line that positions the
        // string, so measuring with the regular widths puts every anchored bold
        // string off its anchor -- 3.34 pt for a centred title, and the whole
        // 6.7 pt for an `Anchor::End` one.
        let name = "pcDNA3.1(+)-mCherry-WPRE";
        let mk = |anchor: Anchor| Scene {
            width: 400.0,
            height: 100.0,
            title: "map".into(),
            items: vec![Item::Text {
                x: 200.0,
                y: 50.0,
                size: 15.0,
                anchor,
                color: "#000000".into(),
                bold: true,
                text: name.into(),
            }],
        };
        let x_of = |eps: &str| -> f64 {
            let line = eps
                .lines()
                .find(|l| l.contains("moveto") && l.contains("show"))
                .expect("the label");
            let t: Vec<&str> = line.split_whitespace().collect();
            let i = t.iter().position(|x| *x == "moveto").unwrap();
            t[i - 2].parse().unwrap()
        };
        let bold_w = text_width_in(name, 15.0, true);
        let regular_w = text_width_in(name, 15.0, false);
        assert!(bold_w > regular_w + 6.0, "the two weights differ by 6.7 pt");
        let (mid, _) = to_eps(&mk(Anchor::Middle), 1.0);
        assert!(
            (x_of(&mid) - (200.0 - bold_w / 2.0)).abs() < 0.01,
            "{}",
            x_of(&mid)
        );
        let (end, _) = to_eps(&mk(Anchor::End), 1.0);
        assert!(
            (x_of(&end) - (200.0 - bold_w)).abs() < 0.01,
            "{}",
            x_of(&end)
        );
        assert!(mid.contains("/Helvetica-Bold findfont"), "{mid}");
    }

    #[test]
    fn an_eight_digit_hex_colour_is_the_same_colour_here_as_in_the_pdf() {
        // A SnapGene segment can carry `color="#4f7fd0ff"` and `safe_color`
        // passes 4- and 8-digit hex through unnormalised, on purpose. This
        // file's own parser understood only 3 and 6, so that feature drew blue
        // in the SVG, blue in the PDF and solid black in the EPS the author
        // submitted -- with nothing in any `Report` to say so.
        for (with_alpha, without) in [("#4f7fd0ff", "#4f7fd0"), ("#4f7d", "#4f7d")] {
            let s = scene(vec![Item::Path {
                segs: vec![Seg::Move(0.0, 0.0), Seg::Line(10.0, 10.0)],
                fill: Some(with_alpha.into()),
                stroke: None,
                stroke_width: 1.0,
                title: None,
            }]);
            let (eps, _) = to_eps(&s, 1.0);
            assert!(
                !eps.contains("0 0 0 setrgbcolor fill"),
                "{with_alpha} came out black: {eps}"
            );
            let (r, g, b) = crate::pdf::rgb(with_alpha);
            assert!(
                eps.contains(&format!("{} {} {} setrgbcolor fill", n(r), n(g), n(b))),
                "{with_alpha} disagrees with the PDF writer: {eps}"
            );
            // The alpha nibble is dropped, not mixed into a channel.
            if with_alpha != without {
                assert_eq!(crate::pdf::rgb(with_alpha), crate::pdf::rgb(without));
            }
        }
        // Control: the six-digit form this file always handled is unchanged.
        assert_eq!(crate::pdf::rgb("#336699"), (0.2, 0.4, 0.6));
    }

    #[test]
    fn a_parenthesis_in_a_feature_name_cannot_run_off_the_end_of_the_string() {
        // An unescaped ')' ends the PostScript string and the rest of the
        // program becomes garbage -- a whole figure lost to one feature called
        // "aph(3')-Ia", which is a real gene name from this project's own
        // database.
        let s = scene(vec![Item::Text {
            x: 10.0,
            y: 10.0,
            size: 8.0,
            anchor: Anchor::Start,
            color: "#000000".into(),
            bold: false,
            text: "aph(3')-Ia \\ (n)".into(),
        }]);
        let (eps, _) = to_eps(&s, 1.0);
        let lit = eps
            .lines()
            .find(|l| l.contains("show"))
            .expect("the label")
            .to_string();
        assert!(lit.contains("aph\\(3'\\)-Ia \\\\ \\(n\\)"), "{lit}");
        // Balanced once escaping is accounted for.
        let inner = &lit[lit.find('(').unwrap() + 1..lit.rfind(')').unwrap()];
        let mut depth = 0i32;
        let b: Vec<char> = inner.chars().collect();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                '\\' => i += 1,
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        assert_eq!(depth, 0, "unbalanced: {inner}");
    }

    #[test]
    fn a_character_helvetica_cannot_encode_is_reported_not_silently_mangled() {
        let s = scene(vec![Item::Text {
            x: 1.0,
            y: 1.0,
            size: 8.0,
            anchor: Anchor::Start,
            color: "#000000".into(),
            bold: false,
            text: "λ phage".into(),
        }]);
        let (_, rep) = to_eps(&s, 1.0);
        assert_eq!(rep.unencodable, vec!["λ phage".to_string()]);
    }

    #[test]
    fn a_latin1_name_survives_as_octal() {
        let s = scene(vec![Item::Text {
            x: 1.0,
            y: 1.0,
            size: 8.0,
            anchor: Anchor::Start,
            color: "#000000".into(),
            bold: false,
            text: "Ölschläger".into(),
        }]);
        let (eps, rep) = to_eps(&s, 1.0);
        assert!(rep.unencodable.is_empty(), "Latin-1 is representable");
        assert!(eps.contains("\\326"), "O-diaeresis as octal: {eps}");
        assert!(eps.is_ascii(), "and the file stays 7-bit safe");
    }

    #[test]
    fn an_arc_becomes_curves_rather_than_a_polygon() {
        let s = scene(vec![Item::Path {
            segs: vec![
                Seg::Move(100.0, 10.0),
                Seg::Arc {
                    cx: 100.0,
                    cy: 50.0,
                    r: 40.0,
                    from: 0.0,
                    to: 90.0,
                },
            ],
            fill: None,
            stroke: Some("#336699".into()),
            stroke_width: 2.0,
            title: Some("AmpR".into()),
        }]);
        let (eps, _) = to_eps(&s, 1.0);
        assert!(eps.contains("curveto"), "{eps}");
        assert!(eps.contains("% AmpR"), "the tooltip survives as a comment");
        assert!(eps.contains("0.2 0.4 0.6 setrgbcolor"), "{eps}");
    }

    #[test]
    fn the_same_scene_produces_the_same_bytes_twice() {
        let s = scene(vec![
            Item::Circle {
                cx: 100.0,
                cy: 50.0,
                r: 30.0,
                stroke: "#000000".into(),
                stroke_width: 1.0,
            },
            Item::Text {
                x: 100.0,
                y: 50.0,
                size: 9.0,
                anchor: Anchor::Middle,
                color: "#ff0000".into(),
                bold: true,
                text: "pUC19".into(),
            },
        ]);
        assert_eq!(to_eps(&s, 1.3).0, to_eps(&s, 1.3).0);
    }

    #[test]
    fn scaling_scales_the_type_too() {
        // The property `page::Fit` exists to warn about, checked here at the
        // emitter: a figure exported half as wide has half-size type in it.
        let s = scene(vec![Item::Text {
            x: 10.0,
            y: 10.0,
            size: 12.0,
            anchor: Anchor::Start,
            color: "#000000".into(),
            bold: false,
            text: "x".into(),
        }]);
        assert!(to_eps(&s, 1.0).0.contains("12 scalefont"));
        assert!(to_eps(&s, 0.5).0.contains("6 scalefont"));
    }
}
