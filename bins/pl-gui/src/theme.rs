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
                muted: Color32::from_rgb(0x74, 0x83, 0x8c),
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

/// Black or white text on a coloured band, whichever stays legible.
///
/// Uses relative luminance rather than a simple average: a saturated green and
/// a saturated blue of the same mean are nowhere near equally bright.
pub fn on_color(bg: Color32) -> Color32 {
    let f = |c: u8| {
        let s = c as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    let l = 0.2126 * f(bg.r()) + 0.7152 * f(bg.g()) + 0.0722 * f(bg.b());
    if l > 0.42 {
        Color32::from_rgb(0x14, 0x1a, 0x1e)
    } else {
        Color32::from_rgb(0xf4, 0xf7, 0xf8)
    }
}

pub fn apply(visuals: &mut Visuals) {
    visuals.panel_fill = if visuals.dark_mode {
        Color32::from_rgb(0x16, 0x1a, 0x1d)
    } else {
        Color32::from_rgb(0xfa, 0xfb, 0xfc)
    };
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
        assert_eq!(on_color(Color32::from_rgb(0, 0, 255)).r(), 0xf4);
        assert_eq!(on_color(Color32::from_rgb(255, 255, 0)).r(), 0x14);
        assert_eq!(on_color(Color32::WHITE).r(), 0x14);
        assert_eq!(on_color(Color32::BLACK).r(), 0xf4);
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
            // Muted is small supporting text; AA large-text threshold.
            assert!(
                contrast(p.muted, bg) >= 3.0,
                "muted: {:.1}",
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
}
