//! Plasmid maps as SVG, from Rust.
//!
//! `docs/PLAN.md` v0.1: "SVG export via serialized DOM → `resvg`/`svg2pdf` in
//! Rust. Engine-independent, CI-diffable, strictly better than Chromium
//! `printToPDF`. Never use `html2canvas`."
//!
//! # Why this exists alongside `@polylinker/circular-map`
//!
//! ADR-1 assumed a Tauri shell, in which the desktop app and the browser tool
//! would share one TypeScript renderer. The shell is egui instead — a
//! deliberate departure — so the desktop app cannot call the TypeScript one and
//! a second implementation is unavoidable.
//!
//! That is a cost, and the way to make it pay is to treat the two as
//! independent implementations of one specification rather than as a copy:
//! `crates/pl-draw/tests/agreement.rs` replays a fixture generated from the
//! TypeScript through this crate's helpers — `angle`, `polar`, `ranges`,
//! `place_column`, `isotonic`, `safe_color`, `nice_step`, `commas`, `esc`, `n`
//! — and checks that the two compute the same numbers. Two renderers that agree
//! about their arithmetic are better evidence than one that nobody checks.
//!
//! **It is a helper-level check and not a picture-level one.** It never builds a
//! [`Molecule`], never calls [`scene`], and never compares an arc, a radius, an
//! arrowhead or a label column, so a swapped `Arrow::Start`/`Arrow::End` or a
//! moved label anchor passes it untouched. Until 2026-07 this paragraph said the
//! harness rendered the same molecule through both renderers, which is how the
//! origin-spanning label anchor in `mid_base` survived: there was no oracle.
//!
//! # What is guaranteed
//!
//! **Byte-identical output for identical input**, on every platform. Nothing
//! here reads a clock, a font, a locale or the filesystem: text width is
//! estimated from a constant, floats are rounded before formatting, and
//! iteration order is fixed. That is what makes the output diffable in CI and
//! usable as a figure that does not move between machines.
//!
//! Conventions match the TypeScript renderer and the field: base 1 at twelve
//! o'clock, coordinates increasing **clockwise**, 1-based inclusive.

use pl_core::{Molecule, Strand};

pub mod contrast;
pub mod eps;
mod labels;
pub mod page;
pub mod pdf;
pub mod scene;
pub mod trace;
pub use labels::{isotonic, place_column, LabelBox, Placement};
pub use scene::{Anchor, Item, Scene, Seg};

#[cfg(test)]
mod tests;

const TAU: f64 = std::f64::consts::TAU;

/// Rendering knobs. Defaults produce a figure-sized map.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub width: f64,
    pub height: f64,
    pub ring_width: f64,
    pub font_size: f64,
    /// Draw the base-position ruler.
    pub ruler: bool,
    /// Below this many degrees a feature is a tick, not an arrow — an
    /// arrowhead smaller than a pixel reads as dirt on the figure.
    pub min_feature_degrees: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            width: 720.0,
            height: 720.0,
            ring_width: 18.0,
            font_size: 12.0,
            ruler: true,
            min_feature_degrees: 1.2,
        }
    }
}

/// The ink this crate chooses itself, as opposed to a colour out of a file.
///
/// Named rather than spelled out at each use site because they are audited:
/// `contrast::tests::every_colour_this_crate_chooses_meets_wcag_aa_on_white`
/// measures these constants, and a literal repeated at the use site is a
/// literal the audit cannot see. Changing the feature-label fill to `#a0a0a0`
/// (2.61:1 on white) used to leave every gate green.
///
/// Each is the twin of a key in the TypeScript renderer's `DEFAULT_THEME`;
/// `contrast::tests::the_typescript_palette_matches_ours_and_also_passes` pins
/// the pairs together so one side cannot be fixed without the other.
pub mod ink {
    /// `labelFill` — feature label text, the most numerous text in a map.
    pub const LABEL_FILL: &str = "#22262a";
    /// `titleFill` — the molecule name at the centre.
    pub const TITLE_FILL: &str = "#16191c";
    /// `subtitleFill`/`tickStroke` — the bp count and the ruler numbers.
    pub const SUBTITLE_FILL: &str = "#6b7280";
    /// `backboneStroke` — the ring itself.
    pub const BACKBONE_STROKE: &str = "#33383d";
    /// `featureStroke` — the outline around a feature arrow.
    pub const FEATURE_STROKE: &str = "#2b2f34";
    /// `leaderStroke` — the line from the ring to a label.
    pub const LEADER_STROKE: &str = "#868d95";
}

/// What was drawn, and what could not be.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub labels_placed: usize,
    /// Labels that would have overlapped, or that had no room for even one
    /// character, and were dropped.
    ///
    /// Returned rather than silently omitted: a map missing three labels looks
    /// exactly like a plasmid with three fewer features.
    pub labels_hidden: Vec<String>,
    /// Features whose coordinates describe nothing drawable.
    pub malformed: Vec<String>,
    /// Features drawn from only *some* of their segments, by full name.
    ///
    /// A joined feature copied out of a larger parent record —
    /// `CDS join(100..200,5000..6000)` on a 1000 bp plasmid — keeps one segment
    /// that describes something and one that does not. The map can only draw
    /// the first, and a 101 bp single-exon arrow labelled `orfX` is
    /// indistinguishable from a real 101 bp `orfX`. Half a feature is a worse
    /// lie than no feature, so it is named here.
    pub partly_drawn: Vec<String>,
    /// Labels shortened with a trailing `...` because the canvas was too narrow
    /// for the whole name, by full name.
    ///
    /// The ring's radius reserves room for the widest label, but that
    /// reservation is capped at 30% of the canvas so that one 60-character name
    /// cannot collapse the map to nothing. Past the cap the name no longer fits
    /// in what was reserved, and the choice is a clipped label or a shortened
    /// one. A clipped label is cropped by the `viewBox`, the `/MediaBox` and the
    /// `%%BoundingBox` alike — silently, in the typesetter's hands — so it is
    /// shortened here and said so. The feature's own `<title>` still carries
    /// the whole name, so nothing is lost from the SVG itself — only from what
    /// a reader of the printed figure can see, which is why it is reported.
    pub labels_truncated: Vec<String>,
}

/// Round to two decimals. Float noise triples an SVG's size and destroys
/// byte-identity between platforms for no visible gain.
pub fn n(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    // `-0` and `0` must format the same or two identical pictures differ.
    let r = if r == 0.0 { 0.0 } else { r };
    format!("{r}")
}

/// XML-escape text destined for a text node or an attribute value.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // XML 1.0 forbids most control characters outright, so escaping
            // them would still produce a document that will not parse. A
            // feature name out of a binary `.dna` payload can contain one.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// A colour safe to interpolate into an attribute, or the fallback.
///
/// Colours arrive inside `.dna` files from Addgene and from collaborators, so
/// they are not author-controlled. The same check the TypeScript renderer
/// makes, for the same reason.
pub fn safe_color(v: Option<&str>, fallback: &str) -> String {
    let Some(v) = v.map(str::trim).filter(|s| !s.is_empty()) else {
        return fallback.to_string();
    };

    // #rgb, #rgba, #rrggbb, #rrggbbaa
    let hex = v.strip_prefix('#').unwrap_or("");
    let hex_ok = matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());

    // rgb()/rgba()/hsl()/hsla() carrying only numbers, %, commas, slashes and
    // spaces. Real files use these, and refusing them would silently grey out
    // a map that another tool coloured correctly.
    let func_ok = ["rgb(", "rgba(", "hsl(", "hsla("].iter().any(|p| {
        v.strip_prefix(p)
            .and_then(|rest| rest.strip_suffix(')'))
            .is_some_and(|inner| {
                inner
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b"eE+-.,%/ \t\n\x0b\x0c\r".contains(&b))
            })
    });

    // A bare CSS colour keyword, plus the two SVG paint keywords.
    let word_ok = (1..=32).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_alphabetic());

    if hex_ok || func_ok || word_ok {
        v.to_string()
    } else {
        fallback.to_string()
    }
}

pub fn colour_for(kind: &str) -> &'static str {
    match kind {
        "CDS" | "gene" => "#4f7fd0",
        "promoter" | "RBS" => "#4aa564",
        "terminator" | "polyA_signal" => "#c05c5c",
        "rep_origin" | "origin" => "#c07e2e",
        "primer_bind" => "#7e8a97",
        "protein_bind" => "#b87bb0",
        "misc_feature" => "#8b7bb8",
        _ => "#7f8a95",
    }
}

pub fn polar(cx: f64, cy: f64, r: f64, a: f64) -> (f64, f64) {
    (cx + r * a.sin(), cy - r * a.cos())
}

/// Angle in radians clockwise from twelve o'clock, for a 1-based base.
///
/// The positive modulo, rather than a saturating subtraction, is what makes
/// base 0 land just *before* the origin instead of on it — the same reading
/// `baseToAngle` takes, and the reason `angle(b + 1, len)` closes an arc that
/// ends at the last base.
pub fn angle(base: u64, len: u64) -> f64 {
    if len == 0 {
        return 0.0;
    }
    let (l, shifted) = (len as i64, base as i64 - 1);
    let frac = ((shifted % l) + l) % l;
    (frac as f64 / len as f64) * TAU
}

/// The angular ranges a segment occupies, splitting an origin-spanning one.
///
/// 1-based inclusive, matching `pl-core`. `segmentRanges` returns the same
/// spans 0-based half-open; `tests/agreement.rs` converts and compares.
pub fn ranges(start: u64, end: u64, len: u64, circular: bool) -> Vec<(u64, u64)> {
    if len == 0 {
        return Vec::new();
    }
    // A segment lying wholly past the end of the molecule describes nothing
    // here. Clamping both endpoints independently collapses it onto a 1 bp
    // range at the last base — drawn, and labelled with the real feature's
    // name, at 359.6 degrees. That is fabrication, which is worse than loss,
    // so the caller reports it as malformed instead.
    if start > len && end > len {
        return Vec::new();
    }
    // The mirror image, which was missing on both renderers. A segment wholly
    // below base 1 — `0-0`, which the SnapGene reader produces from an importer
    // that wrote 0-based coordinates — names no base either, and
    // `clamp(1, len)` on both endpoints collapsed it onto a 1 bp arc at base 1
    // under the real feature's name. The same fabrication as above, at the
    // other end of the molecule.
    if start < 1 && end < 1 {
        return Vec::new();
    }
    let s = start.clamp(1, len);
    let e = end.clamp(1, len);
    if s <= e {
        return vec![(s, e)];
    }
    if !circular {
        // A line has no origin to cross; the only honest reading is the span
        // the coordinates name.
        return vec![(e.min(s), s.max(e))];
    }
    vec![(s, len), (1, e)]
}

/// The base a feature's label should point at: the middle of its whole extent.
///
/// Accumulated across the parts, because half the *total* span added to the
/// first part is not the same thing and the difference is a quarter turn. The
/// old form was `parts[0].0 + (span / 2).min(parts[0].1 - parts[0].0)`: for a
/// 502 bp feature at 999..500 on a 1000 bp plasmid the parts are
/// `[(999, 1000), (1, 500)]`, so the clamp pinned the anchor to base 1000 —
/// 359.6 degrees, twelve o'clock — and because the column is chosen by
/// `angle.sin() >= 0.0` and sin(359.6°) is -0.006, the label went to the *left*
/// column with a leader pointing at 2 bases of the 502. The middle is base 250,
/// at 89.6 degrees, in the right column. The identical feature at 100..601
/// anchored correctly, so the picture depended on where the origin happened to
/// sit, which it must never do.
///
/// `featureMidBase` in the TypeScript renderer accumulates the same way. The
/// half is floored rather than rounded, which keeps a single-part feature's
/// anchor bit-identical to what this crate has always drawn and leaves the two
/// renderers at most one base apart at a part boundary.
fn mid_base(parts: &[(u64, u64)], span: u64) -> u64 {
    let half = span / 2;
    let mut acc = 0u64;
    for &(a, b) in parts {
        let w = b - a + 1;
        // `acc <= half` at every iteration, so the subtraction cannot wrap:
        // the loop only continues while `acc + w < half`.
        if acc + w >= half {
            return a + (half - acc);
        }
        acc += w;
    }
    parts.first().map_or(1, |p| p.0)
}

/// How wide a label is going to be, in scene units.
///
/// The estimate, not Helvetica's real advances: it is what the ring radius
/// reserves room with and what the TypeScript renderer measures with, and the
/// invariant that a label ends inside the canvas has to be stated in one unit
/// or the other. Both call sites use this so they cannot drift apart.
fn label_width(name: &str, font_size: f64) -> f64 {
    name.chars().count() as f64 * font_size * 0.55
}

/// A label shortened to what the canvas can actually hold, or `None` if not
/// even one character and an ellipsis fit.
///
/// `Some(name.to_string())` when the whole name fits, which is the case for
/// every map where the radius reservation was not capped — the caller compares
/// against the original to decide whether anything was lost.
///
/// The ellipsis is three ASCII full stops, not U+2026: the PDF writer would
/// carry the real character as WinAnsi 0x85, but the EPS writer asks the viewer
/// for `Helvetica` with its own StandardEncoding, where 0x85 is not an ellipsis
/// at all. Three dots are the same three dots in all three formats.
fn fit_label(name: &str, room: f64, font_size: f64) -> Option<String> {
    if label_width(name, font_size) <= room {
        return Some(name.to_string());
    }
    const ELLIPSIS: &str = "...";
    let mut kept = String::new();
    for c in name.chars() {
        let mut trial = kept.clone();
        trial.push(c);
        if label_width(&(trial.clone() + ELLIPSIS), font_size) > room {
            break;
        }
        kept = trial;
    }
    if kept.is_empty() {
        None
    } else {
        Some(kept + ELLIPSIS)
    }
}

/// Build the device-independent picture.
///
/// The one place the map's geometry lives. [`circular_svg`] and
/// [`circular_pdf`] both render this, which is why they cannot disagree about
/// where anything is — there is nothing above the level of ink for them to
/// disagree about.
pub fn scene(mol: &Molecule, opts: Options) -> (Scene, Report) {
    let mut report = Report::default();
    let mut items: Vec<Item> = Vec::new();
    let len = mol.span().max(1);
    let circular = mol.topology.is_circular();
    let (cx, cy) = (opts.width / 2.0, opts.height / 2.0);

    // Reserve room for the widest label so the ring is as large as it can be
    // without labels running off the canvas.
    let widest = mol
        .features
        .iter()
        .map(|f| label_width(&f.name, opts.font_size))
        .fold(0.0_f64, f64::max);
    let margin = (widest + 34.0).min(opts.width.min(opts.height) * 0.3);
    let ro = (opts.width.min(opts.height) / 2.0 - margin).max(40.0);
    // What is left for a label once the ring has taken its radius, on either
    // side: a label runs outward from `cx ± (ro + 26)`, so both columns have the
    // same room. Uncapped the reservation closes with 8 units to spare whatever
    // the name, but the `.min(30%)` cap above drops the `widest` term from `ro`
    // while the label still grows with it, and the `.max(40)` floor does the
    // same on a small canvas. Past either, the name no longer fits in what was
    // reserved and gets shortened below rather than cropped by the viewBox.
    let room = cx - (ro + 26.0);
    let ri = ro - opts.ring_width;
    let mid_r = (ro + ri) / 2.0;

    // --- backbone ---
    if circular {
        items.push(Item::Circle {
            cx,
            cy,
            r: mid_r,
            stroke: ink::BACKBONE_STROKE.into(),
            stroke_width: 1.25,
        });
    } else {
        // A linear molecule drawn as a closed ring would be a lie about
        // topology, so it gets a gap.
        let gap = 0.06 * TAU;
        let (x0, y0) = polar(cx, cy, mid_r, gap / 2.0);
        items.push(Item::Path {
            segs: vec![
                Seg::Move(x0, y0),
                Seg::Arc {
                    cx,
                    cy,
                    r: mid_r,
                    from: gap / 2.0,
                    to: TAU - gap / 2.0,
                },
            ],
            fill: None,
            stroke: Some(ink::BACKBONE_STROKE.into()),
            stroke_width: 1.25,
            title: None,
        });
    }

    // --- ruler ---
    if opts.ruler {
        let step = nice_step(len as f64 / 12.0);
        let mut base = step;
        while base <= len {
            let a = angle(base, len);
            let (x0, y0) = polar(cx, cy, ri - 4.0, a);
            let (x1, y1) = polar(cx, cy, ri - 9.0, a);
            items.push(Item::Path {
                segs: vec![Seg::Move(x0, y0), Seg::Line(x1, y1)],
                fill: None,
                stroke: Some(ink::SUBTITLE_FILL.into()),
                stroke_width: 1.0,
                title: None,
            });
            let (tx, ty) = polar(cx, cy, ri - 18.0, a);
            items.push(Item::Text {
                x: tx,
                y: ty,
                size: opts.font_size * 0.72,
                anchor: Anchor::Middle,
                color: ink::SUBTITLE_FILL.into(),
                bold: false,
                text: commas(base),
            });
            base += step;
        }
    }

    // --- features ---
    struct Label {
        text: String,
        angle: f64,
        weight: f64,
    }
    let mut anchors: Vec<Label> = Vec::new();

    for f in &mol.features {
        let mut parts: Vec<(u64, u64)> = Vec::with_capacity(f.segments.len());
        // Counted per segment, not over the whole feature. `ranges` returns
        // nothing for a segment lying wholly past the end, and a feature with
        // one such segment and one good one still has a non-empty `parts` — so
        // the all-or-nothing check below never fired for it and half of
        // `CDS join(100..200,5000..6000)` on a 1000 bp plasmid went out as a
        // whole 101 bp `orfX` with nothing said.
        let mut lost_segments = 0usize;
        for s in &f.segments {
            let r = ranges(s.start, s.end, len, circular);
            if r.is_empty() {
                lost_segments += 1;
            }
            parts.extend(r);
        }
        if parts.is_empty() {
            report.malformed.push(f.name.clone());
            continue;
        }
        if lost_segments > 0 {
            report.partly_drawn.push(f.name.clone());
        }
        let colour = safe_color(f.color(), colour_for(&f.kind));
        let span: u64 = parts.iter().map(|(a, b)| b - a + 1).sum();
        let degrees = (span as f64 / len as f64) * 360.0;
        let arrow_on = match f.strand {
            Strand::Forward => parts.len() as isize - 1,
            Strand::Reverse => 0,
            _ => -1,
        };

        for (i, &(a, b)) in parts.iter().enumerate() {
            if degrees < opts.min_feature_degrees {
                // Below a pixel an arrowhead reads as dirt on the figure, so a
                // very short feature is a tick instead.
                let ang = angle(a, len);
                let (x0, y0) = polar(cx, cy, ri, ang);
                let (x1, y1) = polar(cx, cy, ro, ang);
                items.push(Item::Path {
                    segs: vec![Seg::Move(x0, y0), Seg::Line(x1, y1)],
                    fill: None,
                    stroke: Some(colour.clone()),
                    stroke_width: 1.75,
                    title: Some(f.name.clone()),
                });
            } else {
                let arrow = if i as isize == arrow_on {
                    match f.strand {
                        Strand::Reverse => Arrow::Start,
                        _ => Arrow::End,
                    }
                } else {
                    Arrow::None
                };
                items.push(Item::Path {
                    segs: arc_segs(cx, cy, ri, ro, angle(a, len), angle(b + 1, len), arrow),
                    fill: Some(colour.clone()),
                    stroke: Some(ink::FEATURE_STROKE.into()),
                    stroke_width: 0.6,
                    title: Some(f.name.clone()),
                });
            }
        }

        let mid = mid_base(&parts, span);
        anchors.push(Label {
            text: f.name.clone(),
            angle: angle(mid, len),
            weight: 1.0 + (1.0 + span as f64).log10(),
        });
    }

    // --- labels, placed exactly ---
    let line_h = opts.font_size + 3.0;
    let pad = 8.0;
    let mut overlay: Vec<Item> = Vec::new();
    for right in [true, false] {
        let idx: Vec<usize> = (0..anchors.len())
            .filter(|&i| (anchors[i].angle.sin() >= 0.0) == right)
            .collect();
        let boxes: Vec<LabelBox> = idx
            .iter()
            .map(|&i| LabelBox {
                ideal: polar(cx, cy, ro + 14.0, anchors[i].angle).1,
                height: line_h,
                weight: anchors[i].weight,
            })
            .collect();
        let placed = place_column(&boxes, pad + opts.font_size, opts.height - pad);
        for d in &placed.dropped {
            report.labels_hidden.push(anchors[idx[*d]].text.clone());
        }
        for (k, &i) in idx.iter().enumerate() {
            let Some(y) = placed.positions[k] else {
                continue;
            };
            let text = match fit_label(&anchors[i].text, room, opts.font_size) {
                Some(t) => {
                    if t != anchors[i].text {
                        report.labels_truncated.push(anchors[i].text.clone());
                    }
                    t
                }
                None => {
                    // Not even one character and an ellipsis fit. Drawing the
                    // leader with nothing on the end of it would look like a
                    // renderer bug rather than a canvas too small to hold the
                    // name, so the label goes, and it is named.
                    report.labels_hidden.push(anchors[i].text.clone());
                    continue;
                }
            };
            let dir = if right { 1.0 } else { -1.0 };
            let lx = cx + dir * (ro + 26.0);
            let (tx, ty) = polar(cx, cy, ro + 2.0, anchors[i].angle);
            let (ex, ey) = polar(cx, cy, ro + 12.0, anchors[i].angle);
            overlay.push(Item::Path {
                segs: vec![
                    Seg::Move(tx, ty),
                    Seg::Line(ex, ey),
                    Seg::Line(lx - dir * 4.0, y),
                ],
                fill: None,
                stroke: Some(ink::LEADER_STROKE.into()),
                stroke_width: 0.9,
                title: None,
            });
            overlay.push(Item::Text {
                x: lx,
                y,
                size: opts.font_size,
                anchor: if right { Anchor::Start } else { Anchor::End },
                color: ink::LABEL_FILL.into(),
                bold: false,
                text,
            });
            report.labels_placed += 1;
        }
    }

    // --- centre ---
    let title = if mol.name.is_empty() {
        "unnamed".to_string()
    } else {
        mol.name.clone()
    };
    overlay.push(Item::Text {
        x: cx,
        y: cy - 4.0,
        size: opts.font_size * 1.25,
        anchor: Anchor::Middle,
        color: ink::TITLE_FILL.into(),
        bold: true,
        text: title.clone(),
    });
    overlay.push(Item::Text {
        x: cx,
        y: cy + opts.font_size + 2.0,
        size: opts.font_size * 0.9,
        anchor: Anchor::Middle,
        color: ink::SUBTITLE_FILL.into(),
        bold: false,
        text: format!("{} bp", commas(len)),
    });

    items.extend(overlay);
    (
        Scene {
            width: opts.width,
            height: opts.height,
            title,
            items,
        },
        report,
    )
}

/// Render a molecule as a standalone SVG document.
pub fn circular_svg(mol: &Molecule, opts: Options) -> (String, Report) {
    let (sc, report) = scene(mol, opts);
    (svg_of(&sc), report)
}

/// Render a molecule as a one-page PDF.
///
/// The same [`Scene`] as [`circular_svg`], so the two are the same picture.
/// `Report` carries what the *drawing* could not show; the second report is
/// what the PDF's font could not encode.
pub fn circular_pdf(mol: &Molecule, opts: Options) -> (Vec<u8>, Report, pdf::Report) {
    let (sc, report) = scene(mol, opts);
    let (bytes, pdf_report) = pdf::to_pdf(&sc);
    (bytes, report, pdf_report)
}

/// A scene as SVG.
pub fn svg_of(sc: &Scene) -> String {
    let mut body = String::new();
    for item in &sc.items {
        match item {
            Item::Circle {
                cx,
                cy,
                r,
                stroke,
                stroke_width,
            } => body.push_str(&format!(
                r##"<circle cx="{}" cy="{}" r="{}" fill="none" stroke="{stroke}" stroke-width="{}"/>"##,
                n(*cx),
                n(*cy),
                n(*r),
                n(*stroke_width)
            )),
            Item::Path {
                segs,
                fill,
                stroke,
                stroke_width,
                title,
            } => {
                let d = svg_path(segs);
                let fill = fill.clone().unwrap_or_else(|| "none".into());
                let stroke = stroke.clone().unwrap_or_else(|| "none".into());
                match title {
                    Some(t) => body.push_str(&format!(
                        r##"<path d="{d}" fill="{fill}" stroke="{stroke}" stroke-width="{}"><title>{}</title></path>"##,
                        n(*stroke_width),
                        esc(t)
                    )),
                    None => body.push_str(&format!(
                        r##"<path d="{d}" fill="{fill}" stroke="{stroke}" stroke-width="{}"/>"##,
                        n(*stroke_width)
                    )),
                }
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
                let a = match anchor {
                    Anchor::Start => "start",
                    Anchor::Middle => "middle",
                    Anchor::End => "end",
                };
                let weight = if *bold { r##" font-weight="600""## } else { "" };
                body.push_str(&format!(
                    r##"<text x="{}" y="{}" font-size="{}"{weight} fill="{color}" text-anchor="{a}" dominant-baseline="middle">{}</text>"##,
                    n(*x),
                    n(*y),
                    n(*size),
                    esc(text)
                ));
            }
        }
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}" font-family="system-ui, -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif"><title>{}</title>{body}</svg>"##,
        n(sc.width),
        n(sc.height),
        n(sc.width),
        n(sc.height),
        esc(&sc.title)
    )
}

/// A path's segments as an SVG `d` attribute.
///
/// Converts each centre-form arc into SVG's endpoint form. The sweep flag is 1
/// for increasing angle, which is clockwise on screen; the large-arc flag is
/// set past a half turn, and a sweep of a full turn or more is split, because
/// SVG's endpoint form cannot express an arc whose ends coincide.
fn svg_path(segs: &[Seg]) -> String {
    let mut d = String::new();
    for seg in segs {
        match *seg {
            Seg::Move(x, y) => d.push_str(&format!("M{},{}", n(x), n(y))),
            Seg::Line(x, y) => d.push_str(&format!("L{},{}", n(x), n(y))),
            Seg::Arc {
                cx,
                cy,
                r,
                from,
                to,
            } => {
                let sweep_flag = if to >= from { 1 } else { 0 };
                let total = (to - from).abs();
                let steps = if total >= TAU { 4 } else { 1 };
                let step = (to - from) / steps as f64;
                for i in 0..steps {
                    let t1 = from + step * (i + 1) as f64;
                    let (x, y) = polar(cx, cy, r, t1);
                    let large = if step.abs() > std::f64::consts::PI {
                        1
                    } else {
                        0
                    };
                    d.push_str(&format!(
                        "A{},{} 0 {large} {sweep_flag} {},{}",
                        n(r),
                        n(r),
                        n(x),
                        n(y)
                    ));
                }
            }
            Seg::Close => d.push('Z'),
        }
    }
    d
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Arrow {
    None,
    Start,
    End,
}

/// One feature arc, with its arrowhead, as device-independent segments.
///
/// Was an SVG `d` string. It has to be segments now so the PDF back end can
/// turn the arcs into Béziers, and so both back ends draw the same shape rather
/// than one parsing the other's output.
fn arc_segs(cx: f64, cy: f64, ri: f64, ro: f64, a0: f64, a1_in: f64, arrow: Arrow) -> Vec<Seg> {
    let mut a1 = a1_in;
    if a1 <= a0 {
        a1 += TAU;
    }
    let sweep = a1 - a0;
    let mid = (ri + ro) / 2.0;
    // Clamped to half the arc, so a short feature degrades to a triangle
    // instead of inverting into a bow tie.
    let head = if arrow == Arrow::None {
        0.0
    } else {
        (8.0 / mid).min(sweep * 0.5)
    };
    let barb = ((ro - ri) * 0.35).min(2.5);

    let mut segs = Vec::new();
    let mv = |r: f64, a: f64| {
        let (x, y) = polar(cx, cy, r, a);
        Seg::Move(x, y)
    };
    let ln = |r: f64, a: f64| {
        let (x, y) = polar(cx, cy, r, a);
        Seg::Line(x, y)
    };
    let arc = |r: f64, from: f64, to: f64| Seg::Arc {
        cx,
        cy,
        r,
        from,
        to,
    };

    match arrow {
        Arrow::End => {
            let base = a1 - head;
            segs.push(mv(ro, a0));
            segs.push(arc(ro, a0, base));
            if head > 0.0 {
                segs.push(ln(ro + barb, base));
                segs.push(ln(mid, a1));
                segs.push(ln(ri - barb, base));
            }
            segs.push(ln(ri, base));
            segs.push(arc(ri, base, a0));
        }
        Arrow::Start => {
            let base = a0 + head;
            segs.push(mv(ro, a1));
            segs.push(arc(ro, a1, base));
            if head > 0.0 {
                segs.push(ln(ro + barb, base));
                segs.push(ln(mid, a0));
                segs.push(ln(ri - barb, base));
            }
            segs.push(ln(ri, base));
            segs.push(arc(ri, base, a1));
        }
        Arrow::None => {
            segs.push(mv(ro, a0));
            segs.push(arc(ro, a0, a1));
            segs.push(ln(ri, a1));
            segs.push(arc(ri, a1, a0));
        }
    }
    segs.push(Seg::Close);
    segs
}

/// Round a tick interval up to something a human would have chosen.
pub fn nice_step(raw: f64) -> u64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 1;
    }
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    };
    ((step * mag) as u64).max(1)
}

pub fn commas(v: u64) -> String {
    let s = v.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
