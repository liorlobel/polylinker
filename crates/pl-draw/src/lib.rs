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
//! `crates/pl-draw/tests/agreement.rs` renders the same molecule through both
//! and asserts they describe the same picture. Two renderers that agree are
//! better evidence than one that nobody checks.
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

mod labels;
pub use labels::{isotonic, place_column, LabelBox, Placement};

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

/// What was drawn, and what could not be.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub labels_placed: usize,
    /// Labels that would have overlapped and were dropped.
    ///
    /// Returned rather than silently omitted: a map missing three labels looks
    /// exactly like a plasmid with three fewer features.
    pub labels_hidden: Vec<String>,
    /// Features whose coordinates describe nothing drawable.
    pub malformed: Vec<String>,
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
        "rep_origin" | "origin" => "#d08a3e",
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

/// Render a molecule as a standalone SVG document.
pub fn circular_svg(mol: &Molecule, opts: Options) -> (String, Report) {
    let mut report = Report::default();
    let len = mol.span().max(1);
    let circular = mol.topology.is_circular();
    let (cx, cy) = (opts.width / 2.0, opts.height / 2.0);

    // Reserve room for the widest label so the ring is as large as it can be
    // without labels running off the canvas.
    let widest = mol
        .features
        .iter()
        .map(|f| f.name.chars().count() as f64 * opts.font_size * 0.55)
        .fold(0.0_f64, f64::max);
    let margin = (widest + 34.0).min(opts.width.min(opts.height) * 0.3);
    let ro = (opts.width.min(opts.height) / 2.0 - margin).max(40.0);
    let ri = ro - opts.ring_width;

    let mut body = String::new();
    let mut overlay = String::new();

    // --- backbone ---
    if circular {
        body.push_str(&format!(
            r##"<circle cx="{}" cy="{}" r="{}" fill="none" stroke="#33383d" stroke-width="1.25"/>"##,
            n(cx),
            n(cy),
            n((ro + ri) / 2.0)
        ));
    } else {
        // A linear molecule drawn as a closed ring would be a lie about
        // topology.
        let gap = 0.06 * TAU;
        let (x0, y0) = polar(cx, cy, (ro + ri) / 2.0, gap / 2.0);
        let (x1, y1) = polar(cx, cy, (ro + ri) / 2.0, TAU - gap / 2.0);
        body.push_str(&format!(
            r##"<path d="M{},{} A{},{} 0 1 1 {},{}" fill="none" stroke="#33383d" stroke-width="1.25"/>"##,
            n(x0),
            n(y0),
            n((ro + ri) / 2.0),
            n((ro + ri) / 2.0),
            n(x1),
            n(y1)
        ));
    }

    // --- ruler ---
    if opts.ruler {
        let step = nice_step(len as f64 / 12.0);
        let mut base = step;
        while base <= len {
            let a = angle(base, len);
            let (x0, y0) = polar(cx, cy, ri - 4.0, a);
            let (x1, y1) = polar(cx, cy, ri - 9.0, a);
            body.push_str(&format!(
                r##"<path d="M{},{}L{},{}" stroke="#8a9199" stroke-width="1" fill="none"/>"##,
                n(x0),
                n(y0),
                n(x1),
                n(y1)
            ));
            let (tx, ty) = polar(cx, cy, ri - 18.0, a);
            body.push_str(&format!(
                r##"<text x="{}" y="{}" font-size="{}" fill="#8a9199" text-anchor="middle" dominant-baseline="middle">{}</text>"##,
                n(tx),
                n(ty),
                n(opts.font_size * 0.72),
                commas(base)
            ));
            base += step;
        }
    }

    // --- features ---
    struct Anchor {
        text: String,
        angle: f64,
        weight: f64,
    }
    let mut anchors: Vec<Anchor> = Vec::new();

    for f in &mol.features {
        let parts: Vec<(u64, u64)> = f
            .segments
            .iter()
            .flat_map(|s| ranges(s.start, s.end, len, circular))
            .collect();
        if parts.is_empty() {
            report.malformed.push(f.name.clone());
            continue;
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
                let ang = angle(a, len);
                let (x0, y0) = polar(cx, cy, ri, ang);
                let (x1, y1) = polar(cx, cy, ro, ang);
                body.push_str(&format!(
                    r##"<path d="M{},{}L{},{}" stroke="{colour}" stroke-width="1.75" fill="none"><title>{}</title></path>"##,
                    n(x0),
                    n(y0),
                    n(x1),
                    n(y1),
                    esc(&f.name)
                ));
            } else {
                let arrow = if i as isize == arrow_on {
                    match f.strand {
                        Strand::Reverse => Arrow::Start,
                        _ => Arrow::End,
                    }
                } else {
                    Arrow::None
                };
                body.push_str(&format!(
                    r##"<path d="{}" fill="{colour}" stroke="#2b2f34" stroke-width="0.6"><title>{}</title></path>"##,
                    arc_path(cx, cy, ri, ro, angle(a, len), angle(b + 1, len), arrow),
                    esc(&f.name)
                ));
            }
        }

        let mid = parts[0].0 + (span / 2).min(parts[0].1 - parts[0].0);
        anchors.push(Anchor {
            text: f.name.clone(),
            angle: angle(mid, len),
            weight: 1.0 + (1.0 + span as f64).log10(),
        });
    }

    // --- labels, placed exactly ---
    let line_h = opts.font_size + 3.0;
    let pad = 8.0;
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
            let dir = if right { 1.0 } else { -1.0 };
            let lx = cx + dir * (ro + 26.0);
            let (tx, ty) = polar(cx, cy, ro + 2.0, anchors[i].angle);
            let (ex, ey) = polar(cx, cy, ro + 12.0, anchors[i].angle);
            overlay.push_str(&format!(
                r##"<path d="M{},{}L{},{}L{},{}" fill="none" stroke="#aab1b8" stroke-width="0.9"/>"##,
                n(tx),
                n(ty),
                n(ex),
                n(ey),
                n(lx - dir * 4.0),
                n(y)
            ));
            overlay.push_str(&format!(
                r##"<text x="{}" y="{}" font-size="{}" fill="#22262a" text-anchor="{}" dominant-baseline="middle">{}</text>"##,
                n(lx),
                n(y),
                n(opts.font_size),
                if right { "start" } else { "end" },
                esc(&anchors[i].text)
            ));
            report.labels_placed += 1;
        }
    }

    // --- centre ---
    let title = if mol.name.is_empty() {
        "unnamed"
    } else {
        &mol.name
    };
    overlay.push_str(&format!(
        r##"<text x="{}" y="{}" font-size="{}" font-weight="600" fill="#16191c" text-anchor="middle" dominant-baseline="middle">{}</text>"##,
        n(cx),
        n(cy - 4.0),
        n(opts.font_size * 1.25),
        esc(title)
    ));
    overlay.push_str(&format!(
        r##"<text x="{}" y="{}" font-size="{}" fill="#6b7280" text-anchor="middle" dominant-baseline="middle">{} bp</text>"##,
        n(cx),
        n(cy + opts.font_size + 2.0),
        n(opts.font_size * 0.9),
        commas(len)
    ));

    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}" font-family="system-ui, -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif"><title>{}</title>{body}{overlay}</svg>"##,
        n(opts.width),
        n(opts.height),
        n(opts.width),
        n(opts.height),
        esc(title)
    );
    (svg, report)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Arrow {
    None,
    Start,
    End,
}

/// One feature arc, with its arrowhead.
fn arc_path(cx: f64, cy: f64, ri: f64, ro: f64, a0: f64, a1_in: f64, arrow: Arrow) -> String {
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

    let mut d = String::new();
    let p = |r: f64, a: f64| {
        let (x, y) = polar(cx, cy, r, a);
        format!("{},{}", n(x), n(y))
    };
    let arc = |r: f64, from: f64, to: f64, sweep_flag: u8| {
        let large = if (to - from).abs() > std::f64::consts::PI {
            1
        } else {
            0
        };
        format!("A{},{} 0 {large} {sweep_flag} {}", n(r), n(r), p(r, to))
    };

    match arrow {
        Arrow::End => {
            let base = a1 - head;
            d.push_str(&format!("M{}", p(ro, a0)));
            d.push_str(&arc(ro, a0, base, 1));
            if head > 0.0 {
                d.push_str(&format!(
                    "L{}L{}L{}",
                    p(ro + barb, base),
                    p(mid, a1),
                    p(ri - barb, base)
                ));
            }
            d.push_str(&format!("L{}", p(ri, base)));
            d.push_str(&arc(ri, base, a0, 0));
        }
        Arrow::Start => {
            let base = a0 + head;
            d.push_str(&format!("M{}", p(ro, a1)));
            d.push_str(&arc(ro, a1, base, 0));
            if head > 0.0 {
                d.push_str(&format!(
                    "L{}L{}L{}",
                    p(ro + barb, base),
                    p(mid, a0),
                    p(ri - barb, base)
                ));
            }
            d.push_str(&format!("L{}", p(ri, base)));
            d.push_str(&arc(ri, base, a1, 1));
        }
        Arrow::None => {
            d.push_str(&format!("M{}", p(ro, a0)));
            d.push_str(&arc(ro, a0, a1, 1));
            d.push_str(&format!("L{}", p(ri, a1)));
            d.push_str(&arc(ri, a1, a0, 0));
        }
    }
    d.push('Z');
    d
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
