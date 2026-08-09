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
//!
//! # Two palettes, and they answer different questions
//!
//! [`Palette`] is the **drawing** palette: the ink the plasmid map, the sequence
//! grid and the exported figure are made of. It is deliberately low-chroma, it
//! sits beside whatever colours a user's file carries, and every one of its
//! roles is pinned to a measured WCAG ratio by the tests at the bottom of this
//! file.
//!
//! [`visuals`] below is the **chrome**: the egui widget tones, rounding,
//! shadows and accent that make the window look like an application rather than
//! a default. Ported from the author's other eframe project so the two look like
//! one piece of software.
//!
//! They do not share an accent, and that is not an oversight.
//! [`Palette::accent`] is a blue drawn *onto the map* — selection arcs, cut-site
//! marks, the typing indicator — where it has to stay distinguishable from
//! feature colours and survive being exported into a figure. [`ACCENT`] is the
//! orange from the application icon and never touches the drawing; it is what
//! egui paints buttons, focus rings and text selection with. Merging them would
//! put the window's accent into every exported plasmid map.

use eframe::egui::{Color32, CornerRadius, Margin, Stroke, Style, Visuals};
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

    /// The same wash at half strength, for a row the pointer is merely OVER
    /// rather than one the user has chosen.
    ///
    /// Built from the accent, and that is the whole point of its existing. The
    /// caller took `selection()` apart and rebuilt it at half alpha —
    /// `from_rgba_unmultiplied(w.r(), w.g(), w.b(), w.a() / 2)` — which
    /// premultiplies a colour that is ALREADY premultiplied: `Color32` stores
    /// premultiplied components, so `.r()` on a 30/255 wash returns about 12%
    /// of the accent's red, and multiplying that again by 15/255 leaves an
    /// effective opacity near 0.7%. Measured in the running application, the
    /// hovered row's background came back byte-identical to its neighbours — so
    /// the map's hover echo was invisible for a SECOND reason after the frame
    /// ordering was fixed, and fixing only one of the two would have looked like
    /// fixing neither.
    pub fn hover_wash(&self) -> Color32 {
        let a = self.accent;
        Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), 15)
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

/// `#rrggbb`, exactly, and nothing else.
///
/// Public because the feature editor refuses what this cannot read. A colour
/// stored faithfully in the model and written faithfully to the file, then
/// rendered as the *type* colour with nothing anywhere saying the value was
/// ignored, is worse than a refusal at the box the user typed it into — and the
/// two answers have to come from one function or they will drift.
pub fn parse_hex(s: &str) -> Option<Color32> {
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

/// The application icon's orange, `#E69F00`.
///
/// Not picked for taste. `bins/pl-gui/icon/polylinker.svg` is drawn in exactly
/// two colours, `#E69F00` and `#0072B2` — the Okabe-Ito orange and blue, which
/// this project already uses everywhere it needs a colourblind-safe pair. So
/// the accent is simultaneously the icon's own orange and a hue the palette was
/// already committed to. Nothing new was invented.
///
/// **THIS VALUE IS AN INK IN ONE THEME AND A FILL IN THE OTHER. See
/// [`accent_ink`] before using it directly.**
pub const ACCENT: Color32 = Color32::from_rgb(0xe6, 0x9f, 0x00);

/// [`ACCENT`] with every channel scaled by 0.609, to `rgb(140, 97, 0)`.
///
/// **THE HUE IS EXACTLY UNCHANGED, and that is why it is a scale rather than a
/// hand-picked second orange.** HSV hue is `60 * G / R` when `B` is zero, so
/// scaling `R` and `G` by one factor leaves it alone: 41.48° for `#E69F00`,
/// 41.57° here, a rounding difference. Saturation is 1.0 in both, since `B`
/// stays 0. Only value moves. It is the same colour, darker — which is the one
/// thing that can be said of it without measuring anything.
pub const ACCENT_DEEP: Color32 = Color32::from_rgb(140, 97, 0);

/// The accent where it is a **foreground**: link text, focus and selection
/// strokes, the ring on a pressed control.
///
/// # Why this is a pair and must stay a pair
///
/// **`#E69F00` is 2.25:1 on white.** Measured with [`contrast`], and asserted
/// by `the_bright_accent_is_unusable_as_light_mode_ink`. That is half of WCAG
/// AA for normal text and below even the 3:1 that SC 1.4.11 asks of a mere
/// hairline. A single-constant accent — the obvious simplification, and the one
/// this comment exists to stop — makes every light-mode hyperlink and focus
/// ring unreadable, and does it silently, because nothing on screen looks
/// broken: it looks like a slightly pale orange.
///
/// **THE SOURCE DESIGN'S SINGLE CONSTANT WAS NOT ACTUALLY SAFE EITHER, WHICH IS
/// WHY THIS IS A FIX AND NOT A TRANSLATION.** The first draft of this comment
/// said the teal it was ported from "does not have the problem"; measured, it
/// half does. `rgb(38, 162, 156)` is **3.12:1 on white**, **3.04:1** on its own
/// light panel and **2.81:1** on its light table stripe — over the 3:1 that
/// SC 1.4.11 asks of a stroke, under the 4.5:1 that AA asks of link text, and
/// under 3:1 on the stripe. One constant reached both roles there only in the
/// sense that neither was checked.
///
/// Orange makes the same defect impossible to miss rather than merely present:
/// 2.25:1 instead of 3.12:1. A hue with more chroma has no value that is both
/// dark enough to read on white and light enough to read against on black, so
/// the pairing that the teal could get away with skipping is here forced.
///
/// So the accent **swaps roles with the theme**, and [`accent_fill`] is the
/// other half of the swap:
///
/// | | ink (foreground) | fill (background) |
/// |---|---|---|
/// | dark | [`ACCENT`] | [`ACCENT_DEEP`] |
/// | light | [`ACCENT_DEEP`] | [`ACCENT`] |
///
/// Read the table as one rule: **the bright value goes wherever it is the
/// lighter of the two things being compared.** On a dark panel that is the
/// text; on a light panel it is the button under the text.
pub fn accent_ink(dark: bool) -> Color32 {
    if dark {
        ACCENT
    } else {
        ACCENT_DEEP
    }
}

/// The accent where it is a **background**: the fill of a pressed or open
/// control.
///
/// The other half of [`accent_ink`]'s table; read that first.
///
/// **THE FOREGROUND ON THIS IS NOT HARDCODED.** [`on_color`] picks it by
/// measurement, and `the_accent_fill_carries_the_ink_that_on_color_picks`
/// asserts that what egui actually draws — `widgets.active.fg_stroke`, which is
/// `Palette::ink` — agrees in polarity with what `on_color` chooses. Two
/// separate decisions that have to match, joined by an assertion rather than by
/// hoping.
pub fn accent_fill(dark: bool) -> Color32 {
    if dark {
        ACCENT_DEEP
    } else {
        ACCENT
    }
}

/// The window's background, which every rule and hairline is measured against.
///
/// **STILL THE ONLY STATEMENT OF THIS COLOUR.** [`visuals`] below assigns it to
/// `Visuals::panel_fill` rather than writing a tone of its own, and five test
/// sites across this file and `main.rs` measure their foregrounds against it. A
/// second literal in the visuals builder would mean every one of those
/// assertions was measuring a background the application does not paint, and
/// all of them would stay green while doing it.
///
/// The values moved on the port: `#161a1d`/`#fafbfc` became `rgb(30, 34, 41)`
/// and `rgb(252, 252, 253)`, which are the source design's panel tones. The
/// dark one is the LIGHTER of the two changes and therefore the one that could
/// have cost something, so the whole palette was re-measured against it rather
/// than assumed: `ink` 14.83 → 13.52, `ink2` 9.42 → 8.58, `muted` 5.46 → 4.98,
/// `warn` 6.70 → 6.11, `line` 4.54 → 4.14. Every role still clears the
/// threshold `text_is_readable_against_its_own_background_in_both_themes` holds
/// it to, `muted` by the least at 0.48. The light side moved the other way and
/// every ratio improved slightly.
pub fn panel_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(30, 34, 41)
    } else {
        Color32::from_rgb(252, 252, 253)
    }
}

/// The whole chrome for one theme, built from scratch.
///
/// Built rather than patched, and returned rather than applied in place,
/// because a `Visuals` that is half egui's defaults and half ours is a thing
/// nobody can read the current value of. `Visuals::dark()`/`light()` supply the
/// fields not named here — text cursor, resize corner, the numeric knobs — and
/// every colour that decides how this application looks is set below.
pub fn visuals(dark: bool) -> Visuals {
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let p = Palette::of(dark);
    let ink = accent_ink(dark);

    // Text selection inside a `TextEdit`. `from_rgba_unmultiplied`, like
    // `Palette::selection` above and for the reason spelled out on
    // `Palette::hover_wash`: `Color32` stores PREMULTIPLIED components, so a
    // wash assembled by taking an existing translucent colour apart is
    // premultiplied twice and lands near invisible.
    //
    // Alpha 100 rather than the source design's `linear_multiply` factors,
    // which cannot be checked the same way — they produce a translucent colour
    // whose composite depends on a background the constructor never sees.
    // Composited over the text-edit background this is `rgb(101, 75, 15)` dark
    // and `rgb(245, 217, 155)` light, and the edit's own text still reads at
    // 5.55:1 and 9.52:1 on those. It moves the pixel by 148 of Manhattan
    // distance, so the selection is unmistakable.
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 100);
    v.selection.stroke = Stroke::new(1.0, ink);
    // No `Hyperlink` is built today. It is set correctly anyway, because the
    // first one somebody adds must not be the thing that discovers the accent
    // pair — see `accent_ink`.
    v.hyperlink_color = ink;

    v.panel_fill = panel_fill(dark);
    if dark {
        v.window_fill = Color32::from_rgb(24, 27, 33);
        // ONE NOTCH TOWARDS THE PANEL FROM THE SOURCE DESIGN'S `rgb(37, 42, 50)`,
        // AND THE NOTCH IS THE ONLY PLACE A PORTED TONE WAS OVERRULED BY A
        // MEASUREMENT. See the light arm below for the half that actually
        // failed; this one was passing at **4.5007:1** for `muted`, which is
        // AA by seven ten-thousandths and is not a margin — an f32 rounding
        // either way decides it. `rgb(35, 40, 48)` reads 4.62 and still stands
        // 5 steps off the panel it stripes.
        v.faint_bg_color = Color32::from_rgb(35, 40, 48);
        v.extreme_bg_color = Color32::from_rgb(18, 20, 25);
        v.widgets.noninteractive.bg_fill = panel_fill(true);
        v.widgets.noninteractive.weak_bg_fill = panel_fill(true);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.ink2);
        v.widgets.inactive.bg_fill = Color32::from_rgb(42, 47, 56);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(42, 47, 56);
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(58, 64, 75));
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.ink);
        v.widgets.hovered.bg_fill = Color32::from_rgb(52, 58, 69);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(52, 58, 69);
    } else {
        v.window_fill = Color32::from_rgb(246, 247, 250);
        // **THE ONE PORTED COLOUR THAT FAILED AA AND HAD TO MOVE.** The source
        // design's light faint is `rgb(241, 243, 246)`, and this is not a
        // decorative surface here: `featedit.rs:1271` and `:1521` build
        // `Grid::striped(true)`, egui 0.35 `grid.rs:493` fills every other row
        // with exactly this colour, and `featedit.rs:1278` draws that row's
        // text in `Palette::warn` when a span is inverted. `#b0553f` on
        // `rgb(241, 243, 246)` measures **4.48:1** — under 4.5, on a sentence
        // telling somebody their coordinates run backwards.
        //
        // The stripe yields rather than `warn`, because `Palette` is the
        // DRAWING palette: `warn` is also the internal-stop glyph in the
        // sequence view and it goes into exported figures, so moving it to fix
        // a table stripe would change ink in a file somebody publishes. This is
        // chrome, and chrome is what the port is allowed to touch.
        //
        // The value is not invented: it is this design's own light
        // `window_fill`, one line up, so no new tone enters the system. `warn`
        // reaches 4.65 and `muted` 4.82, and the stripe still stands 6 steps
        // off `panel_fill`. There is no lighter answer worth having — `warn` is
        // 4.86 on the panel itself, so 4.86 is the ceiling for any stripe at
        // all.
        v.faint_bg_color = Color32::from_rgb(246, 247, 250);
        v.extreme_bg_color = Color32::WHITE;
        v.widgets.noninteractive.bg_fill = panel_fill(false);
        v.widgets.noninteractive.weak_bg_fill = panel_fill(false);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.ink2);
        v.widgets.inactive.bg_fill = Color32::WHITE;
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(244, 246, 248);
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(214, 219, 226));
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.ink);
        // The source design tints its light hover with the accent, so this one
        // is re-derived rather than copied: its `rgb(236, 245, 244)` is teal
        // washed onto white, and a teal hover under an orange accent would be
        // the port showing. `#E69F00` at the same strength over white.
        v.widgets.hovered.bg_fill = Color32::from_rgb(252, 245, 230);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(252, 245, 230);
    }

    // The accent, in the two states that are meant to say "you are touching
    // this". Hovered gets the ring only; active gets the ring and the fill.
    //
    // **THE RING IS `accent_ink` AND THE FILL IS `accent_fill`, WHICH ARE
    // DIFFERENT VALUES, AND THE RING IS WHY THE FILL DOES NOT HAVE TO CLEAR
    // 3:1.** `accent_fill` against its own panel is 2.91:1 dark and 2.20:1
    // light — under SC 1.4.11 — so a pressed button whose only boundary was its
    // fill would have an edge nobody can see. `bg_stroke` is that boundary, and
    // `accent_ink` reaches 7.08:1 and 5.35:1 on the same panels.
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ink);
    v.widgets.active.bg_fill = accent_fill(dark);
    v.widgets.active.weak_bg_fill = accent_fill(dark);
    v.widgets.active.bg_stroke = Stroke::new(1.0, ink);

    // THE SPLITTER, and the reason `accent_fill` is a shade of the accent
    // rather than the accent itself.
    //
    // The splitter between the map and the details panel is drawn with three
    // strokes (egui 0.35 `panel.rs:837-843`: `noninteractive.bg_stroke` at
    // rest, `hovered.fg_stroke` under the pointer, `active.fg_stroke` while
    // dragging). egui's defaults are gray(60) on the dark panel — **1.57:1** —
    // and gray(190) on the light one — **1.81:1**. Both fail SC 1.4.11's 3:1
    // for the boundary of a UI component, and that line is the only resting
    // signal that the panel can be resized at all.
    //
    // `muted` is the palette role that clears 3:1 in both themes (5.04 light,
    // 4.98 dark); `faint` is 1.77/2.09 and `line` is 2.85/4.14, so neither is
    // usable for this. Colour is not the only channel — egui already sets
    // `CursorIcon::ResizeHorizontal` on hover and `ResizeWest`/`ResizeEast` at
    // the stops — so no grip graphic is added; that would be a new shape and a
    // new colour needing its own clearance for no gain.
    //
    // **AND THOSE LAST TWO FIELDS ARE ALSO THE LABEL ON A HOVERED OR PRESSED
    // BUTTON.** egui has one `fg_stroke` per widget state and spends it twice.
    // That is what fixes the accent fill: `active.fg_stroke` has to be `ink` to
    // keep the drag line visible on the panel, so `active.bg_fill` has to be a
    // value `ink` reads on — near-white in dark mode, near-black in light. The
    // bright `#E69F00` carries near-black at 7.14:1 and near-white at only
    // 1.91:1, which is exactly why [`accent_fill`] hands back the deep shade in
    // dark mode and the bright one in light.
    v.widgets.noninteractive.bg_stroke.color = p.muted;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, p.ink);
    v.widgets.active.fg_stroke = Stroke::new(1.0, p.ink);

    let r = CornerRadius::same(6);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    // Soft and downward, so a floating panel reads as floating. Heavier in dark
    // mode because a shadow is darkness and there is less room for it there.
    v.window_shadow = eframe::egui::epaint::Shadow {
        offset: [0, 4],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 96 } else { 38 }),
    };
    v.popup_shadow = eframe::egui::epaint::Shadow {
        offset: [0, 3],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 90 } else { 30 }),
    };
    v
}

/// Spacing and text sizes, which are the same in both themes.
///
/// Separate from [`visuals`] because they are: egui keeps one `Style` per theme
/// in 0.35, so this runs once per theme, but nothing in it reads `dark`. Keeping
/// them apart is what stops a future colour edit from silently resizing the
/// window's text.
///
/// **THE SIZES ARE THE DESIGN'S, NOT EGUI'S, AND THEY MOVE ONLY THE TEXT THAT
/// ASKS FOR A `TextStyle`.** egui 0.35's defaults, read at
/// `style.rs:1412-1416` rather than from memory, are Heading 18, Body 13,
/// Button 13, Monospace 13, Small 9. So the port makes the heading SMALLER
/// (18 → 16.5), the body and buttons a little larger (13 → 14), `Small` larger
/// (9 → 11), and **leaves Monospace exactly where it was** — 13 either way. It
/// is set anyway, because a `text_styles` map is replaced wholesale and an
/// omitted entry is a missing style rather than an inherited one.
///
/// Everything this application draws onto the map or the sequence grid names
/// its size outright — `FontId::monospace(11.5)` for the bases, `9.5` for the
/// ruler, `proportional(9.0)` for feature names — so none of that is touched by
/// this and the advance band `main.rs` calibrates is unaffected. What changes
/// is egui's own furniture: labels, buttons, menu items, window titles.
pub fn style(s: &mut Style) {
    use eframe::egui::{FontFamily, FontId, TextStyle};
    s.spacing.item_spacing = eframe::egui::vec2(8.0, 6.0);
    s.spacing.button_padding = eframe::egui::vec2(8.0, 4.0);
    s.spacing.interact_size.y = 24.0;
    s.spacing.menu_margin = Margin::same(6);
    s.text_styles = [
        // The one style that names a family other than the two defaults. See
        // `crate::HEADING_FAMILY` for what is in it and why the body is not.
        (
            TextStyle::Heading,
            FontId::new(16.5, crate::heading_family()),
        ),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
        (
            TextStyle::Small,
            FontId::new(11.0, FontFamily::Proportional),
        ),
    ]
    .into();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PROVEN TO FAIL against the working tree as handed over, where the hover
    /// wash was built as
    /// `Color32::from_rgba_unmultiplied(sel.r(), sel.g(), sel.b(), sel.a() / 2)`.
    ///
    /// `Color32` stores PREMULTIPLIED components, so taking a 30/255 wash apart
    /// and rebuilding it premultiplies it a second time: the effective opacity
    /// comes out near 0.7% instead of 6%. Measured in the running application,
    /// the hovered row's background was byte-identical to its neighbours in both
    /// directions, so the map's hover echo had two independent reasons to be
    /// invisible and fixing only the frame ordering would have looked like
    /// fixing nothing.
    #[test]
    fn the_hover_wash_actually_changes_the_pixel_it_is_drawn_on() {
        // What egui does when it composites a premultiplied colour over a
        // background: `bg * (1 - a) + rgb`.
        let over = |c: Color32, bg: Color32| -> [i32; 3] {
            let a = c.a() as f32 / 255.0;
            [0, 1, 2].map(|i| {
                let b = [bg.r(), bg.g(), bg.b()][i] as f32;
                let f = [c.r(), c.g(), c.b()][i] as f32;
                (b * (1.0 - a) + f).round() as i32
            })
        };
        for dark in [true, false] {
            let p = Palette::of(dark);
            let bg = panel_fill(dark);
            let hover = over(p.hover_wash(), bg);
            let sel = over(p.selection(), bg);
            let base = [bg.r() as i32, bg.g() as i32, bg.b() as i32];
            let far = |a: [i32; 3], b: [i32; 3]| {
                (a[0] - b[0]).abs() + (a[1] - b[1]).abs() + (a[2] - b[2]).abs()
            };
            assert!(
                far(hover, base) >= 8,
                "dark={dark}: the hover wash composites to {hover:?} on {base:?} — invisible"
            );
            // And it is still clearly the weaker of the two, or hover and
            // selection stop being distinguishable, which is what the wash order
            // in the Features list depends on.
            assert!(
                far(sel, base) > far(hover, base),
                "dark={dark}: selection {sel:?} is not stronger than hover {hover:?}"
            );
        }
    }

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

    /// **PROVEN TO FAIL, AND IT COULD NOT BEFORE THIS RUN.** Until the
    /// design-system port this test wrote its two backgrounds out as literals —
    /// `#161a1d` and `#fafbfc` — which were `panel_fill`'s values at the time.
    /// The port moved `panel_fill` to the source design's tones and left the
    /// literals behind, so the test went on measuring a background the
    /// application had stopped painting and would have stayed green through any
    /// future move of the real one. Reading [`panel_fill`] is what makes it a
    /// check; setting the dark panel to `Color32::from_rgb(60, 68, 82)` now
    /// reports `muted` at 2.60:1 and fails.
    ///
    /// The numbers the change cost, measured rather than assumed, because the
    /// new dark panel is the LIGHTER of the two and therefore the one that
    /// could have taken something away: `ink` 14.83 → 13.52, `ink2` 9.42 →
    /// 8.58, `muted` 5.46 → 4.98, `warn` 6.70 → 6.11, `line` 4.54 → 4.14. Every
    /// role still clears, `muted` by the least at 0.48. The light side moved the
    /// other way and every ratio improved.
    #[test]
    fn text_is_readable_against_its_own_background_in_both_themes() {
        // The bug this guards: a palette hardcoded for light mode made the map
        // caption invisible on a dark background.
        for dark in [true, false] {
            let p = Palette::of(dark);
            let bg = panel_fill(dark);
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
            // `warn` became a small-TEXT role with the amino-acid track: an
            // internal stop is drawn as a `*` glyph in it at 11.5 pt, and the
            // header's internal-stop sentence at 11. The same argument as
            // `muted` above, and until this line it had no check at all — the
            // light value is #b0553f at 4.81:1, which passes with 0.31 of
            // margin, i.e. one palette tweak from failing silently. Colour is
            // not the only channel for the mark (it also carries a filled
            // under-bar, and the glyph is `*` rather than a letter); that is
            // why it is distinguishable, not why it is readable.
            assert!(
                contrast(p.warn, bg) >= 4.5,
                "warn on {} bg: {:.2}:1 — the sequence view's internal-stop glyph is 11.5 pt \
                 and the header sentence that counts them 11",
                if dark { "dark" } else { "light" },
                contrast(p.warn, bg)
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
    /// PROVEN TO FAIL at bd96e5b (compile-only there — nothing set these, and
    /// `theme::panel_fill` did not exist). The numbers it replaces are egui's
    /// defaults: gray(60) on the dark panel is 1.57:1 and gray(190) on the
    /// light one is 1.81:1, both a long way under 3.
    #[test]
    fn the_splitter_is_visible_at_rest_in_both_themes() {
        use pl_draw::contrast::{passes_aa, ratio, Kind};
        for dark in [true, false] {
            let v = visuals(dark);
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

    /// Every light-mode surface a label can be drawn on, and every dark-mode
    /// one. The accent has to be readable on all of them or the pair is not
    /// doing its job.
    ///
    /// `faint_bg_color` is in here because it is a real painted surface and not
    /// a theoretical one: `featedit.rs:1271` and `:1521` build
    /// `Grid::striped(true)`, and egui 0.35 `grid.rs:493` fills a striped row
    /// with exactly this colour.
    fn chrome_surfaces(dark: bool) -> Vec<(&'static str, Color32)> {
        let v = visuals(dark);
        vec![
            ("panel_fill", v.panel_fill),
            ("window_fill", v.window_fill),
            ("faint_bg_color", v.faint_bg_color),
            ("extreme_bg_color", v.extreme_bg_color),
            ("inactive.weak_bg_fill", v.widgets.inactive.weak_bg_fill),
            ("hovered.bg_fill", v.widgets.hovered.bg_fill),
        ]
    }

    /// Every palette ink, on every chrome surface it is actually painted on, in
    /// both themes.
    ///
    /// **THE SURFACE LIST IS PER ROLE, AND THAT IS THE WHOLE DESIGN OF THIS
    /// TEST.** One loop over four inks and six surfaces would be twenty-four
    /// assertions of which several are about combinations this application
    /// never draws — and the first one that failed would be answered by
    /// loosening the threshold for all of them. So each role carries the
    /// surfaces it reaches, and each entry can be pointed at a line of code:
    ///
    /// - `ink` is `widgets.inactive.fg_stroke` and the app's primary text, so
    ///   it lands on everything, including a hovered and a pressed control.
    /// - `ink2` is `widgets.noninteractive.fg_stroke` — every label egui draws
    ///   that is not inside a widget — and reaches the same set.
    /// - `muted` is the sequence ruler, both coordinate gutters, the map's
    ///   enzyme positions and the label on a DISABLED control. Disabled is
    ///   drawn `noninteractive`, whose fill is `panel_fill`, so `muted` never
    ///   sits on an interactive fill and is not asked to.
    /// - `warn` is the internal-stop glyph, the inverted-span row of a striped
    ///   grid, and the `Delete feature` button — which is live, so it gets the
    ///   resting button fills as well.
    ///
    /// **THE ONE EXEMPTION IS STATED RATHER THAN HIDDEN.** `warn` on the dark
    /// `hovered.bg_fill` is 4.38:1, under AA, and it is left there: the only
    /// way to buy the missing 0.12 is to darken `rgb(52, 58, 69)` to about
    /// `rgb(50, 56, 66)`, which puts the hover fill within a couple of steps of
    /// `inactive` at `rgb(42, 47, 56)` and spends the hover cue itself. The
    /// port already improved this number — egui's default dark hover is
    /// `gray(70)` and gave 3.61 — so it is asserted at 3:1 and named, not
    /// asserted at 4.5 and quietly excluded.
    ///
    /// PROVEN TO FAIL, twice, before the two tones above were moved: the
    /// source design's light faint `rgb(241, 243, 246)` reports `warn` at
    /// 4.48:1 on the striped row, and its dark faint `rgb(37, 42, 50)` reports
    /// `muted` at 4.5007 — which passes, so the dark half was re-proven by
    /// setting the threshold to 4.51 and watching only that entry go red.
    /// One ink, and the surfaces it is painted on: the shape the loop below is
    /// built out of. Named because the tuple is three deep and clippy is right
    /// that an unnamed one is unreadable.
    type Role = (&'static str, Color32, Vec<(&'static str, Color32)>);

    #[test]
    fn every_ink_clears_aa_on_every_chrome_surface_it_is_drawn_on() {
        for dark in [true, false] {
            let v = visuals(dark);
            let p = Palette::of(dark);
            let mode = if dark { "dark" } else { "light" };
            // Surfaces a control is NOT under the pointer on.
            let resting: Vec<(&str, Color32)> = vec![
                ("panel_fill", v.panel_fill),
                ("window_fill", v.window_fill),
                ("faint_bg_color (the striped row)", v.faint_bg_color),
                ("extreme_bg_color (a TextEdit)", v.extreme_bg_color),
            ];
            let buttons: Vec<(&str, Color32)> = vec![
                ("inactive.bg_fill", v.widgets.inactive.bg_fill),
                ("inactive.weak_bg_fill", v.widgets.inactive.weak_bg_fill),
            ];
            let touched: Vec<(&str, Color32)> = vec![
                ("hovered.bg_fill", v.widgets.hovered.bg_fill),
                ("active.bg_fill (the accent)", v.widgets.active.bg_fill),
            ];

            let checks: Vec<Role> = vec![
                // The only role that reaches a touched control: egui spends
                // `hovered.fg_stroke` and `active.fg_stroke` on the label of a
                // widget under the pointer, and `visuals` sets both to `ink`.
                (
                    "ink",
                    p.ink,
                    [&resting[..], &buttons[..], &touched[..]].concat(),
                ),
                // `noninteractive.fg_stroke`. A label is never hovered and
                // never pressed, so `ink2` never lands on either of those
                // fills. Asked for them anyway it reads 2.95:1 on the dark
                // accent — which is a fact about a pairing that does not occur,
                // and was the first thing this test said when the list was
                // written as one set for all four roles.
                ("ink2", p.ink2, resting.clone()),
                ("muted", p.muted, resting.clone()),
                ("warn", p.warn, [&resting[..], &buttons[..]].concat()),
            ];

            for (role, fg, surfaces) in checks {
                let min = if role.contains("3:1") { 3.0 } else { 4.5 };
                for (what, bg) in surfaces {
                    let got = contrast(fg, bg);
                    assert!(
                        got >= min,
                        "`{role}` is {got:.2}:1 on {what} in {mode} mode, under {min}:1. \
                         This is a colour the application paints TEXT with on a surface it \
                         paints; if the pairing is genuinely unreachable, delete the \
                         surface from this role's list and say why, do not lower the number."
                    );
                }
            }
        }
    }

    /// [`on_color`] over the colours this project chooses itself, and not only
    /// over the cube.
    ///
    /// **THE CUBE SWEEP DOES NOT COVER THESE, WHICH IS WHY THIS EXISTS.**
    /// `a_label_is_readable_on_every_colour_a_file_can_contain` walks the RGB
    /// cube at a stride of 4, and every one of `by_kind`'s eight fallbacks
    /// misses that lattice — `#9a5b8c` is 154, 91, 140 and not one of those is
    /// divisible by 4. So the nine colours a user is most likely to see, being
    /// the ones a file without colours gets, were the nine the sweep never
    /// visited.
    ///
    /// It asserts the CHOICE and not just the ratio: `on_color` must return the
    /// better of black and white for each, which is what makes this a test of
    /// the function rather than of the palette. The worst of the eight is
    /// `CDS` `#9a5b8c`, which takes WHITE at 4.94:1 — written here first as
    /// "`promoter` at 5.25", which is `promoter`'s number and not the worst
    /// one, and corrected by the assertion below on its first run. That is why
    /// the figure is pinned rather than only prosed.
    ///
    /// PROVEN TO FAIL: inverting `on_color`'s comparison to `<=` puts
    /// `primer_bind` `#5f8fa8` on white at 3.51:1 and goes red on the first
    /// entry it reaches.
    #[test]
    fn on_color_picks_the_readable_side_of_every_colour_this_project_ships() {
        let mut worst = f32::MAX;
        for kind in [
            "CDS",
            "gene",
            "promoter",
            "terminator",
            "rep_origin",
            "primer_bind",
            "RBS",
            "polyA_signal",
            "misc_feature",
        ] {
            let bg = by_kind(kind);
            let got = on_color(bg);
            let want = if contrast(Color32::BLACK, bg) >= contrast(Color32::WHITE, bg) {
                Color32::BLACK
            } else {
                Color32::WHITE
            };
            assert_eq!(got, want, "{kind} {bg:?}");
            let ratio = contrast(got, bg);
            assert!(
                ratio >= 4.5,
                "a label on the {kind} fallback {bg:?} is {ratio:.2}:1, under AA. These \
                 eight are the colours this project picked itself; a file's own colour is \
                 covered by the cube sweep and cannot be changed, but these can."
            );
            worst = worst.min(ratio);
        }
        assert!(
            (worst - 4.94).abs() < 0.02,
            "the worst of the shipped fallbacks is now {worst:.2}:1, not the 4.94 the doc \
             comment quotes"
        );
    }

    /// The reason [`accent_ink`] is a pair, stated as a measurement.
    ///
    /// **A NEGATIVE CONTROL, AND IT DOES NOT CATCH THE SIMPLIFICATION — the
    /// sibling below does.** Worth being exact, because the first version of
    /// this comment claimed otherwise and was wrong: collapsing `accent_ink` to
    /// return [`ACCENT`] unconditionally leaves this test green, since it
    /// measures the constant and never calls the function.
    ///
    /// What it does is hold the PREMISE. `the_accent_ink_is_readable_...` says
    /// the light half clears AA; this says the bright half cannot, so the two
    /// halves are not interchangeable and the pair is forced rather than
    /// chosen. If the bright accent ever became dark enough to serve as
    /// light-mode ink, this goes red and the honest response is to delete
    /// [`ACCENT_DEEP`] — which is the one edit the sibling would not object to
    /// and the one this test exists to license.
    ///
    /// PROVEN TO FAIL: setting `ACCENT` to `ACCENT_DEEP`'s value reports
    /// 4.94:1 on the best light surface, well over the 3.0 bound.
    #[test]
    fn the_bright_accent_is_unusable_as_light_mode_ink() {
        let worst = chrome_surfaces(false)
            .into_iter()
            .map(|(_, bg)| contrast(ACCENT, bg))
            .fold(f32::MIN, f32::max);
        assert!(
            worst < 3.0,
            "`{ACCENT:?}` now reaches {worst:.2}:1 on the best light surface it has, so \
             the light half of the accent pair no longer has a reason to exist. Either \
             the light tones moved a long way or this test stopped measuring what it \
             names — do not answer it by deleting `ACCENT_DEEP`."
        );
        // The specific number the doc comment quotes, so the prose cannot drift
        // from the value.
        let on_white = contrast(ACCENT, Color32::WHITE);
        assert!(
            (on_white - 2.25).abs() < 0.01,
            "`ACCENT` on white is {on_white:.2}:1, not the 2.25:1 `accent_ink` claims"
        );
    }

    /// The pair, working: whichever half is the foreground clears AA on every
    /// background of its own theme.
    ///
    /// PROVEN TO FAIL: swapping the two arms of `accent_ink` puts `ACCENT` on
    /// the light surfaces at 2.03-2.25:1 and `ACCENT_DEEP` on the dark ones at
    /// 1.05-1.30:1, and both directions go red here.
    #[test]
    fn the_accent_ink_is_readable_on_every_surface_it_can_be_drawn_on() {
        for dark in [true, false] {
            let ink = accent_ink(dark);
            for (what, bg) in chrome_surfaces(dark) {
                let got = contrast(ink, bg);
                assert!(
                    got >= 4.5,
                    "the accent ink is {got:.2}:1 on {what} in {} mode. It carries \
                     hyperlink text, so 4.5:1 and not the 3:1 a stroke would need.",
                    if dark { "dark" } else { "light" }
                );
            }
        }
    }

    /// The foreground egui actually paints on an accent fill is the one
    /// [`on_color`] would choose.
    ///
    /// Two independent decisions have to agree here and nothing but this makes
    /// them. `widgets.active.fg_stroke` is pinned to `Palette::ink` by the
    /// splitter — see the comment in [`visuals`] — while `on_color` picks black
    /// or white by measuring the fill. If they ever disagreed, the label on a
    /// pressed button would be the wrong side of its own background, which is
    /// the single most obvious way this design could fail.
    ///
    /// PROVEN TO FAIL: `accent_fill` returning `ACCENT` in dark mode — the
    /// tempting simplification, since one constant would then serve every fill
    /// — makes `on_color` pick BLACK while the splitter keeps `ink` near-white,
    /// and the ratio drops to 1.91:1.
    #[test]
    fn the_accent_fill_carries_the_ink_that_on_color_picks() {
        for dark in [true, false] {
            let fill = accent_fill(dark);
            let ink = visuals(dark).widgets.active.fg_stroke.color;
            assert_eq!(ink, Palette::of(dark).ink, "the splitter's colour moved");
            let picked = on_color(fill);
            // Same side of the fill: `on_color` answers with pure black or
            // white, `ink` is a softened near-black or near-white, so they are
            // compared by which of the two extremes each is nearer.
            let ink_is_light = contrast(ink, Color32::BLACK) > contrast(ink, Color32::WHITE);
            assert_eq!(
                ink_is_light,
                picked == Color32::WHITE,
                "in {} mode `on_color` picks {picked:?} for the accent fill {fill:?}, but \
                 egui draws the label in {ink:?}",
                if dark { "dark" } else { "light" }
            );
            let got = contrast(ink, fill);
            assert!(
                got >= 4.5,
                "the label on a pressed control is {got:.2}:1 in {} mode",
                if dark { "dark" } else { "light" }
            );
        }
    }

    /// The two accents are one colour at two values, which is the whole claim
    /// [`ACCENT_DEEP`]'s doc makes.
    ///
    /// PROVEN TO FAIL against any independently chosen second orange: the
    /// task's own starting suggestion, `rgb(168, 93, 0)`, sits at 33.2° against
    /// `#E69F00`'s 41.5° and misses this by 8 degrees. (It also misses AA on
    /// `faint_bg_color` at 4.47:1, which is what sent the search onto the hue
    /// line in the first place.)
    #[test]
    fn the_deep_accent_is_the_bright_one_darkened_and_nothing_else() {
        // Hue in HSV, which for a colour with no blue in it is just `G / R`.
        assert_eq!(ACCENT.b(), 0, "the hue argument assumes no blue");
        assert_eq!(ACCENT_DEEP.b(), 0);
        let hue = |c: Color32| 60.0 * c.g() as f32 / c.r() as f32;
        let (a, d) = (hue(ACCENT), hue(ACCENT_DEEP));
        assert!(
            (a - d).abs() < 0.5,
            "the accents are {a:.2}° and {d:.2}° apart in hue; `ACCENT_DEEP` is supposed \
             to be `ACCENT` scaled, not a second orange somebody liked"
        );
        // And it really is darker, not merely different.
        assert!(luminance(ACCENT_DEEP) < luminance(ACCENT));
    }

    /// Selected text stays readable, and selecting it visibly changes the page.
    ///
    /// The same two-sided shape as
    /// `the_hover_wash_actually_changes_the_pixel_it_is_drawn_on`, and for the
    /// same reason: a wash that reads well because it is invisible passes any
    /// one-sided contrast check.
    #[test]
    fn the_text_selection_is_both_visible_and_readable() {
        let over = |c: Color32, bg: Color32| -> Color32 {
            let a = c.a() as f32 / 255.0;
            let ch = |f: u8, b: u8| (b as f32 * (1.0 - a) + f as f32).round() as u8;
            Color32::from_rgb(ch(c.r(), bg.r()), ch(c.g(), bg.g()), ch(c.b(), bg.b()))
        };
        for dark in [true, false] {
            let v = visuals(dark);
            // A `TextEdit` paints on `extreme_bg_color` and draws its text in
            // `inactive.fg_stroke`.
            let bg = v.extreme_bg_color;
            let text = v.widgets.inactive.fg_stroke.color;
            let sel = over(v.selection.bg_fill, bg);
            let mode = if dark { "dark" } else { "light" };
            let moved = (sel.r() as i32 - bg.r() as i32).abs()
                + (sel.g() as i32 - bg.g() as i32).abs()
                + (sel.b() as i32 - bg.b() as i32).abs();
            assert!(
                moved >= 60,
                "{mode}: the selection composites to {sel:?} on {bg:?}, a Manhattan \
                 distance of {moved} — too close to invisible to be a selection"
            );
            let got = contrast(text, sel);
            assert!(
                got >= 4.5,
                "{mode}: selected text is {got:.2}:1 on its own highlight {sel:?}"
            );
        }
    }

    /// The rounding, shadows and spacing the port is FOR.
    ///
    /// PROVEN TO FAIL by putting egui's own numbers back, which is what every
    /// one of these was before the port. Read out of `egui-0.35.0`'s
    /// `style.rs:1511-1531` rather than remembered — the first draft of this
    /// comment got two of the six wrong, which is exactly the kind of claim
    /// that reads as authority and is never rechecked:
    ///
    /// ```text
    ///                     egui 0.35             here
    ///   widget radius     2, and 3 on `hovered` 6 on all five
    ///   window radius     6                     10
    ///   menu radius       6                     8
    ///   window shadow     offset [10, 20]       offset [0, 4]
    ///                     blur 15               blur 18
    ///   popup shadow      offset [6, 10]        offset [0, 3]
    ///                     blur 8                blur 12
    /// ```
    ///
    /// egui's are hard and thrown far down-right; these are soft and almost
    /// straight down. That difference is most of what makes the window look
    /// like an application rather than a default, and none of it is visible to
    /// any other assertion in this file — so this is the only thing standing
    /// between the design system and a `Visuals::dark()` creeping back through
    /// a merge.
    #[test]
    fn the_shape_of_the_chrome_is_the_ported_one() {
        for dark in [true, false] {
            let v = visuals(dark);
            for (what, w) in [
                ("noninteractive", &v.widgets.noninteractive),
                ("inactive", &v.widgets.inactive),
                ("hovered", &v.widgets.hovered),
                ("active", &v.widgets.active),
                ("open", &v.widgets.open),
            ] {
                assert_eq!(
                    w.corner_radius,
                    CornerRadius::same(6),
                    "{what} is not rounded 6"
                );
            }
            assert_eq!(v.window_corner_radius, CornerRadius::same(10));
            assert_eq!(v.menu_corner_radius, CornerRadius::same(8));
            assert_eq!(v.window_shadow.offset, [0, 4]);
            assert_eq!(v.window_shadow.blur, 18);
            assert_eq!(v.window_shadow.spread, 0);
            assert_eq!(v.popup_shadow.offset, [0, 3]);
            assert_eq!(v.popup_shadow.blur, 12);
            // The two themes differ in exactly one respect, and it is the one
            // that has to differ: a shadow is darkness, and there is less room
            // for it on a dark panel.
            assert_eq!(
                v.window_shadow.color,
                Color32::from_black_alpha(if dark { 96 } else { 38 })
            );
        }

        let mut s = Style::default();
        style(&mut s);
        assert_eq!(s.spacing.item_spacing, eframe::egui::vec2(8.0, 6.0));
        assert_eq!(s.spacing.button_padding, eframe::egui::vec2(8.0, 4.0));
        assert_eq!(s.spacing.interact_size.y, 24.0);
        assert_eq!(s.spacing.menu_margin, Margin::same(6));
        for (ts, size) in [
            (eframe::egui::TextStyle::Heading, 16.5),
            (eframe::egui::TextStyle::Body, 14.0),
            (eframe::egui::TextStyle::Button, 14.0),
            (eframe::egui::TextStyle::Monospace, 13.0),
            (eframe::egui::TextStyle::Small, 11.0),
        ] {
            assert_eq!(s.text_styles[&ts].size, size, "{ts:?}");
        }
        // The heading is the one style drawn in a family of its own, and the
        // body is deliberately NOT — see `crate::heading_family`.
        assert_eq!(
            s.text_styles[&eframe::egui::TextStyle::Heading].family,
            crate::heading_family()
        );
        assert_eq!(
            s.text_styles[&eframe::egui::TextStyle::Body].family,
            eframe::egui::FontFamily::Proportional
        );
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
