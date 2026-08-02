//! A [`pl_draw::Scene`] painted with egui.
//!
//! # Why this exists, and why it is not in `pl-draw`
//!
//! Before this module there was NO Scene→egui path in the application at all.
//! `map.rs` paints with `egui::Shape` from its own geometry and shares only the
//! *layout* arithmetic (`pl_draw::ring`) with the exporters, which build a
//! `Scene` internally and hand back bytes; the one place the GUI called
//! `pl_draw::scene` it threw the scene away on the spot and kept the `Report`.
//! So a second picture — the gel — had two options: re-implement
//! `pl_gel::render` in egui, or paint the `Scene` the crate already produces.
//!
//! The second is smaller AND stronger. `pl-gel`'s renderer is 726 lines whose
//! margin arithmetic exists because it was got wrong under fire — a viewBox
//! that clipped `3180` to `318`, and `AarI+BamHI+BsiWI+SacII` to
//! `AarI+BamHI+BsiWI+SacI`, naming a digest nobody ran — and seven tests pin
//! it. Duplicating that layout would leave those seven tests pinning the FILE
//! while describing the SCREEN. Painting the scene means the screen and the
//! exported figure are one object, not two layouts that agree today.
//!
//! It cannot live in `pl-draw`: crates under `crates/` have zero external
//! dependencies and `egui` is one. So it is here, and `pl-draw` stays clean.
//!
//! # What it is NOT
//!
//! **Not a general painter, and it says so rather than drawing a wrong shape.**
//! `egui::Shape::convex_polygon` fills only convex outlines, so a concave
//! filled subpath would come out as something other than itself. Every filled
//! path `pl_gel::render` emits is a rectangle, so this is exact for the gel;
//! anything else must check before relying on it.
//!
//! **`bold` is not rendered, and is not faked.** `font_definitions` installs
//! IBM Plex Sans and Mono in regular only, egui has no synthetic emboldening,
//! and `RichText::strong()` is a colour change rather than a weight. In a gel
//! scene `bold: true` marks a co-migrating band label — a real signal — so the
//! caller must carry that signal in words as well. See the gel's disclosure
//! strip. Pretending the weight was drawn would be the lie.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use pl_draw::scene::{Anchor, Item, Scene, Seg};

/// Where a scene landed on screen, and what is under the pointer.
pub struct Painted {
    /// Hit rectangles for the paths that carry a title, innermost last —
    /// `Item::Path::title` is populated by `pl-gel` with things like
    /// `"1kb ladder well"` and `"2000/2100"`, and throwing them away would mean
    /// the SVG had tooltips the app did not.
    pub hits: Vec<(Rect, String)>,
    /// Items this painter refused to draw because their colour did not parse.
    ///
    /// COUNTED, not merely skipped. Refusing is right — see [`colour`] — but a
    /// refusal nothing reports is a band, a well or a label missing from the
    /// pane with no trace anywhere, and the pane just looks emptier. The caller
    /// surfaces this; the gel's disclosure strip already carries counts.
    pub skipped: usize,
}

impl Painted {
    /// The title of the smallest titled path under `p`.
    ///
    /// Smallest, not first: the background is a titled-path-shaped thing in
    /// some scenes and it covers everything.
    pub fn hover(&self, p: Pos2) -> Option<&str> {
        self.hits
            .iter()
            .filter(|(r, _)| r.contains(p))
            .min_by(|a, b| {
                (a.0.area())
                    .partial_cmp(&b.0.area())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(_, t)| t.as_str())
    }
}

/// The scale that fits `scene` into `avail`, before any clamping.
pub fn fit_scale(scene: &Scene, avail: Vec2) -> f32 {
    if scene.width <= 0.0 || scene.height <= 0.0 {
        return 1.0;
    }
    let sx = avail.x / scene.width as f32;
    let sy = avail.y / scene.height as f32;
    sx.min(sy)
}

/// A colour from a scene, or `None`.
///
/// Never a silent black: every colour in a `Scene` has been through
/// `pl_draw::safe_color`, so an unparseable one means the scene is not what
/// this thinks it is, and drawing it black would hide that.
fn colour(hex: &str) -> Option<Color32> {
    pl_draw::contrast::parse_hex(hex).map(|(r, g, b)| Color32::from_rgb(r, g, b))
}

/// Paint `scene` at `scale`, with its top-left at `origin`.
///
/// The caller owns the fit — see [`fit_scale`] — because the gel view has a
/// floor on it that this module has no business knowing about.
pub fn paint(painter: &egui::Painter, scene: &Scene, origin: Pos2, scale: f32) -> Painted {
    let at = |x: f64, y: f64| Pos2::new(origin.x + x as f32 * scale, origin.y + y as f32 * scale);
    let mut hits = Vec::new();
    let mut skipped = 0usize;

    for item in &scene.items {
        match item {
            Item::Path {
                segs,
                fill,
                stroke,
                stroke_width,
                title,
            } => {
                let pts = flatten(segs, &at);
                if pts.len() < 2 {
                    continue;
                }
                if let Some(f) = fill.as_deref() {
                    match colour(f) {
                        // Closed and convex only; see the module docs. A gel is
                        // rectangles, so this is exact here.
                        Some(c) => {
                            painter.add(egui::Shape::convex_polygon(pts.clone(), c, Stroke::NONE));
                        }
                        None => skipped += 1,
                    }
                }
                if let Some(s) = stroke.as_deref() {
                    match colour(s) {
                        Some(c) => {
                            painter.add(egui::Shape::line(
                                pts.clone(),
                                Stroke::new((*stroke_width as f32 * scale).max(0.5), c),
                            ));
                        }
                        None => skipped += 1,
                    }
                }
                if let Some(t) = title {
                    hits.push((bounds(&pts), t.clone()));
                }
            }
            Item::Circle {
                cx,
                cy,
                r,
                stroke,
                stroke_width,
            } => match colour(stroke) {
                Some(s) => {
                    painter.circle_stroke(
                        at(*cx, *cy),
                        *r as f32 * scale,
                        Stroke::new((*stroke_width as f32 * scale).max(0.5), s),
                    );
                }
                None => skipped += 1,
            },
            Item::Text {
                x,
                y,
                size,
                anchor,
                color,
                text,
                ..
            } => {
                let Some(c) = colour(color) else {
                    skipped += 1;
                    continue;
                };
                // CENTER vertically, because `pl_draw::scene` states that a
                // text item's baseline is the MIDDLE of the glyphs — SVG's
                // `dominant-baseline: middle`, which is what the layout that
                // produced these coordinates assumed.
                let align = match anchor {
                    Anchor::Start => Align2::LEFT_CENTER,
                    Anchor::Middle => Align2::CENTER_CENTER,
                    Anchor::End => Align2::RIGHT_CENTER,
                };
                painter.text(
                    at(*x, *y),
                    align,
                    text,
                    FontId::proportional(*size as f32 * scale),
                    c,
                );
            }
        }
    }
    Painted { hits, skipped }
}

/// A path's points, in screen coordinates.
///
/// `Seg::Arc` is sampled through `pl_draw::scene::arc_to_beziers` — the same
/// conversion the PDF back end uses, so a curve on screen and a curve in the
/// file are the same curve rather than two approximations.
fn flatten(segs: &[Seg], at: &impl Fn(f64, f64) -> Pos2) -> Vec<Pos2> {
    let mut pts: Vec<Pos2> = Vec::with_capacity(segs.len() + 4);
    let (mut cx, mut cy) = (0.0f64, 0.0f64);
    for seg in segs {
        match seg {
            Seg::Move(x, y) | Seg::Line(x, y) => {
                pts.push(at(*x, *y));
                cx = *x;
                cy = *y;
            }
            Seg::Arc {
                cx: ax,
                cy: ay,
                r,
                from,
                to,
            } => {
                let start = pl_draw::scene::on_circle(*ax, *ay, *r, *from);
                if pts.is_empty() || (start.0 - cx).abs() > 1e-9 || (start.1 - cy).abs() > 1e-9 {
                    pts.push(at(start.0, start.1));
                }
                let mut p0 = start;
                for b in pl_draw::scene::arc_to_beziers(*ax, *ay, *r, *from, *to) {
                    for k in 1..=8 {
                        let t = k as f64 / 8.0;
                        let u = 1.0 - t;
                        let (w0, w1, w2, w3) =
                            (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                        pts.push(at(
                            w0 * p0.0 + w1 * b[0] + w2 * b[2] + w3 * b[4],
                            w0 * p0.1 + w1 * b[1] + w2 * b[3] + w3 * b[5],
                        ));
                    }
                    p0 = (b[4], b[5]);
                }
                cx = p0.0;
                cy = p0.1;
            }
            // The polygon and the polyline both close themselves; repeating
            // the first point would put a zero-length segment in the stroke.
            Seg::Close => {}
        }
    }
    pts
}

fn bounds(pts: &[Pos2]) -> Rect {
    let mut r = Rect::NOTHING;
    for p in pts {
        r.extend_with(*p);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_draw::scene::{Item, Scene, Seg};

    fn rect_scene() -> Scene {
        Scene {
            width: 100.0,
            height: 50.0,
            title: "t".into(),
            items: vec![
                Item::Path {
                    segs: vec![
                        Seg::Move(10.0, 10.0),
                        Seg::Line(30.0, 10.0),
                        Seg::Line(30.0, 20.0),
                        Seg::Line(10.0, 20.0),
                        Seg::Close,
                    ],
                    fill: Some("#f2f4f7".into()),
                    stroke: None,
                    stroke_width: 0.0,
                    title: Some("2000/2100".into()),
                },
                Item::Text {
                    x: 40.0,
                    y: 15.0,
                    size: 9.0,
                    anchor: Anchor::Start,
                    color: "#9aa4b0".into(),
                    bold: false,
                    text: "2000/2100".into(),
                },
            ],
        }
    }

    #[test]
    fn a_fit_scale_puts_the_whole_scene_inside_the_space_offered() {
        let sc = rect_scene();
        for avail in [
            Vec2::new(200.0, 200.0),
            Vec2::new(50.0, 400.0),
            Vec2::new(400.0, 25.0),
        ] {
            let s = fit_scale(&sc, avail);
            assert!(sc.width as f32 * s <= avail.x + 1e-3, "{avail:?}");
            assert!(sc.height as f32 * s <= avail.y + 1e-3, "{avail:?}");
        }
        // A degenerate scene does not divide by zero.
        let empty = Scene {
            width: 0.0,
            height: 0.0,
            title: String::new(),
            items: vec![],
        };
        assert!(fit_scale(&empty, Vec2::new(10.0, 10.0)).is_finite());
    }

    /// The tooltips `pl-gel` writes into the scene must survive the trip to
    /// the screen; the SVG has them and the app would otherwise not.
    #[test]
    fn a_titled_path_becomes_a_hit_rectangle_at_the_scale_it_was_drawn() {
        let ctx = crate::test_ctx();
        let mut out = None;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            out = Some(paint(ui.painter(), &rect_scene(), Pos2::new(5.0, 7.0), 2.0));
        });
        let p = out.expect("painted");
        assert_eq!(p.hits.len(), 1);
        let (r, title) = &p.hits[0];
        assert_eq!(title, "2000/2100");
        // Scene (10,10)-(30,20) at scale 2 from origin (5,7).
        assert!((r.min.x - 25.0).abs() < 0.01, "{r:?}");
        assert!((r.min.y - 27.0).abs() < 0.01, "{r:?}");
        assert!((r.max.x - 65.0).abs() < 0.01, "{r:?}");
        assert!((r.max.y - 47.0).abs() < 0.01, "{r:?}");
        assert_eq!(p.hover(Pos2::new(30.0, 30.0)), Some("2000/2100"));
        assert_eq!(p.hover(Pos2::new(300.0, 300.0)), None);
    }

    /// An unparseable colour is skipped, not drawn black.
    ///
    /// Every colour in a `Scene` has been through `pl_draw::safe_color`, so one
    /// that does not parse means the scene is not what this module thinks it
    /// is — and a black rectangle on a dark gel is invisible, which is the
    /// worst way to find that out.
    #[test]
    fn a_colour_that_is_not_a_colour_is_refused_rather_than_defaulted() {
        assert_eq!(colour("#15181c"), Some(Color32::from_rgb(0x15, 0x18, 0x1c)));
        assert_eq!(colour("#fff"), Some(Color32::from_rgb(255, 255, 255)));
        assert_eq!(colour("url(#gradient)"), None);
        assert_eq!(colour(""), None);
    }
}
