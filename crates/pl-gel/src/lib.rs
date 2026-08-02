//! Agarose gel simulation — what the gel will look like, and what it will not tell you.
//!
//! # What this is for
//!
//! Before running a diagnostic digest you want to know whether the result will
//! be readable: will the two fragments you are trying to distinguish actually
//! separate, or arrive as one band? That question has a useful answer. "Exactly
//! how many millimetres will the 1,384 bp fragment travel" does not, and this
//! module is built around the difference.
//!
//! # Three ways a gel simulator lies, and what is done instead
//!
//! **Extrapolating past the ladder.** A calibration curve is only a curve where
//! there were bands. Beyond them a fitted polynomial keeps going, confidently,
//! and puts a 40 kb fragment at a specific place on a 2% gel where in reality
//! it never leaves the well. Here, fragments outside the gel's resolving range
//! are [`Placement::TooLarge`] or [`Placement::TooSmall`] and are *listed* — not
//! drawn at a made-up position, and not silently dropped either.
//!
//! **Drawing every fragment as its own band.** A digest producing 2,000 and
//! 2,100 bp fragments shows *one* band on a 1% gel. A picture with two lines
//! 0.4 mm apart says the digest is diagnostic when it is not, which is the
//! failure that costs a week. [`Simulation::groups`] merges bands closer than
//! the band width and says how many fragments are in each.
//!
//! **A curve that can run backwards.** See [`spline`]: an ordinary cubic
//! through real, unevenly spaced ladder points overshoots enough to swap two
//! bands.
//!
//! # Where the numbers come from
//!
//! Two very different places, and [`Calibration`] keeps them apart, because the
//! difference is the whole question of whether an answer means anything.
//!
//! [`Calibration::Measured`] is a ladder somebody actually ran and measured:
//! sizes against distances. That is a real calibration, and interpolating
//! between its points is trustworthy.
//!
//! [`Calibration::Model`] is the fallback when nobody has measured anything. It
//! places migration linearly in `log10(length)` across the percentage's
//! published resolving range — the first-order description every hand-drawn
//! standard curve approximates. It is a **planning aid**: good enough to say
//! "those two will not separate", not good enough to size an unknown band from.
//! [`Simulation::caveat`] returns that sentence, so a UI cannot forget it.

pub mod render;
pub mod spline;

pub use spline::Monotone;

/// Where a set of band positions came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Calibration {
    /// A ladder that was run and measured: `(bp, distance in mm from the well)`.
    Measured,
    /// Derived from the agarose percentage and a published resolving range.
    /// Approximate by construction.
    Model,
}

/// Standard agarose percentages and the fragment sizes they resolve, in bp.
///
/// The conventional table, as it appears in every supplier's guide and in
/// Sambrook. These are the sizes a gel at that percentage *separates* — the
/// range over which two nearby fragments give two bands — and are the honest
/// domain of the model curve. Percentages between rows are interpolated, and
/// that interpolation is monotone in the same way everything else here is: more
/// agarose never resolves larger fragments.
pub const RESOLVING_RANGES: &[(f64, u64, u64)] = &[
    (0.5, 1_000, 30_000),
    (0.7, 800, 12_000),
    (1.0, 500, 10_000),
    (1.2, 400, 7_000),
    (1.5, 200, 3_000),
    (2.0, 50, 2_000),
];

/// A DNA ladder: the fragment sizes it contains.
///
/// Sizes only. They are facts published by every supplier, and are not the
/// supplier's product; the names here are descriptive rather than anyone's
/// trademark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ladder {
    pub name: &'static str,
    pub sizes: &'static [u64],
}

/// The ladders almost every gel uses.
pub const LADDERS: &[Ladder] = &[
    Ladder {
        name: "1kb",
        sizes: &[
            500, 1_000, 1_500, 2_000, 3_000, 4_000, 5_000, 6_000, 8_000, 10_000,
        ],
    },
    Ladder {
        name: "100bp",
        sizes: &[100, 200, 300, 400, 500, 600, 700, 800, 900, 1_000, 1_500],
    },
    Ladder {
        name: "1kb-plus",
        sizes: &[
            100, 200, 300, 400, 500, 650, 850, 1_000, 1_650, 2_000, 3_000, 4_000, 5_000, 6_000,
            8_000, 10_000, 12_000,
        ],
    },
];

/// Look a ladder up by name.
pub fn ladder(name: &str) -> Option<Ladder> {
    LADDERS
        .iter()
        .find(|l| l.name.eq_ignore_ascii_case(name))
        .copied()
}

/// Settings for a run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Conditions {
    pub agarose_percent: f64,
    /// How far the dye front ran, in mm — the gel's usable length.
    pub run_mm: f64,
    /// How wide a band is, in mm.
    ///
    /// The dominant uncertainty in the whole model, and the number that decides
    /// whether two fragments count as resolved. 1.5 mm is a reasonable mini-gel
    /// band; a heavily loaded lane is worse. It is a parameter because no single
    /// value is right, and a caller changing it is making a judgement this
    /// module cannot make for them.
    pub band_mm: f64,
}

impl Default for Conditions {
    fn default() -> Self {
        Conditions {
            agarose_percent: 1.0,
            run_mm: 80.0,
            band_mm: 1.5,
        }
    }
}

/// Where one fragment ends up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// Distance from the well, in mm.
    At(f64),
    /// Larger than this gel resolves: it sits at or near the well, and exactly
    /// where is not knowable from this model.
    TooLarge,
    /// Smaller than this gel resolves: it runs with or ahead of the dye front.
    TooSmall,
}

impl Placement {
    pub fn mm(&self) -> Option<f64> {
        match self {
            Placement::At(d) => Some(*d),
            _ => None,
        }
    }
}

/// One fragment, placed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub bp: u64,
    pub placement: Placement,
}

/// Fragments that will appear as a single band.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// Distance from the well, in mm — the mean of the members.
    pub mm: f64,
    /// Sizes in the group, ascending. More than one means they co-migrate.
    pub sizes: Vec<u64>,
}

impl Group {
    /// Would a person see one band here where there is more than one fragment?
    pub fn is_merged(&self) -> bool {
        self.sizes.len() > 1
    }
}

/// A gel with a calibration curve.
#[derive(Debug, Clone, PartialEq)]
pub struct Gel {
    conditions: Conditions,
    /// log10(bp) → distance in mm. Decreasing: bigger fragments run less far.
    curve: Monotone,
    calibration: Calibration,
    range: (u64, u64),
}

impl Gel {
    /// A gel calibrated from a ladder somebody actually measured.
    ///
    /// `points` are `(bp, distance from the well in mm)`. The resolving range
    /// becomes the ladder's own span, because that is exactly where the
    /// measurements are.
    pub fn measured(conditions: Conditions, points: &[(u64, f64)]) -> Result<Gel, spline::Error> {
        let mut pts: Vec<(f64, f64)> = points
            .iter()
            .map(|(bp, mm)| ((*bp as f64).log10(), *mm))
            .collect();
        pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let curve = Monotone::new(&pts)?;
        let lo = points.iter().map(|p| p.0).min().unwrap_or(0);
        let hi = points.iter().map(|p| p.0).max().unwrap_or(0);
        Ok(Gel {
            conditions,
            curve,
            calibration: Calibration::Measured,
            range: (lo, hi),
        })
    }

    /// A gel from the agarose percentage alone. Approximate — see the module
    /// docs and [`Simulation::caveat`].
    pub fn modelled(mut conditions: Conditions) -> Gel {
        // This constructor is infallible and public, so it must not panic on a
        // caller that skipped validation. A non-finite or non-positive agarose
        // percentage has no resolving range — `resolving_range` returns (0, 0),
        // whose log10 is -inf and cannot form a curve — so fall back to a
        // standard 1% gel; keep `run_mm` finite for the same reason. The shipped
        // CLI filters agarose to 0.3..=4.0, so this only catches a hand-built
        // value that reached the public constructor directly.
        if !(conditions.agarose_percent.is_finite() && conditions.agarose_percent > 0.0) {
            conditions.agarose_percent = 1.0;
        }
        if !conditions.run_mm.is_finite() {
            conditions.run_mm = 0.0;
        }
        let (lo, hi) = resolving_range(conditions.agarose_percent);
        // Linear in log10(length) between the ends of the resolving range: the
        // largest resolvable fragment just clear of the well, the smallest just
        // short of the dye front. Two knots, so the curve *is* that line — it
        // does not pretend to more structure than there is evidence for.
        let near_well = conditions.run_mm * 0.12;
        let near_front = conditions.run_mm * 0.92;
        let curve = Monotone::new(&[
            ((lo as f64).log10(), near_front),
            ((hi as f64).log10(), near_well),
        ])
        .expect("two distinct finite knots");
        Gel {
            conditions,
            curve,
            calibration: Calibration::Model,
            range: (lo, hi),
        }
    }

    pub fn conditions(&self) -> Conditions {
        self.conditions
    }
    pub fn calibration(&self) -> &Calibration {
        &self.calibration
    }
    /// The smallest and largest fragment this gel can place, in bp.
    pub fn range(&self) -> (u64, u64) {
        self.range
    }

    /// Where one fragment runs.
    pub fn place(&self, bp: u64) -> Placement {
        if bp == 0 || bp < self.range.0 {
            return Placement::TooSmall;
        }
        if bp > self.range.1 {
            return Placement::TooLarge;
        }
        Placement::At(self.curve.at((bp as f64).log10()))
    }

    /// Run a lane.
    pub fn run(&self, fragments: &[u64]) -> Simulation {
        let mut bands: Vec<Band> = fragments
            .iter()
            .map(|bp| Band {
                bp: *bp,
                placement: self.place(*bp),
            })
            .collect();
        bands.sort_by_key(|b| b.bp);

        // Merge what a person would see as one band. Single-linkage on the band
        // width: three fragments 1 mm apart in a row are one smear, and calling
        // the outer two resolved because they are 2 mm apart would be right
        // about the arithmetic and wrong about the picture.
        let mut placed: Vec<(f64, u64)> = bands
            .iter()
            .filter_map(|b| b.placement.mm().map(|mm| (mm, b.bp)))
            .collect();
        placed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut groups: Vec<Group> = Vec::new();
        let mut last_mm = f64::NEG_INFINITY;
        for (mm, bp) in placed {
            if !groups.is_empty() && (mm - last_mm).abs() <= self.conditions.band_mm {
                let g = groups.last_mut().expect("non-empty");
                let n = g.sizes.len() as f64;
                // Running mean, so a group sits where the smear is.
                g.mm = (g.mm * n + mm) / (n + 1.0);
                g.sizes.push(bp);
            } else {
                groups.push(Group {
                    mm,
                    sizes: vec![bp],
                });
            }
            last_mm = mm;
        }
        for g in &mut groups {
            g.sizes.sort_unstable();
        }

        Simulation {
            bands,
            groups,
            calibration: self.calibration.clone(),
            agarose_percent: self.conditions.agarose_percent,
            range: self.range,
        }
    }
}

/// The result of running a lane.
#[derive(Debug, Clone, PartialEq)]
pub struct Simulation {
    /// Every fragment, in size order, including the ones that cannot be placed.
    pub bands: Vec<Band>,
    /// What will actually be visible, in migration order.
    pub groups: Vec<Group>,
    pub calibration: Calibration,
    pub agarose_percent: f64,
    pub range: (u64, u64),
}

impl Simulation {
    /// Fragments too large for this gel to place.
    pub fn too_large(&self) -> Vec<u64> {
        self.bands
            .iter()
            .filter(|b| b.placement == Placement::TooLarge)
            .map(|b| b.bp)
            .collect()
    }

    /// Fragments too small for this gel to place.
    pub fn too_small(&self) -> Vec<u64> {
        self.bands
            .iter()
            .filter(|b| b.placement == Placement::TooSmall)
            .map(|b| b.bp)
            .collect()
    }

    /// Groups holding more than one fragment — the ones that look like a single
    /// band.
    pub fn merged(&self) -> Vec<&Group> {
        self.groups.iter().filter(|g| g.is_merged()).collect()
    }

    /// Can a person tell these two fragment sizes apart on this gel?
    ///
    /// The question the whole module exists to answer.
    pub fn resolves(&self, a: u64, b: u64) -> bool {
        if a == b {
            return false;
        }
        self.placed(a)
            && self.placed(b)
            && !self
                .groups
                .iter()
                .any(|g| g.sizes.contains(&a) && g.sizes.contains(&b))
    }

    fn placed(&self, bp: u64) -> bool {
        self.bands
            .iter()
            .any(|x| x.bp == bp && matches!(x.placement, Placement::At(_)))
    }

    /// What must be said alongside this result.
    ///
    /// Returned rather than left to a UI to remember, for the same reason the
    /// Golden Gate report refuses to print a fidelity percentage: the number
    /// looks equally authoritative either way.
    pub fn caveat(&self) -> String {
        let mut s = match self.calibration {
            Calibration::Measured => "Positions interpolated from a measured ladder.".to_string(),
            Calibration::Model => format!(
                "Positions are modelled from {}% agarose, not measured: migration is \
                 taken as linear in log10(length) across the published resolving range. \
                 Good enough to say whether two fragments will separate; not good enough \
                 to size an unknown band.",
                trim_percent(self.agarose_percent)
            ),
        };
        if !self.too_large().is_empty() || !self.too_small().is_empty() {
            s.push_str(&format!(
                " This gel resolves {}-{} bp; anything outside that is listed rather \
                 than drawn, because where it runs is not knowable from here.",
                self.range.0, self.range.1
            ));
        }
        s
    }
}

/// The resolving range at an arbitrary agarose percentage.
///
/// Interpolated between the tabulated rows and clamped outside them: a 0.3% gel
/// is not in the table, and guessing what it resolves by extrapolating two rows
/// would be inventing data.
pub fn resolving_range(percent: f64) -> (u64, u64) {
    let lo_pts: Vec<(f64, f64)> = RESOLVING_RANGES
        .iter()
        .map(|(p, lo, _)| (*p, (*lo as f64).log10()))
        .collect();
    let hi_pts: Vec<(f64, f64)> = RESOLVING_RANGES
        .iter()
        .map(|(p, _, hi)| (*p, (*hi as f64).log10()))
        .collect();
    // Both bounds fall as the gel gets denser, so the same monotone
    // interpolation applies and more agarose can never resolve larger DNA.
    let lo = Monotone::new(&lo_pts).expect("the table is sorted and distinct");
    let hi = Monotone::new(&hi_pts).expect("the table is sorted and distinct");
    (
        10f64.powf(lo.at(percent)).round() as u64,
        10f64.powf(hi.at(percent)).round() as u64,
    )
}

fn trim_percent(p: f64) -> String {
    let s = format!("{p:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gel() -> Gel {
        Gel::modelled(Conditions::default())
    }

    #[test]
    fn a_non_finite_or_zero_agarose_does_not_panic_the_model() {
        // `Gel::modelled` is infallible and public; a caller that skipped
        // validation and passed NaN/inf/0 agarose used to reach `resolving_range`
        // returning (0, 0), whose log10 is -inf, and panic in `Monotone::new`. It
        // now falls back to a usable standard gel instead of aborting.
        for bad in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            let g = Gel::modelled(Conditions {
                agarose_percent: bad,
                ..Default::default()
            });
            assert!(matches!(g.calibration(), Calibration::Model));
            let (lo, hi) = g.range();
            assert!(
                lo > 0 && hi >= lo,
                "bad agarose {bad} gave range {lo}..{hi}"
            );
        }
        // A non-finite run distance is likewise absorbed rather than propagated
        // into the curve.
        let _ = Gel::modelled(Conditions {
            run_mm: f64::NAN,
            ..Default::default()
        });
    }

    #[test]
    fn a_bigger_fragment_never_runs_further() {
        // The one property that must hold everywhere, checked densely rather
        // than at a handful of sizes.
        for percent in [0.5, 0.7, 0.8, 1.0, 1.2, 1.5, 2.0] {
            let g = Gel::modelled(Conditions {
                agarose_percent: percent,
                ..Default::default()
            });
            let (lo, hi) = g.range();
            let mut prev = f64::INFINITY;
            let mut n = 0;
            for bp in (lo..=hi).step_by(((hi - lo) / 4000).max(1) as usize) {
                if let Placement::At(mm) = g.place(bp) {
                    assert!(
                        mm <= prev + 1e-9,
                        "{percent}% gel: {bp} bp ran to {mm} after {prev}"
                    );
                    prev = mm;
                    n += 1;
                }
            }
            assert!(n > 100, "{percent}% gel placed only {n} sizes");
        }
    }

    #[test]
    fn co_migrating_fragments_are_reported_as_one_band() {
        // The failure this exists to prevent: a picture with two lines 0.4 mm
        // apart says the digest is diagnostic when it is not.
        let s = gel().run(&[2_000, 2_100, 6_000]);
        assert_eq!(s.groups.len(), 2, "{:?}", s.groups);
        let merged = s.merged();
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].sizes, vec![2_000, 2_100]);
        assert!(!s.resolves(2_000, 2_100), "these will look like one band");
        assert!(s.resolves(2_000, 6_000));
    }

    #[test]
    fn a_run_of_close_fragments_is_one_smear_not_two_resolved_ends() {
        // Single linkage on purpose. Four bands each within a band width of the
        // next are one smear; calling the outer two resolved because they are
        // far apart would be right about the arithmetic and wrong about the
        // picture.
        let g = Gel::modelled(Conditions {
            band_mm: 4.0,
            ..Default::default()
        });
        let s = g.run(&[3_000, 3_400, 3_800, 4_300]);
        assert_eq!(s.groups.len(), 1, "{:?}", s.groups);
        assert!(!s.resolves(3_000, 4_300));
    }

    #[test]
    fn a_fragment_the_gel_cannot_resolve_is_listed_and_not_drawn() {
        // Not extrapolated to a confident wrong position, and not silently
        // dropped either -- both produce a picture missing a fragment the
        // digest really makes.
        let g = Gel::modelled(Conditions {
            agarose_percent: 2.0,
            ..Default::default()
        });
        let s = g.run(&[20, 800, 40_000]);
        assert_eq!(s.too_small(), vec![20]);
        assert_eq!(s.too_large(), vec![40_000]);
        assert_eq!(s.groups.len(), 1, "only the one it can place");
        assert_eq!(s.bands.len(), 3, "but all three are still reported");
        assert!(s.caveat().contains("listed"), "{}", s.caveat());
        assert!(!s.resolves(20, 40_000), "neither one has a position");
    }

    #[test]
    fn a_measured_ladder_reproduces_its_own_bands_exactly() {
        // If a calibration does not put the ladder back where it was measured,
        // it is not a calibration.
        let measured = [
            (10_000u64, 9.0f64),
            (6_000, 15.0),
            (4_000, 21.0),
            (3_000, 26.0),
            (2_000, 34.0),
            (1_500, 41.0),
            (1_000, 51.0),
            (500, 68.0),
        ];
        let g = Gel::measured(Conditions::default(), &measured).unwrap();
        for (bp, mm) in measured {
            match g.place(bp) {
                Placement::At(x) => assert!((x - mm).abs() < 1e-9, "{bp} bp: {x} vs {mm}"),
                p => panic!("{bp} bp was not placed: {p:?}"),
            }
        }
        assert_eq!(g.range(), (500, 10_000));
        assert_eq!(*g.calibration(), Calibration::Measured);
        assert!(g.run(&[3_000]).caveat().contains("measured"));
    }

    #[test]
    fn a_modelled_calibration_admits_what_it_cannot_be_used_for() {
        let modelled = gel().run(&[3_000]);
        assert_eq!(modelled.calibration, Calibration::Model);
        let c = modelled.caveat();
        assert!(c.contains("not measured"), "{c}");
        assert!(
            c.contains("not good enough to size an unknown band"),
            "the caveat must say what it cannot be used for: {c}"
        );
    }

    #[test]
    fn more_agarose_never_resolves_larger_dna() {
        let mut prev = (u64::MAX, u64::MAX);
        for i in 0..=60 {
            let p = 0.5 + i as f64 * 0.025;
            let (lo, hi) = resolving_range(p);
            assert!(
                lo <= prev.0 && hi <= prev.1,
                "{p}%: {lo}..{hi} after {prev:?}"
            );
            assert!(lo < hi);
            prev = (lo, hi);
        }
    }

    #[test]
    fn the_tabulated_percentages_come_back_as_tabulated() {
        for (p, lo, hi) in RESOLVING_RANGES {
            let (a, b) = resolving_range(*p);
            // Round-tripped through log10, so allow the rounding.
            assert!((a as i64 - *lo as i64).abs() <= 1, "{p}%: {a} vs {lo}");
            assert!((b as i64 - *hi as i64).abs() <= 1, "{p}%: {b} vs {hi}");
        }
    }

    #[test]
    fn a_dense_gel_separates_small_fragments_where_a_loose_one_cannot() {
        // The advice a user actually wants out of this: run it at 2%.
        let loose = Gel::modelled(Conditions {
            agarose_percent: 0.7,
            ..Default::default()
        });
        let dense = Gel::modelled(Conditions {
            agarose_percent: 2.0,
            ..Default::default()
        });
        assert!(
            !loose.run(&[200, 300]).resolves(200, 300),
            "0.7% cannot even place a 200 bp fragment"
        );
        assert!(dense.run(&[200, 300]).resolves(200, 300));
    }

    #[test]
    fn every_shipped_ladder_is_sorted_distinct_and_plausible() {
        for l in LADDERS {
            assert!(l.sizes.len() >= 2, "{}", l.name);
            for w in l.sizes.windows(2) {
                assert!(w[0] < w[1], "{} is not sorted: {:?}", l.name, l.sizes);
            }
            assert!(*l.sizes.first().unwrap() >= 50);
            assert!(*l.sizes.last().unwrap() <= 50_000);
            assert_eq!(ladder(l.name), Some(*l));
        }
        assert_eq!(ladder("nope"), None);
    }

    #[test]
    fn an_empty_lane_is_an_empty_gel_and_not_an_error() {
        let s = gel().run(&[]);
        assert!(s.bands.is_empty() && s.groups.is_empty());
        assert!(!s.caveat().is_empty());
    }

    #[test]
    fn a_fragment_of_zero_bases_has_no_position() {
        assert_eq!(gel().place(0), Placement::TooSmall);
    }

    #[test]
    fn the_same_lane_runs_the_same_way_twice() {
        let a = gel().run(&[500, 1_000, 1_010, 4_000]);
        let b = gel().run(&[500, 1_000, 1_010, 4_000]);
        assert_eq!(a, b);
    }
}
