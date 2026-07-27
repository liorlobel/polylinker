//! What a map *is*, before it is written as anything.
//!
//! One geometry, two back ends. `docs/PLAN.md` calls for "SVG export via
//! serialized DOM → `resvg`/`svg2pdf`", but there is nothing to convert: we
//! generate the drawing, so parsing our own SVG back in order to re-emit it as
//! PDF would add a large dependency tree to translate between two
//! representations we already own. A [`Scene`] is built once and rendered
//! twice.
//!
//! That is also the only way SVG and PDF cannot drift. Two independent
//! emitters would need an agreement harness like the one `pl-draw` keeps
//! against the TypeScript renderer; one scene needs nothing, because there is
//! nothing to disagree about above the level of ink.
//!
//! # Arcs
//!
//! Held in **centre form** — centre, radius, start and end angle — because
//! that is how every arc here is generated. SVG wants endpoint form with
//! large-arc and sweep flags; PDF has no arc operator at all and wants cubic
//! Béziers. Each back end converts from the centre form, and neither has to
//! reverse-engineer the other's.
//!
//! Angles follow the field's convention and the rest of this crate's: zero at
//! twelve o'clock, increasing **clockwise**, with screen `y` pointing down, so
//! a point is `(cx + r·sin θ, cy − r·cos θ)`.

/// One piece of a path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Move(f64, f64),
    Line(f64, f64),
    /// A circular arc, centre-parameterised. `to` may be less than `from`,
    /// which means the arc runs anticlockwise.
    Arc {
        cx: f64,
        cy: f64,
        r: f64,
        from: f64,
        to: f64,
    },
    Close,
}

/// Where a text item's `x` sits relative to the string.
///
/// SVG says this declaratively with `text-anchor`. PDF has no such thing and
/// must measure the string, which is why [`crate::pdf`] carries font metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// One drawable thing.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Path {
        segs: Vec<Seg>,
        /// Already passed through `safe_color`; never raw file input.
        fill: Option<String>,
        stroke: Option<String>,
        stroke_width: f64,
        /// A tooltip in SVG. PDF has no equivalent and drops it, which is
        /// stated rather than silently true.
        title: Option<String>,
    },
    /// Kept distinct from a path so SVG can emit `<circle>`, which is what a
    /// reader of the file expects a circle to look like.
    Circle {
        cx: f64,
        cy: f64,
        r: f64,
        stroke: String,
        stroke_width: f64,
    },
    Text {
        x: f64,
        y: f64,
        size: f64,
        anchor: Anchor,
        color: String,
        bold: bool,
        /// Baseline is the *middle* of the glyphs, matching SVG's
        /// `dominant-baseline: middle`, which is what the layout assumes.
        text: String,
    },
}

/// A whole picture, in device-independent coordinates.
///
/// Origin top-left, `y` down — SVG's convention, because that is what the
/// layout code was written in. PDF flips it once, at the moment of emission.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub width: f64,
    pub height: f64,
    pub title: String,
    pub items: Vec<Item>,
}

/// The point on a circle at an angle, in this crate's convention.
#[inline]
pub fn on_circle(cx: f64, cy: f64, r: f64, a: f64) -> (f64, f64) {
    (cx + r * a.sin(), cy - r * a.cos())
}

/// An arc as cubic Béziers, for back ends without an arc operator.
///
/// Returns the control points of each segment as
/// `(c1x, c1y, c2x, c2y, ex, ey)`, in the same coordinate space as the input.
/// The caller has already emitted the arc's start point.
///
/// Split so no segment sweeps more than 90°, and each is the standard
/// approximation with `α = (4/3)·tan(Δ/4)` applied to the parameterisation's
/// own derivative. The error of that approximation over a quarter circle is
/// about 2.7 × 10⁻⁴ of the radius — a fifth of a micron on a 100 mm figure,
/// and far below the 0.01 coordinate resolution everything else here rounds to.
pub fn arc_to_beziers(cx: f64, cy: f64, r: f64, from: f64, to: f64) -> Vec<[f64; 6]> {
    let sweep = to - from;
    if sweep == 0.0 || r <= 0.0 {
        return Vec::new();
    }
    let steps = (sweep.abs() / (std::f64::consts::FRAC_PI_2))
        .ceil()
        .max(1.0) as usize;
    let delta = sweep / steps as f64;
    // For P(t) = (cx + r·sin t, cy − r·cos t), P'(t) = (r·cos t, r·sin t).
    let alpha = (4.0 / 3.0) * (delta / 4.0).tan();

    let mut out = Vec::with_capacity(steps);
    for i in 0..steps {
        let t0 = from + delta * i as f64;
        let t1 = t0 + delta;
        let (x0, y0) = on_circle(cx, cy, r, t0);
        let (x1, y1) = on_circle(cx, cy, r, t1);
        let (d0x, d0y) = (r * t0.cos(), r * t0.sin());
        let (d1x, d1y) = (r * t1.cos(), r * t1.sin());
        out.push([
            x0 + alpha * d0x,
            y0 + alpha * d0y,
            x1 - alpha * d1x,
            y1 - alpha * d1y,
            x1,
            y1,
        ]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Evaluate a cubic Bézier at `t`.
    fn bez(p0: (f64, f64), c: &[f64; 6], t: f64) -> (f64, f64) {
        let u = 1.0 - t;
        let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        (
            b0 * p0.0 + b1 * c[0] + b2 * c[2] + b3 * c[4],
            b0 * p0.1 + b1 * c[1] + b2 * c[3] + b3 * c[5],
        )
    }

    /// The claim the PDF back end rests on: the Béziers really are the arc.
    ///
    /// Asserted by distance from the centre at many points along every
    /// segment, which is the property that matters — a control-point formula
    /// can be plausible and still bulge.
    #[test]
    fn beziers_stay_on_the_circle_they_approximate() {
        let (cx, cy, r) = (100.0, 250.0, 80.0);
        let mut worst: f64 = 0.0;
        for &(from, to) in &[
            (0.0, std::f64::consts::FRAC_PI_2),
            (0.0, std::f64::consts::PI),
            (0.0, std::f64::consts::TAU),
            (0.3, 0.31),
            (1.0, -2.0),
            (-0.5, 4.0),
            (std::f64::consts::TAU, 0.0),
        ] {
            let mut prev = on_circle(cx, cy, r, from);
            for seg in arc_to_beziers(cx, cy, r, from, to) {
                for k in 0..=32 {
                    let (x, y) = bez(prev, &seg, k as f64 / 32.0);
                    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
                    worst = worst.max((d - r).abs());
                }
                prev = (seg[4], seg[5]);
            }
            // And it must finish where the arc finishes.
            let end = on_circle(cx, cy, r, to);
            assert!(
                (prev.0 - end.0).abs() < 1e-9 && (prev.1 - end.1).abs() < 1e-9,
                "arc {from}..{to} ended at {prev:?}, not {end:?}"
            );
        }
        // 2.7e-4 of the radius is the textbook bound for a quarter circle.
        assert!(worst < r * 3.0e-4, "worst deviation {worst} on r = {r}");
    }

    #[test]
    fn a_zero_length_arc_emits_nothing() {
        assert!(arc_to_beziers(0.0, 0.0, 10.0, 1.0, 1.0).is_empty());
        assert!(arc_to_beziers(0.0, 0.0, 0.0, 0.0, 1.0).is_empty());
    }

    #[test]
    fn an_arc_is_split_so_no_segment_sweeps_more_than_a_quarter_turn() {
        // A single Bézier cannot represent much more than 90 degrees without
        // visible error, and a full circle in one segment degenerates.
        use std::f64::consts::{FRAC_PI_2, PI, TAU};
        assert_eq!(arc_to_beziers(0.0, 0.0, 5.0, 0.0, FRAC_PI_2).len(), 1);
        assert_eq!(arc_to_beziers(0.0, 0.0, 5.0, 0.0, PI).len(), 2);
        assert_eq!(arc_to_beziers(0.0, 0.0, 5.0, 0.0, TAU).len(), 4);
        assert_eq!(arc_to_beziers(0.0, 0.0, 5.0, 0.0, -TAU).len(), 4);
        assert_eq!(arc_to_beziers(0.0, 0.0, 5.0, 0.0, 0.01).len(), 1);
    }

    #[test]
    fn twelve_oclock_is_angle_zero_and_angles_run_clockwise() {
        // The convention the whole crate shares. Getting it backwards produces
        // a map that is not a stylistic variation, it is wrong-looking.
        let (x, y) = on_circle(0.0, 0.0, 10.0, 0.0);
        assert!(
            (x - 0.0).abs() < 1e-12 && (y + 10.0).abs() < 1e-12,
            "{x},{y}"
        );
        let (x, y) = on_circle(0.0, 0.0, 10.0, std::f64::consts::FRAC_PI_2);
        assert!(
            (x - 10.0).abs() < 1e-12 && y.abs() < 1e-12,
            "a quarter turn should be three o'clock, got {x},{y}"
        );
    }
}
