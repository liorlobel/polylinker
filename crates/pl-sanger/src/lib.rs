//! Sanger reads against a reference: where they sit, and what differs.
//!
//! This answers the most-asked question in a cloning lab — *did my clone work?*
//! — and the ways to answer it wrongly are specific and worth naming, because
//! each one produces a confident, plausible, wrong answer.
//!
//! # Both orientations are tried, always
//!
//! Sequencing with a reverse primer is routine, and a reverse read compared
//! forward matches nothing. A tool that assumes orientation reports a perfect
//! clone as garbage, which is the expensive direction of wrong: the clone gets
//! thrown away.
//!
//! # Quality is not decoration
//!
//! A Sanger read is unreliable for its first few dozen bases and after roughly
//! 700, and the basecaller says so. Reporting a Q8 disagreement next to a Q50
//! one, with the same weight, buries the single real mutation in forty pieces
//! of noise. Every discrepancy here carries its [`Confidence`], and the counts
//! are reported separately.
//!
//! Low-confidence differences are still **reported**, never dropped. Trimming
//! them away silently is the other failure: the bases someone most wants to
//! look at when a clone came back strange are often exactly the ragged ends.
//!
//! # The reference may be circular
//!
//! A read spanning the origin of a plasmid is ordinary. It is aligned against
//! the sequence doubled, and the coordinates come back on the original.
//!
//! # What this is not
//!
//! Not a basecaller and not a variant caller. It compares one read to one
//! reference. Deciding that three reads agreeing on a change means the clone
//! carries a mutation is a judgement with a lot of context in it, and this
//! crate does not make it.

pub mod align;

pub use align::{AlignError, Alignment, Op, Scoring};

/// Why a read produced no [`Report`].
///
/// [`compare`] returns `Option` and throws this away, which is fine for a
/// caller that only wants the report; [`compare_reporting`] keeps it. The
/// distinction it exists for is between *this read is not from this reference*
/// and *this reference is too large to align exhaustively against*: the first
/// is an answer about a clone, the second is an answer about a limit, and
/// before 2026-07-28 the second was not an answer at all but an aborted
/// process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unplaced {
    /// The read or the reference was empty.
    Empty,
    /// The seeds did not agree on a diagonal and the exhaustive fallback found
    /// nothing either. The ordinary "this is not that clone" answer.
    NotFound,
    /// The seeds did not agree, and the reference is too large for the
    /// exhaustive fallback within [`Params::max_traceback_bytes`].
    ///
    /// Distinct from [`Unplaced::NotFound`] on purpose: nothing was ruled out
    /// here, the search was declined.
    RefusedTooLarge(AlignError),
}

impl std::fmt::Display for Unplaced {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unplaced::Empty => write!(f, "nothing to compare"),
            Unplaced::NotFound => write!(f, "the read could not be placed on this reference"),
            Unplaced::RefusedTooLarge(e) => write!(
                f,
                "the read shares too few seeds with this reference to be \
                 placed, and it was not aligned against all of it: {e}"
            ),
        }
    }
}

/// How much to believe a difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// The basecaller was confident here. Worth acting on.
    High,
    /// Below the quality threshold: this is as likely to be the read as the
    /// clone.
    Low,
    /// The file carried no quality values at all, so there is nothing to say.
    /// Distinct from `Low`, which is a measurement.
    Unknown,
}

/// One place where the read and the reference disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discrepancy {
    /// 1-based on the reference, on its plus strand.
    pub ref_pos: u64,
    /// 1-based in the read **as it was sequenced**, so it can be found in the
    /// chromatogram. For a reverse read this counts from the other end than the
    /// alignment does, which is the whole point of reporting it.
    pub read_pos: u64,
    pub kind: Op,
    /// `-` for an insertion in the read.
    pub ref_base: u8,
    /// `-` for a deletion.
    pub read_base: u8,
    pub quality: Option<u8>,
    pub confidence: Confidence,
}

/// Settings.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    pub scoring: Scoring,
    /// Phred score at or above which a difference is called [`Confidence::High`].
    ///
    /// 20 — one error in a hundred — is the conventional line and is what
    /// trace-viewing tools draw. It is a threshold on a continuum, not a fact.
    pub min_quality: u8,
    /// k-mer length for placing the read before aligning it.
    pub seed_k: usize,
    /// Ceiling on the traceback the exhaustive fallback may allocate.
    ///
    /// The fallback aligns against the entire reference, doubled first if it is
    /// circular, and its traceback is `3·(read+1)·(reference+1)` bytes. Nothing
    /// upstream bounds the reference, so on a genome-scale one this was 19 GB
    /// and an aborted process. Raise it if you genuinely want to align a read
    /// against a chromosome and have the memory for it.
    pub max_traceback_bytes: usize,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            scoring: Scoring::default(),
            min_quality: 20,
            seed_k: 12,
            max_traceback_bytes: align::DEFAULT_TRACEBACK_BUDGET,
        }
    }
}

/// The comparison.
#[derive(Debug, Clone)]
pub struct Report {
    pub alignment: Alignment,
    /// Was the read reverse-complemented to align it?
    pub reversed: bool,
    /// Did the alignment cross the origin of a circular reference?
    pub wrapped: bool,
    /// In reference order.
    pub discrepancies: Vec<Discrepancy>,
    /// Aligned columns that matched, over all aligned columns.
    pub identity: f64,
    /// 1-based inclusive span of the reference this read covers.
    pub covered: (u64, u64),
    /// The stretch of the read the basecaller stands behind, 1-based inclusive
    /// in the read as sequenced. `None` when the file carried no qualities.
    pub reliable: Option<(u64, u64)>,
}

impl Report {
    pub fn count(&self, c: Confidence) -> usize {
        self.discrepancies
            .iter()
            .filter(|d| d.confidence == c)
            .count()
    }

    /// Did anything that cannot be dismissed differ?
    ///
    /// [`Confidence::Unknown`] counts against being clean, and that asymmetry
    /// is the point: a file with no quality values gives no grounds to dismiss
    /// a difference, and treating "we cannot tell" as "it is fine" would print
    /// *no difference worth acting on* directly underneath a difference.
    pub fn clean(&self) -> bool {
        self.discrepancies
            .iter()
            .all(|d| d.confidence == Confidence::Low)
    }
}

/// Compare one read to one reference.
///
/// `quality` may be empty. Returns `None` when either sequence is empty or the
/// read cannot be placed at all; [`compare_reporting`] says which.
pub fn compare(
    read: &[u8],
    quality: &[u8],
    reference: &[u8],
    circular: bool,
    p: &Params,
) -> Option<Report> {
    compare_reporting(read, quality, reference, circular, p).ok()
}

/// [`compare`], keeping the reason there is no report.
pub fn compare_reporting(
    read: &[u8],
    quality: &[u8],
    reference: &[u8],
    circular: bool,
    p: &Params,
) -> Result<Report, Unplaced> {
    compare_reporting_until(read, quality, reference, circular, p, &|| false)
        .expect("a comparison that is never asked to stop always finishes")
}

/// [`compare_reporting`], abandonable.
///
/// `None` the first time `stop` says true; see
/// [`align::semiglobal_within_until`] for where it is polled and why the
/// alignment fill is the part that needed the hook.
///
/// # Why this exists
///
/// A GUI holds several reads against one construct and re-compares all of them
/// whenever the construct changes — an edit, an undo, or a click onto another
/// document tab. Without this, each superseded comparison runs to the last
/// cell: the receiver has gone, so its `send` fails, but only *afterwards*.
/// [`Params::max_traceback_bytes`] bounds one comparison's memory and says
/// nothing about how many of them are in flight, and the seeds-failed fallback
/// — the ordinary outcome of a failed sequencing reaction, which is the file a
/// user is most anxious about — is the expensive branch. `bins/pl-gui/src/doc.rs`
/// records what the same omission cost the enzyme digest: 30 workers spawned,
/// 16 live at once, 29 of them producing an answer nobody could still read.
pub fn compare_reporting_until(
    read: &[u8],
    quality: &[u8],
    reference: &[u8],
    circular: bool,
    p: &Params,
    stop: &dyn Fn() -> bool,
) -> Option<Result<Report, Unplaced>> {
    let m = read.len();
    let n = reference.len();
    if m == 0 || n == 0 {
        return Some(Err(Unplaced::Empty));
    }

    // A read crossing the origin is ordinary on a plasmid; doubling the
    // reference is what makes it findable, and the coordinates come back on the
    // original below.
    let doubled: Vec<u8>;
    let target: &[u8] = if circular {
        doubled = [reference, reference].concat();
        &doubled
    } else {
        reference
    };

    let rc = pl_core::reverse_complement(read);
    let mut best: Option<(Alignment, bool)> = None;
    // Kept so that "we declined to search" is never reported as "we searched
    // and found nothing". It only survives if *neither* orientation produced
    // an alignment.
    let mut refused: Option<AlignError> = None;
    for (seq, reversed) in [(read, false), (rc.as_slice(), true)] {
        // BOTH ORIENTATIONS ARE ALIGNED, one after the other, so a stopped
        // comparison must be able to give up between them as well as inside
        // one: the reverse pass costs everything the forward pass did.
        // `locate` is not polled — it is bounded to a 4 MiB reference by
        // `align::locate`'s own MAX_SEED_REFERENCE, and it is the unbounded DP
        // below it that this hook exists for.
        if stop() {
            return None;
        }
        // Place the read cheaply first, then align inside that window. Falling
        // back to the whole reference when the seeds do not agree keeps a poor
        // read slow rather than unplaced — up to the point where "slow" stops
        // being the cost. The windowed path is bounded by the window (a read's
        // length plus 200), so only the fallback can exceed the budget.
        let a = match align::locate(seq, target, p.seed_k, 100) {
            Some((lo, hi)) => align::semiglobal_within_until(
                seq,
                &target[lo..hi],
                &p.scoring,
                p.max_traceback_bytes,
                stop,
            )?
            .map(|mut a| {
                a.ref_start += lo;
                a.ref_end += lo;
                a
            }),
            None => align::semiglobal_within_until(
                seq,
                target,
                &p.scoring,
                p.max_traceback_bytes,
                stop,
            )?,
        };
        match a {
            Ok(a) => {
                if best.as_ref().is_none_or(|(b, _)| a.score > b.score) {
                    best = Some((a, reversed));
                }
            }
            Err(e @ (AlignError::TracebackTooLarge { .. } | AlignError::OutOfMemory { .. })) => {
                refused.get_or_insert(e);
            }
            Err(AlignError::Empty) => {}
        }
    }
    let (alignment, reversed) = match best {
        Some(b) => b,
        None => {
            return Some(Err(match refused {
                Some(e) => Unplaced::RefusedTooLarge(e),
                None => Unplaced::NotFound,
            }))
        }
    };

    // Quality belongs to the read as sequenced. For a reversed placement, read
    // position `qi` in the aligned (reverse-complemented) read is position
    // `m - 1 - qi` in the original read, so index the original quality there
    // rather than reversing the whole buffer. Reversing only lines up when
    // `quality.len() == m`; a short quality vector — a damaged `.ab1` whose PBAS2
    // and PCON2 tags differ in length, which `pl-abif` reads independently and
    // never forces equal — would otherwise pin every flag to the wrong end and
    // shift them all by `m - quality.len()`. Out of range gives `None`
    // (`Unknown`), the same graceful fall the forward path already takes.
    let quality_at = |qi: usize| -> Option<u8> {
        if reversed {
            m.checked_sub(1 + qi).and_then(|i| quality.get(i)).copied()
        } else {
            quality.get(qi).copied()
        }
    };

    let wrapped = circular && alignment.ref_end > n;
    let mut discrepancies = Vec::new();
    let mut ri = alignment.ref_start; // 0-based reference cursor
    let mut qi = 0usize; // 0-based cursor in the aligned read
    for op in &alignment.ops {
        let q = quality_at(qi);
        let conf = match q {
            None => Confidence::Unknown,
            Some(v) if v >= p.min_quality => Confidence::High,
            Some(_) => Confidence::Low,
        };
        let ref_pos = (ri % n.max(1)) as u64 + 1;
        // Back to the read as sequenced, so a position can be found in the
        // chromatogram rather than in an internal reversed copy.
        let read_pos = if reversed {
            (m - qi) as u64
        } else {
            qi as u64 + 1
        };
        match op {
            Op::Match => {
                ri += 1;
                qi += 1;
            }
            Op::Mismatch => {
                discrepancies.push(Discrepancy {
                    ref_pos,
                    read_pos,
                    kind: Op::Mismatch,
                    ref_base: reference[ri % n],
                    read_base: if reversed { rc[qi] } else { read[qi] },
                    quality: q,
                    confidence: conf,
                });
                ri += 1;
                qi += 1;
            }
            Op::Insertion => {
                discrepancies.push(Discrepancy {
                    ref_pos,
                    read_pos,
                    kind: Op::Insertion,
                    ref_base: b'-',
                    read_base: if reversed { rc[qi] } else { read[qi] },
                    quality: q,
                    confidence: conf,
                });
                qi += 1;
            }
            Op::Deletion => {
                discrepancies.push(Discrepancy {
                    ref_pos,
                    read_pos,
                    kind: Op::Deletion,
                    ref_base: reference[ri % n],
                    read_base: b'-',
                    // A deleted base has no quality of its own; the neighbour's
                    // is the closest honest thing, and it is why this is not
                    // silently `Unknown`.
                    quality: q,
                    confidence: conf,
                });
                ri += 1;
            }
        }
    }

    let covered = (
        (alignment.ref_start % n) as u64 + 1,
        ((alignment.ref_end + n - 1) % n) as u64 + 1,
    );
    Some(Ok(Report {
        identity: alignment.identity(),
        reliable: reliable_window(quality, p),
        alignment,
        reversed,
        wrapped,
        discrepancies,
        covered,
    }))
}

/// The stretch of a read its qualities stand behind — Mott trimming.
///
/// Each base scores `p_limit − p_error`, where `p_error` comes from its Phred
/// value and `p_limit` from [`Params::min_quality`]; the answer is the
/// contiguous run with the greatest total. This is the method Phred itself
/// uses, and it is here instead of a sliding mean because a mean overshoots:
/// averaged over twenty bases, thirteen bases of Q5 still pass while the
/// window is leaving a good region, so the reported reliable stretch ran
/// thirteen bases into rubbish.
///
/// `None` when there are no qualities, or when no run of the read is reliable
/// at all — which is a real answer about a failed read, not an error.
///
/// This is only ever *reported*. Nothing outside it is discarded, because on a
/// read that came back strange the ragged ends are often the part worth
/// looking at.
pub fn reliable_window(quality: &[u8], p: &Params) -> Option<(u64, u64)> {
    if quality.is_empty() {
        return None;
    }
    let limit = 10f64.powf(-(p.min_quality as f64) / 10.0);
    let (mut best, mut best_span) = (0f64, None);
    let (mut cur, mut start) = (0f64, 0usize);
    for (i, q) in quality.iter().enumerate() {
        let s = limit - 10f64.powf(-(*q as f64) / 10.0);
        if cur <= 0.0 {
            cur = s;
            start = i;
        } else {
            cur += s;
        }
        if cur > best {
            best = cur;
            best_span = Some((start as u64 + 1, i as u64 + 1));
        }
    }
    best_span
}

#[cfg(test)]
mod tests {
    use super::*;

    const REF: &[u8] = b"GGATCCTTAACCGGTTAAGCTTGCATGCCTGCAGGTCGACTCTAGAGGATCCCCGGGTACCGAGCTCGAATTCACTGGCCGTCGTTTTACAACGTCGTGACTGGGAAAACCCTGGCGTTACCCAACTTAATCGCCTTGCAGCACATCCCCCTTTCGCCAGCTGGCGTAATAGCGAAGAGGCCCGCACCGATCGCCCTTCCCA";

    fn q(n: usize, v: u8) -> Vec<u8> {
        vec![v; n]
    }

    /// Walking the alignment must reproduce both sequences, and every column
    /// must say what it is.
    ///
    /// This is the assertion to make about an indel. Where an ambiguous gap
    /// lands is genuinely undetermined — when the bases flanking a 6 bp
    /// deletion repeat, two placements score identically and both are correct —
    /// so pinning one of them tests the tie-break, not the aligner. This holds
    /// for either.
    fn check_alignment(aligned_read: &[u8], reference: &[u8], a: &Alignment) {
        let (mut ri, mut qi) = (a.ref_start, 0usize);
        let same = |x: u8, y: u8| x.eq_ignore_ascii_case(&y);
        for (k, op) in a.ops.iter().enumerate() {
            match op {
                Op::Match => {
                    assert!(
                        same(aligned_read[qi], reference[ri % reference.len()]),
                        "column {k} claims a match of {} and {}",
                        aligned_read[qi] as char,
                        reference[ri % reference.len()] as char
                    );
                    ri += 1;
                    qi += 1;
                }
                Op::Mismatch => {
                    ri += 1;
                    qi += 1;
                }
                Op::Deletion => ri += 1,
                Op::Insertion => qi += 1,
            }
        }
        assert_eq!(qi, aligned_read.len(), "every read base is accounted for");
        assert_eq!(ri, a.ref_end, "the reference span matches the ops");
    }

    #[test]
    fn a_perfect_read_is_clean_and_lands_where_it_came_from() {
        let read = &REF[40..140];
        let r = compare(read, &q(100, 50), REF, false, &Params::default()).unwrap();
        assert!(!r.reversed);
        assert!(r.clean());
        assert!(r.discrepancies.is_empty());
        assert_eq!(r.identity, 1.0);
        assert_eq!(r.covered, (41, 140));
        check_alignment(read, REF, &r.alignment);
    }

    #[test]
    fn a_reverse_primer_read_is_recognised_rather_than_condemned() {
        // Sequencing with a reverse primer is routine. Comparing it forward
        // matches nothing, and reporting a good clone as garbage is the
        // expensive direction of wrong -- the clone gets thrown away.
        let fwd = &REF[40..140];
        let read = pl_core::reverse_complement(fwd);
        let r = compare(&read, &q(100, 50), REF, false, &Params::default()).unwrap();
        assert!(r.reversed, "the read is the other way round");
        assert!(r.clean());
        assert_eq!(r.covered, (41, 140));
    }

    #[test]
    fn a_point_mutation_is_reported_at_the_right_place_on_both_strands() {
        let mut read = REF[40..140].to_vec();
        let was = read[30];
        read[30] = if was == b'A' { b'C' } else { b'A' };
        let r = compare(&read, &q(100, 50), REF, false, &Params::default()).unwrap();
        assert_eq!(r.discrepancies.len(), 1);
        let d = &r.discrepancies[0];
        assert_eq!(d.ref_pos, 71, "1-based: 40 + 30 + 1");
        assert_eq!(d.read_pos, 31);
        assert_eq!(d.ref_base, was);
        assert_eq!(d.confidence, Confidence::High);

        // The same mutation, sequenced the other way, is the same mutation.
        let rev = pl_core::reverse_complement(&read);
        let r2 = compare(&rev, &q(100, 50), REF, false, &Params::default()).unwrap();
        assert!(r2.reversed);
        assert_eq!(r2.discrepancies.len(), 1);
        let d2 = &r2.discrepancies[0];
        assert_eq!(
            d2.ref_pos, 71,
            "the reference position does not depend on the primer"
        );
        assert_eq!(
            d2.read_pos, 70,
            "but the read position counts from the read's own 5' end"
        );
        assert_eq!(d2.ref_base, was);
    }

    #[test]
    fn a_low_quality_disagreement_is_reported_but_not_believed() {
        // The failure this prevents: forty ragged end-of-read bases reported
        // with the same weight as one real mutation, so the real one is missed.
        let mut read = REF[40..140].to_vec();
        read[2] = if read[2] == b'A' { b'C' } else { b'A' };
        let mut qual = q(100, 50);
        qual[2] = 6;
        let r = compare(&read, &qual, REF, false, &Params::default()).unwrap();
        assert_eq!(r.discrepancies.len(), 1, "still reported, never dropped");
        assert_eq!(r.discrepancies[0].confidence, Confidence::Low);
        assert_eq!(r.discrepancies[0].quality, Some(6));
        assert!(r.clean(), "nothing here is worth acting on");
    }

    #[test]
    fn a_file_with_no_qualities_says_unknown_rather_than_guessing() {
        let mut read = REF[40..140].to_vec();
        read[30] = if read[30] == b'A' { b'C' } else { b'A' };
        let r = compare(&read, &[], REF, false, &Params::default()).unwrap();
        assert_eq!(r.discrepancies[0].confidence, Confidence::Unknown);
        assert_eq!(r.discrepancies[0].quality, None);
        assert!(r.reliable.is_none());
        assert!(
            !r.clean(),
            "a difference we cannot judge is not a difference we can dismiss"
        );
    }

    #[test]
    fn quality_follows_the_read_when_the_read_is_reversed() {
        // Forgetting to reverse the qualities alongside the bases puts every
        // confidence flag at the wrong end of the read, which is invisible
        // until the one real mutation is the one that gets dismissed.
        let mut read = REF[40..140].to_vec();
        read[2] = if read[2] == b'A' { b'C' } else { b'A' };
        let mut qual = q(100, 50);
        qual[2] = 6; // bad base at the read's 5' end

        let rev = pl_core::reverse_complement(&read);
        // The machine that sequenced this strand emitted quality indexed along
        // what it emitted, so reversing the bases reverses the qualities.
        let qrev: Vec<u8> = qual.iter().rev().copied().collect();
        let r = compare(&rev, &qrev, REF, false, &Params::default()).unwrap();
        assert!(r.reversed);
        assert_eq!(r.discrepancies.len(), 1);
        assert_eq!(
            r.discrepancies[0].confidence,
            Confidence::Low,
            "the low quality belongs to this base, whichever way the read is stored"
        );
    }

    #[test]
    fn a_read_across_the_origin_is_found_on_a_circular_reference() {
        let n = REF.len();
        let read = [&REF[n - 40..], &REF[..60]].concat();
        let r = compare(&read, &q(100, 50), REF, true, &Params::default()).unwrap();
        assert!(r.wrapped, "it crosses the origin");
        assert!(r.clean());
        assert_eq!(r.covered.0, (n - 40) as u64 + 1);
        assert_eq!(r.covered.1, 60);

        // And a linear reference genuinely cannot hold it.
        let lin = compare(&read, &q(100, 50), REF, false, &Params::default()).unwrap();
        assert!(!lin.clean(), "forcing it onto a line costs something");
    }

    #[test]
    fn an_indel_is_one_event_and_the_bases_are_named() {
        let mut read = REF[40..140].to_vec();
        let deleted: Vec<u8> = read.drain(50..56).collect();
        let r = compare(&read, &q(94, 50), REF, false, &Params::default()).unwrap();
        let dels: Vec<&Discrepancy> = r
            .discrepancies
            .iter()
            .filter(|d| d.kind == Op::Deletion)
            .collect();
        assert_eq!(dels.len(), 6, "{:?}", r.discrepancies);
        assert!(
            r.discrepancies.iter().all(|d| d.kind == Op::Deletion),
            "six bases gone, and nothing invented alongside: {:?}",
            r.discrepancies
        );
        // The bases are named rather than merely counted — but *which* six
        // depends on where the gap sits, and the flanks here repeat, so two
        // placements score the same. Both name a run of six real reference
        // bases at consecutive positions, and that is the checkable claim.
        assert!(dels.iter().all(|d| d.ref_base != b'-'));
        assert_eq!(
            dels.last().unwrap().ref_pos - dels[0].ref_pos,
            5,
            "one run, not six scattered losses"
        );
        assert_eq!(deleted.len(), 6);
        check_alignment(&read, REF, &r.alignment);
    }

    #[test]
    fn the_reliable_window_excludes_the_ragged_ends() {
        let mut qual = q(600, 50);
        for x in qual.iter_mut().take(35) {
            *x = 5;
        }
        for x in qual.iter_mut().skip(560) {
            *x = 5;
        }
        let (a, b) = reliable_window(&qual, &Params::default()).expect("a good middle");
        assert!(a > 20 && a <= 40, "starts after the bad head: {a}");
        assert!((540..=570).contains(&b), "ends before the bad tail: {b}");
    }

    #[test]
    fn an_empty_read_or_reference_is_not_an_answer() {
        assert!(compare(b"", &[], REF, false, &Params::default()).is_none());
        assert!(compare(b"ACGT", &[], b"", false, &Params::default()).is_none());
        assert_eq!(
            compare_reporting(b"", &[], REF, false, &Params::default()).unwrap_err(),
            Unplaced::Empty
        );
    }

    #[test]
    fn a_reference_too_large_for_the_fallback_is_refused_by_name_not_by_aborting() {
        // The path: a failed sequencing reaction returns junk, `locate` finds
        // fewer than three agreeing seeds, and `compare` falls back to
        // aligning against the *whole* reference -- doubled first, because the
        // reference is circular. On a 4.6 Mb genome that fallback asked for
        // 19 GB in three `vec![0u8; ...]` calls and the process aborted: no
        // report, no error, nothing naming the read or the reference.
        //
        // The budget is set low here so the shape is testable at a size that
        // fits in a test. What is being pinned is that exceeding it produces a
        // *distinguishable* answer, not silence and not "could not be placed".
        let junk = vec![b'C'; 300];
        // Between the two: the doubled reference needs 3 x 301 x 401 =
        // 362,103 bytes, a seeded 100 nt read's window at most 60,903.
        let p = Params {
            max_traceback_bytes: 200_000,
            ..Default::default()
        };
        assert!(align::locate(&junk, REF, p.seed_k, 100).is_none());
        match compare_reporting(&junk, &[], REF, true, &p) {
            Err(Unplaced::RefusedTooLarge(e)) => {
                let said = e.to_string();
                assert!(said.contains("300 nt read"), "{said}");
                assert!(said.contains("over the"), "{said}");
            }
            // Not `{other:?}`: a Report here is 20 kB of ops and discrepancies
            // in the failure output, which buries the one fact that matters.
            Ok(r) => panic!(
                "the fallback ran rather than being refused: {} columns at {:.2} identity",
                r.alignment.ops.len(),
                r.identity
            ),
            Err(e) => panic!("wrong reason: {e:?}"),
        }
        // The lossy wrapper still returns None, so no existing caller breaks.
        assert!(compare(&junk, &[], REF, true, &p).is_none());

        // Control 1: the same junk read against the same reference with the
        // shipped budget still goes down the fallback and still comes back
        // with a real (bad) alignment. "Keeps a poor read slow rather than
        // unplaced" is unchanged at every size that can actually be run.
        let ok = compare_reporting(&junk, &[], REF, true, &Params::default())
            .expect("the fallback still runs when it fits");
        assert!(ok.identity < 0.8, "junk aligns badly: {}", ok.identity);

        // Control 2: a read the seeds *do* place is bounded by its window, so
        // the fallback budget never comes into it.
        let read = REF[40..140].to_vec();
        let placed = compare_reporting(&read, &q(100, 50), REF, false, &p)
            .expect("a seeded read is not affected by the fallback budget");
        assert!(placed.clean());
        assert_eq!(placed.covered.0, 41);
    }

    /// PROVEN TO FAIL before this change: there was no `stop` at all, so a
    /// superseded comparison ran to its last Gotoh cell whatever the caller
    /// wanted. `bins/pl-gui/src/reads.rs` created an `AtomicBool` for exactly
    /// this and had nowhere to hand it, which made its "Close read" button a
    /// store into a flag nothing loaded.
    ///
    /// Asserted on the SEEDS-FAILED path, because that is the expensive one:
    /// the windowed path is bounded by the read's length plus 200 and would
    /// pass this test by being too small to matter.
    #[test]
    fn a_comparison_can_be_stopped_and_the_stop_is_polled_while_it_runs() {
        let junk = vec![b'C'; 300];
        let p = Params::default();
        // The premise: no seeds, so both orientations take the whole-reference
        // fallback rather than a window.
        assert!(align::locate(&junk, REF, p.seed_k, 100).is_none());

        // Already stopped: no report, and — the reason the hook is before the
        // allocation — nothing was asked of the allocator to find that out.
        assert!(compare_reporting_until(&junk, &[], REF, true, &p, &|| true).is_none());

        // It is polled DURING the work, not merely once at the door. A
        // predicate that never says true must be consulted more times than
        // there are calls into the aligner, or "stop" would only ever be
        // honoured between orientations.
        let polls = std::cell::Cell::new(0usize);
        let got = compare_reporting_until(&junk, &[], REF, true, &p, &|| {
            polls.set(polls.get() + 1);
            false
        })
        .expect("never asked to stop");
        assert!(
            polls.get() > 2 * junk.len(),
            "the aligner polled {} times for a {} nt read against two orientations \
             — that is not once per row",
            polls.get(),
            junk.len()
        );

        // And the answer is the same one the un-abandonable entry point gives:
        // the hook must not change what a comparison says.
        let want = compare_reporting(&junk, &[], REF, true, &p);
        assert_eq!(got.map(|r| r.covered), want.map(|r| r.covered));

        // A stop part-way through is still a stop, not a partial report. The
        // predicate lets the forward orientation finish and then refuses.
        let seen = std::cell::Cell::new(0usize);
        assert!(
            compare_reporting_until(&junk, &[], REF, true, &p, &|| {
                seen.set(seen.get() + 1);
                seen.get() > junk.len()
            })
            .is_none(),
            "a comparison stopped mid-alignment must produce no report at all"
        );
    }
}
