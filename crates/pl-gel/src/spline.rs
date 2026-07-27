//! Monotone cubic Hermite interpolation — Fritsch & Carlson (1980).
//!
//! A gel calibration curve must be **monotone**: a longer fragment can never
//! run further than a shorter one. An ordinary cubic spline through measured
//! ladder points does not guarantee that. It overshoots between knots, and
//! where a ladder has an uneven gap — 3 kb, 4 kb, 6 kb, 10 kb, which is what
//! real ladders look like — the overshoot is large enough to reverse the order
//! of two bands. The picture then shows a 4 kb fragment above a 3 kb one, which
//! is not a rounding error but a wrong answer about which band is which.
//!
//! Fritsch–Carlson fixes the tangents so monotonicity holds *by construction*
//! rather than by luck: compute the naive tangents, then clamp any that would
//! let the cubic turn around. This is the same algorithm as SciPy's
//! `PchipInterpolator`, which is what the gate compares against.
//!
//! Reference: F. N. Fritsch and R. E. Carlson, "Monotone Piecewise Cubic
//! Interpolation", *SIAM Journal on Numerical Analysis* 17(2):238–246, 1980.

/// A monotone interpolant through a set of knots.
#[derive(Debug, Clone, PartialEq)]
pub struct Monotone {
    xs: Vec<f64>,
    ys: Vec<f64>,
    /// One tangent per knot, already clamped for monotonicity.
    ms: Vec<f64>,
}

/// Why a set of knots could not be interpolated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Fewer than two knots: there is nothing to interpolate between.
    TooFewPoints,
    /// Two knots share an x. Which y is right is not a question the
    /// interpolator can answer, and picking one silently would hide a
    /// duplicated ladder band.
    RepeatedX,
    /// The knots are not sorted by x.
    Unsorted,
    /// A knot is NaN or infinite.
    NotFinite,
}

impl Monotone {
    /// Build from knots sorted by `x`.
    ///
    /// The knots themselves are not required to be monotone in `y` — the
    /// algorithm handles a curve that rises and falls — but each *interval* is
    /// interpolated without overshooting its own endpoints.
    pub fn new(points: &[(f64, f64)]) -> Result<Monotone, Error> {
        if points.len() < 2 {
            return Err(Error::TooFewPoints);
        }
        if points.iter().any(|(x, y)| !x.is_finite() || !y.is_finite()) {
            return Err(Error::NotFinite);
        }
        for w in points.windows(2) {
            if w[1].0 == w[0].0 {
                return Err(Error::RepeatedX);
            }
            if w[1].0 < w[0].0 {
                return Err(Error::Unsorted);
            }
        }

        let n = points.len();
        let xs: Vec<f64> = points.iter().map(|p| p.0).collect();
        let ys: Vec<f64> = points.iter().map(|p| p.1).collect();

        // Secant slopes.
        let d: Vec<f64> = (0..n - 1)
            .map(|i| (ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i]))
            .collect();

        // Interior tangents: the weighted harmonic mean of the neighbouring
        // secants, which is what keeps the cubic inside the data. A sign change
        // (or a flat step) pins the tangent to zero — an extremum sits exactly
        // at the knot rather than between two of them.
        let mut ms = vec![0.0f64; n];
        ms[0] = end_tangent(d[0], if n > 2 { d[1] } else { d[0] }, xs[1] - xs[0], {
            if n > 2 {
                xs[2] - xs[1]
            } else {
                xs[1] - xs[0]
            }
        });
        ms[n - 1] = end_tangent(
            d[n - 2],
            if n > 2 { d[n - 3] } else { d[n - 2] },
            xs[n - 1] - xs[n - 2],
            if n > 2 {
                xs[n - 2] - xs[n - 3]
            } else {
                xs[n - 1] - xs[n - 2]
            },
        );
        for i in 1..n - 1 {
            let (h0, h1) = (xs[i] - xs[i - 1], xs[i + 1] - xs[i]);
            let (d0, d1) = (d[i - 1], d[i]);
            ms[i] = if d0 * d1 <= 0.0 {
                0.0
            } else {
                let w0 = 2.0 * h1 + h0;
                let w1 = h1 + 2.0 * h0;
                (w0 + w1) / (w0 / d0 + w1 / d1)
            };
        }

        Ok(Monotone { xs, ys, ms })
    }

    /// The interpolated value at `x`.
    ///
    /// Outside the knots this **clamps to the nearest endpoint** rather than
    /// extrapolating. Extrapolating a calibration curve past the smallest and
    /// largest bands of the ladder that produced it is exactly where a gel
    /// prediction stops meaning anything, and the caller has no way to notice a
    /// cubic quietly running off. [`Self::domain`] says where the answers are
    /// real; a caller that cares must ask.
    pub fn at(&self, x: f64) -> f64 {
        let n = self.xs.len();
        if x <= self.xs[0] {
            return self.ys[0];
        }
        if x >= self.xs[n - 1] {
            return self.ys[n - 1];
        }
        // The interval containing x.
        let mut lo = 0usize;
        let mut hi = n - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if self.xs[mid] <= x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let h = self.xs[lo + 1] - self.xs[lo];
        let t = (x - self.xs[lo]) / h;
        let (t2, t3) = (t * t, t * t * t);
        // Hermite basis.
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        h00 * self.ys[lo]
            + h10 * h * self.ms[lo]
            + h01 * self.ys[lo + 1]
            + h11 * h * self.ms[lo + 1]
    }

    /// The range of `x` the knots actually cover.
    pub fn domain(&self) -> (f64, f64) {
        (self.xs[0], self.xs[self.xs.len() - 1])
    }

    pub fn knots(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.xs.iter().copied().zip(self.ys.iter().copied())
    }
}

/// One-sided three-point tangent, clamped so the end interval cannot overshoot.
///
/// This is the rule SciPy uses, and it matters at exactly the place a gel
/// calibration is most fragile: the largest and smallest ladder bands, where
/// there is data on one side only.
fn end_tangent(d0: f64, d1: f64, h0: f64, h1: f64) -> f64 {
    let m = ((2.0 * h0 + h1) * d0 - h0 * d1) / (h0 + h1);
    if m * d0 <= 0.0 {
        0.0
    } else if d0 * d1 <= 0.0 && m.abs() > (3.0 * d0).abs() {
        3.0 * d0
    } else {
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_passes_through_every_knot_exactly() {
        // A ladder band must land where the ladder says it lands, or the
        // calibration is not a calibration.
        let pts = [(1.0, 10.0), (2.0, 8.0), (3.0, 7.5), (5.0, 2.0)];
        let m = Monotone::new(&pts).unwrap();
        for (x, y) in pts {
            assert!((m.at(x) - y).abs() < 1e-12, "at {x}: {} vs {y}", m.at(x));
        }
    }

    #[test]
    fn an_uneven_ladder_cannot_reverse_two_bands() {
        // The reason this file is not a plain cubic spline. Real ladders have
        // uneven gaps -- 3, 4, 6, 10 kb -- and an ordinary spline overshoots
        // between them by enough to put the 4 kb band above the 3 kb one. That
        // is not a rounding error, it is the wrong answer about which band is
        // which.
        let pts = [
            (f64::log10(500.0), 62.0),
            (f64::log10(1000.0), 48.0),
            (f64::log10(3000.0), 30.0),
            (f64::log10(4000.0), 28.0),
            (f64::log10(6000.0), 24.0),
            (f64::log10(10000.0), 18.0),
        ];
        let m = Monotone::new(&pts).unwrap();
        let (lo, hi) = m.domain();
        let mut prev = f64::INFINITY;
        for i in 0..=20000 {
            let x = lo + (hi - lo) * i as f64 / 20000.0;
            let y = m.at(x);
            assert!(
                y <= prev + 1e-12,
                "the curve turned back at x={x}: {y} after {prev}"
            );
            prev = y;
        }
    }

    #[test]
    fn no_interval_overshoots_its_own_endpoints() {
        let pts = [(0.0, 0.0), (1.0, 1.0), (2.0, 1.02), (3.0, 5.0)];
        let m = Monotone::new(&pts).unwrap();
        for w in pts.windows(2) {
            let (lo, hi) = (w[0].1.min(w[1].1), w[0].1.max(w[1].1));
            for i in 0..=200 {
                let x = w[0].0 + (w[1].0 - w[0].0) * i as f64 / 200.0;
                let y = m.at(x);
                assert!(
                    y >= lo - 1e-9 && y <= hi + 1e-9,
                    "between {:?} and {:?}, at {x} the curve reached {y}",
                    w[0],
                    w[1]
                );
            }
        }
    }

    #[test]
    fn outside_the_ladder_it_clamps_instead_of_extrapolating() {
        // Past the largest and smallest bands of the ladder there is no
        // calibration, and a cubic left to run free goes somewhere confident
        // and wrong. Clamping is visibly flat; extrapolation is not visibly
        // anything.
        let m = Monotone::new(&[(1.0, 10.0), (2.0, 8.0), (3.0, 4.0)]).unwrap();
        assert_eq!(m.at(-50.0), 10.0);
        assert_eq!(m.at(0.999), 10.0);
        assert_eq!(m.at(1000.0), 4.0);
        assert_eq!(m.domain(), (1.0, 3.0));
    }

    #[test]
    fn a_flat_step_stays_flat() {
        let m = Monotone::new(&[(0.0, 5.0), (1.0, 5.0), (2.0, 5.0)]).unwrap();
        for i in 0..=100 {
            assert!((m.at(i as f64 / 50.0) - 5.0).abs() < 1e-12);
        }
    }

    #[test]
    fn bad_knots_are_refused_by_name() {
        assert_eq!(Monotone::new(&[]), Err(Error::TooFewPoints));
        assert_eq!(Monotone::new(&[(1.0, 1.0)]), Err(Error::TooFewPoints));
        assert_eq!(
            Monotone::new(&[(1.0, 1.0), (1.0, 2.0)]),
            Err(Error::RepeatedX),
            "a duplicated ladder band is a question, not something to average"
        );
        assert_eq!(
            Monotone::new(&[(2.0, 1.0), (1.0, 2.0)]),
            Err(Error::Unsorted)
        );
        assert_eq!(
            Monotone::new(&[(1.0, f64::NAN), (2.0, 1.0)]),
            Err(Error::NotFinite)
        );
    }

    #[test]
    fn two_knots_are_a_straight_line() {
        let m = Monotone::new(&[(0.0, 0.0), (10.0, 5.0)]).unwrap();
        for i in 0..=10 {
            let x = i as f64;
            assert!((m.at(x) - x / 2.0).abs() < 1e-12, "at {x}: {}", m.at(x));
        }
    }
}
