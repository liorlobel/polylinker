//! Pairing the survivors, ranking them, and driving the whole search.
//!
//! # Gate, then order — not one or the other
//!
//! Pure lexicographic ordering is wrong for the soft criteria: comparison on
//! continuous quantities never ties, so a 0.01 °C difference in ΔTm would
//! outrank a 5 kcal/mol difference in hairpin ΔG. That flips rankings on noise
//! well below the model's own accuracy, which `docs/PLAN.md` §7.2 puts at
//! roughly ±0.5-1 °C on Tm. Ordering by a key finer than the error bar is
//! theatre.
//!
//! Pure weighted sum is wrong for the hard criteria: a primer that binds in
//! three places is not "worse", it is wrong, and folding specificity into a sum
//! lets a candidate buy its way past a tier-1 hazard with a good GC content.
//!
//! So the hard criteria are a gate with no weights at all, and the soft ones
//! order the survivors. Each soft term is normalised by the width of its own
//! accepted range, so it lands in `[0, 1]` and contributes at most 1.0 at the
//! edge of acceptability — which is what makes the weights honest: they are
//! stated preferences between already-acceptable options, not pseudo-physical
//! constants.
//!
//! # Two things about the order, and both are load-bearing
//!
//! **It is total.** The key is `(quantised penalty, f.lo, f.len, r.lo, r.len)`,
//! and `(lo, len, side)` determines an oligo, so no two distinct pairs share
//! the whole key. Ties are common — normalised terms quantise — and breaking
//! them by `Vec` position is stable within one run and changes the moment the
//! enumeration bounds shift by a base.
//!
//! **The penalty is quantised before comparison**, to 1e-6. `pl_thermo::tm`
//! goes through `ln`, which comes from the platform libm and is not guaranteed
//! identical across x86_64 and aarch64 in the last ulp. IEEE `+ - * /` are;
//! transcendentals are not. Without quantisation the honest claim would be
//! "deterministic on a given platform and probably identical across platforms",
//! which is not a guarantee. With it, a last-ulp difference cannot reorder two
//! pairs and the structural key decides.
//!
//! # One check that is deliberately absent, because it could not fire
//!
//! A "facing off-target pair" rule — a forward primer and a reverse primer's
//! *second* site pointing at each other — appears in every design of this
//! feature. It is not implemented, because [`crate::specificity`] rejects any
//! candidate with any second site at all, so by the time pairing happens no
//! survivor has one. Shipping it would add a line to the report saying a check
//! was applied and passed, about a check that cannot fail. If the off-target
//! scan is ever softened from zero-tolerance, this is the rule to add back.

use crate::fold;
use crate::oligo::{evaluate, span_bases, Candidate, Side};
use crate::params::{Constraints, Mode};
use crate::report::{
    js, specificity_caveat, Geom, Pair, Reason, Report, SpecNote, Tally, NO_SPECIFICITY_NOTE,
    RT_PCR_CAVEAT,
};
use crate::specificity::{self, SeedIndex};
use crate::tail::{self, Tail};
use crate::{DesignError, Region};
use pl_core::iupac::reverse_complement;
use pl_primer::Strand;
use pl_thermo::tm;

/// One designed primer, as it will be ordered.
///
/// The footprint and the tail are **declared**, not derived: this crate chose
/// those bases. That is the difference from `pl_primer::Binding`, which derives
/// the split by annealing, and it is why `tm` here is computed from
/// [`Primer::footprint`] and never read back off a `Binding` — see the crate
/// doc for the measured reason.
#[derive(Debug, Clone, PartialEq)]
pub struct Primer {
    pub side: Side,
    /// The 3' portion that pairs with the template.
    pub footprint: Vec<u8>,
    /// The 5' portion that does not. **Never in [`Primer::tm`].**
    pub tail: Option<Tail>,
    /// 1-based plus-strand coordinates of the **footprint only**. The tail
    /// pairs with nothing on this molecule, so it has no coordinates here, and
    /// giving it any would be the same category error as giving it a Tm.
    pub start: u64,
    pub end: u64,
    /// Footprint Tm, Celsius.
    pub tm: f64,
    /// The whole oligo's Tm, which the reaction sees only from cycle 3 onward,
    /// once the tail is templated. `None` when there is no tail. Never labelled
    /// "Tm" and never used for the early cycles' annealing temperature.
    pub tm_full: Option<f64>,
    /// Footprint %GC. A 60% GC tail on a 45% GC footprint reports 52%, which
    /// describes neither the annealing behaviour nor anything else real.
    pub gc: f64,
    pub dg_three_prime: f64,
    /// The **footprint's** hairpin. This is the number `oligo::evaluate` gated
    /// on and the number the ranking uses.
    pub hairpin: fold::Structure,
    /// The **footprint's** 3'-end self-dimer, gated and ranked.
    pub self_dimer_three: fold::Structure,
    /// The whole oligo's hairpin — tail included — or `None` when there is no
    /// tail.
    ///
    /// **Reported, not gated.** A tail is 5' of the footprint so it correctly
    /// contributes nothing to Tm; a hairpin is not a Tm. It is a property of the
    /// molecule that goes to the synthesiser, and the tail is part of that
    /// molecule. Measured over 40 random 4 kb templates with `--add-5 NotI
    /// --add-3 XhoI --spacer TTGGCA`: 166 of 350 ordered oligos carry a
    /// whole-oligo hairpin at or past the −5.0 kcal/mol gate the same run
    /// applied to footprints, the worst at −9.7 with a 6 bp stem pairing spacer
    /// and site bases against footprint bases. Gating on it would change which
    /// pairs exist; printing it beside the gated number is what stops
    /// `hairpin_dg37: 0.000` sitting three fields after an oligo whose real
    /// screen value is −9.7.
    pub hairpin_full: Option<fold::Structure>,
    /// The whole oligo's 3'-end self-dimer. Reported, not gated; see
    /// [`Primer::hairpin_full`].
    pub self_dimer_three_full: Option<fold::Structure>,
}

impl Primer {
    /// The only thing that goes to a synthesiser.
    pub fn oligo(&self) -> Vec<u8> {
        match &self.tail {
            Some(t) => [&t.bases[..], &self.footprint[..]].concat(),
            None => self.footprint.clone(),
        }
    }
    pub fn name(&self, stem: &str) -> String {
        format!(
            "{stem}_{}{}",
            self.start,
            match self.side {
                Side::Fwd => "F",
                Side::Rev => "R",
            }
        )
    }
    pub(crate) fn json(&self) -> String {
        let oligo = self.oligo();
        format!(
            "{{\"oligo\": {}, \"length\": {}, \"footprint\": {}, \"footprint_length\": {}, \
             \"tail\": {}, \"tail_length\": {}, \"tail_adds\": {}, \"gc_percent\": {:.3}, \
             \"tm\": {:.3}, \"tm_basis\": \"footprint\", \"tm_whole_oligo\": {}, \
             \"three_prime_dg37\": {:.3}, \"structure_basis\": \"footprint\", \
             \"hairpin_dg37\": {:.3}, \"hairpin_dg37_whole_oligo\": {}, \
             \"self_dimer_3p_dg37\": {:.3}, \"self_dimer_3p_dg37_whole_oligo\": {}, \
             \"start\": {}, \"end\": {}, \"strand\": {}}}",
            js(&String::from_utf8_lossy(&oligo)),
            oligo.len(),
            js(&String::from_utf8_lossy(&self.footprint)),
            self.footprint.len(),
            match &self.tail {
                Some(t) => js(&String::from_utf8_lossy(&t.bases)),
                None => "null".into(),
            },
            self.tail.as_ref().map(|t| t.len()).unwrap_or(0),
            match &self.tail {
                Some(t) => js(t.enzyme.name),
                None => "null".into(),
            },
            self.gc,
            self.tm,
            match self.tm_full {
                Some(t) => format!("{t:.3}"),
                None => "null".into(),
            },
            self.dg_three_prime,
            self.hairpin.dg,
            match self.hairpin_full {
                Some(s) => format!("{:.3}", s.dg),
                None => "null".into(),
            },
            self.self_dimer_three.dg,
            match self.self_dimer_three_full {
                Some(s) => format!("{:.3}", s.dg),
                None => "null".into(),
            },
            self.start,
            self.end,
            js(self.side.as_str())
        )
    }
}

/// Enumerate, gate, pair, rank.
pub(crate) fn run(
    template: &[u8],
    circular: bool,
    region: Region,
    c: &Constraints,
) -> Result<Report, DesignError> {
    let n = template.len() as u64;
    let ni = n as i64;
    let region_bp = region.len(n);
    // Unrolled: the region occupies `[s, e]`, and `e` may run past `n` when it
    // wraps. Every later coordinate lives in this frame and is reduced modulo
    // `n` exactly once, where it is reported.
    let s = region.start as i64 - 1;
    let e = s + region_bp as i64 - 1;

    // Two footprints may ABUT. The separation gate is `f.hi >= r.lo`, so
    // `r.lo == f.hi + 1` is accepted and the shortest span two `len_min`
    // footprints can occupy is `2 * len_min`, not one more. The `+ 1` that used
    // to be here refused a region of exactly `2 * len_min` bases with the
    // sentence "the shortest product two 20 nt primers can make is 41 bases" --
    // while the same tool, given one more base of region, reported a pair
    // occupying exactly 40 of them and `pl_clone::pcr` agreed on 40. An
    // arithmetic claim the next run disproves is worse than no claim.
    let shortest_product = 2 * c.len_min;
    if c.mode == Mode::Within && region_bp < shortest_product as u64 {
        return Err(DesignError::RegionTooShort {
            bp: region_bp,
            shortest_product,
            len_min: c.len_min,
        });
    }
    if c.mode == Mode::Contain && !circular {
        // `flank` bounds the primer's OUTER end, so a `Mode::Contain` footprint
        // needs NO template outside the region: the forward may start at
        // `lo = s` and the reverse may end at `hi = e`, and `--flank 0` pinning
        // both to the selection is the seamless-cloning case `Mode::Contain`'s
        // own doc describes. This guard used to count the bases *outside* the
        // region and refuse when both sides had fewer than `len_min` of them,
        // which refused `--region 1..n` on a linear molecule -- "amplify this
        // whole fragment", the commonest design there is -- and steered the
        // user to `--mode within`, whose product is missing both ends of the
        // selection. What actually has to hold is that at least one enumerated
        // span FITS on each side, which is a different arithmetic entirely.
        if s < 0 || ni - 1 - e < 0 {
            return Err(DesignError::OutsideTemplate {
                start: region.start,
                end: region.end,
                bp: n,
            });
        }
        // Forward: the earliest 5' start `flank` allows is `max(0, s - flank)`,
        // and the shortest footprint from there must still end on the molecule.
        let f_room = ni - (s - c.flank as i64).max(0);
        // Reverse: the latest 3' end `flank` allows is `min(n - 1, e + flank)`,
        // and the shortest footprint back from there must not run past base 0.
        let r_room = (e + c.flank as i64).min(ni - 1) + 1;
        let f_short = f_room < c.len_min as i64;
        let r_short = r_room < c.len_min as i64;
        if f_short || r_short {
            let (which, available) = match (f_short, r_short) {
                (true, true) => ("at either end of the region", f_room.min(r_room)),
                (true, false) => ("at the start of the region", f_room),
                (false, true) => ("at the end of the region", r_room),
                // Guarded by the `if` above; a branch that cannot be reached is
                // not a branch worth inventing a message for.
                (false, false) => unreachable!("one of the two sides is short"),
            };
            return Err(DesignError::NoFlank {
                which,
                available: available.max(0) as u64,
                needed: c.len_min,
                bp: n,
            });
        }
    }

    let index = if c.specificity {
        SeedIndex::build(template, c.off_seed, circular)
    } else {
        None
    };
    let sp = specificity::params(c.off_seed, c.tm_method);
    let mut tally = Tally::new(c);
    let mut enumerated = 0usize;

    // -- forward candidates ------------------------------------------------
    //
    // `flank` is how far outside the region the primer's OUTER (5') end may
    // sit, so a forward footprint always begins at or before the region's first
    // base and the product always contains the whole region. `--flank 0` then
    // pins the outer ends exactly to the selection, which is the seamless
    // cloning case, and larger values simply allow the primer to back off
    // upstream. Defining `flank` on the 3' end instead would make `--flank 0`
    // impossible to satisfy.
    let mut forward: Vec<Candidate> = Vec::new();
    let (f_from, f_to) = match c.mode {
        Mode::Contain => (s - c.flank as i64, s),
        Mode::Within => (s, e),
    };
    for lo in f_from..=f_to {
        for len in c.len_min..=c.len_max {
            let hi = lo + len as i64 - 1;
            if c.mode == Mode::Within && hi > e {
                continue;
            }
            enumerated += 1;
            match evaluate(template, circular, Side::Fwd, lo, hi, c) {
                Err(r) => tally.bump(r),
                Ok(cand) => {
                    if !accept_specificity(&cand, template, circular, n, index.as_ref(), &sp, c) {
                        tally.bump(Reason::OffTarget);
                        continue;
                    }
                    forward.push(cand);
                }
            }
        }
    }

    // -- reverse candidates ------------------------------------------------
    let mut reverse: Vec<Candidate> = Vec::new();
    let (r_from, r_to) = match c.mode {
        Mode::Contain => (e, e + c.flank as i64),
        Mode::Within => (s, e),
    };
    for hi in r_from..=r_to {
        for len in c.len_min..=c.len_max {
            let lo = hi - len as i64 + 1;
            if c.mode == Mode::Within && lo < s {
                continue;
            }
            enumerated += 1;
            match evaluate(template, circular, Side::Rev, lo, hi, c) {
                Err(r) => tally.bump(r),
                Ok(cand) => {
                    if !accept_specificity(&cand, template, circular, n, index.as_ref(), &sp, c) {
                        tally.bump(Reason::OffTarget);
                        continue;
                    }
                    reverse.push(cand);
                }
            }
        }
    }

    // The geometry a remedy has to know before it names `--flank`: which side
    // the molecule's end has clipped, and whether the region could hold a pair
    // if the user were sent to `--mode within`.
    let geom = |fwd: usize, rev: usize| Geom {
        before: if circular {
            None
        } else {
            Some(s.max(0) as u64)
        },
        after: if circular {
            None
        } else {
            Some((ni - 1 - e).max(0) as u64)
        },
        region_bp,
        forward: fwd > 0,
        reverse: rev > 0,
    };

    if forward.is_empty() || reverse.is_empty() {
        let constraints = tally_advice(
            &tally,
            c,
            forward.len(),
            reverse.len(),
            enumerated,
            geom(forward.len(), reverse.len()),
        );
        return Err(DesignError::NoCandidate {
            enumerated,
            tally: Box::new(tally),
            constraints,
        });
    }

    // The search bound. Quadratic pairing over thousands of survivors is what
    // made a `Within` design over a 1 kb gene take 104 seconds, and a design
    // tool that has to be waited for is a different product. Cutting to the
    // best `max_per_side` by the PER-OLIGO half of the same penalty keeps the
    // cut deterministic and stops it being a second, hidden set of criteria.
    let (survivors_forward, survivors_reverse) = (forward.len(), reverse.len());
    let bound = forward.len() > c.max_per_side || reverse.len() > c.max_per_side;
    cap(&mut forward, c);
    // Restore the enumeration invariant the pairing loop's binary search needs:
    // forward ascending by `lo`. The reverse side is cut further down, once the
    // product window is known, and sorted there.
    forward.sort_by_key(|k| (k.lo, k.len()));

    // -- pairing -----------------------------------------------------------
    //
    // Reverse candidates come out of the loop above with `hi` non-decreasing,
    // so the product-length window is two binary searches rather than a scan.
    // That is what keeps `Within` on a long region tractable: without it the
    // pair count is the product of the two survivor counts.
    let ftail = c.tail_five.as_ref().map(Tail::build).transpose()?;
    let rtail = c.tail_three.as_ref().map(Tail::build).transpose()?;

    // The product window gates the AMPLICON, not the template span, and the
    // two are different the moment there is a tail. Until a reviewer measured
    // it, `--product 140..150 --add-5 EcoRI --add-3 BamHI --spacer TTGGCA`
    // printed "product 140-150 bp" as the constraint and then five amplicons of
    // 164-173 bp: the span obeyed the window and the molecule that runs on the
    // gel did not. Under `--rt`, whose whole point is a 70-150 bp qPCR
    // amplicon, a pair capped at 150 came out at 174. Amplicon length is the
    // one number MIQE requires to be reported, so the gate is on the number
    // that is reported.
    let tail_bp = (ftail.as_ref().map(|t| t.len()).unwrap_or(0)
        + rtail.as_ref().map(|t| t.len()).unwrap_or(0)) as u64;
    if c.product_max <= tail_bp {
        return Err(DesignError::TailsExceedProduct {
            tail_bp,
            product_max: c.product_max,
        });
    }
    // Saturating rather than clamped at zero: a span of 0 is impossible anyway
    // (two abutting footprints already occupy `2 * len_min`), and `PairOverlap`
    // is the gate that says so in its own words.
    let span_min = c.product_min.saturating_sub(tail_bp).max(1);
    let span_max = c.product_max - tail_bp;

    // The reverse cut is conditioned on the forwards that survived theirs.
    //
    // Cutting the two sides independently is positionally blind, and in
    // `Mode::Within` over a long region that is fatal rather than merely lossy:
    // `cap` sorts on the per-oligo penalty with the coordinate only as a
    // tie-break, so the retained candidates are spread uniformly over the
    // region and the expected number of pairs is
    // `max_per_side^2 * window / region_bp`. Measured on a 2 Mb region under
    // `--rt` that is 1.6, and on a fifth of random templates it is zero: the
    // tool refused a region containing hundreds of thousands of valid qPCR
    // pairs, reported "0 pairs were rejected", and blamed Tm -- which 8.9
    // million oligos had just passed. Keeping only reverses some retained
    // forward can actually reach makes `built >= 1` whenever any in-window pair
    // exists at all, and costs one binary search over at most `max_per_side`
    // starts per candidate.
    retain_pairable(&mut reverse, &forward, span_min, span_max);
    cap(&mut reverse, c);
    reverse.sort_by_key(|k| (k.hi, k.len()));

    let mut built = 0usize;
    let mut scored: Vec<(i64, i64, usize, i64, usize, Pair)> = Vec::new();
    // The first few site clashes, kept so the refusal can name the enzyme and
    // the coordinate instead of a count. Deduplicated and capped; see
    // `note_clash`.
    let mut clashes: Vec<tail::SiteClash> = Vec::new();

    for f in &forward {
        let want_lo = f.lo + span_min as i64 - 1;
        let want_hi = f.lo + span_max as i64 - 1;
        let from = reverse.partition_point(|r| r.hi < want_lo);
        for r in &reverse[from..] {
            if r.hi > want_hi {
                break;
            }
            built += 1;
            if f.hi >= r.lo {
                tally.bump(Reason::PairOverlap);
                continue;
            }
            let dtm = (f.tm - r.tm).abs();
            if dtm > c.tm_diff_max {
                tally.bump(Reason::DeltaTm);
                continue;
            }
            let (cross_any, cross_three) = fold::dimer(&f.bases, &r.bases, &Constraints::DG_TABLE);
            if cross_three.dg <= c.dg_dimer_three_prime {
                tally.bump(Reason::CrossDimer);
                continue;
            }

            // The product is the two tails around the template between the
            // footprints' outer ends -- both footprints ARE template
            // substrings, so this is exact and `pl_clone::pcr` is a genuine
            // independent check of it rather than the same arithmetic twice.
            // `span_bases` refuses two things, and only one of them is
            // reachable here: the loop bounds above already guarantee the span
            // is inside `--product`, and both candidates passed `evaluate`, so
            // neither runs off a linear end. What is left is the one-turn cap
            // on a circle -- `hi - lo + 1 > n` -- so this counter means "the
            // amplicon would be longer than the molecule", and nothing else.
            // It used to be `Reason::ProductLength`, labelled "product outside
            // {min}-{max} bp" and remedied with "Widen --product", on a 2,686 bp
            // circle whose reported products were 2,600 bp: a statement that is
            // false for every input that can reach it, beside the one remedy
            // that makes it worse.
            let Some(middle) = span_bases(template, circular, f.lo, r.hi) else {
                tally.bump(Reason::SpanExceedsMolecule);
                continue;
            };
            let mut product: Vec<u8> = Vec::with_capacity(middle.len() + 32);
            if let Some(t) = &ftail {
                product.extend_from_slice(&t.bases);
            }
            product.extend_from_slice(&middle);
            if let Some(t) = &rtail {
                product.extend_from_slice(&reverse_complement(&t.bases));
            }

            let mut warnings: Vec<String> = Vec::new();
            let mut refused = false;
            for (which, t) in [("forward", &ftail), ("reverse", &rtail)] {
                let Some(t) = t else { continue };
                let engineered = engineered_positions(&ftail, &rtail, product.len());
                let internal = tail::internal_sites(&product, t.enzyme, &engineered);
                if !internal.is_empty() {
                    tally.bump(Reason::InternalSite);
                    // The enzyme, the coordinate and the strand were already
                    // computed here and were thrown away, leaving a refusal
                    // that named none of its numbers -- against this crate's
                    // own rule for `DesignError`. Kept now, translated out of
                    // product coordinates into the user's own frame, and
                    // capped so a 40,000-pair run cannot fill a screen.
                    note_clash(
                        &mut clashes,
                        &internal,
                        &ftail,
                        &rtail,
                        f.lo,
                        n,
                        product.len(),
                    );
                    refused = true;
                    break;
                }
                if t.spacer.is_empty() {
                    warnings.push(format!(
                        "the {which} tail adds {} with no spacer: {}",
                        t.enzyme.name,
                        tail::NO_SPACER_WARNING
                    ));
                }
                warnings.push(format!("the {which} tail: {}", t.frame_note(which)));
            }
            if refused {
                continue;
            }
            if let (Some(a), Some(b)) = (&ftail, &rtail) {
                // Keyed on the SITE, not the enzyme name. A primer-dimer is a
                // property of the bases, and two isoschizomers write the same
                // bases: SmaI and XmaI both add CCCGGG, so `--add-5 SmaI
                // --add-3 XmaI` shipped two identical self-complementary tails
                // with no warning at all -- and the remedy this very warning
                // gives ("use different enzymes at the two ends") is what
                // steers a user into that case.
                //
                // `pl_enzymes::methylation`'s doc says the opposite -- "key on
                // the enzyme, never on the recognition sequence" -- and is
                // right, because methylation sensitivity belongs to the
                // protein. Two different questions about the same table.
                if a.enzyme.site == b.enzyme.site
                    && pl_core::iupac::is_palindrome_masks(a.enzyme.site.as_bytes())
                {
                    // Every Type IIP site is a palindrome, so the same site at
                    // both ends makes the two tails exactly complementary to
                    // each other: a designed primer-dimer sitting at the 5'
                    // ends. Different sites at the two ends is what directional
                    // cloning wants anyway.
                    let how = if a.enzyme.name == b.enzyme.name {
                        format!(
                            "both tails add {}, whose site {} is a palindrome",
                            a.enzyme.name, a.enzyme.site
                        )
                    } else {
                        format!(
                            "{} and {} are isoschizomers, so both tails add the same \
                             palindromic site {}",
                            a.enzyme.name, b.enzyme.name, a.enzyme.site
                        )
                    };
                    warnings.push(format!(
                        "{how}, so the two 5' ends are exactly complementary to each other \
                         -- a designed primer-dimer. Use two enzymes whose SITES differ; \
                         directional cloning wants that anyway."
                    ));
                }
            }

            let pair = build_pair(
                template,
                circular,
                n,
                f,
                r,
                &ftail,
                &rtail,
                &product,
                dtm,
                cross_any,
                cross_three,
                warnings,
                c,
            );
            scored.push((
                (pair.penalty * 1e6).round() as i64,
                f.lo,
                f.len(),
                r.lo,
                r.len(),
                pair,
            ));
        }
    }

    if scored.is_empty() {
        let mut constraints = String::new();
        // Said first, for the reason the `--flank 0` clause is said first: it
        // explains the size of the search rather than any one threshold. It
        // used to reach only `Report::warnings`, which exists only on the Ok
        // path -- so on the one run where the cut decided the answer, the user
        // was never told a cut had happened at all.
        if bound {
            constraints.push_str(&bound_note(c));
            constraints.push(' ');
        }
        if built == 0 {
            // Every combination the product window excludes is skipped by the
            // `partition_point`/`break` above, before `built += 1` and before
            // any `tally.bump`, so a window no pair of survivors can satisfy
            // leaves the tally holding candidate-stage counts only. `advice`
            // then falls through to the largest of those and answers "Tm is the
            // binding constraint. Widen the LENGTH range first", which is
            // unfollowable: the enumeration bounds on `lo` and `hi` are fixed
            // by --mode, --flank and --region, so no threshold moves the span.
            // The one number that explains the refusal is this one, and it
            // appeared nowhere.
            let lo_span = (reverse[0].hi - forward[forward.len() - 1].lo + 1).max(0) as u64;
            let hi_span = (reverse[reverse.len() - 1].hi - forward[0].lo + 1).max(0) as u64;
            constraints.push_str(&format!(
                "No pair of survivors has an amplicon inside --product {}..{} bp, so the \
                 window refused every combination before any per-pair threshold saw one. \
                 The survivors reach {} to {} bp of amplicon. Move --product, --region or \
                 --flank; widening --len or --tm only adds candidates between the same \
                 enumeration bounds.",
                c.product_min,
                c.product_max,
                lo_span + tail_bp,
                hi_span + tail_bp
            ));
        } else {
            constraints.push_str(&tally.advice(c, enumerated, built, &clashes, geom(1, 1)));
        }
        return Err(DesignError::NoPair {
            // The PRE-cap counts. Read off `forward`/`reverse` after `cap`
            // truncated them in place, this printed a flat 400 -- `2 *
            // max_per_side` -- on a run where 8.9 million oligos had passed,
            // one line above an attrition table from which the reader subtracts
            // and gets a different number.
            survivors: survivors_forward + survivors_reverse,
            enumerated,
            built,
            tally: Box::new(tally),
            clashes,
            constraints,
        });
    }

    // The key is total by construction -- `(lo, len, side)` determines an
    // oligo, so no two distinct pairs share all five components -- which is
    // what makes an unstable sort correct here, and is also what makes the
    // choice a test rather than a comment. `sort_unstable_by_key` gives
    // pdqsort, whose output for equal keys depends on the whole array; so a
    // future truncation of this key to the penalty alone would make the order
    // of two tied pairs depend on how many other candidates were enumerated,
    // and `determinism.rs` widens `flank` and checks exactly that.
    scored.sort_unstable_by_key(|a| (a.0, a.1, a.2, a.3, a.4));

    // Diversity: adjacent candidates differing by one base score almost
    // identically, so a naive top-N is N views of one primer. A pair is skipped
    // only when BOTH its 3' ends are near an already-accepted pair's -- so a
    // genuinely different reverse primer on the same forward is still offered.
    let mut chosen: Vec<Pair> = Vec::new();
    let mut taken: Vec<(i64, i64)> = Vec::new();
    for (_, _, _, _, _, p) in scored.iter() {
        let key = (p.forward_three_prime, p.reverse_three_prime);
        if taken.iter().any(|(a, b)| {
            (key.0 - a).abs() < c.min_separation as i64
                && (key.1 - b).abs() < c.min_separation as i64
        }) {
            continue;
        }
        taken.push(key);
        chosen.push(p.clone());
        if chosen.len() >= c.max_pairs {
            break;
        }
    }

    // The independent check, run only on the pairs that will be shown.
    for p in &mut chosen {
        p.pcr_check = pcr_length(template, circular, p);
    }

    let mut warnings = Vec::new();
    if c.rt_pcr {
        warnings.push(RT_PCR_CAVEAT.to_string());
    }
    if c.specificity {
        warnings.push(specificity_caveat("this template", n, circular));
    } else {
        warnings.push(NO_SPECIFICITY_NOTE.to_string());
    }
    // The structure limit reaches the user unconditionally, exactly as the
    // specificity limit does. It used to live only in `pl methods design`, a
    // separate verb nobody runs by accident, while every report printed
    // hairpin and dimer free energies with nothing beside them.
    warnings.push(fold::SCREEN_NOTE.to_string());
    // A target the window cannot contain is a criterion the user asked for and
    // did not get. It used to be dropped in silence -- no error, no warning, and
    // no surface anywhere in the report that echoes the value they typed --
    // while the weights line went on printing `product 1.0` beside a term that
    // was structurally zero for every candidate.
    if let Some(t) = c.product_target {
        if !(c.product_min..=c.product_max).contains(&t) {
            warnings.push(format!(
                "the requested product size {t} bp is outside the {}-{} bp product window, \
                 so no amplicon can reach it. The size term still ranks by log distance to \
                 {t}, which means it ranks every candidate at the same end of the window; \
                 put the target inside --product if that is not what you meant.",
                c.product_min, c.product_max
            ));
        }
    }
    // An ambiguity code inside the amplicon but outside both footprints is
    // legal here -- only the footprints have to be unambiguous -- but
    // `Composition::gc_percent` is over unambiguous bases, so the denominator
    // quietly stops being the product length. Refuse or annotate, never
    // silently change a denominator.
    {
        let ambiguous: u64 = chosen
            .iter()
            .map(|p| p.product_ambiguous)
            .max()
            .unwrap_or(0);
        if ambiguous > 0 {
            warnings.push(format!(
                "an amplicon contains {ambiguous} ambiguity code{} outside both footprints. \
                 The %GC printed for it is over the UNAMBIGUOUS bases only, so its \
                 denominator is not the product length. The footprints themselves are \
                 unambiguous -- that is gated -- and the tally counts the candidates that \
                 were removed for spanning one.",
                if ambiguous == 1 { "" } else { "s" }
            ));
        }
    }
    // A partial refusal is the common case and it used to be invisible: with a
    // site inside part of the region the tool still returns pairs, and the only
    // disclosure was a count in the tally. Which enzyme, and where, is what
    // lets a user decide whether to move the region or change the enzyme.
    if !clashes.is_empty() {
        let mut s = String::from(
            "some products were refused because the site being added already occurs \
             inside them. ",
        );
        for cl in &clashes {
            s.push_str(&cl.render());
            s.push_str(". ");
        }
        s.push_str("The pairs below avoid it; a wider --flank or --region may not.");
        warnings.push(s);
    }
    if bound {
        warnings.push(bound_note(c));
    }
    if chosen
        .iter()
        .any(|p| p.forward.tail.is_some() || p.reverse.tail.is_some())
    {
        warnings.push(
            "lowercase is a 5' TAIL. It pairs with nothing, it has no coordinates on this \
             template, and it is not in the Tm. pl design has no in-frame mode and will \
             never pad a tail to preserve a reading frame."
                .to_string(),
        );
        // The Tm split was labelled and the structure split was not, so a
        // report could print `hairpin_dg37: 0.000` beside an oligo whose own
        // screen value is -9.7 -- in the same run that discarded 56 candidates
        // for a hairpin at or below -5.0. The gate cannot move without changing
        // which pairs exist, so the whole-oligo numbers are printed beside the
        // gated ones and this says which is which.
        warnings.push(
            "the hairpin, self-dimer and cross-dimer dG37 values that were GATED and RANKED \
             are the FOOTPRINTS', because that is what the per-oligo gate had to work with. \
             A tail is 5' of the footprint so it rightly stays out of the Tm, but it is part \
             of the molecule that is ordered and it folds: the whole-oligo values are printed \
             beside them, on the 'whole oligo' line and in the JSON, and they are not gated. \
             Read them before ordering."
                .to_string(),
        );
    }

    Ok(Report {
        bp: n,
        circular,
        region,
        region_bp,
        mode: c.mode,
        flank: c.flank,
        method: c.tm_method.describe(),
        constraints: c.describe(),
        dg_note: c.describe_dg(),
        weights: c.weights.describe(),
        enumerated,
        survivors_forward,
        survivors_reverse,
        pairs_built: built,
        tally,
        pairs: chosen,
        specificity: SpecNote {
            ran: c.specificity,
            seed: c.off_seed,
            used_index: index.is_some(),
        },
        warnings,
    })
}

/// Keep the best `max_per_side` by the per-oligo terms, ties by coordinate.
///
/// The same normalised terms and the same weights the pair score uses, minus
/// everything that needs a partner — so this is a projection of the objective,
/// not a second one. **The term set here must be a subset of `score`'s**, and
/// `a_search_bound_cannot_rank_on_a_criterion_the_ranking_ignores` pins it,
/// because it stopped being one: this weighted `self_dimer_any` against
/// `DG_DIMER_ANY`, a quantity `score` has no term for and the report never
/// prints, so the cut ordered candidates on a criterion the reported ranking
/// never applied — the "second, hidden set of criteria" the comment at the call
/// site says it avoids, while the warning told the user the opposite.
///
/// The structure term is therefore the two per-oligo halves of `score`'s
/// six-way structure term, over the same denominators, divided by their own
/// count.
fn cap(v: &mut Vec<Candidate>, c: &Constraints) {
    if v.len() <= c.max_per_side {
        return;
    }
    let w = &c.weights;
    let half = ((c.tm_max - c.tm_min) / 2.0).max(1e-9);
    let len_span = (c.len_max - c.len_min).max(1) as f64;
    let unit = |dg: f64, limit: f64| (dg / limit).clamp(0.0, 1.0);
    let key = |k: &Candidate| -> i64 {
        let t_tm = ((k.tm - c.tm_opt).abs() / half).clamp(0.0, 1.0);
        let t_dg = (unit(k.hairpin.dg, c.dg_hairpin)
            + unit(k.self_dimer_three.dg, c.dg_dimer_three_prime))
            / 2.0;
        let t_end = unit(k.dg_three_prime, c.dg_three_prime);
        let t_clamp = f64::from(!(c.gc_clamp_min..=c.gc_clamp_max).contains(&k.clamp));
        let t_len = ((k.len() as f64 - c.len_opt as f64).abs() / len_span).clamp(0.0, 1.0);
        let outside = if k.gc < c.gc_min {
            c.gc_min - k.gc
        } else if k.gc > c.gc_max {
            k.gc - c.gc_max
        } else {
            0.0
        };
        let t_gc = (outside / Constraints::GC_NORM).clamp(0.0, 1.0);
        let p = w.tm * t_tm
            + w.structure * t_dg
            + w.three_prime * t_end
            + w.gc_clamp * t_clamp
            + w.length * t_len
            + w.gc * t_gc;
        (p * 1e6).round() as i64
    };
    // Total, so the cut cannot depend on the sort's arrangement of equal keys.
    v.sort_unstable_by_key(|k| (key(k), k.lo, k.len()));
    v.truncate(c.max_per_side);
}

/// Drop reverse candidates that no retained forward can pair with.
///
/// `forward` is sorted ascending by `lo`. A reverse ending at `hi` pairs with a
/// forward starting at `lo` exactly when `lo + span_min - 1 <= hi <= lo +
/// span_max - 1`, i.e. `hi - span_max + 1 <= lo <= hi - span_min + 1`, so one
/// `partition_point` per candidate answers it.
///
/// This runs **before** `cap` on the reverse side, and that order is the whole
/// point: it is what makes the surviving 200 cluster around the retained
/// forwards instead of being spread over the region. See the call site for the
/// measurement.
fn retain_pairable(
    reverse: &mut Vec<Candidate>,
    forward: &[Candidate],
    span_min: u64,
    span_max: u64,
) {
    if forward.is_empty() {
        return;
    }
    let los: Vec<i64> = forward.iter().map(|f| f.lo).collect();
    let keep = |r: &Candidate| {
        let lo_from = r.hi - span_max as i64 + 1;
        let lo_to = r.hi - span_min as i64 + 1;
        let from = los.partition_point(|l| *l < lo_from);
        from < los.len() && los[from] <= lo_to
    };
    // Never empty the side here. If nothing pairs, the search genuinely has no
    // amplicon inside `--product`, and that is a refusal the pairing loop has to
    // be able to describe with `built == 0`; leaving `reverse` empty instead
    // would report "no reverse candidate survived", which is a different claim
    // and a false one -- they survived, the window is what has no room.
    if reverse.iter().any(keep) {
        reverse.retain(keep);
    }
}

/// The sentence that discloses the search bound.
///
/// One function rather than one string per path: it reaches the success
/// report's warnings AND the `NoPair` refusal, and those two used to say
/// different things -- the refusal said nothing at all, on exactly the run where
/// the cut was what emptied the search.
fn bound_note(c: &Constraints) -> String {
    format!(
        "more than {} candidates survived on one side, so only the best {} per side \
         were paired. That is a bound on the search, not a criterion: the cut is by \
         the per-oligo terms of the same score the pairs are ranked by -- Tm, hairpin, \
         3'-end self-dimer, 3'-end stability, GC clamp, length, GC -- and by nothing \
         else. Narrow the region or --flank if you want the whole space considered.",
        c.max_per_side, c.max_per_side
    )
}

fn tally_advice(
    t: &Tally,
    c: &Constraints,
    fwd: usize,
    rev: usize,
    enumerated: usize,
    geom: Geom,
) -> String {
    let plural = |n: usize| if n == 1 { "candidate" } else { "candidates" };
    let side = if fwd == 0 && rev == 0 {
        String::new()
    } else if fwd == 0 {
        format!(
            "{rev} reverse {} survived and no forward one did. ",
            plural(rev)
        )
    } else {
        format!(
            "{fwd} forward {} survived and no reverse one did. ",
            plural(fwd)
        )
    };
    format!("{side}{}", t.advice(c, enumerated, 0, &[], geom))
}

/// The most distinct site clashes any one refusal will describe.
///
/// Three, because the useful information is *which enzyme and where*, and a
/// fourth coordinate does not add any: with 40,000 pairs built the same
/// template site reappears in tens of thousands of products, and a list is not
/// a diagnosis.
const MAX_CLASHES: usize = 3;

/// Record an unintended site, translated into the user's coordinates.
///
/// Deduplicated by the whole clash, so the same template site found under
/// thousands of different products is reported once. Deterministic: the
/// pairing loop's order is, and this only ever appends.
#[allow(clippy::too_many_arguments)]
fn note_clash(
    out: &mut Vec<tail::SiteClash>,
    found: &[tail::InternalSite],
    ftail: &Option<Tail>,
    rtail: &Option<Tail>,
    span_start: i64,
    n: u64,
    product_len: usize,
) {
    if out.len() >= MAX_CLASHES {
        return;
    }
    let ft = ftail.as_ref().map(Tail::len).unwrap_or(0);
    let rt = rtail.as_ref().map(Tail::len).unwrap_or(0);
    for s in found {
        let c = tail::SiteClash::locate(s, ft, rt, product_len, span_start, n);
        if !out.contains(&c) {
            out.push(c);
            if out.len() >= MAX_CLASHES {
                return;
            }
        }
    }
}

/// Where the two engineered sites sit in the product, 1-based.
///
/// Known exactly because the tail records its own layout; nothing is inferred
/// from the sequence, which is what lets "the site you asked for" be told from
/// "a site that will ruin your insert".
fn engineered_positions(
    ftail: &Option<Tail>,
    rtail: &Option<Tail>,
    product_len: usize,
) -> Vec<u64> {
    let mut v = Vec::new();
    if let Some(t) = ftail {
        v.push(t.site_offset() as u64 + 1);
    }
    if let Some(t) = rtail {
        let site_len = t.enzyme.site.len();
        v.push((product_len - t.spacer.len() - site_len) as u64 + 1);
    }
    v
}

#[allow(clippy::too_many_arguments)]
fn accept_specificity(
    cand: &Candidate,
    template: &[u8],
    circular: bool,
    n: u64,
    index: Option<&SeedIndex>,
    sp: &pl_primer::Params,
    c: &Constraints,
) -> bool {
    if !c.specificity {
        return true;
    }
    let strand = match cand.side {
        Side::Fwd => Strand::Forward,
        Side::Rev => Strand::Reverse,
    };
    let intended = (cand.start(n), cand.end(n), strand);
    let scan = specificity::scan(&cand.bases, template, circular, intended, index, sp);
    // Consumed, so `Scan::anchored` is a check and not a claim. It can only be
    // false if enumeration and the scan disagree about which site the candidate
    // came from -- a bug in this crate, never a property of the molecule -- and
    // that disagreement is how a design ends up describing a different product
    // from the one it drew. Cheap enough to leave on in debug builds; in
    // release the answer does not depend on it.
    debug_assert!(
        scan.anchored || scan.unscannable,
        "the intended site {intended:?} was not among the bindings for a footprint taken \
         from the template"
    );
    scan.is_unique()
}

#[allow(clippy::too_many_arguments)]
fn build_pair(
    template: &[u8],
    circular: bool,
    n: u64,
    f: &Candidate,
    r: &Candidate,
    ftail: &Option<Tail>,
    rtail: &Option<Tail>,
    product: &[u8],
    dtm: f64,
    cross_any: fold::Structure,
    cross_three: fold::Structure,
    warnings: Vec<String>,
    c: &Constraints,
) -> Pair {
    let _ = (template, circular);
    let forward = to_primer(f, ftail.clone(), n, c);
    let reverse = to_primer(r, rtail.clone(), n, c);
    let product_bp = (r.hi - f.lo + 1) as u64
        + ftail.as_ref().map(|t| t.len()).unwrap_or(0) as u64
        + rtail.as_ref().map(|t| t.len()).unwrap_or(0) as u64;
    let comp = pl_core::iupac::Composition::of(product);
    let (penalty, terms) = score(f, r, dtm, cross_any, cross_three, product_bp, c);
    // The gated cross-dimer is the two FOOTPRINTS'. The two oligos that are
    // ordered carry the tails, and two tails that pair are a designed
    // primer-dimer the `a.enzyme.site == b.enzyme.site` guard below only sees
    // when both tails add the same palindromic site. Reported, not gated.
    let cross_dimer_three_full = if ftail.is_some() || rtail.is_some() {
        Some(fold::dimer(&forward.oligo(), &reverse.oligo(), &Constraints::DG_TABLE).1)
    } else {
        None
    };

    Pair {
        forward_three_prime: f.three_prime(),
        reverse_three_prime: r.three_prime(),
        forward,
        reverse,
        penalty,
        terms,
        product_start: (f.lo.rem_euclid(n as i64)) as u64 + 1,
        product_end: (r.hi.rem_euclid(n as i64)) as u64 + 1,
        product_bp,
        product_gc: comp.gc_percent().unwrap_or(0.0),
        // Not a judgement, a denominator. `gc_percent` is over unambiguous
        // bases, so this is how far the %GC above is from being a fraction of
        // `product_bp`.
        product_ambiguous: comp.other,
        delta_tm: dtm,
        cross_dimer_any: cross_any,
        cross_dimer_three: cross_three,
        cross_dimer_three_full,
        pcr_check: Err("not run".into()),
        warnings,
    }
}

fn to_primer(cand: &Candidate, tail: Option<Tail>, n: u64, c: &Constraints) -> Primer {
    let whole = tail
        .as_ref()
        .map(|t| [&t.bases[..], &cand.bases[..]].concat());
    // The whole-oligo Tm is computed for reporting and NEVER used to balance
    // the pair or to set the early cycles' annealing temperature.
    let tm_full = whole
        .as_ref()
        .and_then(|o| tm(o, &c.tm_method).ok().map(|x| x.tm));
    // The whole-oligo structure numbers, likewise reported and not gated. The
    // gate is on the footprint because that is what `oligo::evaluate` had; the
    // oligo that is ordered is this one, and it folds against itself tail
    // included.
    let hairpin_full = whole
        .as_ref()
        .map(|o| fold::hairpin(o, &Constraints::DG_TABLE));
    let self_dimer_three_full = whole
        .as_ref()
        .map(|o| fold::dimer(o, o, &Constraints::DG_TABLE).1);
    Primer {
        side: cand.side,
        footprint: cand.bases.clone(),
        tail,
        start: cand.start(n),
        end: cand.end(n),
        tm: cand.tm,
        tm_full,
        gc: cand.gc,
        dg_three_prime: cand.dg_three_prime,
        hairpin: cand.hairpin,
        self_dimer_three: cand.self_dimer_three,
        hairpin_full,
        self_dimer_three_full,
    }
}

/// The weighted sum, with its terms kept so the total can be decomposed.
///
/// Written as one straight-line expression with the terms in a fixed order,
/// not a `.map().sum()` over a collection whose length varies with the
/// constraints: float addition is not associative, and the order has to be a
/// property of the code rather than of the data.
fn score(
    f: &Candidate,
    r: &Candidate,
    dtm: f64,
    cross_any: fold::Structure,
    cross_three: fold::Structure,
    product_bp: u64,
    c: &Constraints,
) -> (f64, Vec<(&'static str, f64)>) {
    let w = &c.weights;
    let half = ((c.tm_max - c.tm_min) / 2.0).max(1e-9);

    let t_dtm = (dtm / c.tm_diff_max.max(1e-9)).clamp(0.0, 1.0);
    let t_tm = ((((f.tm - c.tm_opt).abs() + (r.tm - c.tm_opt).abs()) / 2.0) / half).clamp(0.0, 1.0);

    let unit = |dg: f64, limit: f64| (dg / limit).clamp(0.0, 1.0);
    let t_dg = (unit(f.hairpin.dg, c.dg_hairpin)
        + unit(r.hairpin.dg, c.dg_hairpin)
        + unit(f.self_dimer_three.dg, c.dg_dimer_three_prime)
        + unit(r.self_dimer_three.dg, c.dg_dimer_three_prime)
        + unit(cross_three.dg, c.dg_dimer_three_prime)
        + unit(cross_any.dg, c.dg_dimer_any))
        / 6.0;

    let t_end =
        (unit(f.dg_three_prime, c.dg_three_prime) + unit(r.dg_three_prime, c.dg_three_prime)) / 2.0;

    // Log distance, because 400 vs 500 bp matters far less than 100 vs 200.
    //
    // Normalised by the LARGER of the two log distances to the window edges.
    // Dividing by `ln(product_max / target)` alone made the term degenerate as
    // the target approached the ceiling -- at `--product 100..500 --product-opt
    // 499` the normaliser is `ln(500/499)` = 0.002, so a 428 bp and a 286 bp
    // amplicon both scored the full 1.0, a step function rather than the log
    // scale the flag advertises -- and the `product_max > target` guard sent
    // `target == product_max` to the `_ => 0.0` arm, switching a criterion the
    // user asked for off entirely while the report still printed `product 1.0`
    // among the weights.
    let t_amp = match c.product_target {
        Some(target) if target > 0 => {
            let up = (c.product_max as f64 / target as f64).ln().abs();
            let down = (target as f64 / c.product_min.max(1) as f64).ln().abs();
            let span = up.max(down);
            if span < 1e-9 {
                // A window with no width either side of the target: every
                // admissible amplicon is the target, so there is nothing to
                // rank and 0.0 is the honest term rather than a divide-by-zero.
                0.0
            } else {
                ((product_bp as f64 / target as f64).ln().abs() / span).clamp(0.0, 1.0)
            }
        }
        _ => 0.0,
    };

    let in_band = |k: usize| (c.gc_clamp_min..=c.gc_clamp_max).contains(&k);
    let t_clamp = (f64::from(!in_band(f.clamp)) + f64::from(!in_band(r.clamp))) / 2.0;

    let len_span = (c.len_max - c.len_min).max(1) as f64;
    let t_len = (((f.len() as f64 - c.len_opt as f64).abs()
        + (r.len() as f64 - c.len_opt as f64).abs())
        / 2.0
        / len_span)
        .clamp(0.0, 1.0);

    let outside = |gc: f64| {
        if gc < c.gc_min {
            c.gc_min - gc
        } else if gc > c.gc_max {
            gc - c.gc_max
        } else {
            0.0
        }
    };
    let t_gc = (((outside(f.gc) + outside(r.gc)) / 2.0) / Constraints::GC_NORM).clamp(0.0, 1.0);

    let penalty = w.delta_tm * t_dtm
        + w.tm * t_tm
        + w.structure * t_dg
        + w.three_prime * t_end
        + w.product * t_amp
        + w.gc_clamp * t_clamp
        + w.length * t_len
        + w.gc * t_gc;

    (
        penalty,
        vec![
            ("delta_tm", w.delta_tm * t_dtm),
            ("tm", w.tm * t_tm),
            ("structure", w.structure * t_dg),
            ("three_prime", w.three_prime * t_end),
            ("product", w.product * t_amp),
            ("gc_clamp", w.gc_clamp * t_clamp),
            ("length", w.length * t_len),
            ("gc", w.gc * t_gc),
        ],
    )
}

/// Ask `pl_clone::pcr` what it makes of the same two oligos.
///
/// Two implementations of one question is the point of running it, so a
/// disagreement is reported rather than resolved in favour of either.
fn pcr_length(template: &[u8], circular: bool, p: &Pair) -> Result<u64, String> {
    let t = pl_clone::Dseq::new(&String::from_utf8_lossy(template), circular);
    let f = String::from_utf8_lossy(&p.forward.oligo()).to_string();
    let r = String::from_utf8_lossy(&p.reverse.oligo()).to_string();
    match pl_clone::pcr(&f, &r, &t) {
        Ok(d) => Ok(d.watson.len() as u64),
        Err(e) => Err(e.to_string()),
    }
}
