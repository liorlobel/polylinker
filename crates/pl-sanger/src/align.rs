//! Semi-global affine alignment with traceback.
//!
//! [`crate`] needs to say *which bases* differ, not just how many, so this
//! keeps a traceback — which is why it does not reuse `pl_features::align`,
//! whose job is to score a feature and which throws the path away.
//!
//! # The shape of the alignment
//!
//! The read is consumed entirely; the reference may hang off both ends for
//! free. A Sanger read is a few hundred bases from somewhere inside a plasmid,
//! so charging it for the thousands of reference bases on either side would
//! make every read look terrible.
//!
//! End gaps in the *read* are deliberately **not** free. Free ones would make
//! this a local alignment, which quietly trims whatever does not match —
//! exactly the bases a person is trying to inspect when a clone came back
//! wrong. Bad ends get reported and flagged by quality instead of vanishing.
//!
//! # Gotoh, in two rows
//!
//! Affine gaps, because one 12 bp deletion is one event and not twelve. Scores
//! are kept two rows at a time; only the traceback is materialised in full, at
//! one byte per cell per matrix.
//!
//! # Three bytes a cell has a ceiling, and it is asked for
//!
//! "Only the traceback" is still `3·(read+1)·(reference+1)` bytes, and until
//! 2026-07-28 nothing bounded it. That is fine for the plasmid this crate was
//! written against and is not fine one level up: when [`locate`] cannot place a
//! read — the ordinary outcome of a failed sequencing reaction — the caller
//! falls back to aligning against the *whole* reference, doubled first if it is
//! circular. A 700 nt read against a 4.6 Mb circular genome asks for 19.3 GB in
//! three allocations, and `vec![0u8; …]` does not return an error for that: it
//! trips Rust's allocation error hook and aborts the process, with no report,
//! no error and nothing naming the read or the reference.
//!
//! So the size is checked before it is asked for, and refused by name. A
//! refusal that says *19.3 GB for a 700 nt read against 9.2 Mb of reference* is
//! a different thing from a process that vanishes.

use std::collections::HashMap;

/// One column of an alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Match,
    Mismatch,
    /// A base in the read with no counterpart in the reference — an insertion
    /// in the clone.
    Insertion,
    /// A base in the reference the read does not have — a deletion.
    Deletion,
}

/// Scoring. The defaults are the usual DNA set and are not tuned to anything.
#[derive(Debug, Clone, Copy)]
pub struct Scoring {
    pub match_score: i32,
    pub mismatch: i32,
    /// Charged once per gap, in addition to [`Self::gap_extend`] per base.
    pub gap_open: i32,
    pub gap_extend: i32,
}

impl Default for Scoring {
    fn default() -> Self {
        Scoring {
            match_score: 1,
            mismatch: -2,
            gap_open: -5,
            gap_extend: -1,
        }
    }
}

/// Where a read sits on a reference, and how.
#[derive(Debug, Clone, PartialEq)]
pub struct Alignment {
    /// 0-based half-open, in the reference.
    pub ref_start: usize,
    pub ref_end: usize,
    pub score: i32,
    /// Left to right along the read.
    pub ops: Vec<Op>,
}

impl Alignment {
    pub fn matches(&self) -> usize {
        self.ops.iter().filter(|o| **o == Op::Match).count()
    }

    /// Matches over aligned columns. Not over the read's length: a read with a
    /// 30 bp deletion should not score as 100% because every base it *does*
    /// have is right.
    pub fn identity(&self) -> f64 {
        if self.ops.is_empty() {
            return 0.0;
        }
        self.matches() as f64 / self.ops.len() as f64
    }
}

const NEG: i32 = i32::MIN / 4; // room to add penalties without overflowing

/// Why an alignment was not produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlignError {
    /// The read or the reference was empty.
    Empty,
    /// The traceback would need more memory than it was allowed.
    ///
    /// Named rather than approximated, because "this read could not be placed"
    /// and "this reference is too big to align exhaustively against" are
    /// different answers and only one of them is about the read.
    TracebackTooLarge {
        read: usize,
        reference: usize,
        /// Bytes the three traceback matrices would need.
        need: usize,
        budget: usize,
    },
    /// The traceback fits the budget and the allocator still refused it.
    ///
    /// Reached through `try_reserve`, so this is a value and not an abort.
    OutOfMemory { need: usize },
}

impl std::fmt::Display for AlignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlignError::Empty => write!(f, "nothing to align"),
            AlignError::TracebackTooLarge {
                read,
                reference,
                need,
                budget,
            } => write!(
                f,
                "aligning a {read} nt read against {reference} nt of reference \
                 needs {} of traceback, over the {} allowed",
                mib(*need),
                mib(*budget)
            ),
            AlignError::OutOfMemory { need } => {
                write!(f, "could not allocate {} of traceback", mib(*need))
            }
        }
    }
}

fn mib(bytes: usize) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GiB", bytes as f64 / (1u64 << 30) as f64)
    } else {
        format!("{:.1} MiB", bytes as f64 / (1u64 << 20) as f64)
    }
}

/// What [`semiglobal`] will spend on traceback unless told otherwise.
///
/// 512 MiB, which at three bytes a cell buys a 1 kb read against about 175 kb
/// of reference — every plasmid, cosmid and fosmid, and a doubled 85 kb circle.
/// Past that the exhaustive fallback is not a slow right answer, it is an
/// allocation the machine will not survive: the 4.6 Mb E. coli genome the
/// fallback was reachable with wants 19.3 GB.
pub const DEFAULT_TRACEBACK_BUDGET: usize = 512 << 20;

/// Bytes of traceback an alignment of these lengths needs.
///
/// `None` on overflow, which is itself an answer: a size that does not fit a
/// `usize` is not one to allocate.
pub fn traceback_bytes(read: usize, reference: usize) -> Option<usize> {
    read.checked_add(1)?
        .checked_mul(reference.checked_add(1)?)?
        .checked_mul(3)
}

/// A zeroed buffer, or `None` rather than a dead process.
///
/// `vec![0u8; n]` has no failure mode short of `handle_alloc_error`, which
/// aborts. `try_reserve_exact` followed by `resize` cannot reallocate, so the
/// resize cannot fail either.
fn zeroed(n: usize) -> Option<Vec<u8>> {
    let mut v: Vec<u8> = Vec::new();
    v.try_reserve_exact(n).ok()?;
    v.resize(n, 0);
    Some(v)
}

/// Align `read` somewhere in `reference`, spending at most
/// [`DEFAULT_TRACEBACK_BUDGET`] on the traceback.
///
/// `reference` bounds the region considered; pass the whole reference to
/// consider all of it.
pub fn semiglobal(read: &[u8], reference: &[u8], sc: &Scoring) -> Result<Alignment, AlignError> {
    semiglobal_within(read, reference, sc, DEFAULT_TRACEBACK_BUDGET)
}

/// [`semiglobal`] with the traceback budget named explicitly.
pub fn semiglobal_within(
    read: &[u8],
    reference: &[u8],
    sc: &Scoring,
    budget: usize,
) -> Result<Alignment, AlignError> {
    semiglobal_within_until(read, reference, sc, budget, &|| false)
        .expect("an alignment that is never asked to stop always finishes")
}

/// [`semiglobal_within`], abandonable.
///
/// `stop` is polled once before the traceback matrices are allocated and once
/// per row of the fill, and the answer is `None` the first time it says true.
/// A row is `reference.len() + 1` cells, so on the widest reference this will
/// align — the [`DEFAULT_TRACEBACK_BUDGET`] admits about 175 kb against a 1 kb
/// read — a row is the whole check interval, and the check itself is one
/// relaxed poll per hundred thousand cells.
///
/// The budget bounds MEMORY and nothing bounds TIME: 512 MiB of traceback is
/// about 180 M Gotoh cells, and a caller that re-runs this on every keystroke
/// accumulates workers each of which will finish that whether or not anyone
/// still wants the answer. Dropping the caller's channel does not help — the
/// send fails, but only after the last cell.
///
/// `&dyn Fn() -> bool` rather than an `AtomicBool`, matching
/// [`pl_core::orf::find_orfs_until`]: the flag belongs to whatever is
/// coordinating the workers, and this crate does not need to know it is a flag.
///
/// The traceback walk itself is NOT polled. It is `O(read + reference)` steps
/// over memory already allocated, against the `O(read × reference)` fill above
/// it, so a poll there would buy nothing measurable.
pub fn semiglobal_within_until(
    read: &[u8],
    reference: &[u8],
    sc: &Scoring,
    budget: usize,
    stop: &dyn Fn() -> bool,
) -> Option<Result<Alignment, AlignError>> {
    // `Result<Option<_>, _>` inside and `transpose` at the door, so every `?`
    // below reads exactly as it did before the hook was added. Reversing the
    // two — `Option<Result<_, _>>` all the way down — costs a `match` at each
    // of the five failure points and buys nothing.
    fill(read, reference, sc, budget, stop).transpose()
}

/// [`semiglobal_within_until`]'s body. `Ok(None)` is "asked to stop".
fn fill(
    read: &[u8],
    reference: &[u8],
    sc: &Scoring,
    budget: usize,
    stop: &dyn Fn() -> bool,
) -> Result<Option<Alignment>, AlignError> {
    let m = read.len();
    let n = reference.len();
    if m == 0 || n == 0 {
        return Err(AlignError::Empty);
    }

    // Checked *before* anything is asked for. Asking and finding out is not an
    // option here: the failure mode of an oversized `vec![0u8; …]` is an
    // aborted process, which cannot be caught, reported or attributed to the
    // read that caused it.
    let need = traceback_bytes(m, n).ok_or(AlignError::TracebackTooLarge {
        read: m,
        reference: n,
        need: usize::MAX,
        budget,
    })?;
    if need > budget {
        return Err(AlignError::TracebackTooLarge {
            read: m,
            reference: n,
            need,
            budget,
        });
    }

    // BEFORE the allocation, not only inside the fill. `need` is up to 512 MiB
    // and this is the request a caller's cancel flag most wants to stop: a
    // superseded worker that has not started yet must not take half a gigabyte
    // away from the one somebody is actually waiting for.
    if stop() {
        return Ok(None);
    }

    // Traceback, one byte per cell per matrix. 0 = came from M, 1 = from X
    // (gap in the read), 2 = from Y (gap in the reference).
    let w = n + 1;
    let cells = (m + 1) * w;
    let mut tb_m = zeroed(cells).ok_or(AlignError::OutOfMemory { need })?;
    let mut tb_x = zeroed(cells).ok_or(AlignError::OutOfMemory { need })?;
    let mut tb_y = zeroed(cells).ok_or(AlignError::OutOfMemory { need })?;

    // Row 0. A leading run of reference with no read against it is free, which
    // is what lets the read sit anywhere.
    let mut pm = vec![NEG; w];
    let mut px = vec![0i32; w];
    let mut py = vec![NEG; w];
    pm[0] = 0;

    let (mut cm, mut cx, mut cy) = (vec![NEG; w], vec![NEG; w], vec![NEG; w]);

    for i in 1..=m {
        // Once per row. The inner loop below is `n` cells, so this is one
        // relaxed poll per reference length rather than per cell, and the
        // matrices are freed on the way out.
        // Once per row. The inner loop below is `n` cells, so this is one
        // relaxed poll per reference length rather than per cell, and the
        // matrices are freed on the way out.
        if stop() {
            return Ok(None);
        }
        cm[0] = NEG;
        cx[0] = NEG;
        // The read hanging off the start of the reference: charged, so the
        // aligner prefers to place the read inside.
        cy[0] = sc.gap_open + sc.gap_extend * i as i32;
        for j in 1..=n {
            let s = if eq(read[i - 1], reference[j - 1]) {
                sc.match_score
            } else {
                sc.mismatch
            };
            // M: read[i-1] against reference[j-1].
            let (best, from) = max3(pm[j - 1], px[j - 1], py[j - 1]);
            cm[j] = best.saturating_add(s);
            tb_m[i * w + j] = from;

            // X: reference[j-1] against a gap — deletion from the read.
            let open = cm[j - 1].saturating_add(sc.gap_open + sc.gap_extend);
            let ext = cx[j - 1].saturating_add(sc.gap_extend);
            if open >= ext {
                cx[j] = open;
                tb_x[i * w + j] = 0;
            } else {
                cx[j] = ext;
                tb_x[i * w + j] = 1;
            }

            // Y: read[i-1] against a gap — insertion in the read.
            let open = pm[j].saturating_add(sc.gap_open + sc.gap_extend);
            let ext = py[j].saturating_add(sc.gap_extend);
            if open >= ext {
                cy[j] = open;
                tb_y[i * w + j] = 0;
            } else {
                cy[j] = ext;
                tb_y[i * w + j] = 2;
            }
        }
        std::mem::swap(&mut pm, &mut cm);
        std::mem::swap(&mut px, &mut cx);
        std::mem::swap(&mut py, &mut cy);
    }

    // The read is consumed, so the answer is on row m. A trailing run of
    // reference is free, so the best column wins outright.
    let mut best = NEG;
    let mut bj = 0usize;
    let mut bmat = 0u8;
    for j in 0..=n {
        if pm[j] > best {
            best = pm[j];
            bj = j;
            bmat = 0;
        }
        if py[j] > best {
            best = py[j];
            bj = j;
            bmat = 2;
        }
    }

    // Walk back.
    let mut ops = Vec::with_capacity(m + 8);
    let (mut i, mut j, mut mat) = (m, bj, bmat);
    while i > 0 {
        // At the very start of the reference there is nothing left to align
        // against, so the remaining read bases can only be insertions. Stating
        // that here rather than relying on the traceback bytes matters: column
        // zero is never written by the fill loop, so a path that reaches it
        // reads whatever the array was initialised to.
        if j == 0 {
            ops.push(Op::Insertion);
            i -= 1;
            continue;
        }
        let idx = i * w + j;
        match mat {
            0 => {
                ops.push(if eq(read[i - 1], reference[j - 1]) {
                    Op::Match
                } else {
                    Op::Mismatch
                });
                mat = tb_m[idx];
                i -= 1;
                j -= 1;
            }
            1 => {
                ops.push(Op::Deletion);
                mat = tb_x[idx];
                j -= 1;
            }
            _ => {
                ops.push(Op::Insertion);
                mat = tb_y[idx];
                i -= 1;
            }
        }
    }
    // A gap in the read that reached row 0 is the free leading skip, not a
    // deletion: drop it, and it fixes `ref_start`.
    while matches!(ops.last(), Some(Op::Deletion)) {
        ops.pop();
        j += 1;
    }
    ops.reverse();

    let consumed: usize = ops
        .iter()
        .filter(|o| matches!(o, Op::Match | Op::Mismatch | Op::Deletion))
        .count();
    Ok(Some(Alignment {
        ref_start: j,
        ref_end: j + consumed,
        score: best,
        ops,
    }))
}

/// Case-insensitive, and `N` matches nothing in particular.
///
/// An `N` is *absence of information*, so it is scored as a mismatch rather
/// than a free pass. Treating it as matching everything would make a read whose
/// basecaller gave up look like a perfect clone.
#[inline]
fn eq(a: u8, b: u8) -> bool {
    let (a, b) = (a.to_ascii_uppercase(), b.to_ascii_uppercase());
    a == b && a != b'N'
}

#[inline]
fn max3(m: i32, x: i32, y: i32) -> (i32, u8) {
    if m >= x && m >= y {
        (m, 0)
    } else if x >= y {
        (x, 1)
    } else {
        (y, 2)
    }
}

/// A reference window likely to contain the read, from shared k-mers.
///
/// Full dynamic programming over a whole genome would be correct and far too
/// slow, so the read is first placed by exact k-mer seeds voting on a diagonal.
/// Returns `None` when too few seeds agree, and the caller then falls back to
/// aligning against everything — a slow right answer beats a fast wrong one.
pub fn locate(read: &[u8], reference: &[u8], k: usize, slack: usize) -> Option<(usize, usize)> {
    if read.len() < k || reference.len() < k {
        return None;
    }
    // A reference too large to seed here is also too large for the caller's
    // bounded traceback to align (`semiglobal_within`'s budget refuses it before
    // allocating), so return None and let that path refuse it by name rather than
    // build a multi-gigabyte k-mer index and abort in the allocator at chromosome
    // scale — the "process vanishes with no report" outcome the traceback budget
    // set out to remove, just relocated to the seeding step that runs first.
    const MAX_SEED_REFERENCE: usize = 4 << 20;
    if reference.len() > MAX_SEED_REFERENCE {
        return None;
    }
    let mut index: HashMap<&[u8], Vec<usize>> = HashMap::new();
    for j in 0..=reference.len() - k {
        index.entry(&reference[j..j + k]).or_default().push(j);
    }
    // Diagonals, bucketed so that a few indels do not split the vote.
    let mut votes: HashMap<i64, u32> = HashMap::new();
    for i in (0..=read.len() - k).step_by(3) {
        if let Some(hits) = index.get(&read[i..i + k]) {
            // A k-mer occurring all over the plasmid is evidence of nothing.
            if hits.len() > 8 {
                continue;
            }
            for &j in hits {
                *votes.entry((j as i64 - i as i64) / 32).or_default() += 1;
            }
        }
    }
    let (&bucket, &count) = votes.iter().max_by_key(|(b, c)| (**c, -**b))?;
    if count < 3 {
        return None;
    }
    let d = bucket * 32;
    let lo = (d - slack as i64).max(0) as usize;
    let hi = (d + read.len() as i64 + slack as i64).clamp(0, reference.len() as i64) as usize;
    if lo >= hi {
        return None;
    }
    Some((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops_string(a: &Alignment) -> String {
        a.ops
            .iter()
            .map(|o| match o {
                Op::Match => '=',
                Op::Mismatch => 'X',
                Op::Insertion => 'I',
                Op::Deletion => 'D',
            })
            .collect()
    }

    #[test]
    fn a_read_lands_where_it_came_from_and_costs_nothing_for_the_rest() {
        let reference = b"TTTTTTTTTTGGCCAATTCCGGAATTCCGGTTAACCGGTTAAAAAAAAAA";
        let read = &reference[10..40];
        let a = semiglobal(read, reference, &Scoring::default()).unwrap();
        assert_eq!(a.ref_start, 10);
        assert_eq!(a.ref_end, 40);
        assert_eq!(ops_string(&a), "=".repeat(30));
        assert_eq!(a.score, 30, "the flanking reference is free");
        assert_eq!(a.identity(), 1.0);
    }

    #[test]
    fn a_point_change_is_one_column_and_not_a_pair_of_gaps() {
        let reference = b"TTTTTTTTTTGGCCAATTCCGGAATTCCGGTTAACCGGTTAAAAAAAAAA";
        let mut read = reference[10..40].to_vec();
        read[15] ^= 0b100; // C<->G, A<->E... any different base
        read[15] = if reference[25] == b'A' { b'C' } else { b'A' };
        let a = semiglobal(&read, reference, &Scoring::default()).unwrap();
        assert_eq!(a.ref_start, 10);
        assert_eq!(a.ops.len(), 30, "no gaps: {}", ops_string(&a));
        assert_eq!(a.ops[15], Op::Mismatch);
        assert_eq!(a.ops.iter().filter(|o| **o == Op::Mismatch).count(), 1);
    }

    #[test]
    fn one_deletion_of_twelve_bases_costs_one_opening_not_twelve() {
        // The reason for affine gaps: a 12 bp dropout is one event. With a
        // linear penalty the aligner would rather scatter mismatches, and the
        // report would describe a mutant that does not exist.
        let reference = b"GGCCAATTCCGGAATTCCGGTTAACCGGTTAACCGGATCGATCGATCGTAGCTAGCTAGCT";
        let mut read = reference.to_vec();
        read.drain(20..32);
        let a = semiglobal(&read, reference, &Scoring::default()).unwrap();
        let s = ops_string(&a);
        assert_eq!(s.matches('D').count(), 12, "{s}");
        assert!(!s.contains('X'), "no mismatches invented: {s}");
        // One run, not twelve scattered ones.
        assert_eq!(s.split('D').filter(|p| p.is_empty()).count(), 11, "{s}");
    }

    #[test]
    fn an_insertion_in_the_read_is_reported_as_an_insertion() {
        let reference = b"GGCCAATTCCGGAATTCCGGTTAACCGGTTAACCGGATCGATCGATCGTAGCTAGCTAGCT";
        let mut read = reference.to_vec();
        for (i, b) in b"TTTTTT".iter().enumerate() {
            read.insert(25 + i, *b);
        }
        let a = semiglobal(&read, reference, &Scoring::default()).unwrap();
        let s = ops_string(&a);
        assert_eq!(s.matches('I').count(), 6, "{s}");
        assert_eq!(a.ref_end - a.ref_start, reference.len());
    }

    #[test]
    fn an_n_is_absence_of_evidence_and_not_a_free_pass() {
        // A basecaller that gave up must not make a clone look perfect.
        let reference = b"GGCCAATTCCGGAATTCCGGTTAACCGGTTAACCGG";
        let mut read = reference.to_vec();
        read[10] = b'N';
        let a = semiglobal(&read, reference, &Scoring::default()).unwrap();
        assert_eq!(a.ops[10], Op::Mismatch);
        assert!(a.identity() < 1.0);
    }

    #[test]
    fn lower_case_is_display_information_and_never_changes_the_answer() {
        let reference = b"GGCCAATTCCGGAATTCCGGTTAACCGGTTAACCGG";
        let lower = reference.to_ascii_lowercase();
        let a = semiglobal(&lower, reference, &Scoring::default()).unwrap();
        assert_eq!(a.identity(), 1.0);
        assert_eq!(a.score, reference.len() as i32);
    }

    #[test]
    fn the_read_is_consumed_even_when_its_ends_are_rubbish() {
        // Free end gaps in the read would make this a local alignment, which
        // trims away whatever does not match -- precisely the bases someone is
        // looking at when a clone came back wrong.
        let reference = b"GGCCAATTCCGGAATTCCGGTTAACCGGTTAACCGGATCGATCGATCGTAG";
        let read = [b"CCCCCCCC".as_slice(), &reference[10..40], b"GGGGGGGG"].concat();
        let a = semiglobal(&read, reference, &Scoring::default()).unwrap();
        let consumed: usize = a
            .ops
            .iter()
            .filter(|o| matches!(o, Op::Match | Op::Mismatch | Op::Insertion))
            .count();
        assert_eq!(
            consumed,
            read.len(),
            "every read base appears: {}",
            ops_string(&a)
        );
    }

    #[test]
    fn seeding_finds_the_window_and_admits_when_it_cannot() {
        let reference: Vec<u8> = b"ACGTTGCAAGGCTTAACCGGATCGGATCCAAGCTTGGTACCGAGCTCGGATCCACTAGT"
            .repeat(20)
            .to_vec();
        // A unique stretch to seed on: the repeat above deliberately is not.
        let mut reference = reference;
        let unique = b"TTGACCAGGTCACATTGCCAGATACGGTTACCAAGCTTGATTCCGGAAATGCA";
        let at = 500;
        reference.splice(at..at + unique.len(), unique.iter().copied());
        let read = &reference[at..at + unique.len()];
        let (lo, hi) = locate(read, &reference, 12, 100).expect("seeds agree");
        assert!(lo <= at && hi >= at + read.len(), "window {lo}..{hi}");

        // Nothing in common: say so rather than guess.
        let junk = b"CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC";
        assert!(locate(junk, &reference, 12, 100).is_none());
    }

    #[test]
    fn a_traceback_too_big_to_allocate_is_named_and_not_attempted() {
        // `vec![0u8; n]` has no error path: an oversized one aborts the
        // process through `handle_alloc_error`, which no caller can catch,
        // report or attribute. So the size is computed and compared first,
        // and the refusal names the two lengths that produced it.
        let reference = b"GGCCAATTCCGGAATTCCGGTTAACCGGTTAACCGGATCGATCGATCGTAG";
        let read = &reference[10..40];
        // 30 x 51 x 3 is 4743 bytes; a 1 kB budget cannot hold it.
        let e = semiglobal_within(read, reference, &Scoring::default(), 1024).unwrap_err();
        match e {
            AlignError::TracebackTooLarge {
                read: r,
                reference: n,
                need,
                budget,
            } => {
                assert_eq!(r, 30);
                assert_eq!(n, reference.len());
                assert_eq!(need, traceback_bytes(30, reference.len()).unwrap());
                assert_eq!(budget, 1024);
            }
            other => panic!("{other:?}"),
        }
        let said = e.to_string();
        assert!(said.contains("30 nt read"), "{said}");
        assert!(said.contains("51"), "{said}");

        // The arithmetic the check runs on, at the size that made this matter:
        // a 700 nt read against a doubled 4.6 Mb genome. Computed, never
        // allocated -- if this test allocated it, it would be the bug.
        assert_eq!(traceback_bytes(700, 9_200_000), Some(19_347_602_103));
        assert!(traceback_bytes(700, 9_200_000).unwrap() > DEFAULT_TRACEBACK_BUDGET);
        // And a size that does not fit a usize is an answer, not a panic.
        assert_eq!(traceback_bytes(usize::MAX, usize::MAX), None);
    }

    #[test]
    fn an_alignment_that_fits_the_budget_is_unchanged_by_the_check() {
        // The control. The budget must not perturb any alignment that was
        // always affordable, and the default must comfortably hold the
        // plasmid-sized work this crate exists for.
        let reference = b"TTTTTTTTTTGGCCAATTCCGGAATTCCGGTTAACCGGTTAAAAAAAAAA";
        let read = &reference[10..40];
        let a = semiglobal(read, reference, &Scoring::default()).unwrap();
        let b = semiglobal_within(read, reference, &Scoring::default(), 1 << 20).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.ref_start, 10);
        assert_eq!(a.identity(), 1.0);

        // A 1 kb read against a doubled 85 kb plasmid still fits the default.
        assert!(traceback_bytes(1_000, 170_000).unwrap() < DEFAULT_TRACEBACK_BUDGET);

        // An empty input is still its own answer and not a size complaint.
        assert_eq!(
            semiglobal(b"", reference, &Scoring::default()).unwrap_err(),
            AlignError::Empty
        );
    }
}
