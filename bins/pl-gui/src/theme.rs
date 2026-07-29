//! Colours.
//!
//! A muted, low-chroma palette: a plasmid map is read for a long time and often
//! ends up in a figure, so saturated defaults are the wrong choice.
//!
//! Every colour is resolved through [`Palette`] against the *current* theme.
//! Hardcoding one set made the whole map near-invisible in dark mode, which is
//! what most people's systems are set to.
//!
//! Feature colours from the file always win — those are the user's decision,
//! and they are the same ink in either theme.

use eframe::egui::{Color32, Visuals};
use pl_core::Feature;

/// Theme-resolved UI colours.
#[derive(Clone, Copy)]
pub struct Palette {
    /// Primary text.
    pub ink: Color32,
    /// Secondary text: still meant to be read.
    pub ink2: Color32,
    /// Tertiary text: labels, units, counts.
    ///
    /// Tertiary, not decorative. It carries the sequence view's column ruler,
    /// both coordinate gutters and the map's enzyme positions — all of them
    /// numbers a reader has to get right, none of them above 11 pt. So it is
    /// held to AA for normal text, 4.5:1, and not to the 3:1 large-text
    /// threshold it used to be checked against: the light value was 3.78:1 and
    /// the ruler it now prints is 9.5 pt.
    pub muted: Color32,
    /// Leader lines and hairlines.
    pub faint: Color32,
    /// The backbone and its ticks.
    pub line: Color32,
    pub accent: Color32,
    pub warn: Color32,
}

impl Palette {
    pub fn of(dark: bool) -> Self {
        if dark {
            Palette {
                ink: Color32::from_rgb(0xe8, 0xed, 0xef),
                ink2: Color32::from_rgb(0xb4, 0xc0, 0xc6),
                muted: Color32::from_rgb(0x84, 0x92, 0x99),
                faint: Color32::from_rgb(0x4a, 0x55, 0x5c),
                line: Color32::from_rgb(0x76, 0x84, 0x8b),
                accent: Color32::from_rgb(0x6f, 0xa8, 0xd0),
                warn: Color32::from_rgb(0xe0, 0x8a, 0x70),
            }
        } else {
            Palette {
                ink: Color32::from_rgb(0x1c, 0x22, 0x26),
                ink2: Color32::from_rgb(0x3d, 0x4a, 0x52),
                // #74838c was 3.78:1 on #fafbfc — fine as a hairline, short of
                // AA for the 9.5 pt column ruler and the coordinate gutters
                // this role now prints. Darkened to 4.99:1, which is still
                // clearly a step below `ink2` at 8.84:1.
                muted: Color32::from_rgb(0x62, 0x6f, 0x78),
                faint: Color32::from_rgb(0xb8, 0xc2, 0xc8),
                line: Color32::from_rgb(0x8d, 0x99, 0xa0),
                accent: Color32::from_rgb(0x2f, 0x6f, 0x9a),
                warn: Color32::from_rgb(0xb0, 0x55, 0x3f),
            }
        }
    }

    /// A translucent wash for the selected row, in whichever theme.
    pub fn selection(&self) -> Color32 {
        let a = self.accent;
        Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), 30)
    }
}

/// Fallbacks by GenBank feature key, for files that carry no colour.
fn by_kind(kind: &str) -> Color32 {
    match kind {
        "CDS" => Color32::from_rgb(0x9a, 0x5b, 0x8c),
        "gene" => Color32::from_rgb(0x7a, 0x5b, 0x9a),
        "promoter" => Color32::from_rgb(0x3f, 0x8f, 0x4f),
        "terminator" => Color32::from_rgb(0xb0, 0x55, 0x3f),
        "rep_origin" => Color32::from_rgb(0x3f, 0x6f, 0x9a),
        "primer_bind" => Color32::from_rgb(0x5f, 0x8f, 0xa8),
        "RBS" | "polyA_signal" => Color32::from_rgb(0xa8, 0x8a, 0x3f),
        _ => Color32::from_rgb(0x7d, 0x8a, 0x86),
    }
}

fn parse_hex(s: &str) -> Option<Color32> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color32::from_rgb(
        (v >> 16) as u8,
        ((v >> 8) & 0xff) as u8,
        (v & 0xff) as u8,
    ))
}

pub fn feature_color(f: &Feature) -> Color32 {
    f.color()
        .and_then(parse_hex)
        .unwrap_or_else(|| by_kind(&f.kind))
}

/// Relative luminance, per WCAG. Each channel is linearised first, because a
/// saturated green and a saturated blue with the same mean are nowhere near
/// equally bright.
fn luminance(c: Color32) -> f32 {
    let f = |c: u8| {
        let s = c as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
}

/// WCAG contrast ratio, 1.0 to 21.0.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Black or white text on a coloured band, whichever actually contrasts more.
///
/// Two things here were measured rather than chosen, because a feature's colour
/// comes out of the user's file and can be any of the 16.7 million:
///
/// **Compare, do not threshold.** This used to switch on luminance crossing
/// 0.42, and sweeping the RGB cube put the worst case at **2.08:1** — less than
/// half of AA — around mid-tones like `(104, 192, 120)`, an entirely ordinary
/// feature green. No threshold fixes that; the best one available only reaches
/// 4.01. Picking whichever of the two colours has the higher ratio is optimal
/// by construction and needs no constant at all.
///
/// **Pure black and white, not softened near-black and near-white.** With
/// `#141a1e`/`#f4f7f8` the worst case is 4.04:1 and 10.9% of colours cannot
/// reach AA with either. With `#000000`/`#ffffff` the worst case over the whole
/// cube is **4.58:1**, so every possible feature colour clears 4.5. Softer
/// greys look better on the nine colours we chose ourselves and lose the
/// guarantee on the ones we did not.
pub fn on_color(bg: Color32) -> Color32 {
    if contrast(Color32::BLACK, bg) >= contrast(Color32::WHITE, bg) {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

/// The window's background, which every rule and hairline is measured against.
pub fn panel_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x16, 0x1a, 0x1d)
    } else {
        Color32::from_rgb(0xfa, 0xfb, 0xfc)
    }
}

pub fn apply(visuals: &mut Visuals) {
    let dark = visuals.dark_mode;
    visuals.panel_fill = panel_fill(dark);

    // The splitter between the map and the details panel is drawn with these
    // three strokes (egui 0.35 `panel.rs`: `noninteractive.bg_stroke` at rest,
    // `hovered.fg_stroke` under the pointer, `active.fg_stroke` while
    // dragging). egui's defaults are gray(60) on the dark panel — **1.57:1** —
    // and gray(190) on the light one — **1.81:1**. Both fail SC 1.4.11's 3:1
    // for the boundary of a UI component, and that line is now the only resting
    // signal that the panel can be resized at all.
    //
    // `muted` is the palette role that clears 3:1 in both themes (3.78 light,
    // 5.46 dark); `faint` is 1.75/2.29 and `line` is 2.82/4.54, so neither is
    // usable for this. Colour is not the only channel — egui already sets
    // `CursorIcon::ResizeHorizontal` on hover and `ResizeWest`/`ResizeEast` at
    // the stops — so no grip graphic is added; that would be a new shape and a
    // new colour needing its own clearance for no gain.
    let p = Palette::of(dark);
    visuals.widgets.noninteractive.bg_stroke.color = p.muted;
    visuals.widgets.hovered.fg_stroke.color = p.ink;
    visuals.widgets.active.fg_stroke.color = p.ink;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colours_from_files_are_honoured() {
        assert_eq!(
            parse_hex("#3f6f9a"),
            Some(Color32::from_rgb(0x3f, 0x6f, 0x9a))
        );
        assert_eq!(parse_hex("#FFFFFF"), Some(Color32::WHITE));
    }

    #[test]
    fn malformed_colours_fall_back_rather_than_panic() {
        assert_eq!(parse_hex("3f6f9a"), None, "missing hash");
        assert_eq!(
            parse_hex("#abc"),
            None,
            "short form is not what files write"
        );
        assert_eq!(parse_hex("#gggggg"), None);
        assert_eq!(parse_hex("#δβγδβγ"), None, "must not slice multibyte text");
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn a_features_own_colour_beats_the_fallback() {
        let mut f = Feature::new("x", "CDS");
        let mut s = pl_core::Segment::new(1, 10);
        s.color = Some("#123456".into());
        f.segments.push(s);
        assert_eq!(feature_color(&f), Color32::from_rgb(0x12, 0x34, 0x56));
    }

    #[test]
    fn a_feature_without_a_colour_gets_one_from_its_kind() {
        let mut f = Feature::new("x", "promoter");
        f.segments.push(pl_core::Segment::new(1, 10));
        assert_eq!(feature_color(&f), by_kind("promoter"));
    }

    #[test]
    fn label_contrast_flips_with_luminance_not_average() {
        // Saturated blue and saturated yellow have similar means and very
        // different brightness; both must come out readable.
        assert_eq!(on_color(Color32::from_rgb(0, 0, 255)), Color32::WHITE);
        assert_eq!(on_color(Color32::from_rgb(255, 255, 0)), Color32::BLACK);
        assert_eq!(on_color(Color32::WHITE), Color32::BLACK);
        assert_eq!(on_color(Color32::BLACK), Color32::WHITE);
    }

    /// Relative luminance, for asserting that text is actually readable.
    fn luma(c: Color32) -> f32 {
        let f = |v: u8| {
            let s = v as f32 / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luma(a), luma(b));
        let (hi, lo) = if x > y { (x, y) } else { (y, x) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn text_is_readable_against_its_own_background_in_both_themes() {
        // The bug this guards: a palette hardcoded for light mode made the map
        // caption invisible on a dark background.
        for dark in [true, false] {
            let p = Palette::of(dark);
            let bg = if dark {
                Color32::from_rgb(0x16, 0x1a, 0x1d)
            } else {
                Color32::from_rgb(0xfa, 0xfb, 0xfc)
            };
            // WCAG AA for normal text is 4.5:1; these are the text roles.
            assert!(
                contrast(p.ink, bg) >= 4.5,
                "ink on {} bg: {:.1}:1",
                if dark { "dark" } else { "light" },
                contrast(p.ink, bg)
            );
            assert!(
                contrast(p.ink2, bg) >= 4.5,
                "ink2: {:.1}",
                contrast(p.ink2, bg)
            );
            // Muted is small text too — the sequence view's column ruler is
            // 9.5 pt and its coordinate gutters 11 — so it is held to the same
            // 4.5:1 and not to the 18 pt large-text threshold. PROVEN TO FAIL
            // before this run: light `muted` was #74838c at 3.78:1, sampled
            // from the rendered ruler as #74838c on a #fafbfc panel.
            assert!(
                contrast(p.muted, bg) >= 4.5,
                "muted on {} bg: {:.2}:1 — the ruler and both gutters are drawn \
                 in this and none of them is large text",
                if dark { "dark" } else { "light" },
                contrast(p.muted, bg)
            );
            // The backbone must be visible without competing with features.
            assert!(
                contrast(p.line, bg) >= 2.0,
                "line: {:.1}",
                contrast(p.line, bg)
            );
        }
    }

    #[test]
    fn a_label_is_readable_on_every_colour_a_file_can_contain() {
        // A feature's colour comes out of the user's file, so this has to hold
        // for all 16.7 million and not just the nine this project picks. The
        // cube is swept at a stride of 4, which is 262,144 colours -- enough to
        // find the worst case, which sits at (240, 20, 36) and reaches 4.58:1.
        //
        // The numbers this replaced: switching on a luminance threshold of 0.42
        // bottomed out at 2.08:1 around (104, 192, 120), an ordinary feature
        // green, and no threshold does better than 4.01. Softened near-black
        // and near-white bottom out at 4.04 and leave 10.9% of colours below
        // AA. Both were measured, and neither was visible by looking.
        let mut worst = f32::MAX;
        let mut worst_at = Color32::BLACK;
        let mut r = 0u16;
        while r < 256 {
            let mut g = 0u16;
            while g < 256 {
                let mut b = 0u16;
                while b < 256 {
                    let bg = Color32::from_rgb(r as u8, g as u8, b as u8);
                    let c = contrast(on_color(bg), bg);
                    if c < worst {
                        worst = c;
                        worst_at = bg;
                    }
                    b += 4;
                }
                g += 4;
            }
            r += 4;
        }
        assert!(
            worst >= 4.5,
            "worst label contrast is {worst:.2}:1 on rgb({}, {}, {}), below WCAG AA",
            worst_at.r(),
            worst_at.g(),
            worst_at.b()
        );
        assert!((worst - 4.58).abs() < 0.02, "{worst}");
    }

    /// The splitter's resting line is the only thing on screen saying the panel
    /// can be resized, so it is a UI component boundary and SC 1.4.11 applies.
    ///
    /// PROVEN TO FAIL at bd96e5b (compile-only there — `apply` did not set
    /// these, and `theme::panel_fill` did not exist). The numbers it replaces
    /// are egui's defaults: gray(60) on `#161a1d` is 1.57:1 and gray(190) on
    /// `#fafbfc` is 1.81:1, both a long way under 3.
    #[test]
    fn the_splitter_is_visible_at_rest_in_both_themes() {
        use pl_draw::contrast::{passes_aa, ratio, Kind};
        for dark in [true, false] {
            let mut v = if dark {
                Visuals::dark()
            } else {
                Visuals::light()
            };
            apply(&mut v);
            let bg = panel_fill(dark);
            let bg = (bg.r(), bg.g(), bg.b());
            for (what, c) in [
                ("at rest", v.widgets.noninteractive.bg_stroke.color),
                ("hovered", v.widgets.hovered.fg_stroke.color),
                ("dragging", v.widgets.active.fg_stroke.color),
            ] {
                let fg = (c.r(), c.g(), c.b());
                assert!(
                    passes_aa(fg, bg, Kind::Graphic),
                    "the splitter {what} in {} mode is {:.2}:1",
                    if dark { "dark" } else { "light" },
                    ratio(fg, bg)
                );
            }
        }
    }

    #[test]
    fn the_contrast_ratio_is_the_wcag_one() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 1e-4);
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 1e-5);
        // Symmetric.
        let a = Color32::from_rgb(0x4f, 0x7f, 0xd0);
        assert_eq!(contrast(a, Color32::WHITE), contrast(Color32::WHITE, a));
    }
}
