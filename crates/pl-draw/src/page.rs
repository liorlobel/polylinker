//! Physical page size, and what journals ask for.
//!
//! # Why a figure needs a physical size at all
//!
//! A [`Scene`](crate::Scene) is in abstract units. An SVG with `width="720"`
//! means 720 CSS pixels, which is 190.5 mm at 96 dpi — nearly double the
//! single-column width of most journals. Dropped into a manuscript it gets
//! scaled to fit, and every font in it scales with it: an 8 pt label becomes
//! 3.5 pt, below every journal's minimum, and the figure is rejected or
//! silently unreadable in print.
//!
//! So the export path takes a **physical width** and derives everything else
//! from it. The consequence that matters is the one people get wrong: scaling a
//! figure down scales its text down. [`Fit::min_font_pt`] reports the smallest
//! type in the figure *at the requested physical size*, and
//! [`Fit::type_too_small`] compares it against the preset's floor — so the
//! problem is caught before submission rather than by a copy editor.
//!
//! # Presets
//!
//! Column widths and minimum type sizes as journals publish them in their
//! author guidelines. They are requirements, not opinions, and they differ
//! enough between publishers to be worth writing down.

/// A journal's figure requirements.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    /// Single-column width, mm.
    pub single_mm: f64,
    /// Full-width (double-column) width, mm.
    pub double_mm: f64,
    /// Smallest permitted type, in points, at final printed size.
    pub min_font_pt: f64,
    /// Minimum raster resolution for line art, dpi. Vector formats are exempt
    /// and are what these publishers prefer.
    pub min_dpi_line_art: f64,
}

/// Figure requirements from published author guidelines.
///
/// Widths are the ones the guidelines state. Where a publisher gives a range
/// the narrower single-column figure is used, because a figure that is too wide
/// gets scaled down — and scaling down is what shrinks the type below the
/// minimum.
pub const PRESETS: &[Preset] = &[
    Preset {
        name: "nature",
        single_mm: 89.0,
        double_mm: 183.0,
        min_font_pt: 5.0,
        min_dpi_line_art: 300.0,
    },
    Preset {
        name: "science",
        single_mm: 55.0,
        double_mm: 180.0,
        min_font_pt: 6.0,
        min_dpi_line_art: 300.0,
    },
    Preset {
        name: "cell",
        single_mm: 85.0,
        double_mm: 174.0,
        min_font_pt: 6.0,
        min_dpi_line_art: 300.0,
    },
    Preset {
        name: "plos",
        single_mm: 83.0,
        double_mm: 173.0,
        min_font_pt: 8.0,
        min_dpi_line_art: 300.0,
    },
    Preset {
        name: "elsevier",
        single_mm: 90.0,
        double_mm: 190.0,
        min_font_pt: 6.0,
        min_dpi_line_art: 300.0,
    },
    // A generic A4 single column, for anything not listed.
    Preset {
        name: "generic",
        single_mm: 85.0,
        double_mm: 170.0,
        min_font_pt: 6.0,
        min_dpi_line_art: 300.0,
    },
];

pub fn preset(name: &str) -> Option<Preset> {
    PRESETS
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .copied()
}

/// Points per inch. A PostScript point is 1/72 inch, by definition — this is
/// not a rendering choice and is the same in PDF, EPS and SVG's `pt` unit.
pub const PT_PER_INCH: f64 = 72.0;
/// Millimetres per inch, exactly.
pub const MM_PER_INCH: f64 = 25.4;

/// How a scene maps onto paper.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    /// Requested physical width, mm.
    pub width_mm: f64,
    /// Physical height that follows from the scene's aspect ratio, mm.
    pub height_mm: f64,
    /// Scene units to points. Everything in the figure is multiplied by this.
    pub scale: f64,
    /// Page size in PostScript points, which is what PDF and EPS want.
    pub width_pt: f64,
    pub height_pt: f64,
    /// The smallest type in the figure once scaled, in points.
    ///
    /// `None` when the figure has no text at all.
    pub min_font_pt: Option<f64>,
}

impl Fit {
    /// Fit a scene to a physical width.
    pub fn to_width_mm(scene: &crate::Scene, width_mm: f64) -> Fit {
        let width_pt = width_mm / MM_PER_INCH * PT_PER_INCH;
        let scale = if scene.width > 0.0 {
            width_pt / scene.width
        } else {
            1.0
        };
        let height_pt = scene.height * scale;
        // The smallest *text* size, not the smallest anything: a hairline rule
        // scaled to 0.1 pt still prints, and a 3 pt label does not read.
        let min_font = scene
            .items
            .iter()
            .filter_map(|i| match i {
                crate::Item::Text { size, .. } => Some(*size * scale),
                _ => None,
            })
            .fold(None, |a: Option<f64>, s| Some(a.map_or(s, |a| a.min(s))));
        Fit {
            width_mm,
            height_mm: height_pt / PT_PER_INCH * MM_PER_INCH,
            scale,
            width_pt,
            height_pt,
            min_font_pt: min_font,
        }
    }

    /// Is any type in this figure below the preset's floor?
    ///
    /// The check that exists because scaling a figure down scales its text
    /// down, which is invisible on screen and fatal in print.
    pub fn type_too_small(&self, p: &Preset) -> bool {
        self.min_font_pt.is_some_and(|s| s < p.min_font_pt)
    }

    /// Pixel dimensions at a given resolution, for a raster export.
    pub fn pixels(&self, dpi: f64) -> (u32, u32) {
        let w = (self.width_mm / MM_PER_INCH * dpi).round().max(1.0);
        let h = (self.height_mm / MM_PER_INCH * dpi).round().max(1.0);
        (w as u32, h as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Anchor, Item, Scene};

    fn scene_with(width: f64, height: f64, font_sizes: &[f64]) -> Scene {
        Scene {
            width,
            height,
            title: "t".into(),
            items: font_sizes
                .iter()
                .map(|s| Item::Text {
                    x: 0.0,
                    y: 0.0,
                    size: *s,
                    anchor: Anchor::Start,
                    color: "#000".into(),
                    bold: false,
                    text: "x".into(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_figure_asked_for_in_millimetres_comes_out_that_many_millimetres() {
        // 85 mm is 240.94 pt, because a point is exactly 1/72 inch and an inch
        // is exactly 25.4 mm. Neither of those is a rendering choice.
        let s = scene_with(720.0, 360.0, &[]);
        let f = Fit::to_width_mm(&s, 85.0);
        assert!(
            (f.width_pt - 240.9448818897638).abs() < 1e-9,
            "{}",
            f.width_pt
        );
        assert!((f.width_mm - 85.0).abs() < 1e-12);
        // Aspect ratio is preserved: half as tall as wide.
        assert!((f.height_mm - 42.5).abs() < 1e-9, "{}", f.height_mm);
        assert!((f.height_pt - f.width_pt / 2.0).abs() < 1e-9);
    }

    #[test]
    fn scaling_a_figure_down_scales_its_type_down_and_that_is_reported() {
        // The whole reason this module exists. A 720-unit-wide scene with a
        // 12 pt label, printed at 85 mm, has 4 pt type — below every journal's
        // floor, invisible on screen, and fatal at the proof stage.
        let s = scene_with(720.0, 360.0, &[12.0, 18.0]);
        let f = Fit::to_width_mm(&s, 85.0);
        let smallest = f.min_font_pt.expect("there is text");
        assert!(
            (smallest - 12.0 * 240.9448818897638 / 720.0).abs() < 1e-9,
            "{smallest}"
        );
        assert!(smallest < 4.1 && smallest > 4.0, "{smallest}");
        for p in PRESETS {
            assert!(
                f.type_too_small(p),
                "{} demands {} pt and this figure has {smallest}",
                p.name,
                p.min_font_pt
            );
        }

        // At full width it is fine for the looser presets.
        let wide = Fit::to_width_mm(&s, 183.0);
        assert!(!wide.type_too_small(&preset("nature").unwrap()));
    }

    #[test]
    fn the_smallest_type_is_what_is_reported_not_the_average() {
        let s = scene_with(100.0, 100.0, &[20.0, 6.0, 14.0]);
        let f = Fit::to_width_mm(&s, 100.0 / 72.0 * 25.4); // scale of 1
        assert!((f.scale - 1.0).abs() < 1e-12);
        // Not an exact comparison: the width goes to millimetres and back
        // through two exact-but-irrational-in-binary conversions, so 6.0
        // returns as 5.999999999999999. Asserting equality here would be
        // testing the round-trip rather than which size was picked.
        let got = f.min_font_pt.expect("there is text");
        assert!((got - 6.0).abs() < 1e-9, "{got}");
    }

    #[test]
    fn a_figure_with_no_text_has_no_type_to_be_too_small() {
        let s = scene_with(720.0, 360.0, &[]);
        let f = Fit::to_width_mm(&s, 20.0);
        assert_eq!(f.min_font_pt, None);
        assert!(!f.type_too_small(&preset("plos").unwrap()));
    }

    #[test]
    fn pixel_dimensions_follow_the_physical_size_and_the_resolution() {
        let s = scene_with(720.0, 360.0, &[]);
        let f = Fit::to_width_mm(&s, 85.0);
        // 85 mm at 300 dpi is 85/25.4*300 = 1003.9 px.
        assert_eq!(f.pixels(300.0), (1004, 502));
        assert_eq!(f.pixels(600.0), (2008, 1004));
        // Never zero, however small the figure: 0.01 mm at 72 dpi is 0.028 px,
        // which rounds to nothing, and a raster export of zero by zero pixels
        // is a file that no viewer will open.
        //
        // The `.max(1)` guard belongs on the *left* of this comparison and used
        // to be: `pixels().0.max(1) == 1` is true whether `pixels` returns 0 or
        // 1, so the guard it exists to protect could be deleted with the suite
        // still green. Both dimensions are read, because only the width was.
        assert_eq!(Fit::to_width_mm(&s, 0.01).pixels(72.0), (1, 1));
    }

    #[test]
    fn a_raster_export_is_never_zero_pixels_in_either_dimension() {
        // The height guard is a separate line of code from the width guard and
        // needs its own case: a scene 4000 units wide and 1 unit tall, printed
        // at 85 mm, is 0.02 mm tall.
        let tall = scene_with(4000.0, 1.0, &[]);
        let f = Fit::to_width_mm(&tall, 85.0);
        assert!(f.height_mm < 0.03, "{} mm", f.height_mm);
        let (w, h) = f.pixels(300.0);
        assert!(w >= 1 && h >= 1, "{w} x {h}");
        assert_eq!(h, 1, "rounded away to nothing, then floored to one");
    }

    #[test]
    fn every_preset_is_plausible_and_findable() {
        for p in PRESETS {
            assert!(p.single_mm > 40.0 && p.single_mm < 120.0, "{}", p.name);
            assert!(p.double_mm > p.single_mm, "{}", p.name);
            assert!(p.double_mm < 250.0, "{} wider than a page", p.name);
            assert!(p.min_font_pt >= 5.0 && p.min_font_pt <= 12.0, "{}", p.name);
            assert!(p.min_dpi_line_art >= 300.0, "{}", p.name);
            assert_eq!(preset(p.name), Some(*p));
            assert_eq!(preset(&p.name.to_uppercase()), Some(*p));
        }
        assert_eq!(preset("no such journal"), None);
    }

    #[test]
    fn a_zero_width_scene_does_not_divide_by_zero() {
        let s = scene_with(0.0, 0.0, &[]);
        let f = Fit::to_width_mm(&s, 85.0);
        assert!(f.scale.is_finite() && f.height_pt.is_finite());
    }
}
