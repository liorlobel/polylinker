//! A [`Scene`](crate::Scene) as pixels.
//!
//! # Text
//!
//! Glyph outlines come from [`crate::font`], which fills them from the
//! Liberation Sans faces committed under `crates/pl-draw/fonts/`. Three things
//! about text here are not free choices, and each is the same choice the vector
//! back ends already made:
//!
//! - **The pen advances by [`crate::pdf::text_width_in`]**, not by the face's
//!   own `hmtx`. That is the number the layout truncated, cropped and anchored
//!   against; see `font.rs`.
//! - **The baseline drops by [`crate::pdf::BASELINE_DROP_EM`]**, the same
//!   constant the PDF and the EPS share. The scene's `y` is the visual middle
//!   of the glyphs, matching SVG's `dominant-baseline: middle`, and all three
//!   raster/vector back ends now resolve it identically.
//! - **`bold` selects the 700 face**, because the PDF draws Helvetica-Bold and
//!   measures with `HELVETICA_BOLD`. The SVG writes `font-weight="600"`, which
//!   CSS matching resolves to 700 in a family offering only 400 and 700.
//!
//! A character the face cannot spell is reported rather than drawn as a blank,
//! mirroring `pdf::Report::unencodable`.
//!
//! # Coverage
//!
//! Spans, not an accumulator: each pixel row is sampled at [`SUB`] sub-scanlines,
//! every edge crossing on a sub-scanline is collected and sorted, the winding
//! number is run along them, and the intervals where it is non-zero are added to
//! the row with **exact** horizontal coverage. Analytic in x, sampled in y.
//!
//! THE OBVIOUS ALTERNATIVE IS WRONG HERE, and it was tried. The classic
//! signed-area accumulator — one cell per pixel, prefix-sum along the row,
//! `min(|acc|, 1)` for nonzero — is smaller and faster, and it double-counts.
//! Where two subpaths partially cover the same pixel their coverages **add**
//! before the clamp, so a true union is not what comes out. That is harmless
//! for a glyph or a feature arc, whose subpaths do not overlap. It is not
//! harmless for a stroke, which in this design *is* a pile of overlapping quads
//! and discs: measured on a 1.25 px ring, the shape this crate draws its
//! backbone with, it inked **19.9% above the annulus** at radius 8, because a
//! ring that thin is almost entirely antialiased edge and has no interior for
//! the clamp to save.
//!
//! Running the winding along sorted crossings gives the union directly, so
//! overlap cannot inflate anything. It is the algorithm a Python prototype of
//! this module was validated with, at 99.3% of pixels identical to resvg on a
//! real map, and the port should have kept it.
//!
//! # Stroking is a fill, because the joins are round
//!
//! The SVG root says `stroke-linecap="round" stroke-linejoin="round"` and the
//! PDF emits `1 J 1 j`, so round is already this crate's agreed stroke model —
//! and that makes stroke-to-fill nearly free. A stroke is the union of one quad
//! per segment and one disc per joint and cap, and a nonzero fill of that union
//! is exactly the stroke. **Provided every subpath is wound alike**: two
//! overlapping quads wound opposite cancel, and the stroke gets a hole where it
//! is thickest. [`wound`] is what guarantees it.
//!
//! That normalisation applies to stroke geometry and **must never** touch fill
//! geometry. A glyph's counter and a donut's hole are cut by the outer and
//! inner contours being wound *opposite* on purpose. Measured while prototyping
//! this: normalising glyph contours produced 1.44× the correct ink, because
//! every `o` filled in solid.
//!
//! # Flattening
//!
//! Arcs come to us in centre form with a radius, so the segment count is driven
//! by sagitta in **device** pixels, not by a fixed count. A fixed count is the
//! trap because it is right at one radius only: [`arc_points`] emits exactly
//! 720 segments at a radius of 5252 px, the last radius at which a 720-gon's
//! sagitta `r(1 − cos(π/720))` is still under [`FLATNESS`].
//!
//! WHICH WAY IT IS WRONG EITHER SIDE OF THAT MATTERS, and this paragraph had it
//! backwards until 2026-08-04, when it read "invisible at 200 px radius and
//! visibly polygonal at 800". At a radius of 800 px a 720-gon sags 0.0076 px —
//! finer than `FLATNESS` by a factor of 6.6, where the rule itself emits only
//! 281 segments — and its sagitta does not reach half a pixel until a radius of
//! 52525 px, which no page holds. So a fixed count is not a quality trap at
//! figure radii. It is a work trap, and the work is at the small end.
//!
//! Every joint and cap of a stroked path is a [`disc`] of the stroke's
//! half-width — 0.625 px for the 1.25 px backbone — which the rule flattens to
//! 8 points, and [`stroke_of`] strings one on every vertex of the line it is
//! stroking. The backbone ring at a radius of 800 px is 282 points, so it
//! carries 283 of those octagons; a fixed 720-gon would build the same picture
//! out of 720 ring points and 721 720-gons.
//!
//! # Determinism, stated rather than claimed
//!
//! `crates/pl-draw/tests/agreement.rs` pins its tolerance at `1e-6` and says
//! why: "`sin`/`cos` are not specified to the last bit by either standard".
//! SVG, PDF and EPS escape that because [`crate::n`] rounds every coordinate to
//! two decimals before it reaches the file. **A raster has no such step.** A
//! last-bit difference in `sin` moves a flattened vertex, which moves a coverage
//! value, which can move a byte.
//!
//! So this module does not claim byte-identical output across platforms. It is
//! deterministic for one build on one platform — asserted in `tests.rs` — and
//! whether the stronger claim is bought (with a deterministic trig module) is a
//! product decision recorded in the PNG task notes, not something to assume
//! here.

use crate::png::Image;
use crate::scene::{Item, Scene, Seg};

/// How far a flattened chord may sag from the true curve, in device pixels.
///
/// A twentieth of a pixel, which is an order of magnitude ABOVE what an 8-bit
/// coverage value can express and not below it — this comment said below until
/// 2026-08-04. [`span`] resolves horizontal coverage analytically, so on a
/// near-vertical edge 1/255 of a pixel of displacement is already one coverage
/// step, and FLATNESS is 12.75 coverage steps wide.
///
/// What it buys is therefore not the placement of any one edge but the length
/// of the whole curve, which is what a stroked ring's ink is a measure of and
/// what `a_ring_of_any_size_inks_the_same_fraction_of_its_area` bounds at every
/// radius. Driving it down to 1/255, where the claim above would be true, would
/// take the flattening at radius 800 px from 281 segments to 1004 — a factor of
/// 3.6, for a curve the ring test cannot tell from the circle either way, its
/// detection floor being around 16 segments.
const FLATNESS: f64 = 0.05;

/// What the raster could not draw.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Strings holding a character the committed face cannot spell. Not silent:
    /// a label that quietly lost a glyph is a label somebody would publish.
    /// Mirrors [`crate::pdf::Report::unencodable`].
    pub unencodable: Vec<String>,
    /// Colours no parser here understood, and so did not draw at all.
    pub unparsed_colours: Vec<String>,
}

/// A polygon in device space.
type Poly = Vec<(f64, f64)>;

/// Twice the signed area — the sign is the winding, the magnitude is unused.
fn twice_area(p: &Poly) -> f64 {
    let mut s = 0.0;
    for i in 0..p.len() {
        let (ax, ay) = p[i];
        let (bx, by) = p[(i + 1) % p.len()];
        s += ax * by - bx * ay;
    }
    s
}

/// The same polygon, wound positively.
///
/// For **stroke** geometry only. See the module comment: doing this to a fill
/// fills in every hole it has.
fn wound(mut p: Poly) -> Poly {
    if twice_area(&p) < 0.0 {
        p.reverse();
    }
    p
}

/// Points along a circular arc, in this crate's angle convention.
///
/// Zero at twelve o'clock, increasing clockwise, screen y down — so a point is
/// `(cx + r·sin θ, cy − r·cos θ)`, matching [`crate::scene::on_circle`].
fn arc_points(cx: f64, cy: f64, r: f64, from: f64, to: f64, scale: f64, out: &mut Poly) {
    let sweep = (to - from).abs();
    let rd = r * scale;
    if !(rd.is_finite() && sweep.is_finite()) || rd <= 0.0 || sweep <= 0.0 {
        return;
    }
    // sagitta = r(1 − cos(θ/2)); solve for the θ that keeps it under FLATNESS.
    let n = if rd <= FLATNESS {
        1
    } else {
        let ratio = 1.0 - FLATNESS / rd;
        let theta = 2.0 * ratio.clamp(-1.0, 1.0).acos();
        if theta <= 0.0 {
            1
        } else {
            // Capped: a radius large enough to demand millions of segments is a
            // scene nobody asked for, and an unbounded count here is an
            // out-of-memory rather than a coarse curve.
            (sweep / theta).ceil().clamp(1.0, 4_194_304.0) as usize
        }
    };
    for i in 1..=n {
        let a = from + (to - from) * (i as f64) / (n as f64);
        out.push((cx + r * a.sin(), cy - r * a.cos()));
    }
}

/// A path's segments as closed polygons in device space.
///
/// Returns each subpath and whether it was explicitly closed. `Seg::Close`
/// ends a subpath; a `Seg::Move` after any drawing starts a new one.
fn flatten(segs: &[Seg], scale: f64) -> Vec<(Poly, bool)> {
    let mut out: Vec<(Poly, bool)> = Vec::new();
    let mut cur: Poly = Vec::new();
    let mut start = (0.0, 0.0);
    for s in segs {
        match *s {
            Seg::Move(x, y) => {
                if cur.len() > 1 {
                    out.push((std::mem::take(&mut cur), false));
                } else {
                    cur.clear();
                }
                start = (x, y);
                cur.push(start);
            }
            Seg::Line(x, y) => cur.push((x, y)),
            Seg::Arc {
                cx,
                cy,
                r,
                from,
                to,
            } => {
                if cur.is_empty() {
                    let (x, y) = crate::scene::on_circle(cx, cy, r, from);
                    start = (x, y);
                    cur.push(start);
                }
                arc_points(cx, cy, r, from, to, scale, &mut cur);
            }
            Seg::Close => {
                if cur.len() > 1 {
                    out.push((std::mem::take(&mut cur), true));
                } else {
                    cur.clear();
                }
                cur.push(start);
            }
        }
    }
    if cur.len() > 1 {
        out.push((cur, false));
    }
    out
}

/// A disc, flattened to the same tolerance as everything else.
fn disc(cx: f64, cy: f64, r: f64, scale: f64) -> Poly {
    let mut p = Vec::new();
    // Two half-turns rather than one full one: `arc_points` writes the points
    // after `from`, so a single 2π sweep would omit the start.
    p.push((cx, cy - r));
    arc_points(cx, cy, r, 0.0, std::f64::consts::PI, scale, &mut p);
    arc_points(
        cx,
        cy,
        r,
        std::f64::consts::PI,
        std::f64::consts::TAU,
        scale,
        &mut p,
    );
    p.pop();
    p
}

/// A stroked polyline as polygons whose nonzero fill is the stroke.
fn stroke_of(line: &Poly, closed: bool, w: f64, scale: f64) -> Vec<Poly> {
    let h = w / 2.0;
    if !(h.is_finite()) || h <= 0.0 || line.len() < 2 {
        return Vec::new();
    }
    let mut pts = line.clone();
    if closed && pts.first() != pts.last() {
        pts.push(pts[0]);
    }
    let mut out = Vec::new();
    let mut drawn = 0usize;
    for i in 0..pts.len() - 1 {
        let ((ax, ay), (bx, by)) = (pts[i], pts[i + 1]);
        let (dx, dy) = (bx - ax, by - ay);
        let len = dx.hypot(dy);
        if len < 1e-12 {
            continue;
        }
        drawn += 1;
        let (nx, ny) = (-dy / len * h, dx / len * h);
        out.push(wound(vec![
            (ax + nx, ay + ny),
            (bx + nx, by + ny),
            (bx - nx, by - ny),
            (ax - nx, ay - ny),
        ]));
    }
    if drawn == 0 {
        // Every segment was degenerate. SVG draws a round cap on a zero-length
        // subpath as a dot, and dropping it silently loses a tick mark.
        return vec![wound(disc(pts[0].0, pts[0].1, h, scale))];
    }
    // One disc per vertex: the round joins, and the round caps at each end.
    for &(x, y) in &pts {
        out.push(wound(disc(x, y, h, scale)));
    }
    out
}

/// Sub-scanlines per pixel row.
///
/// [`span`] resolves x analytically and nothing resolves y, so this constant
/// alone sets the vertical resolution: [`Cov::sweep`] adds `1.0 / SUB` per
/// covered sub-scanline, so on a near-horizontal edge coverage lands on
/// multiples of 1/16 and the worst error is half a step, 1/32 of a pixel — 8
/// of the 255 levels an 8-bit channel carries. That is an order of magnitude
/// ABOVE 8-bit resolution rather than below it, which is what this comment said
/// until 2026-08-04, and it is why
/// `a_ring_of_any_size_inks_the_same_fraction_of_its_area` has a residue to
/// charge to quantisation at all.
///
/// Sixteen is where it stops paying. The sweep makes one pass over the active
/// edge list per sub-scanline, so cost is linear in this number, and actually
/// reaching the 8-bit floor would take SUB = 128 — a worst error of 1/256 of a
/// pixel, for eight times the sweep. Sixteen is also what the prototype scored
/// 99.3% against resvg with.
const SUB: usize = 16;

/// One edge of a polygon, ready to be crossed by a scanline.
struct Edge {
    ylo: f64,
    yhi: f64,
    /// x at `ylo`, and dx per unit y.
    x: f64,
    dxdy: f64,
    /// +1 if the edge runs down the page, -1 if up. The winding.
    dir: i32,
}

/// Coverage over the whole canvas, by spans.
struct Cov {
    w: usize,
    h: usize,
    edges: Vec<Edge>,
}

impl Cov {
    fn new(w: usize, h: usize) -> Cov {
        Cov {
            w,
            h,
            edges: Vec::new(),
        }
    }

    fn add(&mut self, polys: &[Poly]) {
        for p in polys {
            for i in 0..p.len() {
                let (x0, y0) = p[i];
                let (x1, y1) = p[(i + 1) % p.len()];
                if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite())
                    || y0 == y1
                {
                    continue; // a horizontal edge crosses no scanline
                }
                let (dir, ax, ay, bx, by) = if y0 < y1 {
                    (1, x0, y0, x1, y1)
                } else {
                    (-1, x1, y1, x0, y0)
                };
                self.edges.push(Edge {
                    ylo: ay,
                    yhi: by,
                    x: ax,
                    dxdy: (bx - ax) / (by - ay),
                    dir,
                });
            }
        }
    }

    /// Composite everything added since the last sweep, then clear.
    fn sweep(&mut self, img: &mut Image, rgb: [u8; 3]) {
        if self.edges.is_empty() {
            return;
        }
        let (w, h) = (self.w, self.h);
        // Only the rows the shape actually touches.
        let ymin = self
            .edges
            .iter()
            .fold(f64::INFINITY, |a, e| a.min(e.ylo))
            .max(0.0);
        let ymax = self
            .edges
            .iter()
            .fold(f64::NEG_INFINITY, |a, e| a.max(e.yhi))
            .min(h as f64);
        if ymin >= ymax {
            self.edges.clear();
            return;
        }
        // Bucket each edge into the first row it reaches, and keep an active
        // list, so a tall figure does not rescan every edge for every row.
        let first = ymin.floor() as usize;
        let last = (ymax.ceil() as usize).min(h);
        let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); last - first + 1];
        for (i, e) in self.edges.iter().enumerate() {
            let r = (e.ylo.max(0.0).floor() as usize).clamp(first, last);
            buckets[r - first].push(i);
        }

        let mut active: Vec<usize> = Vec::new();
        let mut row = vec![0.0f64; w];
        let mut hits: Vec<(f64, i32)> = Vec::new();
        for y in first..last {
            active.extend_from_slice(&buckets[y - first]);
            active.retain(|&i| self.edges[i].yhi > y as f64);
            if active.is_empty() {
                continue;
            }
            row.fill(0.0);
            for s in 0..SUB {
                let sy = y as f64 + (s as f64 + 0.5) / SUB as f64;
                hits.clear();
                for &i in &active {
                    let e = &self.edges[i];
                    if e.ylo <= sy && e.yhi > sy {
                        hits.push((e.x + (sy - e.ylo) * e.dxdy, e.dir));
                    }
                }
                if hits.len() < 2 {
                    continue;
                }
                hits.sort_by(|a, b| a.0.total_cmp(&b.0));
                // Run the winding along the crossings. Where it is non-zero the
                // interval is inside -- THE NONZERO RULE, and a true union, so
                // overlapping subpaths cannot inflate anything.
                let mut wind = 0i32;
                for k in 0..hits.len() - 1 {
                    wind += hits[k].1;
                    if wind != 0 {
                        span(&mut row, hits[k].0, hits[k + 1].0, w, 1.0 / SUB as f64);
                    }
                }
            }
            let px = img.pixels_mut();
            for (x, &cov) in row.iter().enumerate() {
                let c = cov.min(1.0);
                if c > 0.0 {
                    let i = (y * w + x) * 3;
                    for k in 0..3 {
                        let dst = f64::from(px[i + k]);
                        let src = f64::from(rgb[k]);
                        px[i + k] = (dst + (src - dst) * c).round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
        self.edges.clear();
    }
}

/// Add the exact horizontal coverage of `[xa, xb)` to one pixel row.
fn span(row: &mut [f64], xa: f64, xb: f64, w: usize, weight: f64) {
    let xa = xa.max(0.0);
    let xb = xb.min(w as f64);
    if xb <= xa {
        return;
    }
    let ia = xa.floor() as usize;
    let ib = (xb.floor() as usize).min(w - 1);
    if ia == ib {
        row[ia] += (xb - xa) * weight;
        return;
    }
    row[ia] += (((ia + 1) as f64) - xa) * weight;
    for cell in row.iter_mut().take(ib).skip(ia + 1) {
        *cell += weight;
    }
    row[ib] += (xb - ib as f64) * weight;
}

/// A colour string as RGB.
///
/// Handles what [`crate::safe_color`] can emit: `#rgb`, `#rgba`, `#rrggbb`,
/// `#rrggbbaa`, `rgb()`/`rgba()`, `hsl()`/`hsla()`, and the bare keywords. Note
/// that `safe_color` accepts **any** alphabetic word of 1..=32 characters, so a
/// name that is not a colour at all can reach here; `None` is the honest answer
/// and the caller records it rather than guessing.
///
/// Alpha is parsed and discarded: nothing in a scene is translucent today, and
/// silently compositing at an alpha the vector back ends ignore would make the
/// PNG disagree with the PDF.
fn colour(s: &str) -> Option<[u8; 3]> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("none") {
        return None;
    }
    if let Some(hex) = s.strip_prefix('#') {
        let d: Option<Vec<u8>> = hex
            .bytes()
            .map(|b| (b as char).to_digit(16).map(|v| v as u8))
            .collect();
        let d = d?;
        return match d.len() {
            3 | 4 => Some([d[0] * 17, d[1] * 17, d[2] * 17]),
            6 | 8 => Some([d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5]]),
            _ => None,
        };
    }
    if let Some(rest) = s.strip_prefix("rgb").and_then(strip_call) {
        // A percentage channel is a fraction of 255, and nothing downstream
        // rescales it, so it is resolved here.
        let n = numbers(&rest, Some(255.0));
        if n.len() >= 3 {
            let f = |v: f64| v.clamp(0.0, 255.0).round() as u8;
            return Some([f(n[0]), f(n[1]), f(n[2])]);
        }
        return None;
    }
    if let Some(rest) = s.strip_prefix("hsl").and_then(strip_call) {
        // `None` — deliberately no scaling. Saturation and lightness are
        // divided by 100 three lines down, so resolving the `%` here as well
        // would double-count: `hsl(0, 100%, 50%)` would reach `from_hsl` as
        // s=2.55, l=1.275, both clamp to 1.0, and pure red would rasterise
        // white. Hue is in degrees and carries no percentage form at all.
        let n = numbers(&rest, None);
        if n.len() >= 3 {
            return Some(from_hsl(n[0], n[1] / 100.0, n[2] / 100.0));
        }
        return None;
    }
    named(s)
}

/// Strip an optional `a` and the surrounding parentheses of a colour function.
fn strip_call(s: &str) -> Option<String> {
    let s = s.strip_prefix('a').unwrap_or(s);
    let s = s.strip_prefix('(')?.strip_suffix(')')?;
    Some(s.to_string())
}

/// Every number in a colour function's argument list, with each percentage
/// resolved against `pct_of` — or left exactly as written when that is `None`.
///
/// The scale is the caller's because the two colour functions resolve a `%`
/// differently and neither can be guessed from the digits: in `rgb()` it is a
/// fraction of 255, in `hsl()` the caller divides by 100 itself. Passing it in
/// keeps that choice at the site that knows the units.
///
/// `Some(full)` multiplies before dividing — `v * full / 100.0`, not
/// `v * (full / 100.0)`. 255/100 is not representable in binary, so the second
/// form computes 50% as 50 × 2.549999999999999822… = 127.49999999999999, which
/// rounds to 127 where the exact answer 127.5 rounds to 128. Measured: the
/// factored form returned [127, 127, 127] for `rgb(50%,50%,50%)`. `f64::round`
/// breaks the tie away from zero, which is where every other renderer puts it.
///
/// `None` is not "scale by one" for the same reason. `v * 100.0 / 100.0` is not
/// the identity for every `f64`, and `hsl` wants the parsed digits untouched.
///
/// This used to push the raw number on both arms — the `%` was stepped past and
/// otherwise ignored — so a percentage `rgb()` was read as if it were absolute.
/// Measured against `raster::draw` before the fix: `rgb(100%,0%,0%)` rasterised
/// to [100, 0, 0], a near-black maroon, where the SVG back end passes the same
/// string through verbatim and any conformant renderer draws [255, 0, 0];
/// `rgb(50%,50%,50%)` gave [50, 50, 50] against a true mid-grey of [128, 128,
/// 128]; `rgb(100%,100%,100%)` gave [100, 100, 100] for white. `Report::
/// unparsed_colours` was EMPTY in every case, so the PNG and the SVG of one
/// figure disagreed with nothing reported. `safe_color` admits `%` inside
/// `rgb(`/`rgba(`, and the percentage spelling reaches here from a GenBank
/// `/ApEinfo_fwdcolor` qualifier or a `.dna` colour field.
fn numbers(s: &str, pct_of: Option<f64>) -> Vec<f64> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() || b[i] == b'-' || b[i] == b'+' || b[i] == b'.' {
            let start = i;
            i += 1;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            let Ok(v) = s[start..i].parse::<f64>() else {
                continue;
            };
            if i < b.len() && b[i] == b'%' {
                i += 1;
                out.push(match pct_of {
                    Some(full) => v * full / 100.0,
                    None => v,
                });
            } else {
                out.push(v);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// HSL to RGB, hue in degrees, saturation and lightness in 0..=1.
fn from_hsl(h: f64, s: f64, l: f64) -> [u8; 3] {
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let f = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [f(r), f(g), f(b)]
}

/// The CSS keywords this crate's own palettes and defaults actually use.
///
/// Deliberately not all 148. Every name here is one `pl-draw`, `pl-gel` or a
/// GenBank colour qualifier can produce; anything else returns `None` and is
/// reported rather than guessed at, because a wrong colour on a plasmid map is
/// a claim about the picture.
fn named(s: &str) -> Option<[u8; 3]> {
    let mut low = String::with_capacity(s.len());
    for c in s.chars() {
        low.push(c.to_ascii_lowercase());
    }
    Some(match low.as_str() {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "lime" => [0, 255, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "cyan" | "aqua" => [0, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255],
        "gray" | "grey" => [128, 128, 128],
        "silver" => [192, 192, 192],
        "maroon" => [128, 0, 0],
        "olive" => [128, 128, 0],
        "navy" => [0, 0, 128],
        "purple" => [128, 0, 128],
        "teal" => [0, 128, 128],
        "orange" => [255, 165, 0],
        _ => return None,
    })
}

/// The canvas [`draw`] will allocate for this scene at this scale.
///
/// Separate from `draw` so a caller can learn the size *before* the pixels
/// exist. `crate::png_at` uses it to refuse an export that would abort the
/// process, and a refusal that guessed the dimensions from `Fit::pixels`
/// instead would be quoting a number one off the buffer it is describing —
/// `Fit` rounds width and height independently, this rounds height against the
/// already-rounded width.
///
/// Saturating rather than wrapping: `f64 as u32` in Rust saturates, so a scale
/// that overflows `u32` lands on `u32::MAX` and is refused by the budget check
/// rather than wrapping to a small canvas that silently renders the wrong
/// picture.
pub fn size(sc: &Scene, scale: f64) -> (u32, u32) {
    let w = (sc.width * scale).round().max(1.0) as u32;
    let h = (sc.height * scale).round().max(1.0) as u32;
    (w, h)
}

/// Draw a scene at `scale` device pixels per scene unit.
///
/// `background` fills the canvas first. See the module comment for what is not
/// drawn, and [`Report`] for what was skipped in this particular call.
///
/// **This allocates whatever it is asked for.** The canvas is `w * h * 3`
/// bytes and the PNG encoder that usually follows costs a further ~34 bytes a
/// pixel; nothing here bounds either. [`crate::png_budget`] is the bound, and
/// [`crate::png_at`] applies it. A caller reaching this function directly owns
/// that check.
pub fn draw(sc: &Scene, scale: f64, background: [u8; 3]) -> (Image, Report) {
    let (w, h) = size(sc, scale);
    let mut img = Image::filled(w, h, background);
    let mut cov = Cov::new(w as usize, h as usize);
    let mut report = Report::default();

    fn note(report: &mut Report, s: &str) {
        if !report.unparsed_colours.iter().any(|k| k == s) {
            report.unparsed_colours.push(s.to_string());
        }
    }

    for item in &sc.items {
        match item {
            Item::Path {
                segs,
                fill,
                stroke,
                stroke_width,
                ..
            } => {
                let subs = flatten(segs, scale);
                let scaled: Vec<(Poly, bool)> = subs
                    .into_iter()
                    .map(|(p, c)| {
                        (
                            p.into_iter().map(|(x, y)| (x * scale, y * scale)).collect(),
                            c,
                        )
                    })
                    .collect();
                if let Some(f) = fill {
                    match colour(f) {
                        // NOT wound: an authored path's subpath winding is
                        // meaning. See the module comment.
                        Some(rgb) => {
                            let polys: Vec<Poly> = scaled.iter().map(|(p, _)| p.clone()).collect();
                            cov.add(&polys);
                            cov.sweep(&mut img, rgb);
                        }
                        None if !f.trim().eq_ignore_ascii_case("none") => note(&mut report, f),
                        None => {}
                    }
                }
                if let Some(st) = stroke {
                    match colour(st) {
                        Some(rgb) => {
                            let mut polys = Vec::new();
                            for (p, closed) in &scaled {
                                polys.extend(stroke_of(p, *closed, stroke_width * scale, scale));
                            }
                            cov.add(&polys);
                            cov.sweep(&mut img, rgb);
                        }
                        None if !st.trim().eq_ignore_ascii_case("none") => note(&mut report, st),
                        None => {}
                    }
                }
            }
            Item::Circle {
                cx,
                cy,
                r,
                stroke,
                stroke_width,
            } => {
                if let Some(rgb) = colour(stroke) {
                    let ring = disc(cx * scale, cy * scale, r * scale, 1.0);
                    let polys = stroke_of(&ring, true, stroke_width * scale, scale);
                    cov.add(&polys);
                    cov.sweep(&mut img, rgb);
                } else if !stroke.trim().eq_ignore_ascii_case("none") {
                    note(&mut report, stroke);
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
                let Some(rgb) = colour(color) else {
                    if !color.trim().eq_ignore_ascii_case("none") {
                        note(&mut report, color);
                    }
                    continue;
                };
                let polys = text_polys(text, *x, *y, *size, *anchor, *bold, scale, &mut report);
                // NOT wound: a glyph's counter is a contour deliberately wound
                // against its outer, and that is what cuts the hole in an 'o'.
                cov.add(&polys);
                cov.sweep(&mut img, rgb);
            }
        }
    }
    (img, report)
}

/// One string's glyph contours, flattened, in device space.
#[allow(clippy::too_many_arguments)]
fn text_polys(
    text: &str,
    x: f64,
    y: f64,
    size: f64,
    anchor: crate::scene::Anchor,
    bold: bool,
    scale: f64,
    report: &mut Report,
) -> Vec<Poly> {
    use crate::scene::Anchor;
    let bytes = if bold {
        crate::font::BOLD
    } else {
        crate::font::REGULAR
    };
    let Some(face) = crate::font::Face::parse(bytes) else {
        return Vec::new();
    };

    // The width the LAYOUT used, so `Middle` and `End` land where the vector
    // back ends put them.
    let total = crate::pdf::text_width_in(text, size, bold);
    let mut pen = match anchor {
        Anchor::Start => x,
        Anchor::Middle => x - total / 2.0,
        Anchor::End => x - total,
    };
    // The scene's y is the visual middle of the glyphs; the alphabetic baseline
    // sits half an x-height below it. Shared with `pdf` and `eps`.
    let base = y + size * crate::pdf::BASELINE_DROP_EM;
    let k = size / face.units_per_em;

    let mut out: Vec<Poly> = Vec::new();
    let mut missing = false;
    for c in text.chars() {
        let advance = crate::pdf::text_width_in(&c.to_string(), size, bold);
        match face.glyph(c) {
            Some(gid) if gid != 0 || c == ' ' => {
                let mut cur: Poly = Vec::new();
                let mut at = (0.0f64, 0.0f64);
                // Font units are y-up; the scene is y-down.
                let to_dev = |gx: f64, gy: f64| ((pen + gx * k) * scale, (base - gy * k) * scale);
                for step in face.outline(gid) {
                    match step {
                        crate::font::Curve::Move(gx, gy) => {
                            if cur.len() > 2 {
                                out.push(std::mem::take(&mut cur));
                            } else {
                                cur.clear();
                            }
                            at = to_dev(gx, gy);
                            cur.push(at);
                        }
                        crate::font::Curve::Line(gx, gy) => {
                            at = to_dev(gx, gy);
                            cur.push(at);
                        }
                        crate::font::Curve::Quad(cx, cy, gx, gy) => {
                            let c1 = to_dev(cx, cy);
                            let end = to_dev(gx, gy);
                            quad(at, c1, end, &mut cur);
                            at = end;
                        }
                        crate::font::Curve::Close => {
                            if cur.len() > 2 {
                                out.push(std::mem::take(&mut cur));
                            } else {
                                cur.clear();
                            }
                        }
                    }
                }
                if cur.len() > 2 {
                    out.push(cur);
                }
            }
            _ => missing = true,
        }
        pen += advance;
    }
    if missing && !report.unencodable.iter().any(|s| s == text) {
        report.unencodable.push(text.to_string());
    }
    out
}

/// A quadratic Bézier as a polyline, segment count from its own flatness.
///
/// The control point's distance from the chord's midpoint bounds the curve's
/// deviation, and a quadratic's maximum error under `n` uniform segments falls
/// as `1/n²` — so `n = ceil(sqrt(dev / tolerance))` and the tolerance is in
/// device pixels, like every other flattening decision here.
fn quad(p0: (f64, f64), c: (f64, f64), p1: (f64, f64), out: &mut Poly) {
    let mid = ((p0.0 + p1.0) / 2.0, (p0.1 + p1.1) / 2.0);
    let dev = (c.0 - mid.0).hypot(c.1 - mid.1) / 2.0;
    let n = if dev <= FLATNESS {
        1
    } else {
        (dev / FLATNESS).sqrt().ceil().clamp(1.0, 256.0) as usize
    };
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let u = 1.0 - t;
        out.push((
            u * u * p0.0 + 2.0 * u * t * c.0 + t * t * p1.0,
            u * u * p0.1 + 2.0 * u * t * c.1 + t * t * p1.1,
        ));
    }
}

#[cfg(test)]
mod tests;
