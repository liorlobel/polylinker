//! Sanger reads held against the open construct.
//!
//! `pl-abif` and `pl-sanger` have shipped and been tested since the sequencing
//! work landed and had no GUI entry point at all: `pl trace` could draw a
//! chromatogram and `pl sanger` could answer *did my clone work?*, and the
//! application could do neither.
//!
//! # A trace is an ATTACHMENT, not a second document kind
//!
//! `pl_sanger::compare_reporting` takes a read AND a reference and cannot be
//! called with one of them. A `.ab1` on its own answers nothing a cloner asked;
//! the question is about the construct on screen. The trace is the evidence and
//! the molecule is the subject, and anything that makes the trace the subject
//! inverts that.
//!
//! Making it a second document kind would mean opening a trace CLOSES the
//! plasmid — the exact opposite of the need, which is both at once — and would
//! drag `OpLog`, `unsaved()`, the dirty dot, undo/redo, `records_in_file` and
//! the whole `take_over`/`PendingOpen` guard into a thing that is read-only by
//! house rule. Making it a transient panel is the other failure: forward primer
//! plus reverse primer, or a primer walk, is the normal case, so the unit is
//! "the reads on this construct", plural.
//!
//! So reads live on `App`, not on `Document`. They must survive the arrival AND
//! the replacement of a document, because "I opened the wrong plasmid, let me
//! open the right one" is the commonest correction in this workflow and must
//! not cost the user their files. What does NOT survive is the report: a
//! `Report` names a reference, so every one is discarded and re-armed when the
//! document changes.
//!
//! # NOTHING HERE EDITS
//!
//! No `OpKind`, no "accept this base into the sequence" button. A trace is a
//! view. If the user wants the change they make it in the Sequence tab, with
//! the caret already put on the base for them.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

/// Where a read's comparison has got to.
///
/// Deliberately the same shape as `doc::DigestState`, including the cancel
/// flag, and it must be. `align::DEFAULT_TRACEBACK_BUDGET` is 512 MB and the
/// seeds-failed fallback allocates `3·(read+1)·(reference+1)` bytes and runs a
/// Gotoh DP over that many cells. A 900 nt read the seeds cannot place, against
/// a 50 kb circular reference doubled to 100 kb, is 270 MB and about 90 M cells
/// — UNDER the budget, so it runs, on whatever thread called it. That is a
/// multi-second freeze on a FAILED sequencing reaction, which is the file a
/// user is most anxious about.
pub enum CompareState {
    Running {
        rx: Receiver<Result<pl_sanger::Report, pl_sanger::Unplaced>>,
        cancel: Arc<AtomicBool>,
    },
    Done(pl_sanger::Report),
    /// The read produced no report, with the reason. The three read
    /// differently on purpose — see [`unplaced_sentence`].
    Unplaced(pl_sanger::Unplaced),
    /// No molecule is open, so there is nothing to compare against.
    NoReference,
    /// The comparison could not be STARTED. Distinct from `Unplaced` for the
    /// same reason `RefusedTooLarge` is distinct from `NotFound`: nothing about
    /// the read was ruled out here either.
    Failed(String),
}

/// One chromatogram, and what it says about the open construct.
pub struct Read {
    pub path: Option<PathBuf>,
    pub name: String,
    pub trace: pl_abif::Trace,
    pub state: CompareState,
}

impl Read {
    pub fn new(name: String, path: Option<PathBuf>, trace: pl_abif::Trace) -> Read {
        Read {
            path,
            name,
            trace,
            state: CompareState::NoReference,
        }
    }

    /// Start comparing this read to a reference, on a worker.
    pub fn compare(&mut self, reference: &[u8], circular: bool) {
        if let CompareState::Running { cancel, .. } = &self.state {
            cancel.store(true, Ordering::Relaxed);
        }
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        // The worker owns copies: the reference can be edited underneath it,
        // and an alignment against a sequence that has since changed is the
        // stale-answer defect `Document::apply`'s unconditional re-digest
        // exists to prevent.
        let read = self.trace.sequence.clone();
        let qual = self.trace.quality.clone();
        let reference = reference.to_vec();
        let spawned = std::thread::Builder::new()
            .name("sanger".into())
            .spawn(move || {
                let p = pl_sanger::Params::default();
                let r = pl_sanger::compare_reporting(&read, &qual, &reference, circular, &p);
                // Send failing means the document was replaced; that is fine.
                let _ = tx.send(r);
            });
        self.state = match spawned {
            Ok(_) => CompareState::Running { rx, cancel },
            // A thread that will not start is not a read that does not place.
            Err(e) => CompareState::Failed(format!(
                "this read was not compared: no worker thread could be started ({e}).                  Nothing about it was ruled out."
            )),
        };
    }

    /// Collect the worker's answer if it has arrived. True when something
    /// changed, so the caller knows to repaint.
    pub fn poll(&mut self) -> bool {
        let done = match &self.state {
            CompareState::Running { rx, .. } => rx.try_recv().ok(),
            _ => None,
        };
        match done {
            Some(Ok(r)) => {
                self.state = CompareState::Done(r);
                true
            }
            Some(Err(u)) => {
                self.state = CompareState::Unplaced(u);
                true
            }
            None => false,
        }
    }

    pub fn cancel(&self) {
        if let CompareState::Running { cancel, .. } = &self.state {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The stretch of the read the basecaller stands behind, 1-based inclusive
    /// in the read as sequenced.
    ///
    /// A property of the FILE, not of any comparison — `reliable_window` takes
    /// qualities only — so it is available with no molecule open, which is half
    /// of what the panel can honestly show in that state.
    pub fn reliable(&self) -> Option<(u64, u64)> {
        pl_sanger::reliable_window(&self.trace.quality, &pl_sanger::Params::default())
    }

    /// The facts the file carries, which are true whether or not anything has
    /// been compared. `pl trace`'s own lines.
    pub fn header(&self) -> Vec<String> {
        let t = &self.trace;
        let mut out = vec![format!(
            "{} bases · {} ambiguous · {}",
            t.sequence.len(),
            t.ambiguous(),
            match t.mean_quality() {
                Some(q) => format!("mean quality {q:.1}"),
                // `None` means NO NUMBER, and which of the two reasons it is
                // decides what may be printed: `quality_was_dropped` says the
                // file HAS qualities and they could not be read, and "no
                // quality values in this file" about that file is false.
                None if t.quality_was_dropped() =>
                    "the quality values in this file could not be read".into(),
                None => "no quality values in this file".into(),
            }
        )];
        if !t.sample_name.is_empty() {
            out.push(format!("sample {}", t.sample_name));
        }
        if !t.machine.is_empty() || !t.run_start.is_empty() {
            out.push(format!("{} {}", t.machine, t.run_start).trim().to_string());
        }
        out.push(format!("ABIF version {}", t.abif_version));
        // THE MACHINE'S CALL IS WHAT IS DRAWN, and 58% of real files also carry
        // a human's. `Trace::sequence` is `PBAS2` (the basecaller's) and
        // `edited_sequence` is `PBAS1` (a human's) — the opposite way round
        // from the obvious guess. Showing one and hiding the other is how a
        // user reads a sequence nobody meant them to read.
        match (t.edited(), t.edit_distance()) {
            (true, Some(n)) => out.push(format!(
                "a human edited {n} base(s); the machine's call is shown"
            )),
            (true, None) => out.push(
                "a human edited this read and the two differ in length, so there is no \
                 edit count; the machine's call is shown"
                    .into(),
            ),
            (false, _) => {}
        }
        // A SHORT `PCON2`. `qual.get(qi)` returns `None` wherever the quality
        // array is shorter than the read, so a file whose qualities run out
        // mid-read gets High/Low for its head and Unknown for its tail with
        // nothing announcing the change.
        if !t.quality.is_empty() && t.quality.len() < t.sequence.len() {
            out.push(format!(
                "quality values stop at base {} of {}; every difference after that is \
                 reported as unknown rather than dismissed",
                t.quality.len(),
                t.sequence.len()
            ));
        }
        if t.peaks.is_empty() {
            out.push(
                "no base positions in this file, so the trace is drawn without its calls".into(),
            );
        }
        out
    }

    /// The one-sentence verdict, which ALWAYS carries coverage.
    ///
    /// Never a bare identity percentage. 100% identity over 200 aligned columns
    /// on a 5,386 bp plasmid says nothing about the other 5,186 bases, and the
    /// two numbers side by side are the only honest form.
    pub fn verdict(&self, reference_len: u64) -> String {
        match &self.state {
            CompareState::NoReference => {
                "No construct is open, so nothing has been compared. Open the plasmid this \
                 read came from and it will be compared to it."
                    .into()
            }
            CompareState::Running { .. } => "comparing…".into(),
            CompareState::Unplaced(u) => unplaced_sentence(u),
            CompareState::Failed(why) => why.clone(),
            CompareState::Done(r) => {
                let (a, b) = r.covered;
                let span = if b >= a {
                    b - a + 1
                } else {
                    reference_len - a + 1 + b
                };
                let pct = if reference_len == 0 {
                    0.0
                } else {
                    100.0 * span as f64 / reference_len as f64
                };
                let high = r.count(pl_sanger::Confidence::High);
                let unknown = r.count(pl_sanger::Confidence::Unknown);
                let low = r.count(pl_sanger::Confidence::Low);
                let mut s = format!(
                    "covers {a}..{b} of {reference_len} bp ({pct:.0}%) · {} strand · \
                     {:.2}% identity over aligned columns",
                    // ALWAYS printed, as a plain fact and never as a warning.
                    // Sequencing with a reverse primer is routine, and the
                    // absence of a word is not a statement.
                    if r.reversed { "reverse" } else { "forward" },
                    // TWO DECIMALS, because `cmd_sanger` prints two. The same
                    // read read 99.8% here and 99.75% at the terminal, which is
                    // the only number the two Sanger surfaces disagreed on —
                    // and a user checking one against the other has no way to
                    // know which rounded.
                    r.identity * 100.0
                );
                if r.wrapped {
                    s.push_str(" · crosses the origin");
                }
                s.push_str(" · ");
                // THE CLEAN ARM IS SPLIT, and it has to be. `Report::clean` is
                // true whenever every difference is below Q20, so a clone with
                // a real substitution in the ragged tail was headlined "no
                // difference worth acting on" — the one sentence a reader takes
                // away — with "1 more below Q20" after it, where "more" was
                // more than a count the previous clause had just asserted was
                // zero. The module doc says the UI honours `clean()`'s
                // asymmetry exactly; it honoured the Unknown half and inverted
                // the Low half.
                if r.discrepancies.is_empty() {
                    s.push_str("no difference at all over the aligned columns");
                } else if r.clean() {
                    // The row's own hover text, which is true, instead of a
                    // dismissal that is not.
                    s.push_str(&format!(
                        "{low} difference(s), every one below Q20 — as likely to be the read \
                         as the clone"
                    ));
                } else if unknown > 0 {
                    // `Report::clean` counts Unknown AGAINST being clean, and
                    // the UI honours that asymmetry exactly: a file with no
                    // quality values gives no grounds to DISMISS a difference.
                    s.push_str(&format!(
                        "{} difference(s); this file carries no quality values there, so \
                         none of them can be dismissed",
                        high + unknown
                    ));
                } else {
                    s.push_str(&format!("{high} difference(s) at or above Q20"));
                }
                // Only when there is something for them to be MORE than. The
                // clean arm above has already counted them, and appending
                // "1 more" to a sentence that just said there were none was
                // the grammar giving the arithmetic away.
                if low > 0 && !r.clean() {
                    s.push_str(&format!(
                        " · {low} more below Q20, set aside rather than dropped"
                    ));
                }
                s
            }
        }
    }

    /// Which sequence was compared, said out loud.
    pub fn which_sequence(&self) -> &'static str {
        "compared using the basecaller's call (PBAS2), which is what `pl sanger` uses"
    }
}

/// What a read that did not place actually means. The three cases read
/// differently ON PURPOSE.
pub fn unplaced_sentence(u: &pl_sanger::Unplaced) -> String {
    match u {
        pl_sanger::Unplaced::Empty => {
            "there is nothing to compare: this file has no base calls, or the construct has \
             no bases"
                .into()
        }
        // Explicitly NOT "the clone is wrong". The two ordinary causes are the
        // wrong reference file and a failed reaction, and the panel names both.
        pl_sanger::Unplaced::NotFound => {
            "this read does not match this construct. Nothing about the construct was \
             checked. The two ordinary causes are the wrong reference file open and a \
             failed sequencing reaction."
                .into()
        }
        // NOTHING WAS RULED OUT; THE SEARCH WAS DECLINED, and it must not be
        // spelled like NotFound. "We did not look" and "we looked and found
        // nothing" are different answers and only one of them is about the read.
        pl_sanger::Unplaced::RefusedTooLarge(e) => format!(
            "the search was DECLINED, not failed — nothing about this read was ruled out: \
             {e}. `pl sanger` can be given a larger traceback budget."
        ),
    }
}

/// One discrepancy as the columns `pl sanger` prints, so the two cannot drift.
///
/// Returns `(position, change, quality, kind, confidence)`.
pub fn row(d: &pl_sanger::Discrepancy) -> (String, String, String, &'static str, &'static str) {
    (
        format!("ref {}", crate::doc::fmt_int(d.ref_pos)),
        format!("{} → {}", d.ref_base as char, d.read_base as char),
        match d.quality {
            Some(q) => format!("Q{q}"),
            None => "Q?".into(),
        },
        match d.kind {
            pl_sanger::Op::Mismatch => "substitution",
            pl_sanger::Op::Insertion => "insertion",
            pl_sanger::Op::Deletion => "deletion",
            pl_sanger::Op::Match => "match",
        },
        // A WORD, never only a colour.
        match d.confidence {
            pl_sanger::Confidence::High => "confident",
            pl_sanger::Confidence::Low => "below Q20 — as likely to be the read as the clone",
            pl_sanger::Confidence::Unknown => "no quality here, so it cannot be dismissed",
        },
    )
}

/// Where this difference is IN THE READ, and — for a reverse read — why the
/// letter under that peak is not the letter in the row.
///
/// # The read coordinate is on every row, not only the reverse ones
///
/// The panel states the Mott window in READ coordinates ("the basecaller stands
/// behind bases 1..340") and labels every discrepancy in REFERENCE coordinates
/// ("ref 970"). Those are different numbering systems, and `read_pos` — the one
/// number that connects them — was rendered only when the read happened to be
/// reversed. So the panel volunteered the single most useful fact about a
/// Sanger difference, is it inside or outside the stretch the basecaller
/// vouches for, and then withheld the coordinate needed to use it.
///
/// # THE REVERSE-STRAND TRAP
///
/// Verified in `pl-sanger`'s source: for a `reversed` read `read_base` is
/// `rc[qi]` — the base on the REFERENCE's plus strand — while
/// `read_pos = m - qi`, and `rc[qi] == complement(read[m-1-qi])`. So a row
/// reading "ref A → read G" jumps to a chromatogram base labelled C. The panel
/// must say which, permanently, rather than leave the list and the picture
/// disagreeing about a base.
pub fn read_base_note(
    r: &pl_sanger::Report,
    d: &pl_sanger::Discrepancy,
    trace: &[u8],
    reliable: Option<(u64, u64)>,
) -> String {
    let mut s = format!(" (read {}", crate::doc::fmt_int(d.read_pos));
    if r.reversed {
        let at = trace
            .get((d.read_pos as usize).saturating_sub(1))
            .copied()
            .unwrap_or(b'?');
        s.push_str(&format!(", {} on the trace; reverse strand", at as char));
    }
    // IN WORDS, and only when it is true. A difference outside the Mott window
    // is not thereby wrong — `compare_reporting` aligns the whole read on
    // purpose, because on a read that came back strange the ragged ends are
    // often the part worth looking at — but the reader has to be told which
    // side of that line they are standing on.
    if let Some((lo, hi)) = reliable {
        if d.read_pos < lo || d.read_pos > hi {
            s.push_str(", outside the basecaller's window");
        }
    }
    s.push(')');
    s
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A minimal ABIF file.
    ///
    /// A COPY of `pl_abif`'s own test builder, because that one is
    /// `#[cfg(test)]` inside its crate and so is not reachable from here. Kept
    /// byte-for-byte the same layout on purpose: a fixture written by a
    /// different writer would test this module against a file no real
    /// instrument and no other test produces.
    pub fn build(entries: &[(&[u8; 4], i32, i16, &[u8])]) -> Vec<u8> {
        let header_len = 128;
        let dir_off = header_len;
        let mut dir = Vec::new();
        let mut heap = Vec::new();
        let heap_off = dir_off + entries.len() * 28;
        for (name, num, etype, payload) in entries {
            dir.extend_from_slice(*name);
            dir.extend_from_slice(&num.to_be_bytes());
            dir.extend_from_slice(&etype.to_be_bytes());
            dir.extend_from_slice(&1i16.to_be_bytes());
            dir.extend_from_slice(&(payload.len() as i32).to_be_bytes());
            dir.extend_from_slice(&(payload.len() as i32).to_be_bytes());
            if payload.len() <= 4 {
                let mut inline = [0u8; 4];
                inline[..payload.len()].copy_from_slice(payload);
                dir.extend_from_slice(&inline);
            } else {
                dir.extend_from_slice(&((heap_off + heap.len()) as i32).to_be_bytes());
                heap.extend_from_slice(payload);
            }
            dir.extend_from_slice(&0i32.to_be_bytes());
        }
        let mut out = vec![0u8; header_len];
        out[..4].copy_from_slice(b"ABIF");
        out[4..6].copy_from_slice(&101u16.to_be_bytes());
        out[6..10].copy_from_slice(b"tdir");
        out[10..14].copy_from_slice(&1i32.to_be_bytes());
        out[14..16].copy_from_slice(&1023i16.to_be_bytes());
        out[16..18].copy_from_slice(&28i16.to_be_bytes());
        out[18..22].copy_from_slice(&(entries.len() as i32).to_be_bytes());
        out[22..26].copy_from_slice(&((entries.len() * 28) as i32).to_be_bytes());
        out[26..30].copy_from_slice(&(dir_off as i32).to_be_bytes());
        out.extend_from_slice(&dir);
        out.extend_from_slice(&heap);
        out
    }

    /// Gaussian peaks, twelve samples apart, in the channel each base belongs
    /// to — the same generator `pl_draw::trace`'s tests use.
    fn channels(seq: &[u8], order: [u8; 4]) -> (Vec<Vec<u8>>, Vec<u8>) {
        let spacing = 12usize;
        let len = seq.len() * spacing + spacing;
        let mut ch = vec![vec![0u16; len]; 4];
        let mut peaks: Vec<u8> = Vec::new();
        for (i, b) in seq.iter().enumerate() {
            let centre = spacing / 2 + i * spacing;
            peaks.extend_from_slice(&(centre as u16).to_be_bytes());
            let c = order.iter().position(|x| x == b).unwrap_or(0);
            for d in 0..spacing {
                let off = d as i64 - spacing as i64 / 2;
                let v = (8000.0 * (-(off * off) as f64 / 8.0).exp()) as u16;
                let at = centre as i64 + off;
                if at >= 0 && (at as usize) < len {
                    ch[c][at as usize] = ch[c][at as usize].max(v);
                }
            }
        }
        let bytes = ch
            .iter()
            .map(|c| c.iter().flat_map(|v| v.to_be_bytes()).collect())
            .collect();
        (bytes, peaks)
    }

    /// A whole `.ab1`, with traces, calls and qualities.
    pub fn ab1(seq: &[u8], quality: &[u8]) -> Vec<u8> {
        let order = *b"GATC";
        let (ch, peaks) = channels(seq, order);
        build(&[
            (b"DATA", 9, 4, &ch[0]),
            (b"DATA", 10, 4, &ch[1]),
            (b"DATA", 11, 4, &ch[2]),
            (b"DATA", 12, 4, &ch[3]),
            (b"FWO_", 1, 2, &order),
            (b"PLOC", 2, 4, &peaks),
            (b"PBAS", 2, 2, seq),
            (b"PCON", 2, 2, quality),
            (b"SMPL", 1, 18, b"\x08demo-fwd"),
            (b"MCHN", 1, 19, b"3730xl\0"),
            (b"RUND", 1, 19, b"2026-07-31\0"),
        ])
    }

    fn demo() -> pl_core::Molecule {
        let data = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../prototype/demo-construct.gb"
        ))
        .expect("the demo construct");
        pl_fileio::load(&data).expect("it parses").0
    }

    fn done(r: &mut Read) {
        for _ in 0..2_000 {
            if r.poll() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("the comparison never finished");
    }

    /// PROVEN TO FAIL at 78a46f2: nothing under `bins/pl-gui` calls
    /// `pl_abif::parse`, so a `.ab1` had no representation in the app at all.
    ///
    /// The oracle is `pl trace`: the same bytes must give the same base count,
    /// the same ambiguous count and the same mean quality, and the same Mott
    /// window `pl sanger` reports.
    #[test]
    fn an_ab1_parses_to_the_trace_pl_trace_reports_and_the_same_mott_window() {
        let seq = b"ACGTTGCAAGCTTGGATCCAAGGCCTTAAGGCCTTAAGGCC";
        // Ragged at both ends and confident in the middle, which is what a real
        // read looks like and what Mott trimming exists to find.
        let mut q: Vec<u8> = vec![6; seq.len()];
        for x in q.iter_mut().take(34).skip(6) {
            *x = 45;
        }
        let bytes = ab1(seq, &q);
        let t = pl_abif::parse(&bytes).expect("a well-formed ABIF");
        assert_eq!(t.sequence, seq.to_vec());
        assert_eq!(t.ambiguous(), 0);
        assert_eq!(t.base_order, *b"GATC");
        assert_eq!(t.sample_name, "demo-fwd");
        // `Read` must not re-derive the window: it is `reliable_window`'s and
        // the same one `Report::reliable` carries.
        let r = Read::new("demo".into(), None, t);
        let window = r.reliable().expect("qualities are present");
        assert_eq!(
            window,
            pl_sanger::reliable_window(&q, &pl_sanger::Params::default()).expect("a window")
        );
        assert_eq!(window, (7, 34), "the confident stretch, 1-based inclusive");
    }

    /// PROVEN TO FAIL at 78a46f2 (no reads in the GUI), and the assertions are
    /// against `pl_sanger` rather than against this module's own arithmetic.
    ///
    /// Three reads: forward with one planted substitution, the reverse
    /// complement of the same stretch, and one that is not from this plasmid at
    /// all.
    #[test]
    fn what_the_panel_shows_is_what_pl_sanger_reports() {
        let mol = demo();
        let n = mol.len();
        let circular = mol.topology.is_circular();
        let p = pl_sanger::Params::default();

        // 400 bases from the middle, with base 200 of the read changed.
        let mut fwd: Vec<u8> = mol.seq[600..1_000].to_vec();
        let planted = if fwd[199] == b'A' { b'G' } else { b'A' };
        fwd[199] = planted;
        let q = vec![45u8; fwd.len()];
        let rev = pl_core::reverse_complement(&fwd);

        for (label, seq) in [("forward", &fwd), ("reverse", &rev)] {
            let t = pl_abif::parse(&ab1(seq, &q)).expect("well-formed");
            let mut r = Read::new(label.into(), None, t);
            r.compare(&mol.seq, circular);
            done(&mut r);
            let want =
                pl_sanger::compare_reporting(seq, &q, &mol.seq, circular, &p).expect("it places");
            let CompareState::Done(got) = &r.state else {
                panic!("{label}: {}", r.verdict(n));
            };
            assert_eq!(got.reversed, want.reversed, "{label}");
            assert_eq!(got.covered, want.covered, "{label}");
            assert_eq!(
                got.discrepancies.len(),
                want.discrepancies.len(),
                "{label}: {:?}",
                got.discrepancies
            );
            assert_eq!(got.discrepancies, want.discrepancies, "{label}");
            assert_eq!(got.discrepancies.len(), 1, "{label}: the planted change");
            // Coverage is on the line, always, beside the identity — 100%
            // identity over 400 columns says nothing about the other 2,780 bp.
            let v = r.verdict(n);
            assert!(v.contains("covers"), "{v}");
            assert!(v.contains(&format!("of {n} bp")), "{v}");
            assert!(
                v.contains(if label == "reverse" {
                    "reverse strand"
                } else {
                    "forward strand"
                }),
                "{label}: {v}"
            );
        }

        // AND THE REVERSE-STRAND TRAP. `read_base` is on the reference's plus
        // strand while `read_pos` counts in the read as sequenced, so the row
        // and the chromatogram genuinely disagree about the letter — and the
        // panel must say so rather than leave them contradicting each other.
        let t = pl_abif::parse(&ab1(&rev, &q)).expect("well-formed");
        let mut r = Read::new("rev".into(), None, t);
        r.compare(&mol.seq, circular);
        done(&mut r);
        let CompareState::Done(rep) = &r.state else {
            panic!("it places")
        };
        let d = &rep.discrepancies[0];
        let note = read_base_note(rep, d, &rev, r.reliable());
        assert!(note.contains("reverse strand"), "{note}");
        // The read coordinate is on every row, reverse or not — it is the only
        // thing connecting a `ref 800` row to a Mott window stated in read
        // bases. See `read_base_note`.
        assert!(note.contains(&format!("read {}", d.read_pos)), "{note}");
        let on_trace = rev[d.read_pos as usize - 1];
        assert_eq!(
            on_trace,
            pl_core::complement(d.read_base),
            "the letter under the peak is the complement of the row's"
        );
        assert!(note.contains(on_trace as char), "{note}");

        // A READ THAT IS NOT FROM THIS CONSTRUCT. `semiglobal_within` returns
        // an alignment for almost anything, so the honest assertion is not
        // "the panel says NotFound" — it is that the panel says exactly what
        // `pl_sanger` says, including a low identity and a coverage figure, and
        // never a verdict of its own.
        let junk = b"TTTTTTTTTTTTTTTTTTTTGGGGGGGGGGGGGGGGGGGGCCCCCCCCCCCCCCCCCCCC".repeat(4);
        let t = pl_abif::parse(&ab1(&junk, &vec![45u8; junk.len()])).expect("well-formed");
        let mut r = Read::new("junk".into(), None, t);
        r.compare(&mol.seq, circular);
        done(&mut r);
        let want =
            pl_sanger::compare_reporting(&junk, &vec![45u8; junk.len()], &mol.seq, circular, &p);
        match (&r.state, &want) {
            (CompareState::Done(got), Ok(w)) => {
                assert_eq!(got.discrepancies, w.discrepancies);
                assert_eq!(got.covered, w.covered);
                assert!(
                    !got.clean(),
                    "a read that is not this construct came back clean"
                );
            }
            (CompareState::Unplaced(got), Err(w)) => assert_eq!(got, w),
            (a, b) => panic!(
                "the panel and pl_sanger disagree: {} vs {b:?}",
                match a {
                    CompareState::Done(_) => "Done",
                    CompareState::Unplaced(_) => "Unplaced",
                    CompareState::Running { .. } => "Running",
                    CompareState::NoReference => "NoReference",
                    CompareState::Failed(_) => "Failed",
                }
            ),
        }

        // THE READ THAT GENUINELY DOES NOT PLACE, which is `Unplaced::Empty`:
        // a file with base calls the reference has none of. And the three
        // unplaced reasons must not be spelled alike — "we declined to search"
        // is not "we searched and found nothing", and neither is "the clone is
        // wrong".
        assert_eq!(
            pl_sanger::compare_reporting(&fwd, &q, b"", false, &p).err(),
            Some(pl_sanger::Unplaced::Empty)
        );
        let empty = unplaced_sentence(&pl_sanger::Unplaced::Empty);
        let notfound = unplaced_sentence(&pl_sanger::Unplaced::NotFound);
        let refused = unplaced_sentence(&pl_sanger::Unplaced::RefusedTooLarge(
            pl_sanger::AlignError::OutOfMemory { need: 1 },
        ));
        assert!(notfound.contains("Nothing about the construct was checked"));
        assert!(!notfound.to_lowercase().contains("clone is wrong"));
        assert!(refused.contains("DECLINED"), "{refused}");
        assert!(!refused.contains("does not match"), "{refused}");
        assert_ne!(empty, notfound);
        assert_ne!(notfound, refused);
    }

    /// PROVEN TO FAIL against the shipped verdict, twice over.
    ///
    /// `Report::clean` is true whenever every difference is BELOW Q20, so the
    /// ordinary failure — a substitution in a read's low-quality tail — was
    /// headlined "no difference worth acting on", which is the one sentence a
    /// reader takes away and the only one visible on a narrow window. The
    /// qualifier after it then said "1 **more** below Q20", where "more" was
    /// more than a count the clause before it had just asserted was zero.
    ///
    /// And the identity: this prints `{:.2}`, because `cmd_sanger` prints
    /// `{:.2}`. It used to print `{:.1}`, so one read was 99.8% in the panel
    /// and 99.75% at the terminal — the only number the two Sanger surfaces
    /// disagreed on, with nothing to tell a user which had rounded.
    #[test]
    fn a_difference_below_q20_is_reported_rather_than_dismissed() {
        let mol = demo();
        let circular = mol.topology.is_circular();
        // 400 bases from the middle. Confident for 340, ragged after — the
        // shape of every real Sanger read — with the substitution in the tail.
        let mut seq: Vec<u8> = mol.seq[600..1_000].to_vec();
        seq[369] = if seq[369] == b'T' { b'A' } else { b'T' };
        let mut q = vec![45u8; seq.len()];
        for x in q.iter_mut().skip(340) {
            *x = 8;
        }
        let t = pl_abif::parse(&ab1(&seq, &q)).expect("well-formed");
        let mut r = Read::new("tail".into(), None, t);
        r.compare(&mol.seq, circular);
        done(&mut r);
        let CompareState::Done(rep) = &r.state else {
            panic!("it places: {}", r.verdict(mol.len()))
        };
        // The fixture has to be the case this is about: exactly one difference,
        // and `clean()` true because it is below Q20.
        assert_eq!(rep.discrepancies.len(), 1, "{:?}", rep.discrepancies);
        assert_eq!(
            rep.discrepancies[0].confidence,
            pl_sanger::Confidence::Low,
            "the planted change must land in the ragged tail"
        );
        assert!(rep.clean(), "and `clean()` must therefore be true");

        let v = r.verdict(mol.len());
        assert!(!v.contains("no difference worth acting on"), "{v}");
        assert!(!v.contains("more below Q20"), "{v}");
        assert!(v.contains("1 difference(s), every one below Q20"), "{v}");
        assert!(v.contains("as likely to be the read as the clone"), "{v}");

        // TWO DECIMALS, and the same two `pl sanger` prints for this read.
        assert!(
            v.contains(&format!("{:.2}% identity", rep.identity * 100.0)),
            "{v}"
        );
        assert!(v.contains("99.75% identity"), "the CLI's own figure: {v}");

        // AND THE CONTROL. A read with nothing wrong says so in its own words,
        // so "no difference at all" cannot be reached by a read that has one.
        let good: Vec<u8> = mol.seq[600..1_000].to_vec();
        let t = pl_abif::parse(&ab1(&good, &vec![45u8; good.len()])).expect("well-formed");
        let mut r = Read::new("clean".into(), None, t);
        r.compare(&mol.seq, circular);
        done(&mut r);
        let v = r.verdict(mol.len());
        assert!(
            v.contains("no difference at all over the aligned columns"),
            "{v}"
        );
        assert!(v.contains("100.00% identity"), "{v}");
        assert!(!v.contains("below Q20"), "{v}");
    }

    /// A file with no quality values may never produce a tick.
    #[test]
    fn a_file_with_no_qualities_cannot_dismiss_anything() {
        let mol = demo();
        let mut seq: Vec<u8> = mol.seq[600..1_000].to_vec();
        seq[199] = if seq[199] == b'A' { b'G' } else { b'A' };
        let t = pl_abif::parse(&ab1(&seq, b"")).expect("well-formed");
        assert!(t.quality.is_empty());
        let mut r = Read::new("noqual".into(), None, t);
        r.compare(&mol.seq, mol.topology.is_circular());
        done(&mut r);
        let v = r.verdict(mol.len());
        assert!(v.contains("none of them can be dismissed"), "{v}");
        assert!(!v.contains("no difference worth acting on"), "{v}");
        assert!(r.reliable().is_none(), "no qualities, no Mott window");
        assert!(
            r.header().iter().any(|l| l.contains("no quality values")),
            "{:?}",
            r.header()
        );
    }

    /// A damaged file's sentence, pinned on the 68-byte fixture the indexer
    /// already ships. `parse` must refuse it and the refusal must be readable.
    #[test]
    fn the_truncated_fixture_is_refused_with_a_sentence_a_person_can_act_on() {
        let data = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/library-fixture/trace.ab1"
        ))
        .expect("the fixture");
        let e = pl_abif::parse(&data).expect_err("68 bytes is not a chromatogram");
        let s = e.to_string();
        // The sentence sends the user to the right conclusion: the file is
        // not a sequencing read, rather than "parse error".
        assert!(s.contains("no base calls in this file"), "{s}");
        assert!(s.contains("fragment analysis"), "{s}");
    }
}
