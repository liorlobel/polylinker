//! WCAG contrast, computed rather than eyeballed.
//!
//! # Why this is arithmetic and not judgement
//!
//! "Is this label readable?" has a defined answer. WCAG 2.2 keeps the contrast
//! thresholds of 2.1 unchanged, and they are ratios of relative luminance:
//!
//! - **4.5:1** for body text (SC 1.4.3, Level AA)
//! - **3:1** for large text — 18 pt, or 14 pt bold
//! - **3:1** for graphical objects and UI components (SC 1.4.11)
//!
//! Everything here follows from that, so an accessibility claim in this project
//! is a number somebody can recompute rather than an assurance.
//!
//! The gamma step is the part that gets skipped. Relative luminance is *not* a
//! weighted average of the sRGB bytes: each channel is linearised first, and
//! the difference is not a rounding. `#767676` on white is **4.54:1** and
//! passes AA; computed without linearising it comes out at **2.05:1**, less
//! than half, and would be rejected. The error runs the other way against a
//! dark background, where it lets through colours that do not in fact pass.
//! Either way the number is wrong by about a factor of two in the middle of the
//! range, which is exactly where interface greys live.
//!
//! # Colour is never the only channel
//!
//! Contrast is the measurable half. The other half is SC 1.4.1: information
//! must not be carried by colour alone. That is a structural property of a
//! figure rather than a number — a chromatogram draws the base letter as well
//! as colouring the trace, a gel band is at a position, a feature has a name —
//! and it is asserted where those pictures are built, not here.

/// The two thresholds Level AA defines, plus what they apply to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Body text: 4.5:1.
    Text,
    /// 18 pt, or 14 pt bold: 3:1.
    LargeText,
    /// Icons, rules, the boundary of a filled shape: 3:1 (SC 1.4.11).
    Graphic,
}

impl Kind {
    /// The Level AA ratio this kind must reach.
    pub fn min_ratio(&self) -> f64 {
        match self {
            Kind::Text => 4.5,
            Kind::LargeText | Kind::Graphic => 3.0,
        }
    }

    /// Which threshold applies to text of a given size.
    ///
    /// WCAG's "large" is 18 pt, or 14 pt when bold — in *points at final
    /// printed size*, which is why [`crate::page::Fit`] exists: the same label
    /// is large text in a full-page figure and body text in a single-column
    /// one.
    pub fn for_text(size_pt: f64, bold: bool) -> Kind {
        if size_pt >= 18.0 || (bold && size_pt >= 14.0) {
            Kind::LargeText
        } else {
            Kind::Text
        }
    }
}

/// Relative luminance of an sRGB colour, per WCAG.
///
/// Each channel is linearised before weighting. Skipping that — averaging the
/// raw bytes — is the standard mistake, and it is not a small one: `#767676`
/// against white is 4.54:1 correctly and 2.05:1 without the linearisation.
pub fn luminance(rgb: (u8, u8, u8)) -> f64 {
    let f = |c: u8| {
        let c = c as f64 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(rgb.0) + 0.7152 * f(rgb.1) + 0.0722 * f(rgb.2)
}

/// Contrast ratio between two colours, from 1.0 to 21.0.
///
/// Symmetric: which one is the background does not change the answer.
pub fn ratio(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// `#rgb` or `#rrggbb` to bytes. `None` for anything else.
///
/// Named CSS colours are deliberately not resolved: a palette that cannot be
/// measured should fail the audit loudly rather than be assumed black.
pub fn parse_hex(colour: &str) -> Option<(u8, u8, u8)> {
    let h = colour.strip_prefix('#')?;
    let d = |i: usize, n: usize| u8::from_str_radix(h.get(i..i + n)?, 16).ok();
    match h.len() {
        6 => Some((d(0, 2)?, d(2, 2)?, d(4, 2)?)),
        3 => {
            let e = |i: usize| d(i, 1).map(|v| v * 17);
            Some((e(0)?, e(1)?, e(2)?))
        }
        _ => None,
    }
}

/// Does this pair meet Level AA for this kind of content?
pub fn passes_aa(fg: (u8, u8, u8), bg: (u8, u8, u8), kind: Kind) -> bool {
    ratio(fg, bg) >= kind.min_ratio()
}

/// One thing in a figure that does not meet the threshold.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// What it is — a label's text, or a description of the shape.
    pub what: String,
    pub foreground: String,
    pub background: String,
    pub ratio: f64,
    pub required: f64,
    pub kind: Kind,
}

/// Audit every text item and stroked shape in a scene against a background.
///
/// `scale` converts scene units to points at final size, because whether a
/// label counts as large text depends on how big it will actually be printed —
/// see [`Kind::for_text`]. Pass 1.0 to audit in scene units.
///
/// Colours that are not hex are reported as findings with a ratio of 0 rather
/// than skipped: an unmeasurable colour is not a passing one.
pub fn audit(scene: &crate::Scene, background: &str, scale: f64) -> Vec<Finding> {
    let bg = match parse_hex(background) {
        Some(c) => c,
        None => return vec![unmeasurable("the background", background, background)],
    };
    let mut out = Vec::new();
    for item in &scene.items {
        match item {
            crate::Item::Text {
                color,
                size,
                bold,
                text,
                ..
            } => {
                let kind = Kind::for_text(size * scale, *bold);
                match parse_hex(color) {
                    None => out.push(unmeasurable(text, color, background)),
                    Some(fg) => {
                        let r = ratio(fg, bg);
                        if r < kind.min_ratio() {
                            out.push(Finding {
                                what: text.clone(),
                                foreground: color.clone(),
                                background: background.to_string(),
                                ratio: r,
                                required: kind.min_ratio(),
                                kind,
                            });
                        }
                    }
                }
            }
            crate::Item::Circle { stroke, .. } => {
                check_graphic(&mut out, stroke, background, bg, "circle");
            }
            crate::Item::Path {
                stroke: Some(s),
                title,
                ..
            } => {
                let what = title.clone().unwrap_or_else(|| "path".into());
                check_graphic(&mut out, s, background, bg, &what);
            }
            _ => {}
        }
    }
    out
}

fn check_graphic(
    out: &mut Vec<Finding>,
    colour: &str,
    background: &str,
    bg: (u8, u8, u8),
    what: &str,
) {
    match parse_hex(colour) {
        None => out.push(unmeasurable(what, colour, background)),
        Some(fg) => {
            let r = ratio(fg, bg);
            if r < Kind::Graphic.min_ratio() {
                out.push(Finding {
                    what: what.to_string(),
                    foreground: colour.to_string(),
                    background: background.to_string(),
                    ratio: r,
                    required: Kind::Graphic.min_ratio(),
                    kind: Kind::Graphic,
                });
            }
        }
    }
}

fn unmeasurable(what: &str, fg: &str, bg: &str) -> Finding {
    Finding {
        what: what.to_string(),
        foreground: fg.to_string(),
        background: bg.to_string(),
        ratio: 0.0,
        required: Kind::Text.min_ratio(),
        kind: Kind::Text,
    }
}

/// The Okabe–Ito qualitative palette.
///
/// Eight colours chosen by Okabe and Ito (2008) to stay distinguishable under
/// the common forms of colour vision deficiency. It is the default recommended
/// set for scientific figures for exactly the reason a plasmid map needs one:
/// a map distinguishes features only by colour and shape, and the shapes are
/// all arrows.
///
/// Black is first, so a one-colour figure is black.
pub const OKABE_ITO: &[(&str, &str)] = &[
    ("black", "#000000"),
    ("orange", "#e69f00"),
    ("sky blue", "#56b4e9"),
    ("bluish green", "#009e73"),
    ("yellow", "#f0e442"),
    ("blue", "#0072b2"),
    ("vermillion", "#d55e00"),
    ("reddish purple", "#cc79a7"),
];

/// The palette colour for an index, cycling.
pub fn okabe_ito(i: usize) -> &'static str {
    OKABE_ITO[i % OKABE_ITO.len()].1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Anchor, Item, Scene};

    const WHITE: (u8, u8, u8) = (255, 255, 255);
    const BLACK: (u8, u8, u8) = (0, 0, 0);

    #[test]
    fn the_two_ends_of_the_scale_are_where_they_should_be() {
        // 21:1 is the maximum the formula can produce, and 1:1 the minimum.
        assert!((ratio(BLACK, WHITE) - 21.0).abs() < 1e-9);
        assert!((ratio(WHITE, WHITE) - 1.0).abs() < 1e-12);
        // Symmetric: which is the background cannot change the answer.
        assert_eq!(ratio(BLACK, WHITE), ratio(WHITE, BLACK));
    }

    #[test]
    fn the_gamma_step_changes_the_answer_by_about_a_factor_of_two() {
        // #767676 is the darkest grey that passes AA on white, and #808080 --
        // the value everyone reaches for -- does not. That much is the point of
        // having a threshold at all.
        let dark = parse_hex("#767676").unwrap();
        let mid = parse_hex("#808080").unwrap();
        assert!(
            (ratio(dark, WHITE) - 4.54).abs() < 0.01,
            "{}",
            ratio(dark, WHITE)
        );
        assert!(
            (ratio(mid, WHITE) - 3.95).abs() < 0.01,
            "{}",
            ratio(mid, WHITE)
        );

        // Skipping linearisation does not shift the answer slightly, it more
        // than halves it: #767676 comes out at 2.05 rather than 4.54, so the
        // naive calculation *rejects* a colour that passes. Measured, because
        // the first version of this test asserted the error ran the other way
        // and was simply wrong about the direction.
        let naive = |c: (u8, u8, u8)| (c.0 as f64 + c.1 as f64 + c.2 as f64) / 3.0 / 255.0;
        let naive_ratio = |c: (u8, u8, u8)| 1.05 / (naive(c) + 0.05);
        assert!(
            (naive_ratio(dark) - 2.05).abs() < 0.01,
            "{}",
            naive_ratio(dark)
        );
        assert!(
            naive_ratio(dark) < ratio(dark, WHITE) / 2.0,
            "less than half the true ratio"
        );
    }

    #[test]
    fn large_text_is_held_to_the_lower_threshold_at_the_printed_size() {
        // WCAG's "large" is in points at final size, which is why the audit
        // takes a scale: the same label is large text across a page and body
        // text in one column.
        assert_eq!(Kind::for_text(18.0, false), Kind::LargeText);
        assert_eq!(Kind::for_text(17.9, false), Kind::Text);
        assert_eq!(Kind::for_text(14.0, true), Kind::LargeText);
        assert_eq!(Kind::for_text(14.0, false), Kind::Text);
        assert_eq!(Kind::Text.min_ratio(), 4.5);
        assert_eq!(Kind::LargeText.min_ratio(), 3.0);
        assert_eq!(Kind::Graphic.min_ratio(), 3.0);
    }

    #[test]
    fn every_okabe_ito_colour_is_distinct_and_parseable() {
        let mut seen = std::collections::BTreeSet::new();
        for (name, hex) in OKABE_ITO {
            let c = parse_hex(hex).unwrap_or_else(|| panic!("{name} is not hex"));
            assert!(seen.insert(c), "{name} repeats a colour");
        }
        assert_eq!(seen.len(), 8);
        assert_eq!(okabe_ito(0), "#000000", "a one-colour figure is black");
        assert_eq!(okabe_ito(8), okabe_ito(0), "and the cycle wraps");
    }

    #[test]
    fn the_okabe_ito_colours_that_fail_on_white_are_named_rather_than_assumed() {
        // Okabe-Ito is chosen for *distinguishability under colour vision
        // deficiency*, which is a different property from contrast against a
        // background. Two of the eight are light enough to fail the 3:1
        // graphical threshold on white, and pretending otherwise would be the
        // kind of accessibility claim that is worse than none.
        let fails: Vec<&str> = OKABE_ITO
            .iter()
            .filter(|(_, h)| ratio(parse_hex(h).unwrap(), WHITE) < 3.0)
            .map(|(n, _)| *n)
            .collect();
        assert_eq!(
            fails,
            vec!["orange", "sky blue", "yellow"],
            "three of the eight, measured -- an earlier version of this test \
             guessed two and was wrong about orange (2.25:1)"
        );
        // And they are fine on a dark background, which is the actual remedy:
        // yellow is 1.32:1 on white and 15.88:1 on black.
        for (name, hex) in OKABE_ITO {
            let c = parse_hex(hex).unwrap();
            assert!(
                ratio(c, WHITE) >= 3.0 || ratio(c, BLACK) >= 3.0,
                "{name} works against neither background"
            );
        }
    }

    fn text_scene(color: &str, size: f64, bold: bool) -> Scene {
        Scene {
            width: 100.0,
            height: 100.0,
            title: "t".into(),
            items: vec![Item::Text {
                x: 0.0,
                y: 0.0,
                size,
                anchor: Anchor::Start,
                color: color.into(),
                bold,
                text: "AmpR".into(),
            }],
        }
    }

    #[test]
    fn an_audit_names_the_label_that_fails_and_by_how_much() {
        let s = text_scene("#999999", 10.0, false);
        let f = audit(&s, "#ffffff", 1.0);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].what, "AmpR");
        assert_eq!(f[0].kind, Kind::Text);
        assert_eq!(f[0].required, 4.5);
        assert!(f[0].ratio > 2.0 && f[0].ratio < 3.0, "{}", f[0].ratio);

        assert!(audit(&text_scene("#333333", 10.0, false), "#ffffff", 1.0).is_empty());
    }

    #[test]
    fn shrinking_a_figure_can_turn_large_text_into_body_text_and_the_audit_follows() {
        // A 20-unit label at scale 1 is large text and needs 3:1; the same
        // label at half size is body text and needs 4.5:1. A colour between the
        // two passes one and fails the other, which is exactly the case that
        // slips through when a figure is scaled to a column.
        let s = text_scene("#767676", 20.0, false);
        let between = ratio(parse_hex("#8a8a8a").unwrap(), WHITE);
        assert!(between > 3.0 && between < 4.5, "{between}");
        let s2 = text_scene("#8a8a8a", 20.0, false);
        assert!(audit(&s2, "#ffffff", 1.0).is_empty(), "large text: 3:1");
        assert_eq!(
            audit(&s2, "#ffffff", 0.5).len(),
            1,
            "the same label at half size is body text and fails"
        );
        assert!(
            audit(&s, "#ffffff", 0.5).is_empty(),
            "#767676 passes either way"
        );
    }

    #[test]
    fn a_colour_that_cannot_be_measured_is_a_finding_and_not_a_pass() {
        // A palette the audit cannot read must fail loudly. Assuming black
        // would turn every unparseable colour into a silent pass, which is the
        // failure mode an accessibility check exists to not have.
        let s = text_scene("rebeccapurple", 10.0, false);
        let f = audit(&s, "#ffffff", 1.0);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].ratio, 0.0);
        assert!(!audit(&s, "not-a-colour", 1.0).is_empty());
    }

    #[test]
    fn three_hex_digits_mean_the_same_as_six() {
        assert_eq!(parse_hex("#fff"), Some((255, 255, 255)));
        assert_eq!(parse_hex("#000"), Some((0, 0, 0)));
        assert_eq!(parse_hex("#f00"), parse_hex("#ff0000"));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("fff"), None);
        assert_eq!(parse_hex("#gggggg"), None);
    }

    /// The figures this crate actually ships, audited rather than assumed.
    ///
    /// Every accessibility claim in this project should be a number somebody
    /// can recompute. These are the real scenes, at the real colours, against
    /// the background each is drawn on.
    #[test]
    fn the_shipped_chromatogram_palettes_meet_the_graphical_threshold() {
        // A trace curve is a graphical object: SC 1.4.11, 3:1. The classic
        // palette is what every other viewer uses and is kept for that reason,
        // so if it fails, that has to be said rather than quietly claimed.
        use crate::trace::Palette;
        let mut classic_fail = Vec::new();
        let mut accessible_fail = Vec::new();
        for base in b"ACGT" {
            let c = parse_hex(Palette::Classic.color(*base)).expect("hex");
            if ratio(c, WHITE) < 3.0 {
                classic_fail.push(*base as char);
            }
            let a = parse_hex(Palette::Accessible.color(*base)).expect("hex");
            if ratio(a, WHITE) < 3.0 {
                accessible_fail.push(*base as char);
            }
        }
        assert!(
            classic_fail.is_empty(),
            "classic palette fails 3:1 on white for {classic_fail:?}"
        );
        assert!(
            accessible_fail.is_empty(),
            "accessible palette fails 3:1 on white for {accessible_fail:?}"
        );
    }

    #[test]
    fn the_gel_picture_is_readable_on_its_own_background() {
        // The gel draws light on dark, which is how a stained gel photographs,
        // so its contrast must be measured against *that* background and not
        // against white.
        for inverted in [true, false] {
            let bg = if inverted { "#15181c" } else { "#ffffff" };
            let band = if inverted { "#f2f4f7" } else { "#1c2026" };
            let text = if inverted { "#e6e9ee" } else { "#1c2026" };
            let dim = if inverted { "#9aa4b0" } else { "#5d6774" };
            let bgc = parse_hex(bg).unwrap();
            assert!(
                ratio(parse_hex(band).unwrap(), bgc) >= 3.0,
                "bands on {bg}: {:.2}",
                ratio(parse_hex(band).unwrap(), bgc)
            );
            assert!(
                ratio(parse_hex(text).unwrap(), bgc) >= 4.5,
                "labels on {bg}: {:.2}",
                ratio(parse_hex(text).unwrap(), bgc)
            );
            assert!(
                ratio(parse_hex(dim).unwrap(), bgc) >= 4.5,
                "the dim label colour on {bg} is {:.2}, and band sizes use it",
                ratio(parse_hex(dim).unwrap(), bgc)
            );
        }
    }

    #[test]
    fn every_colour_this_crate_chooses_meets_wcag_aa_on_white() {
        // These are our colours, not the file's, so a failure here is our
        // defect. Measuring found three real ones on the first real plasmid
        // tried: the ruler labels at 3.19:1 against a 4.5 requirement, the
        // label leader lines at 2.17:1 against 3.0, and the rep_origin arrow at
        // 2.84:1 against 3.0. All three had looked perfectly fine.
        //
        // A feature arrow is a graphical object (SC 1.4.11, 3:1). A ruler
        // number is body text (4.5:1) -- it is small and it carries a value
        // somebody reads off the figure.
        let kinds = [
            "CDS",
            "gene",
            "promoter",
            "RBS",
            "terminator",
            "polyA_signal",
            "rep_origin",
            "origin",
            "primer_bind",
            "protein_bind",
            "misc_feature",
            "something_unknown",
        ];
        for k in kinds {
            let c = parse_hex(crate::colour_for(k)).unwrap_or_else(|| panic!("{k} is not hex"));
            let r = ratio(c, WHITE);
            assert!(
                r >= Kind::Graphic.min_ratio(),
                "{k} is {} at {r:.2}:1 on white, and a feature arrow needs 3:1",
                crate::colour_for(k)
            );
        }

        // Text drawn by the renderer itself.
        //
        // Read out of `crate::ink`, not spelled out again here: this list used
        // to hold two literals, one of which was the *backbone* stroke labelled
        // "the title" and audited a second time three lines below as the
        // backbone. The feature labels -- the most numerous text in any map --
        // the real title and the feature outlines were in no Rust test at all,
        // so changing the label fill to #a0a0a0 (2.61:1) left every gate green.
        for (what, hex) in [
            ("ruler numbers and the bp count", crate::ink::SUBTITLE_FILL),
            ("feature labels", crate::ink::LABEL_FILL),
            ("the title", crate::ink::TITLE_FILL),
        ] {
            let r = ratio(parse_hex(hex).unwrap(), WHITE);
            assert!(
                r >= Kind::Text.min_ratio(),
                "{what} is {hex} at {r:.2}:1, needs 4.5"
            );
        }
        // Rules, leaders and outlines.
        for (what, hex) in [
            ("leader lines", crate::ink::LEADER_STROKE),
            ("the backbone", crate::ink::BACKBONE_STROKE),
            ("feature outlines", crate::ink::FEATURE_STROKE),
        ] {
            let r = ratio(parse_hex(hex).unwrap(), WHITE);
            assert!(
                r >= Kind::Graphic.min_ratio(),
                "{what} is {hex} at {r:.2}:1, needs 3.0"
            );
        }
    }

    #[test]
    fn every_colour_the_renderer_emits_is_one_of_the_audited_constants() {
        // The audit above measures constants; this is what ties them to the
        // ink that actually reaches a figure. A literal reintroduced at a use
        // site in `scene` would be measured by nothing, which is the state the
        // label fill was in.
        use crate::{Item, Options};
        let mut m = pl_core::Molecule {
            name: "pTEST".into(),
            seq: b"ACGT".iter().cycle().take(4000).copied().collect(),
            topology: pl_core::Topology::Circular,
            ..Default::default()
        };
        for (i, kind) in ["CDS", "promoter", "rep_origin", "misc_feature"]
            .iter()
            .enumerate()
        {
            let mut f = pl_core::Feature::new(format!("f{i}"), *kind);
            f.segments.push(pl_core::Segment::new(
                i as u64 * 500 + 1,
                i as u64 * 500 + 400,
            ));
            m.features.push(f);
        }
        let (sc, _) = crate::scene(&m, Options::default());

        let ours = [
            crate::ink::LABEL_FILL,
            crate::ink::TITLE_FILL,
            crate::ink::SUBTITLE_FILL,
            crate::ink::BACKBONE_STROKE,
            crate::ink::FEATURE_STROKE,
            crate::ink::LEADER_STROKE,
        ];
        let feature_colours: Vec<&str> = [
            "CDS",
            "gene",
            "promoter",
            "RBS",
            "terminator",
            "polyA_signal",
            "rep_origin",
            "origin",
            "primer_bind",
            "protein_bind",
            "misc_feature",
            "anything else",
        ]
        .iter()
        .map(|k| crate::colour_for(k))
        .collect();
        let known = |c: &str| ours.contains(&c) || feature_colours.contains(&c);

        let mut seen = 0;
        for item in &sc.items {
            let colours: Vec<String> = match item {
                Item::Circle { stroke, .. } => vec![stroke.clone()],
                Item::Path { fill, stroke, .. } => {
                    fill.iter().chain(stroke.iter()).cloned().collect()
                }
                Item::Text { color, .. } => vec![color.clone()],
            };
            for c in colours {
                assert!(known(&c), "{c} is drawn but audited nowhere");
                seen += 1;
            }
        }
        assert!(seen > 10, "only {seen} colours in the scene");
    }

    /// The TypeScript renderer's palette, audited and compared with ours.
    ///
    /// The two renderers keep separate copies of these colours — the gate
    /// already compares what they *draw*, but not what they draw it in, so a
    /// palette fix applied to one and not the other would pass every existing
    /// check while shipping two different figures. Reading the file is crude
    /// and it is the only thing that actually notices.
    ///
    /// It also audits the colours the TypeScript side has and Rust does not:
    /// `intron` was `#9aa4ae`, 2.53:1, and nothing else would have caught it.
    #[test]
    fn the_typescript_palette_matches_ours_and_also_passes() {
        const TS: &str = include_str!("../../../packages/circular-map/src/render.ts");
        let theme = TS
            .split_once("const DEFAULT_THEME")
            .expect("the theme is still called that")
            .1;
        let theme = &theme[..theme
            .find(
                "
};",
            )
            .expect("a closing brace")];

        let mut entries: Vec<(String, String)> = Vec::new();
        for line in theme.lines() {
            let line = line.trim().trim_end_matches(',');
            if let Some((k, v)) = line.split_once(':') {
                let v = v.trim().trim_matches('\'');
                if v.starts_with('#') {
                    entries.push((k.trim().to_string(), v.to_string()));
                }
            }
        }
        assert!(entries.len() > 12, "parsed only {} colours", entries.len());

        // Text needs 4.5:1; everything else in a figure is a graphical object.
        let text_keys = ["labelFill", "titleFill", "subtitleFill"];
        for (k, hex) in &entries {
            let c = parse_hex(hex).unwrap_or_else(|| panic!("{k} = {hex} is not hex"));
            let need = if text_keys.contains(&k.as_str()) {
                Kind::Text
            } else {
                Kind::Graphic
            };
            let r = ratio(c, WHITE);
            assert!(
                r >= need.min_ratio(),
                "{k} = {hex} is {r:.2}:1 on white and needs {:.1}:1",
                need.min_ratio()
            );
        }

        // The renderer's own ink, key by key. The loop below can only compare
        // feature *kinds*, because `colour_for` is all it has to compare
        // against, and it skips anything that falls through to the default --
        // so `labelFill`, `titleFill` and the rest were compared with nothing
        // and the two palettes could drift apart one constant at a time.
        for (key, ours) in [
            ("labelFill", crate::ink::LABEL_FILL),
            ("titleFill", crate::ink::TITLE_FILL),
            ("subtitleFill", crate::ink::SUBTITLE_FILL),
            ("tickStroke", crate::ink::SUBTITLE_FILL),
            ("backboneStroke", crate::ink::BACKBONE_STROKE),
            ("featureStroke", crate::ink::FEATURE_STROKE),
            ("leaderStroke", crate::ink::LEADER_STROKE),
        ] {
            let theirs = entries
                .iter()
                .find(|(k, _)| k == key)
                .unwrap_or_else(|| panic!("the TypeScript theme has no {key}"));
            assert_eq!(
                ours, theirs.1,
                "the two renderers disagree about {key}: Rust {ours}, TypeScript {}",
                theirs.1
            );
        }

        // Where both renderers name the same feature kind, they must agree.
        for (k, hex) in &entries {
            let ours = crate::colour_for(k);
            // `colour_for` falls through to one value for anything it does not
            // know, so only compare the keys it really has.
            if ours != crate::colour_for("a kind that certainly does not exist") {
                assert_eq!(
                    ours, hex,
                    "the two renderers disagree about {k}: Rust {ours}, TypeScript {hex}"
                );
            }
        }
    }
}
