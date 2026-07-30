//! The sequence editor's model: caret, selection, coalescing, paste, and the
//! mapping from a gesture to operations.
//!
//! Everything here is plain data and plain functions. Nothing in this file
//! touches egui, so the whole editing model — including the off-by-ones that
//! actually bite — is exercised by ordinary unit tests rather than by driving a
//! window. `main.rs` owns the painting and the input plumbing and calls in.
//!
//! # The one rule
//!
//! **Every change to the bases is an [`OpKind`] handed to [`Document::apply`].**
//! Nothing here writes `Molecule::seq`. That is not stylistic: `OpLog::apply`
//! is where undo, the append-only history, `remap_annotations` and the
//! `WouldCorrupt` gate all live, and a GUI that spliced the sequence itself
//! would bypass every one of them at once while still looking correct on
//! screen.
//!
//! # Two coordinate spaces, named once
//!
//! A **caret** is a `u64` in `0..=n`: the number of bases to its left. It names
//! a *gap*, not a base. Caret 0 is before the first base; caret `n` is after
//! the last. The conversion to an operation is stated in exactly one place,
//! [`Run::to_op`], and it is `at = caret + 1` for an insertion and
//! `start = lo + 1` for a range.
//!
//! A **base coordinate** is 1-based inclusive, as everywhere else in this
//! codebase. `pl-core` reflects bases under reverse complement with
//! `p -> n + 1 - p`; gaps reflect with `c -> n - c`. Two spaces, two formulas,
//! and using the base formula on a caret puts it one base out at every
//! position — see [`transport`].

use std::collections::HashMap;

use pl_core::oplog::{OpId, OpKind};
use pl_core::Molecule;

use crate::doc::{fmt_int, Document};

/// The widest row the view will lay out, and what it uses when it has room.
///
/// Sixty is the GenBank ORIGIN convention and what people read sequence in. It
/// is a *maximum* rather than a constant because it does not fit: sixty
/// monospace cells at 11.5 pt plus this molecule's coordinate gutter is 455.0 pt
/// -- 60 x 6.900 for the cells, 41.0 for the gutter, in IBM Plex Mono at 0.600 em
/// -- and the side panel this view lives in is 380. Read-only that overflow merely
/// clipped the right-hand bases and the ruler, which is how it survived. With a
/// caret it is worse than cosmetic — you cannot click a base you cannot see,
/// and a caret in column 55 gets painted outside the panel — so the row width
/// is measured from the space the view actually has, by [`fit_per_row`], and
/// carried in [`SeqEdit::per_row`] so that the renderer, the click hit-test and
/// Up/Down all read the same number.
pub const MAX_PER_ROW: u64 = 60;

/// How many bases fit, rounded down to a multiple of ten.
///
/// Ten because sequence is read in blocks of ten everywhere else in biology,
/// and a ruler that counts 47 to a row is a ruler nobody can use.
pub fn fit_per_row(bases_width: f32, advance: f32) -> u64 {
    let fits = (bases_width / advance.max(1.0)).floor().max(0.0) as u64;
    (fits / 10 * 10).clamp(10, MAX_PER_ROW)
}

/// The coordinate gutter on the left of every row: the row's first base.
///
/// Measured from the molecule, not fixed at the worst case in the corpus. A
/// constant wide enough for "4,641,652" spends 67.4 pt on every plasmid that
/// will never print more than "8,117"'s 41.0, and those 26.4 pt are not free:
/// they come straight out of the base cells, so a constant gutter pushes the
/// panel width that first reaches a 60-base row up by the same amount — 485.2 pt
/// to 511.6 — and that width is what the map pane gets to keep. The first of
/// those two is measured by bisecting the painter in
/// `the_advance_band_that_keeps_every_per_row_expectation`; the second follows
/// from it by the 26.4 pt difference.
///
/// Every number in this paragraph is a function of the monospace advance and all
/// of them moved when the face did — Hack's 0.602051 em gave 67.6, 41.1, 26.5 and
/// 486.5. They are recorded to one decimal because that is the resolution the
/// bisection has (0.25 pt), not because they are exact.
/// See [`App::DEF_PANEL`](crate::App).
pub fn gutter_w(n: u64, gutter_advance: f32) -> f32 {
    // Digits plus the thousands separators `fmt_int` inserts, because the
    // gutter prints "8,117" and not "8117".
    let digits = if n < 10 { 1 } else { n.ilog10() as usize + 1 };
    let chars = digits + (digits - 1) / 3;
    // 6 pt of air between the number and column 0, which is what the painter's
    // `left_gutter - 6.0` right-alignment assumes, plus 2 pt of slack so a
    // fractional advance never shaves the leading digit.
    (chars as f32 * gutter_advance.max(1.0) + 8.0).max(24.0)
}

/// The row's last coordinate, on the right, when there is width to spare.
///
/// Nine monospace cells holds "4,641,652" — the largest coordinate in the
/// benchmark corpus — plus a little air: 70.1 pt at IBM Plex Mono's 6.900 pt
/// advance, which is when the right-hand coordinate first appears on an 8,117 bp
/// molecule at a panel width of about 555 pt. Not asserted for a real face — the
/// unit tests pass a literal advance — so a face change moves this silently.
fn right_gutter_w(advance: f32) -> f32 {
    9.0 * advance + 8.0
}

/// Every horizontal number the sequence grid uses, in one place.
///
/// Four things derive an x from a column or a column from an x: the painter,
/// the click hit-test (which the drag-selection also routes through), the caret
/// rectangle and the selection rectangle. They used to compute it inline, four
/// times, and they agreed only because the formula was `col * advance` in all
/// four. The moment anyone groups the bases in tens by inserting a real gap,
/// three of them are still right and one is wrong, and the error is zero at
/// column 0 and five whole cells at column 55 — right in a screenshot, wrong at
/// base 47, wrong by a codon and a half at the end of the row.
///
/// So [`RowLayout::col_x`] is the only producer of an x and
/// [`RowLayout::x_col`] the only consumer, and
/// `every_column_round_trips_including_the_last_gap_of_a_full_row` asserts they
/// are exact inverses over the whole legal domain — including `per_row` itself,
/// which names the gap after the row's last base.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowLayout {
    pub per_row: u64,
    pub left_gutter: f32,
    /// Zero unless the row has already reached [`MAX_PER_ROW`] and there is
    /// still width left over. Surplus width buys the gutter; base cells never
    /// do — "4,641,652" costs eight cells, which would take a 380 pt panel from
    /// 40 bases per row to 32.
    pub right_gutter: f32,
    /// Distance from the band's left edge to column 0.
    pub bases_x: f32,
    pub advance: f32,
}

/// Measure a row from the width the view actually has.
pub fn row_layout(avail_w: f32, advance: f32, scrollbar: f32, left_gutter: f32) -> RowLayout {
    let advance = advance.max(1.0);
    let usable = avail_w - left_gutter - scrollbar;
    let per_row = fit_per_row(usable, advance);
    let rg = right_gutter_w(advance);
    // The right gutter is asked for only once the row is already as wide as it
    // is ever going to get, so it can never cost a base.
    let right_gutter = if per_row >= MAX_PER_ROW && usable - per_row as f32 * advance >= rg {
        rg
    } else {
        0.0
    };
    RowLayout {
        per_row,
        left_gutter,
        right_gutter,
        bases_x: left_gutter,
        advance,
    }
}

impl RowLayout {
    /// The left edge of column `col`, measured from the band's left edge.
    ///
    /// `col == per_row` is legal and names the gap after the last base, which is
    /// where the caret sits at the end of a row.
    pub fn col_x(&self, col: u64) -> f32 {
        self.bases_x + col as f32 * self.advance
    }

    /// The column nearest `dx`, measured from **column 0**, not from the band.
    ///
    /// Clamped to `0..=per_row`: a click in the empty space that appears to the
    /// right of a full row at wide panel settings places the caret at the
    /// end-of-row gap, the same as clicking past the end of a line in any text
    /// editor. It must never return `per_row + 1`.
    pub fn x_col(&self, dx: f32) -> u64 {
        ((dx / self.advance).round() as i64).clamp(0, self.per_row as i64) as u64
    }

    /// The **cell** `dx` is inside, or `None` if it is not inside one.
    ///
    /// A different question from [`x_col`](Self::x_col) and it must stay a
    /// different function. A caret is a gap, so it ROUNDS to the nearer
    /// boundary — that is what makes clicking near the right of a glyph put the
    /// caret after it, the way every text editor behaves. A pointer *reading* a
    /// base is asking which cell it is over, which is a FLOOR into that cell.
    ///
    /// Sharing one mapping between the two was wrong over the right half of
    /// every cell: hovering the right of base 585's glyph — visibly under the
    /// `pSC101 ori` ribbon — named base 586 and reported no feature, and at the
    /// last column of a row it named the first base of the *next* row. The
    /// hover line is the non-colour channel for every ribbon above it, so it
    /// contradicted the drawing exactly where a boundary was being read.
    ///
    /// `None` rather than a clamp, for the same reason: past the last cell
    /// there is no base under the pointer, and answering with the nearest one
    /// is the same lie in a different place.
    pub fn x_base(&self, dx: f32) -> Option<u64> {
        if dx < 0.0 {
            return None;
        }
        let c = (dx / self.advance).floor();
        (c < self.per_row as f32).then_some(c as u64)
    }

    /// Width of the cells themselves.
    pub fn band_w(&self) -> f32 {
        self.per_row as f32 * self.advance
    }
}

/// The row `base` lands on at this row width.
///
/// The scroll is restored through this rather than kept in pixels because
/// `ScrollArea::show_rows` maps a pixel offset to a row index and then
/// multiplies by `per_row`: the offset does not change when `per_row` does, so
/// the base at the top of the viewport jumps by the ratio. Measured on the
/// user's 8,117 bp file: scrolled to base 4,000 at 40 per row is offset 1,330;
/// the same offset at 60 per row is base 6,000. The view jumps 2,000 bases
/// forward *while the user is dragging a splitter*, and it is not reversible,
/// because the content shrinks from 2,700 pt to 1,809 and an offset near the
/// bottom is clamped on the way out and not restored on the way back.
pub fn row_of(base: u64, per_row: u64) -> u64 {
    base / per_row.max(1)
}

/// A position *between* bases. See the module docs.
pub type Caret = u64;

// ---------------------------------------------------------------------------
// Where editing is offered at all
// ---------------------------------------------------------------------------

/// Whether this document has a caret space, and if not, why not.
///
/// The three "no bases" cases are genuinely different files and get genuinely
/// different sentences. The engine refuses two of them anyway, but badly: a
/// keystroke on an annotation track measured as *"feature 0 'orphan' segment 0
/// start: 101 is past the 1 bp molecule"*, which is the gate reporting a
/// symptom of a question that should never have been asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Editability {
    /// There is a caret space. Includes the genuinely empty document, whose
    /// caret space is `{0}` — refusing that would mean the editor cannot start
    /// a sequence from nothing, which is arbitrary.
    Editable,
    /// Features, no bases, no declared length. The coordinates belong to a
    /// sequence held somewhere else.
    AnnotationTrack { features: usize },
    /// A declared length and none of the bases it names.
    SequenceAbsent { declared: u64 },
}

impl Editability {
    pub fn of(mol: &Molecule) -> Self {
        if mol.is_annotation_track() {
            Editability::AnnotationTrack {
                features: mol.features.len(),
            }
        } else if mol.sequence_absent() {
            Editability::SequenceAbsent {
                declared: mol.declared_len.unwrap_or(0),
            }
        } else {
            Editability::Editable
        }
    }

    pub fn is_editable(&self) -> bool {
        matches!(self, Editability::Editable)
    }

    /// The sentence shown where the caret would have been, in the user's terms
    /// rather than the gate's.
    pub fn refusal(&self) -> Option<String> {
        match self {
            Editability::Editable => None,
            Editability::AnnotationTrack { features } => Some(format!(
                "This file is an annotation track: it carries {} feature{} and no bases, \
                 so there is nothing here to edit. Open the sequence these coordinates \
                 describe and apply them to it.",
                fmt_int(*features as u64),
                if *features == 1 { "" } else { "s" }
            )),
            Editability::SequenceAbsent { declared } => Some(format!(
                "This file declares {} bases and carries none of them — annotation-only \
                 GenBank. There is nothing here to edit.",
                fmt_int(*declared)
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Selection
// ---------------------------------------------------------------------------

/// A selected arc of the molecule.
///
/// `anchor` and `head` are kept rather than normalised to `[lo, hi]` because
/// Shift+Arrow has to grow and shrink from the end the user is holding; a
/// normalised pair makes Shift+Left after a rightward drag extend the wrong
/// end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// The end that stays put while dragging.
    pub anchor: Caret,
    /// The end that moves. The caret is here.
    pub head: Caret,
    /// True when the selected arc runs from the LOWER caret *backwards through
    /// the origin* to the higher one — the complement of `[lo, hi]`.
    ///
    /// This bit is necessary because a pair of carets on a circle names two
    /// arcs, not one: `(40, 4961)` on a 5,386 bp plasmid is either the 4,921
    /// bases between them or the 465 across the origin, and no *ordering* of
    /// the pair distinguishes them — swapping anchor and head only records
    /// which end the user grabbed first. Only ever true on a circle.
    pub through_origin: bool,
}

impl Selection {
    pub fn point(c: Caret) -> Self {
        Selection {
            anchor: c,
            head: c,
            through_origin: false,
        }
    }

    pub fn lo(&self) -> Caret {
        self.anchor.min(self.head)
    }
    pub fn hi(&self) -> Caret {
        self.anchor.max(self.head)
    }

    /// Fit the selection to this molecule without deciding which arc is meant.
    ///
    /// This is what a *store* uses. [`Selection::canonical`] is what a
    /// *consumer* uses, and the two must not be confused: canonical form is
    /// lossy about direction of travel, so storing it back is what broke every
    /// incremental origin-crossing gesture in this surface. On a 12 bp circle,
    /// Shift+Right at caret `n` builds `{anchor: 12, head: 0, wrap}`; canonical
    /// form collapses that to the empty selection, so the documented gesture
    /// selected nothing at all — and the next keypress read the collapsed value
    /// back and extended the arc the other way round, giving the *complement*
    /// of what was highlighted. One Backspace there takes the rest of the
    /// plasmid. See `shift_right_past_the_end_of_a_circle_adds_the_first_base`.
    pub fn clamped(mut self, n: u64, circular: bool) -> Self {
        self.anchor = self.anchor.min(n);
        self.head = self.head.min(n);
        if !circular {
            // A line has no origin to cross.
            self.through_origin = false;
        }
        self
    }

    /// Put the selection into the only form the op derivation can read.
    ///
    /// Without this, a through-origin selection whose high end sits at `n`
    /// produces `origin = hi + 1 = n + 1`, which `Rotate` refuses ("origin at
    /// 13 is outside a 12 bp molecule") — a refusal the user would experience
    /// as the editor breaking on an ordinary selection at the end of the
    /// sequence.
    ///
    /// **Never store the result.** Both collapses below throw away the fact
    /// that the user travelled across the origin, which the next keystroke
    /// needs; they are correct only as the last step before the arc is turned
    /// into operations, bases, or pixels. See [`Selection::clamped`].
    pub fn canonical(mut self, n: u64, circular: bool) -> Self {
        self = self.clamped(n, circular);
        if !self.through_origin {
            return self;
        }
        let (lo, hi) = (self.lo(), self.hi());
        if hi >= n {
            // The arc {n+1..n} u {1..lo} is just bases 1..=lo.
            self.anchor = 0;
            self.head = lo;
            self.through_origin = false;
        } else if lo == 0 {
            // The arc is just bases hi+1..=n.
            self.anchor = hi;
            self.head = n;
            self.through_origin = false;
        }
        self
    }

    /// How many BASES are selected — not the caret difference, which reports
    /// 4,921 for a 465 bp origin-crossing selection.
    pub fn base_count(&self, n: u64) -> u64 {
        let span = self.hi() - self.lo();
        if self.through_origin {
            n - span
        } else {
            span
        }
    }

    pub fn is_empty(&self, n: u64) -> bool {
        self.base_count(n) == 0
    }
}

// ---------------------------------------------------------------------------
// The pending typing run
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    Insert,
    DeleteBack,
    DeleteForward,
}

/// Keystrokes not yet committed to the log.
///
/// One run is one thing the user would call "what I just typed", and it becomes
/// exactly one operation. The alternative — one operation per keystroke —
/// measured on a 4,641,652 bp molecule carrying 9,000 features at 4.4 ms of
/// main-thread work per keystroke: 100 keystrokes cost 442 ms and 500 cost
/// 2,097 ms, against 13 ms and 12 ms for the same keystrokes as one operation.
/// Memory goes the same way. The log keeps a snapshot every
/// `SNAPSHOT_EVERY = 50` operations and, by its central promise, never evicts
/// one, so at one operation per keystroke that is a 4.6 MB molecule retained
/// per fifty keystrokes — about 93 kB each, for as long as the document is
/// open.
///
/// That was not a hypothetical cost. It is what this application paid until
/// `App::autosave` stopped settling the run at the top of every frame: a run
/// that lives one frame is not a run, and while it did, every mechanism below —
/// `IDLE_SECONDS`, `MAX_CHARS`, the repaint timer that closes an idle run, the
/// "typing" indicator — was unreachable code.
///
/// The cost of buffering is that between keystrokes the log is one run behind
/// the screen. That is the shadow-buffer failure mode, and it is bounded here
/// by committing before anything can observe the document — see
/// [`SeqEdit::commit`] and its callers, and `Run::IDLE_SECONDS`.
///
/// Uniform shape on purpose: a run replaces `removed` committed bases starting
/// at gap `start` with `inserted`. Typing is `removed == 0`, backspacing walks
/// `start` down while `removed` grows, forward-delete grows `removed` alone.
#[derive(Debug, Clone)]
pub struct Run {
    /// Gap in the COMMITTED molecule where the replaced region begins.
    pub start: Caret,
    /// How many committed bases this run removes.
    pub removed: u64,
    /// What it puts there, byte for byte as typed.
    pub inserted: String,
    pub kind: RunKind,
    /// `ctx.input(|i| i.time)` at the last keystroke.
    pub last_input: f64,
}

impl Run {
    /// A pause to think is an undo boundary; the gap between two words is not.
    pub const IDLE_SECONDS: f64 = 1.0;

    /// So one Ctrl+Z never swallows an unbounded amount of typing.
    ///
    /// Not a performance limit: a 500-base `InsertAt` costs the same 3.85 ms at
    /// 4.6 Mb as a 1-base one, because the cost is the defensive
    /// `Molecule::clone` and not the payload.
    pub const MAX_CHARS: usize = 500;

    /// The single operation this run becomes, or `None` if it does nothing.
    ///
    /// Three shapes are deliberately never emitted:
    /// `DeleteRange { len: 0 }` (refused by the engine);
    /// `ReplaceRange { len: 0 }` (accepted, identical in effect to `InsertAt`,
    /// but it derives a different id and describes itself as "replace 0 bp at 5
    /// with 1 bp" in a provenance log that is never rewritten); and
    /// `InsertAt { seq: "" }` (accepted — a no-op that still records an
    /// operation and advances the cursor).
    pub fn to_op(&self) -> Option<OpKind> {
        match (self.removed, self.inserted.is_empty()) {
            (0, true) => None,
            (0, false) => Some(OpKind::InsertAt {
                at: self.start + 1,
                seq: self.inserted.clone(),
            }),
            (len, true) => Some(OpKind::DeleteRange {
                start: self.start + 1,
                len,
            }),
            (len, false) => Some(OpKind::ReplaceRange {
                start: self.start + 1,
                len,
                seq: self.inserted.clone(),
            }),
        }
    }

    /// Where the caret sits with this run applied.
    pub fn caret_after(&self) -> Caret {
        self.start + self.inserted.len() as u64
    }

    /// The three numbers the annotation preview needs, and nothing else.
    pub fn span(&self) -> crate::annot::RunSpan {
        crate::annot::RunSpan {
            start: self.start,
            removed: self.removed,
            inserted: self.inserted.len() as u64,
        }
    }

    pub fn is_full(&self) -> bool {
        self.inserted.len() >= Self::MAX_CHARS || self.removed as usize >= Self::MAX_CHARS
    }

    pub fn is_idle(&self, now: f64) -> bool {
        now - self.last_input >= Self::IDLE_SECONDS
    }
}

// ---------------------------------------------------------------------------
// Paste
// ---------------------------------------------------------------------------

/// A character the paste pipeline will not insert and will not silently drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    pub ch: char,
    pub count: usize,
    /// 1-based character offset of the first occurrence, in the text left
    /// after invisible characters and recognised structure were removed.
    pub first_at: usize,
}

/// What a paste would insert, and everything it would throw away.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PasteReport {
    /// The bases, exactly as written. Case is never touched.
    pub bases: String,
    /// Whitespace and recognised structure: droppable without asking, but said.
    pub dropped: Vec<String>,
    /// Anything else, at most [`MAX_REJECTED_KINDS`] of them, earliest first.
    /// Nothing is inserted until the user consents.
    pub rejected: Vec<Rejected>,
    /// How many *distinct* characters were rejected, including the ones past
    /// the cap.
    pub rejected_kinds: usize,
    /// How many characters in total were rejected.
    pub rejected_total: usize,
    /// The whole paste is refused and there is nothing to offer.
    pub refused: Option<String>,
    /// U or u present: accepted, but `Reverse complement` will rewrite it as
    /// DNA, because `pl-core` has one alphabet and `Molecule` has no field to
    /// say otherwise.
    pub uracil: usize,
    /// Accepted bases that are neither ACGT nor U.
    pub ambiguous: usize,
    /// The paste looks like text rather than sequence, and why.
    pub suspect: Option<String>,
}

impl PasteReport {
    /// One line accounting for everything that was removed.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "pasted {} base{}",
            fmt_int(self.bases.len() as u64),
            if self.bases.len() == 1 { "" } else { "s" }
        );
        if !self.dropped.is_empty() {
            s.push_str(" · dropped ");
            s.push_str(&self.dropped.join(", "));
        }
        if self.uracil > 0 {
            s.push_str(&format!(
                " · includes {} U — this is RNA; Reverse complement will rewrite it as DNA",
                fmt_int(self.uracil as u64)
            ));
        }
        // Said on every paste that has any, not only on the ones that ask.
        // "pasted 15 bases · dropped 4 layout characters" is what
        // "that was a bad hack" produced, and it reads like a clean paste.
        if self.ambiguous > 0 {
            s.push_str(&format!(
                " · {} of them ambiguity codes",
                fmt_int(self.ambiguous as u64)
            ));
        }
        s
    }

    /// Must the user be asked before any of this is inserted?
    pub fn needs_consent(&self) -> bool {
        !self.rejected.is_empty() || self.suspect.is_some()
    }

    /// The consent question, when there is one.
    pub fn consent_question(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        if let Some(s) = &self.suspect {
            lines.push(s.clone());
        }
        if !self.rejected.is_empty() {
            lines.push("Characters that are not nucleotide codes:".into());
            lines.extend(self.rejected.iter().take(12).map(|r| {
                format!(
                    "U+{:04X} '{}' ×{}, first at position {}",
                    r.ch as u32, r.ch, r.count, r.first_at
                )
            }));
            if self.rejected_kinds > 12 {
                lines.push(format!("and {} more kinds", self.rejected_kinds - 12));
            }
        }
        lines.join("\n")
    }
}

/// How many distinct rejected characters are kept for the dialog.
///
/// The dialog lists twelve and counts the rest, so a tally that grows without
/// limit buys nothing: 605 kB of CJK text off a web page has 20,992 distinct
/// characters, and keeping a struct for each of them to display twelve is the
/// cheap half of the same mistake as scanning a `Vec` to find them.
pub const MAX_REJECTED_KINDS: usize = 64;

/// Above this fraction of ambiguity codes, a plain-text paste is confirmed.
///
/// Sixteen of the twenty-six letters are IUPAC codes, so ordinary prose is
/// *silently* a sequence: "that was a bad hack" sanitises to the 15 bases
/// "thatwasabadhack" with nothing rejected and no dialog at all. Real DNA does
/// not look like that — 40% of those characters are ambiguity codes, and a
/// composition above about a fifth is either a degenerate oligo, which is worth
/// one confirming click, or English, which is worth stopping.
///
/// U is deliberately not counted: an RNA paste is 25% U and is not suspicious.
pub const AMBIGUITY_LIMIT: f64 = 0.2;

/// Below this, the user can see what they pasted and does not need telling.
pub const AMBIGUITY_FLOOR: usize = 10;

/// Above this, a paste is confirmed rather than performed.
///
/// An INTENT guard, not a performance guard — saying which decides where the
/// threshold goes, and conflating them produces a limit that is both annoying
/// and useless. A 4 Mb paste is one operation costing a few milliseconds plus a
/// memmove; nothing hangs. What it does is make the document 800x longer.
///
/// The largest thing pasted deliberately in routine cloning is a gene block or
/// a whole insert, at most ~20 kb, into a vector of 3–20 kb. 50 kb sits above
/// every routine paste and orders of magnitude below any genome.
pub const LARGE_PASTE: u64 = 50_000;

/// Would this paste surprise the person who made it?
///
/// The ratio clause earns its keep on the case the absolute number misses: a
/// 20 kb paste into a 2 kb fragment looks innocent by size and is almost
/// certainly a mistake.
pub fn is_a_lot(pasted: u64, document: u64) -> bool {
    pasted > LARGE_PASTE || (document > 0 && pasted > 3 * document)
}

/// Characters that are in the text and not on the screen.
///
/// They come out of Word documents and vendor spec sheets, and this class has
/// already cost this project once: `docs/AUDIT-2026-07.md` records an NBSP from
/// a vendor sheet panicking `pl-clone`'s PCR on a char boundary. The user
/// cannot see they are there, so a count is the only honest report.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00A0}'
            | '\u{202F}'
            | '\u{2009}'
            | '\u{200B}'
            | '\u{200C}'
            | '\u{200D}'
            | '\u{2060}'
            | '\u{FEFF}'
            | '\u{00AD}'
    )
}

/// Is this character a nucleotide code?
///
/// The acceptance predicate, written once. `pl_core::iupac::code_mask` already
/// folds case and already knows about U, so there is no second alphabet table
/// anywhere in this crate.
pub fn is_base(c: char) -> bool {
    c.is_ascii() && pl_core::iupac::code_mask(c as u8) != 0
}

/// Which line is this text's GenBank `ORIGIN`, if it has one.
///
/// Corroborated, because believing it on `starts_with("ORIGIN")` alone is
/// expensive in exactly one direction: everything ABOVE the match is discarded
/// as header and digit-stripping is switched on for everything below. Measured:
/// pasting "GAATTCACGT\nORIGINAL CLONE, 2019\nGGATCCACGT" kept ten bases and
/// silently dropped the other ten, with `rejected` empty so no dialog was
/// raised. The line must be `ORIGIN` on its own (GenBank's is, with only
/// whitespace after it), and either a LOCUS/FEATURES line above it or a `//`
/// terminator below.
fn genbank_origin_line(text: &str) -> Option<usize> {
    let lines: Vec<&str> = text.lines().collect();
    let oi = lines.iter().position(|l| l.trim() == "ORIGIN")?;
    let header = lines[..oi]
        .iter()
        .any(|l| l.starts_with("LOCUS") || l.starts_with("FEATURES"));
    let terminated = lines[oi + 1..]
        .iter()
        .any(|l| l.trim_start().starts_with("//"));
    (header || terminated).then_some(oi)
}

/// Everything a pasted string would put into the sequence, and everything it
/// would not.
///
/// Governing rule: whitespace and *recognised structure* may be dropped
/// silently, because they were never bases. Every other dropped character needs
/// consent. Silently dropping the character you did not expect is how a user
/// ends up with a plasmid missing three bases and no record of when.
pub fn sanitise_paste(text: &str) -> PasteReport {
    let mut report = PasteReport::default();

    // Stage 1: invisibles, removed before the structure test so an NBSP inside
    // "ORIGIN" or a FASTA header cannot defeat it.
    let invis = text.chars().filter(|c| is_invisible(*c)).count();
    let text: String = text.chars().filter(|c| !is_invisible(*c)).collect();
    if invis > 0 {
        report.dropped.push(format!(
            "{invis} invisible character{}",
            if invis == 1 { "" } else { "s" }
        ));
    }

    // Stage 2: structure, decided on the whole text before any character
    // filtering, because whether a digit is a coordinate or junk is a fact
    // about the surrounding format and not about the digit.
    let fasta_headers = text.lines().filter(|l| l.starts_with('>')).count();
    let mut strip_digits = false;
    let body: String = if fasta_headers > 1 {
        report.refused = Some(format!(
            "That is {fasta_headers} FASTA records. Concatenating them would fabricate a \
             chimera nobody asked for. Paste one at a time."
        ));
        return report;
    } else if fasta_headers == 1 || text.lines().any(|l| l.starts_with(';')) {
        for l in text
            .lines()
            .filter(|l| l.starts_with('>') || l.starts_with(';'))
        {
            report.dropped.push(format!("header: {}", l.trim_end()));
        }
        text.lines()
            .filter(|l| !l.starts_with('>') && !l.starts_with(';'))
            .collect::<Vec<_>>()
            .join("")
    } else if let Some(oi) = genbank_origin_line(&text) {
        // More than one record, refused for the same reason the FASTA branch
        // above refuses it, and reachable the same way: "Send to -> File" at
        // NCBI and every multi-record export produce these.
        //
        // Without this, `genbank_origin_line` found the FIRST `ORIGIN` and the
        // `take_while` below stopped at the FIRST `//`, so record 2 was dropped
        // whole — its bases counted by nothing, `rejected` empty, `refused`
        // None, so `needs_consent` was false and the paste went in with no
        // dialog. Worse than silence: the notice was byte-identical to a
        // single-record paste and asserted "this is a whole GenBank record",
        // singular, which is a claim the code could not support. `is_a_lot`
        // could not rescue it either, because it is measured on the already
        // truncated `report.bases`.
        //
        // Both counts, not just `ORIGIN`: a record whose own `//` is missing
        // lets the `take_while` run on into the next record's header and turn
        // its letters into bases, and the terminator count catches that shape
        // even when only one `ORIGIN` line is present.
        let origins = text.lines().filter(|l| l.trim() == "ORIGIN").count();
        let terminators = text
            .lines()
            .filter(|l| l.trim_start().starts_with("//"))
            .count();
        let records = origins.max(terminators);
        if records > 1 {
            report.refused = Some(format!(
                "That is {records} GenBank records. Concatenating them would fabricate a \
                 chimera nobody asked for. Paste one at a time."
            ));
            return report;
        }
        // A GenBank ORIGIN block, read by exactly the reader's own rule
        // (`genbank.rs`: drop ASCII whitespace and ASCII digits). Reusing the
        // rule rather than restating it is the point — a second copy drifts.
        strip_digits = true;
        let block: Vec<&str> = text
            .lines()
            .skip(oi + 1)
            .take_while(|l| !l.trim_start().starts_with("//"))
            .collect();
        let digits: usize = block
            .iter()
            .map(|l| l.matches(char::is_numeric).count())
            .sum();
        report.dropped.push(format!(
            "read as a GenBank ORIGIN block; {} position number character{}",
            fmt_int(digits as u64),
            if digits == 1 { "" } else { "s" }
        ));
        // Everything above ORIGIN is header, and a header is not bases — but
        // if the paste began with bases, saying only how many *digits* went is
        // an accounting line that misses the thing that actually vanished.
        let above: usize = text
            .lines()
            .take(oi)
            .flat_map(|l| l.chars())
            .filter(|c| is_base(*c))
            .count();
        if above > 0 {
            report.dropped.push(format!(
                "{} base-like character{} above the ORIGIN line, as header",
                fmt_int(above as u64),
                if above == 1 { "" } else { "s" }
            ));
        }
        if text.contains("LOCUS") || text.contains("FEATURES") {
            report.dropped.push(
                "this is a whole GenBank record — only its bases are pasted, not its \
                 features or qualifiers; open it as a file to keep them"
                    .into(),
            );
        }
        block.join("")
    } else {
        // Plain text. DIGITS ARE NOT DROPPED HERE, and that asymmetry is the
        // heart of this stage: stripping them is safe only where the structure
        // says they are coordinates. `ACGT1234` is junk or a truncated
        // identifier, and unconditional digit-stripping is how a number in a
        // pasted note vanishes with nothing to notice.
        let lines = text.lines().filter(|l| !l.trim().is_empty()).count();
        if lines > 1 {
            // A wrapped body copied out of Word and a spreadsheet column of
            // separate oligos are textually indistinguishable. Do not guess:
            // make the join visible and one Ctrl+Z away.
            report.dropped.push(format!("joined {lines} lines"));
        }
        text.to_string()
    };

    // Stage 3: the character filter on what survived.
    //
    // The tally is keyed rather than scanned. It was a `Vec` searched linearly
    // per character, which is O(characters × distinct kinds) — and the kind
    // count is not small for text that is not sequence: a page of CJK off the
    // web runs to twenty thousand distinct characters, so each of a million
    // pasted characters walked a twenty-thousand-entry vector. This runs on the
    // UI thread inside the frame that handled Ctrl+V, before any size gate is
    // consulted, so the window simply stops.
    let mut tally: HashMap<char, (usize, usize)> = HashMap::new();
    let mut ws = 0usize;
    let mut stars = 0usize;
    for (i, c) in body.chars().enumerate() {
        if c.is_ascii_whitespace() {
            ws += 1;
        } else if c == '*' {
            // The translation stop marker; `fasta::parse` drops it too. Counted
            // apart from whitespace: calling a stop codon a "layout character"
            // is a small lie in a line whose whole purpose is to be exact.
            stars += 1;
        } else if strip_digits && c.is_ascii_digit() {
            // Already accounted for above.
        } else if is_base(c) {
            if c == 'U' || c == 'u' {
                report.uracil += 1;
            } else if !matches!(c, 'A' | 'C' | 'G' | 'T' | 'a' | 'c' | 'g' | 't') {
                report.ambiguous += 1;
            }
            report.bases.push(c);
        } else {
            // No transliteration. Folding a fullwidth Ａ to A or an en dash to
            // a hyphen is a guess that rewrites the user's characters, which is
            // the same class of fabrication `pl-core` refuses by name when it
            // leaves a past-the-end coordinate alone rather than clamping it
            // onto a real base.
            report.rejected_total += 1;
            let e = tally.entry(c).or_insert((0, i + 1));
            e.0 += 1;
        }
    }
    if ws > 0 {
        report.dropped.push(format!(
            "{} layout character{}",
            ws,
            if ws == 1 { "" } else { "s" }
        ));
    }
    if stars > 0 {
        report.dropped.push(format!(
            "{stars} translation stop marker{}",
            if stars == 1 { "" } else { "s" }
        ));
    }
    report.rejected_kinds = tally.len();
    let mut kinds: Vec<Rejected> = tally
        .into_iter()
        .map(|(ch, (count, first_at))| Rejected {
            ch,
            count,
            first_at,
        })
        .collect();
    // A `HashMap` has no order and the dialog quotes positions, so restore the
    // one the user would recognise: where each character first appears.
    kinds.sort_by_key(|r| r.first_at);
    kinds.truncate(MAX_REJECTED_KINDS);
    report.rejected = kinds;

    // Is this sequence at all? Only asked of plain text: a FASTA record and an
    // ORIGIN block have already said what they are.
    if !strip_digits && fasta_headers == 0 {
        let n = report.bases.len();
        if n >= AMBIGUITY_FLOOR && (report.ambiguous as f64) > AMBIGUITY_LIMIT * n as f64 {
            report.suspect = Some(format!(
                "{} of these {} characters are ambiguity codes rather than A, C, G or T. \
                 Prose pastes as sequence — sixteen of the twenty-six letters are IUPAC \
                 codes — so this may be text rather than DNA.",
                fmt_int(report.ambiguous as u64),
                fmt_int(n as u64)
            ));
        }
    }
    report
}

// ---------------------------------------------------------------------------
// Deriving operations from a gesture
// ---------------------------------------------------------------------------

/// The operations a delete-or-replace of `sel` becomes, and where the caret
/// lands.
///
/// A through-origin selection is two disjoint ranges in the current numbering,
/// and `OpKind` has no wrapping range op — `apply` refuses
/// `start + len - 1 > n`. So it is expressed as **rotate, then one range op**:
/// rotate the arc to the front, then delete or replace `1..=l`.
///
/// The rejected alternative — two `DeleteRange`s, high half first — produces
/// the identical final document and is still wrong. The intermediate state is a
/// plasmid missing half of what the user asked to lose, which they never asked
/// for and never saw; the two ops are gated independently, so the second can be
/// refused after the first is committed, leaving an abandoned half-deletion in
/// an append-only provenance record forever; and the `WouldCorrupt` tally for
/// the second is computed against that intermediate, where an origin-crossing
/// feature has already lost half its bases.
///
/// No rotate-back: the deletion consumes the origin itself, so "put the origin
/// back" names a base that no longer exists. The caller must say so — the
/// plasmid really has been renumbered.
pub fn ops_for_range_edit(
    mol: &Molecule,
    sel: Selection,
    replacement: Option<&str>,
) -> (Vec<OpKind>, Caret) {
    let n = mol.len();
    let sel = sel.canonical(n, mol.topology.is_circular());
    let (lo, hi) = (sel.lo(), sel.hi());

    if !sel.through_origin {
        let len = hi - lo;
        let caret = lo + replacement.map_or(0, |s| s.len() as u64);
        let ops = match (len, replacement) {
            (0, None) => Vec::new(),
            (0, Some("")) => Vec::new(),
            (0, Some(s)) => vec![OpKind::InsertAt {
                at: lo + 1,
                seq: s.to_string(),
            }],
            (len, None) => vec![OpKind::DeleteRange { start: lo + 1, len }],
            (len, Some("")) => vec![OpKind::DeleteRange { start: lo + 1, len }],
            (len, Some(s)) => vec![OpKind::ReplaceRange {
                start: lo + 1,
                len,
                seq: s.to_string(),
            }],
        };
        return (ops, caret);
    }

    // Crossing the origin. `first` is the arc's first base in reading order,
    // `l` its length.
    let first = hi + 1;
    let l = n - (hi - lo);
    let rotate = OpKind::Rotate { origin: first };
    let (second, caret) = match replacement {
        None => (OpKind::DeleteRange { start: 1, len: l }, 0),
        Some("") => (OpKind::DeleteRange { start: 1, len: l }, 0),
        Some(s) => (
            OpKind::ReplaceRange {
                start: 1,
                len: l,
                seq: s.to_string(),
            },
            s.len() as u64,
        ),
    };
    (vec![rotate, second], caret)
}

/// Rehearse a whole gesture before committing any of it.
///
/// A two-op gesture must never half-apply, and a refusal should arrive while
/// the user still knows what they pressed. So this runs on the origin-crossing
/// path *and* whenever a typing or deleting run opens.
///
/// It uses the engine's own gate — `pl_core::oplog::refuse_new_problems`, the
/// same call `OpLog::apply` makes — rather than reimplementing the per-kind
/// `Invalid` tally in the GUI, which is the version of this that rots.
///
/// One clone of the molecule, and one `validate` either side. It used to build a
/// throwaway `OpLog`, which clones the base into `current`, again into
/// `snapshots[None]`, and a fourth time inside `apply`. Measured on a
/// 4,641,652 bp molecule carrying 9,000 features: 14.8 ms then, 5.0 ms now,
/// against a 3.2 ms `Molecule::clone` that is the irreducible part. The
/// docstring here claimed "about 4 ms ... only on the origin-crossing path" and
/// was wrong about both.
pub fn preflight(mol: &Molecule, ops: &[OpKind]) -> Result<(), String> {
    let mut trial = mol.clone();
    let mut was = pl_core::oplog::problem_tally(mol);
    for (i, op) in ops.iter().enumerate() {
        let fail = |e: pl_core::oplog::OpError| format!("cannot {}: {e}", op.describe());
        pl_core::oplog::apply(&mut trial, op).map_err(fail)?;
        pl_core::oplog::refuse_new_problems(&was, &trial).map_err(fail)?;
        // The last operation's result is never compared against anything, and
        // `validate` at 4.6 Mb is not free.
        if i + 1 < ops.len() {
            was = pl_core::oplog::problem_tally(&trial);
        }
    }
    Ok(())
}

/// Where a caret goes when an operation the editor did not issue moves the
/// bases under it.
///
/// Returns the new caret; selections are collapsed by the caller wherever the
/// arc they name might not survive.
pub fn transport(caret: Caret, op: &OpKind, n_before: u64) -> Caret {
    match op {
        // Position matters. A caret before the edit does not move at all, and
        // moving it anyway teleports it to the edit site: `transport(10,
        // DeleteRange { start: 50, len: 10 }, 100)` answered 49.
        OpKind::InsertAt { at, seq } => {
            let gap = at.saturating_sub(1);
            if caret < gap {
                caret
            } else {
                caret + seq.len() as u64
            }
        }
        OpKind::DeleteRange { start, len } => {
            let gap = start.saturating_sub(1);
            if caret <= gap {
                caret
            } else if caret >= gap + len {
                caret - len
            } else {
                // It was inside what went; the near edge is the only place left.
                gap
            }
        }
        OpKind::ReplaceRange { start, len, seq } => {
            let gap = start.saturating_sub(1);
            if caret <= gap {
                caret
            } else if caret >= gap + len {
                caret - len + seq.len() as u64
            } else {
                gap + seq.len() as u64
            }
        }
        // `pl-core` reflects BASES with `p -> n + 1 - p`. GAPS reflect with
        // `c -> n - c`. Reusing the base formula here puts the caret one base
        // out at every position: on "AAAAGGGG" -> "CCCCTTTT" the caret at the
        // A|G boundary (4) must land on the C|T boundary, which is 8 - 4 = 4.
        // Caret 0 and caret n swap, which is right — reversal swaps the ends.
        OpKind::ReverseComplement => n_before.saturating_sub(caret),
        OpKind::Rotate { origin } => {
            if n_before == 0 {
                return 0;
            }
            // Canonicalise the gap to "3' of base p", transport it with the map
            // `Molecule::rotate` uses on features, and read it back as a gap.
            let p = if caret >= 1 { caret } else { n_before };
            ((p - 1 + n_before - (origin - 1)) % n_before) + 1
        }
        _ => caret,
    }
}

// ---------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------

/// Caret, selection, the open run, and the messages the surface owes the user.
#[derive(Default)]
pub struct SeqEdit {
    pub caret: Caret,
    pub sel: Option<Selection>,
    run: Option<Run>,
    /// One line under the sequence: what was refused, what was dropped.
    pub notice: Option<String>,
    /// Rejected keystrokes inside about a second, held as a tally rather than
    /// as one message per keystroke, so key autorepeat on `z` cannot spam.
    rejects: Vec<char>,
    /// When the last keystroke was refused, in `egui::InputState::time`.
    /// `None` rather than `0.0`: `i.time` starts near zero, so a zero sentinel
    /// reads as "a rejection half a second ago" for the app's first seconds.
    reject_at: Option<f64>,
    /// A paste waiting on consent.
    pub pending_paste: Option<(PasteReport, Option<Selection>)>,
    /// Where the caret was at each point in the history.
    ///
    /// The log records operations, not cursors, so undo cannot reconstruct one
    /// and inventing it is how a caret ends up pointing at bases that no longer
    /// exist. `OpId` is content-addressed and `Copy + Hash + Eq`, so two
    /// different edits from the same parent cannot collide. Clamping to
    /// `0..=n` remains the fallback for an id that is not in the map.
    seen: HashMap<Option<OpId>, (Caret, Option<Selection>)>,
    /// The two-operation gestures, so undo and redo can step over both halves.
    ///
    /// Deleting across the origin is a rotate and then a range op — see
    /// [`ops_for_range_edit`] for why it cannot be anything else. One Ctrl+Z
    /// over the range half alone gave back all twelve bases of the worked
    /// example with the origin still moved: "KLABCDEFGHIJ", a numbering that
    /// matches neither the state before the edit nor the state after it, and
    /// every coordinate in the file shifted by two. That is not a partial undo
    /// a user can recognise. Keyed by the id of the second op, valued with the
    /// cursor before the first and the id of the first, which is all `seek`
    /// needs in either direction.
    pairs: HashMap<OpId, (Option<OpId>, OpId)>,
    /// True while the pointer is down and dragging out a selection.
    pub dragging: bool,
    /// Rows the viewport actually showed last frame, so PageUp/PageDown move
    /// by what the user can see rather than by a guessed constant.
    pub visible_rows: u64,
    /// Bases per row, measured from the width the view has. See
    /// [`MAX_PER_ROW`]. Zero only before the first frame; read it through
    /// [`SeqEdit::per_row`], which never returns zero.
    per_row: u64,
}

impl SeqEdit {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn run(&self) -> Option<&Run> {
        self.run.as_ref()
    }

    /// Bases per row. The renderer, the hit-test and Up/Down must all use this
    /// one value, or a click and an arrow key disagree about where a base is.
    pub fn per_row(&self) -> u64 {
        if self.per_row == 0 {
            MAX_PER_ROW
        } else {
            self.per_row
        }
    }

    /// Told by the view once per frame, from the width it measured.
    pub fn set_per_row(&mut self, n: u64) {
        self.per_row = n.max(1);
    }

    /// How many bases the user can see, committed plus whatever is pending.
    pub fn effective_len(&self, mol: &Molecule) -> u64 {
        match &self.run {
            None => mol.len(),
            Some(r) => mol.len() - r.removed + r.inserted.len() as u64,
        }
    }

    /// The bases of `range`, as the user sees them, appended to `out`.
    ///
    /// **One byte in, exactly one ASCII cell out.** The caret indexes
    /// `Molecule::seq`, which is a `Vec<u8>` and documented "not guaranteed to
    /// be valid IUPAC"; the readers keep bytes they do not understand.
    /// `String::from_utf8_lossy` is not length-preserving in either direction —
    /// `b"AC\xF0\x90\x80GT"` is 7 bytes and renders as 5 chars, one U+FFFD per
    /// maximal invalid subpart — so a column index taken through a lossy render
    /// drifts from the base offset, silently, only on the files that are
    /// already unusual. Substituting a fixed placeholder per byte keeps
    /// `column == offset within the row` unconditionally. It also fixes a
    /// latent display defect: a 60-base row could paint as 58 characters while
    /// the ruler beside it still claimed 60.
    pub fn row_text(&self, mol: &Molecule, from: u64, to: u64, out: &mut String) {
        out.clear();
        for i in from..to {
            let b = self.byte_at(mol, i);
            // The placeholder is ASCII on purpose. A prettier glyph such as
            // U+00B7 is not in egui's monospace face, so it falls back to a
            // proportional one with a different advance — which reintroduces
            // the exact column drift this function exists to remove.
            out.push(if b.is_ascii_graphic() { b as char } else { '?' });
        }
    }

    /// The effective byte at gap-left index `i`, committed sequence plus run.
    fn byte_at(&self, mol: &Molecule, i: u64) -> u8 {
        let Some(r) = &self.run else {
            return mol.seq.get(i as usize).copied().unwrap_or(b' ');
        };
        let k = r.inserted.len() as u64;
        let src = if i < r.start {
            i
        } else if i < r.start + k {
            return r.inserted.as_bytes()[(i - r.start) as usize];
        } else {
            i - k + r.removed
        };
        mol.seq.get(src as usize).copied().unwrap_or(b' ')
    }

    // -- messages ----------------------------------------------------------

    pub fn say(&mut self, s: impl Into<String>) {
        self.notice = Some(s.into());
    }

    /// How long a refusal stays on screen once something else has succeeded.
    ///
    /// Long enough to read a sentence, and it costs nothing: the line is under
    /// the sequence, not in the way.
    const REJECT_STICKY: f64 = 5.0;

    /// Clear the notice for a keystroke that went in — unless a refusal is
    /// still on it.
    ///
    /// `egui` delivers ordinary typing as one `Event::Text` per character, so
    /// the guard that kept a refusal alive *within* one event was invisible in
    /// practice: type "ACGZTACG" at human speed and the message naming 'Z' was
    /// erased by the 'T' that followed it, one frame later. The user pressed
    /// eight keys, seven bases went in, and nothing on screen said which one
    /// did not. The module's own comment describes exactly that failure; this
    /// is the version of the rule that survives the real event stream.
    fn clear_notice(&mut self, now: f64, refused_now: bool) {
        if refused_now {
            return;
        }
        if self
            .reject_at
            .is_some_and(|t| now - t < Self::REJECT_STICKY)
        {
            return;
        }
        self.notice = None;
    }

    /// Note a keystroke that is not a nucleotide code.
    ///
    /// Rejected, and said. Not silent: a silently ignored keystroke and a
    /// silently accepted junk base are indistinguishable to the user at the
    /// moment it happens, and `Molecule::validate` will never raise either one
    /// later — it inspects coordinates and never looks at the sequence, so
    /// after `InsertAt { seq: "zzz" }` it returns `[]`. The keystroke is the
    /// only moment this can be said.
    fn reject(&mut self, c: char, now: f64) {
        if self.reject_at.is_none_or(|t| now - t > 1.0) {
            self.rejects.clear();
        }
        self.reject_at = Some(now);
        self.rejects.push(c);
        let mut kinds: Vec<char> = Vec::new();
        for c in &self.rejects {
            if !kinds.contains(c) {
                kinds.push(*c);
            }
        }
        self.notice = Some(if self.rejects.len() == 1 {
            format!(
                "'{c}' is not a nucleotide code. Bases are ACGT, the ambiguity codes \
                 RYSWKMBDHVN, and U."
            )
        } else {
            format!(
                "{} characters ignored: {}",
                self.rejects.len(),
                kinds
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        });
    }

    // -- caret movement ----------------------------------------------------

    /// Place or extend the caret. Any caret movement closes the open run: an
    /// undo boundary belongs wherever the user stopped typing in one place.
    pub fn place(&mut self, doc: &mut Document, to: Caret, extend: bool) {
        self.commit(doc);
        let n = doc.molecule().len();
        let to = to.min(n);
        if extend {
            let anchor = self.sel.map_or(self.caret, |s| s.anchor);
            let tor = self.sel.is_some_and(|s| s.through_origin);
            self.sel = Some(
                Selection {
                    anchor,
                    head: to,
                    through_origin: tor,
                }
                // `clamped`, not `canonical`: this value is stored and read
                // back by the next Shift+Arrow.
                .clamped(n, doc.molecule().topology.is_circular()),
            );
        } else {
            self.sel = None;
        }
        self.caret = to;
    }

    /// Adopt a selection made by the pointer.
    ///
    /// The one way anything outside this module may set `sel` and `caret`. It
    /// closes the open run first, and that is the point: `type_text` extends an
    /// open run without consulting `sel`, so a selection assigned behind the
    /// run's back left the highlighted bases in place and put the typed ones
    /// wherever the run had started: open a run at caret 0 by typing "gg" into
    /// "AAAACCCCGGGGTTTT", drag out the GGGG, type "N", and the answer was
    /// "ggNAAAACCCCGGGGTTTT" — the N ten bases from the highlight, and the
    /// highlight still there.
    pub fn set_selection(&mut self, doc: &mut Document, sel: Selection, caret: Caret) {
        self.commit(doc);
        let mol = doc.molecule();
        let n = mol.len();
        self.sel = Some(sel.clamped(n, mol.topology.is_circular()));
        self.caret = caret.min(n);
    }

    /// Move by `delta` gaps, wrapping on a circle for a single-gap step only.
    ///
    /// A circle has no end and Left/Right stopping dead at base 1 would
    /// contradict the topology the file declares, so those two wrap. Everything
    /// else clamps: `Home`/`End`, and — because both wrap arms below are gated
    /// on `delta == ±1` while `main.rs` passes `±per_row` for Up/Down and
    /// `±per_page` for PageUp/PageDown, and `fit_per_row` never returns 1 —
    /// those four as well. That is deliberate and it is what every text editor
    /// does: a *row* motion is a statement about the layout, and `Home`/`End`
    /// about the numbering, and neither is a claim that the molecule has ends.
    /// The docstring here used to promise that no arrow key ever stops, which
    /// four of the six keys reaching this function have never honoured.
    ///
    /// Crossing the origin with Shift held is initiated by Shift+Left or
    /// Shift+Right — the `over_the_origin` arm is gated on `delta == ±1` too,
    /// and must stay that way: it *toggles* `through_origin`, which is what lets
    /// a selection be walked back, and firing it on a multi-gap step that landed
    /// on an already-wrapping selection would name the complement arc. Once the
    /// bit is set, Shift+Up and Shift+Down extend across the origin a whole row
    /// at a time like any other key.
    pub fn step(&mut self, doc: &mut Document, delta: i64, extend: bool) {
        // Settle FIRST, before the length is read. A caret move closes the run
        // in any case — `place` says so — but this function measured the
        // molecule beforehand, so with a run open every length below was the
        // committed one while the caret was in the coordinates of what is on
        // screen. Type three bases at the end of "ACGT" and press Right: `to`
        // clamped to `min(8, 4)` and the caret jumped back to where the typing
        // had started.
        self.commit(doc);
        let n = doc.molecule().len();
        let circular = doc.molecule().topology.is_circular();
        let here = self.caret;

        // Collapse to the edge, not the edge minus one — the classic wrong
        // version of this.
        //
        // For a WRAPPING arc the ends are the other way round: the arc runs
        // `hi+1 .. n, 1 .. lo`, so its 5' end is the gap `hi` and its 3' end is
        // the gap `lo`. Reading `lo` as the left-hand end there put the caret at
        // the far end of what was highlighted, and the next thing typed landed
        // there.
        if !extend {
            if let Some(s) = self.sel {
                if !s.is_empty(n) && delta.abs() == 1 {
                    let to = match (delta < 0, s.through_origin) {
                        (true, false) => s.lo(),
                        (false, false) => s.hi(),
                        (true, true) => s.hi(),
                        (false, true) => s.lo(),
                    };
                    self.place(doc, to, false);
                    return;
                }
            }
        }

        // Gap 0 and gap `n` are the same point on a circle, and which of the two
        // representatives the head holds is exactly what decides whether the arc
        // wraps. So a step over that point does two things at once: it moves the
        // head to the gap one base *beyond* the origin, and it toggles the wrap
        // bit. Toggling rather than setting is what lets a selection be walked
        // back: with the head at gap 0 and the bit set (bases 11,12 of a 12 bp
        // circle), Shift+Left must give base 11 alone — head 11, no wrap — and a
        // rule that only ever sets the bit leaves the head stuck at 0.
        let over_the_origin = extend
            && circular
            && n > 0
            && ((delta == 1 && here == n) || (delta == -1 && here == 0));
        if over_the_origin {
            let anchor = self.sel.map_or(here, |s| s.anchor);
            let tor = self.sel.is_some_and(|s| s.through_origin);
            let to = if delta == 1 { 1.min(n) } else { n - 1 };
            self.commit(doc);
            self.sel = Some(
                Selection {
                    anchor,
                    head: to,
                    through_origin: !tor,
                }
                .clamped(n, true),
            );
            self.caret = to;
            return;
        }

        let to = if circular && n > 0 && delta == 1 && here == n {
            0
        } else if circular && n > 0 && delta == -1 && here == 0 {
            n
        } else if delta < 0 {
            here.saturating_sub(delta.unsigned_abs())
        } else {
            (here + delta as u64).min(n)
        };
        self.place(doc, to, extend);
    }

    // -- typing ------------------------------------------------------------

    /// Insert text at the caret, or over the selection.
    ///
    /// `egui::Event::Text` carries a `String`, not a `char`, and may hold more
    /// than one, so the accepted characters go in as one run and the rejected
    /// ones are tallied rather than reported individually.
    pub fn type_text(&mut self, doc: &mut Document, text: &str, now: f64) {
        let mut accepted = String::new();
        // A single `Event::Text` can hold accepted and rejected characters at
        // once (IME, dead keys, a fast typist). The rejection message must
        // survive the insertion of its neighbours: clearing the notice because
        // *something* went in is how a user types 40 bases, sees 37, and never
        // learns which three are missing.
        let mut refused_any = false;
        for c in text.chars() {
            if is_base(c) {
                // Case is exactly as typed. `Molecule::seq` is case-preserved
                // by contract because lowercase marks a soft-masked region or a
                // non-annealing primer tail, and an editor that normalises on
                // the first keystroke has destroyed information the user was
                // relying on. There is no case map anywhere on this path.
                accepted.push(c);
            } else {
                refused_any = true;
                self.reject(c, now);
            }
        }
        if accepted.is_empty() {
            return;
        }

        // Extend the open run if this is more of the same thing.
        //
        // Not while anything is selected. A selection is a *different* edit —
        // "replace these bases" rather than "add to what I just typed" — and
        // the pointer can raise one while a run is open. Without this guard the
        // highlighted bases survived and the typed ones went in wherever the
        // run had started, which is the only way this surface could write bases
        // to a place the user was not pointing at.
        let selecting = self.sel.is_some_and(|s| !s.is_empty(doc.molecule().len()));
        if !selecting {
            if let Some(r) = &mut self.run {
                if r.kind == RunKind::Insert && !r.is_full() && !r.is_idle(now) {
                    r.inserted.push_str(&accepted);
                    r.last_input = now;
                    self.caret = r.caret_after();
                    self.clear_notice(now, refused_any);
                    return;
                }
            }
        }
        self.commit(doc);

        let n = doc.molecule().len();
        let sel = self
            .sel
            .map(|s| s.canonical(n, doc.molecule().topology.is_circular()));

        // An origin-crossing replacement needs a rotation and cannot be a run.
        // It is applied whole, immediately; the next keystroke opens a fresh
        // run at the caret it leaves behind.
        if let Some(s) = sel {
            if s.through_origin {
                self.apply_gesture(doc, s, Some(&accepted));
                return;
            }
        }

        let (start, removed) = match sel {
            Some(s) if !s.is_empty(n) => (s.lo(), s.hi() - s.lo()),
            _ => (self.caret.min(n), 0),
        };

        // Ask the gate now rather than at commit time. The verdict on an
        // insertion does not depend on how many bases are inserted, so one
        // trial here stands for the whole run — and a refusal arrives while the
        // user still knows what they pressed, instead of a second later with
        // forty characters in the balance.
        let trial = Run {
            start,
            removed,
            inserted: accepted.clone(),
            kind: RunKind::Insert,
            last_input: now,
        };
        if let Some(op) = trial.to_op() {
            if let Err(e) = preflight(doc.molecule(), std::slice::from_ref(&op)) {
                self.say(format!("{e}. Nothing was changed."));
                return;
            }
        }
        self.caret = trial.caret_after();
        self.run = Some(trial);
        self.sel = None;
        self.clear_notice(now, refused_any);
    }

    /// Backspace. Deletes the selection if there is one, otherwise one base.
    ///
    /// Returns false when nothing was removed — refused by the gate, or the
    /// caret was already at an end. Ctrl+X needs the answer: it must not report
    /// "cut N bases" over a refusal, with the bases on the clipboard and
    /// nothing taken out of the molecule.
    pub fn backspace(&mut self, doc: &mut Document, now: f64) -> bool {
        let n = self.effective_len(doc.molecule());
        if let Some(s) = self.sel {
            if !s.is_empty(doc.molecule().len()) {
                self.commit(doc);
                let s = s.canonical(doc.molecule().len(), doc.molecule().topology.is_circular());
                return self.apply_gesture(doc, s, None);
            }
        }
        if let Some(r) = &mut self.run {
            if r.kind == RunKind::DeleteBack && r.start > 0 && !r.is_full() && !r.is_idle(now) {
                r.start -= 1;
                r.removed += 1;
                r.last_input = now;
                self.caret = r.start;
                self.clear_notice(now, false);
                return true;
            }
        }
        self.commit(doc);
        let c = self.caret.min(doc.molecule().len());
        if c == 0 {
            self.at_the_start(doc, n);
            return false;
        }
        self.open_delete(doc, c - 1, RunKind::DeleteBack, now)
    }

    /// Forward delete. Same contract as [`SeqEdit::backspace`].
    pub fn delete_forward(&mut self, doc: &mut Document, now: f64) -> bool {
        if let Some(s) = self.sel {
            if !s.is_empty(doc.molecule().len()) {
                self.commit(doc);
                let s = s.canonical(doc.molecule().len(), doc.molecule().topology.is_circular());
                return self.apply_gesture(doc, s, None);
            }
        }
        if let Some(r) = &mut self.run {
            let room = r.start + r.removed < doc.molecule().len();
            if r.kind == RunKind::DeleteForward && room && !r.is_full() && !r.is_idle(now) {
                r.removed += 1;
                r.last_input = now;
                self.clear_notice(now, false);
                return true;
            }
        }
        self.commit(doc);
        let n = doc.molecule().len();
        let c = self.caret.min(n);
        if c == n {
            self.at_the_end(doc, n);
            return false;
        }
        self.open_delete(doc, c, RunKind::DeleteForward, now)
    }

    fn open_delete(&mut self, doc: &mut Document, start: Caret, kind: RunKind, now: f64) -> bool {
        let trial = Run {
            start,
            removed: 1,
            inserted: String::new(),
            kind,
            last_input: now,
        };
        if let Some(op) = trial.to_op() {
            if let Err(e) = preflight(doc.molecule(), std::slice::from_ref(&op)) {
                self.say(format!("{e}. Nothing was changed."));
                return false;
            }
        }
        self.caret = trial.caret_after();
        self.run = Some(trial);
        self.sel = None;
        self.clear_notice(now, false);
        true
    }

    /// Backspace at caret 0.
    ///
    /// A no-op on a circle too, deliberately. The document is stored and shown
    /// as a linear array with a distinguished base 1, and deleting the last base
    /// because the user pressed Backspace on row 1 is an edit at the *other end*
    /// of the document from the caret — usually off the bottom of the view, and
    /// on a molecule that fits in one screenful, several cells to the right of a
    /// caret that has not moved. Either way it is not where the user is looking.
    /// The message names the alternative so this does not read as the topology
    /// being ignored.
    fn at_the_start(&mut self, doc: &Document, n: u64) {
        if doc.molecule().topology.is_circular() && n > 0 {
            // Base `n` is on the LAST row, which is the bottom-most row of the
            // grid: `main.rs` lays row `r` out at increasing `y`, so nothing can
            // ever push base `n` above the viewport. This said "off the top of
            // this view" — the direction the companion `at_the_end` correctly
            // gives for base 1 — and paired it with `n / per_row`, which is one
            // row too many whenever `per_row` divides `n` and reads "0 rows
            // away, off the top" for any circle short enough to fit on one row,
            // where base `n` is plainly visible beside the caret.
            let rows = (n - 1) / self.per_row();
            let where_it_is = if rows == 0 {
                ", on this same row".to_string()
            } else {
                format!(
                    ", {} row{} below, at the foot of the sequence",
                    fmt_int(rows),
                    if rows == 1 { "" } else { "s" }
                )
            };
            self.say(format!(
                "The caret is at base 1. On a circle the base before it is base {}{}. \
                 Select it, or use {}, if that is what you meant. This is a display \
                 decision, not a claim that a circle has ends.",
                fmt_int(n),
                where_it_is,
                crate::set_origin_path()
            ));
        } else {
            self.say("The caret is at the start; there is nothing before base 1.");
        }
    }

    fn at_the_end(&mut self, doc: &Document, n: u64) {
        if doc.molecule().topology.is_circular() && n > 0 {
            self.say(format!(
                "The caret is after the last base. On a circle the next base is base 1, at \
                 the top of this view. Select it, or use {}, if that is what you meant.",
                crate::set_origin_path()
            ));
        } else {
            self.say("The caret is at the end; there is nothing after the last base.");
        }
    }

    // -- clipboard ---------------------------------------------------------

    /// The selected bases, raw, case preserved.
    ///
    /// No header and no line breaks: the destination is a primer order form, a
    /// BLAST box or a synthesis field, and a `>` header pasted into an order
    /// form gets ordered while a wrapped body pastes as a sequence with line
    /// breaks inside it. Raw bases are the only form that is correct in every
    /// destination.
    ///
    /// The origin wrap is `Molecule::subseq`'s, not a second implementation of
    /// it — that is the point of routing through it.
    ///
    /// Returns the text and the number of bytes left out of it. `Molecule::seq`
    /// is a `Vec<u8>` documented as "not guaranteed to be valid IUPAC" and the
    /// readers keep bytes they do not understand, so `*b as char` is a Latin-1
    /// transliteration: `b"AC\xF0\x90\x80GT"` reaches the clipboard as
    /// "ACð\u{90}\u{80}GT", which is neither what the grid painted (`AC???GT`,
    /// one cell per byte) nor what the file holds, and pasting it back needs
    /// consent for three characters. Skipping them and saying how many is the
    /// only option that is honest in both destinations.
    pub fn copy(&self, mol: &Molecule) -> Option<(String, usize)> {
        let n = mol.len();
        let s = self.sel?.canonical(n, mol.topology.is_circular());
        if s.is_empty(n) {
            return None;
        }
        let bytes = if s.through_origin {
            mol.subseq(s.hi() + 1, s.lo())?
        } else {
            mol.subseq(s.lo() + 1, s.hi())?
        };
        let text: String = bytes
            .iter()
            .filter(|b| b.is_ascii_graphic())
            .map(|b| *b as char)
            .collect();
        let skipped = bytes.len() - text.len();
        Some((text, skipped))
    }

    /// A paste, after sanitising. Returns true when the caller should raise the
    /// consent dialog.
    pub fn paste(&mut self, doc: &mut Document, text: &str) -> bool {
        self.commit(doc);
        let report = sanitise_paste(text);
        if let Some(why) = &report.refused {
            self.say(why.clone());
            return false;
        }
        if is_a_lot(report.bases.len() as u64, doc.molecule().len()) || report.needs_consent() {
            // Typing is one character at a time and the user watches it fail to
            // appear; a paste is bulk and cannot be audited by eye, so nothing
            // unexpected may be dropped from one without explicit consent.
            //
            // The selection is captured WITH the report because the dialog is
            // about that selection: between the question and the answer the
            // caret can move, and a paste that lands somewhere other than the
            // bases the user was shown is not a confirmed paste.
            let target = self.target(doc);
            self.pending_paste = Some((report, Some(target)));
            return true;
        }
        let target = self.target(doc);
        self.insert_paste(doc, &report, target);
        false
    }

    /// The arc a paste or an insertion would replace: the selection if there is
    /// one, otherwise the caret as an empty span.
    ///
    /// A selection covering **no bases** is not a selection. `type_text` has
    /// always fallen back to the caret in that case; this did not, and one
    /// empty shape is not where the caret is: shift-selecting backwards across
    /// the origin from gap 0 and then shrinking it back to nothing leaves
    /// `{anchor: 0, head: n, through_origin: true}`, whose `canonical` collapses
    /// to gap 0 while the caret is at gap n. Ctrl+V then inserted before base 1
    /// where typing the same characters inserted after base n — shifting every
    /// coordinate in the file by the length of the paste, moving the origin, and
    /// saying nothing, while the readout above the grid had just promised
    /// "numbering unchanged".
    pub fn target(&self, doc: &Document) -> Selection {
        let mol = doc.molecule();
        let n = mol.len();
        self.sel
            .map(|s| s.canonical(n, mol.topology.is_circular()))
            .filter(|s| !s.is_empty(n))
            .unwrap_or(Selection::point(self.caret.min(n)))
    }

    /// Commit a sanitised paste as exactly one operation, so one Ctrl+Z
    /// reverses the whole of it.
    ///
    /// `target` is passed rather than read from `self`: for a confirmed paste
    /// it was captured when the question was asked, and it is the answer to
    /// that question and not to wherever the caret has since got to.
    pub fn insert_paste(&mut self, doc: &mut Document, report: &PasteReport, target: Selection) {
        if report.bases.is_empty() {
            // `InsertAt { seq: "" }` is accepted by the engine and records a
            // real history entry for nothing. A paste that sanitises to nothing
            // is a message, not an operation.
            self.say(format!("{} — nothing to insert", report.summary()));
            return;
        }
        let mol = doc.molecule();
        let target = target.canonical(mol.len(), mol.topology.is_circular());
        let summary = report.summary();
        if self.apply_gesture(doc, target, Some(&report.bases)) {
            self.say(summary);
        }
    }

    // -- the one path to the log -------------------------------------------

    /// Apply a whole gesture: derive the ops, ask the gate, then commit.
    ///
    /// Returns false and says why if anything was refused, having changed
    /// nothing at all.
    fn apply_gesture(&mut self, doc: &mut Document, sel: Selection, seq: Option<&str>) -> bool {
        let had = census(doc);
        let (ops, caret) = ops_for_range_edit(doc.molecule(), sel, seq);
        if ops.is_empty() {
            return true;
        }
        // A pair must never half-apply, so the whole gesture is rehearsed on a
        // throwaway log first, using the engine's own gate as the oracle.
        if ops.len() > 1 {
            if let Err(e) = preflight(doc.molecule(), &ops) {
                self.say(format!("{e}. Nothing was changed."));
                return false;
            }
        }
        let crossed = ops.len() > 1;
        let began_at = doc.log.cursor();
        let mut ids: Vec<OpId> = Vec::new();
        for op in ops {
            if let Err(e) = doc.apply(op.clone()) {
                self.say(format!(
                    "cannot {}: {e}. Nothing was changed.",
                    op.describe()
                ));
                return false;
            }
            if let Some(id) = doc.log.cursor() {
                ids.push(id);
            }
        }
        // Record the pair so one Ctrl+Z steps over both halves. Ids are
        // content-addressed, so two identical pairs from the identical parent
        // are the same pair and collapsing them is correct.
        if crossed && ids.len() == 2 {
            self.pairs.insert(ids[1], (began_at, ids[0]));
        }
        self.caret = caret;
        self.sel = None;
        self.remember(doc);

        // `remap_annotations` drops a feature whose every base disappeared —
        // deliberately, and rightly — but `apply` returns Ok and says nothing.
        // Measured: a 4 bp `ReplaceRange` over a 4 bp AmpR took features from 1
        // to 0 with no signal at all.
        let lost = feature_loss(&had, doc);
        let mut msg = String::new();
        if crossed {
            msg.push_str(
                "the plasmid was renumbered to start at the cut — the deleted bases included \
                 the origin · Ctrl+Z twice. ",
            );
        }
        if let Some(l) = lost {
            msg.push_str(&l);
        }
        if !msg.is_empty() {
            self.say(msg);
        }
        true
    }

    /// Turn the open run into its single operation.
    ///
    /// **Called before anything can observe the document**: saving, exporting,
    /// autosaving, undo, redo, any other edit, a caret move, losing focus. An
    /// autosave that writes `log.current()` while a run is open writes a file
    /// missing the user's last forty keystrokes, and that is the rule whose
    /// absence loses data rather than merely annoying.
    pub fn commit(&mut self, doc: &mut Document) {
        let Some(run) = self.run.take() else { return };
        let Some(op) = run.to_op() else { return };
        let had = census(doc);
        match doc.apply(op.clone()) {
            Ok(()) => {
                self.caret = run.caret_after();
                // The selection is deliberately NOT cleared. While a run is
                // open the selection the pointer made is in the coordinates of
                // what is on screen — committed bases plus the run — and those
                // become the committed coordinates at exactly this moment, so
                // it is valid from here on. Clearing it turned "type over the
                // highlight" into "insert at the caret" for every caller that
                // settles first, which is all of them.
                self.remember(doc);
                // The same report `apply_gesture` makes, and for the same
                // reason. It was on that path only, so typing over a selection
                // and holding Backspace through a feature — both ordinary
                // keyboard gestures, reachable with no pointer at all — removed
                // it and said nothing whatever.
                if let Some(l) = feature_loss(&had, doc) {
                    self.say(l);
                }
            }
            // Pre-flighted when the run opened, so this is close to
            // unreachable for an insertion — but a long delete can newly
            // orphan a coordinate that a one-base delete did not, and losing
            // the typing silently would be worse than saying so.
            Err(e) => {
                self.caret = run.start.min(doc.molecule().len());
                // Here it IS cleared: the run did not apply, so a selection
                // measured against the screen names bases the molecule never
                // had.
                self.sel = None;
                self.say(format!(
                    "cannot {}: {e}. Nothing was changed, and what you typed was discarded.",
                    op.describe()
                ));
            }
        }
    }

    /// Where an undo from `at` should land, when `at` is the tail of a
    /// two-operation gesture. `None` means "one step, as usual".
    pub fn undo_over_pair(&self, at: Option<OpId>) -> Option<Option<OpId>> {
        self.pairs.get(&at?).map(|(before, _)| *before)
    }

    /// The other half: having redone the rotate, `to` is where the range op
    /// that went with it sits.
    pub fn redo_over_pair(&self, at: Option<OpId>) -> Option<OpId> {
        let at = at?;
        self.pairs
            .iter()
            .find(|(_, (_, first))| *first == at)
            .map(|(tail, _)| *tail)
    }

    /// Note where the caret is at this point in the history.
    pub fn remember(&mut self, doc: &Document) {
        self.seen.insert(doc.log.cursor(), (self.caret, self.sel));
    }

    /// Put the caret somewhere defensible after the document changed under it.
    ///
    /// Exact where the cursor has been before, clamped otherwise. The log is a
    /// DAG and a seek can land on any earlier or forked state, so `n` changes
    /// arbitrarily; reconstructing a caret from operations that never recorded
    /// one is how a caret ends up pointing at bases that are gone.
    pub fn restore(&mut self, doc: &Document) {
        let n = doc.molecule().len();
        match self.seen.get(&doc.log.cursor()) {
            Some((c, s)) => {
                self.caret = (*c).min(n);
                self.sel = s.map(|s| s.clamped(n, doc.molecule().topology.is_circular()));
            }
            None => {
                self.caret = self.caret.min(n);
                self.sel = None;
            }
        }
        self.run = None;
    }

    // -- the readout -------------------------------------------------------

    /// The line at the foot of the tab: 1-based, and never a raw caret index.
    ///
    /// For an empty caret it prints the number the operation will carry,
    /// verbatim. Making the readout and the op the same number is the property
    /// that keeps this honest: if they ever disagree, one of them is a bug and
    /// the user is who will notice.
    pub fn readout(&self, mol: &Molecule) -> String {
        let n = self.effective_len(mol);
        let circular = mol.topology.is_circular();

        if let Some(s) = self.sel {
            let s = s.canonical(mol.len(), circular);
            if !s.is_empty(mol.len()) {
                let count = s.base_count(mol.len());
                let (a, b) = if s.through_origin {
                    (s.hi() + 1, s.lo())
                } else {
                    (s.lo() + 1, s.hi())
                };
                // Never a single position while a selection is live: a
                // biologist reading "2,451" with forty bases highlighted takes
                // it as the selection's start, and it would be the caret, which
                // sits at whichever end they dragged to. That mismatch is
                // silent, plausible, and costs a deletion at the wrong end of a
                // cassette.
                let mut out = format!("{}..{} · {} bp", fmt_int(a), fmt_int(b), fmt_int(count));
                if s.through_origin {
                    out.push_str(" · crosses the origin");
                } else if count == mol.len() && count > 0 {
                    out.push_str(" · whole molecule");
                }
                return out;
            }
        }

        let c = self.caret.min(n);
        let at = fmt_int(c + 1);
        match (c, circular) {
            (0, false) => format!("insert at {at} · before base 1"),
            (0, true) => format!(
                "insert at {at} · before base 1, at the origin · every feature's coordinates shift"
            ),
            (c, false) if c == n => {
                format!("insert at {at} · after the last base ({})", fmt_int(n))
            }
            (c, true) if c == n => format!(
                "insert at {at} · after base {}, at the origin · numbering unchanged",
                fmt_int(n)
            ),
            (c, _) => format!("insert at {at} · between {} and {at}", fmt_int(c)),
        }
    }
}

/// The feature names an edit is about to be measured against.
fn census(doc: &Document) -> Vec<String> {
    doc.molecule()
        .features
        .iter()
        .map(|f| f.name.clone())
        .collect()
}

/// Which features an edit removed, as a sentence, or `None` if none went.
fn feature_loss(before: &[String], doc: &Document) -> Option<String> {
    let count_before = before.len();
    let after = doc.molecule().features.len();
    if after >= count_before {
        return None;
    }
    let mut remaining: Vec<String> = doc
        .molecule()
        .features
        .iter()
        .map(|f| f.name.clone())
        .collect();
    let mut gone = Vec::new();
    for name in before {
        match remaining.iter().position(|r| r == name) {
            Some(i) => {
                remaining.remove(i);
            }
            None => gone.push(name.clone()),
        }
    }
    let n = count_before - after;
    let named: Vec<&str> = gone.iter().take(3).map(|s| s.as_str()).collect();
    let list = if named.is_empty() {
        format!("{n} feature(s)")
    } else if gone.len() > 3 {
        format!("{} and {} more", named.join(", "), gone.len() - 3)
    } else {
        named.join(", ")
    };
    Some(format!(
        "{list} removed — {} described bases that no longer exist. Ctrl+Z to undo.",
        if n == 1 { "it" } else { "they" }
    ))
}

#[cfg(test)]
mod tests;
