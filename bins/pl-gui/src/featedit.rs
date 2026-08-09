//! The Feature editor: add a feature, or change one that is already there.
//!
//! # It never constructs a `Feature`
//!
//! Every save clones the feature the document already holds and mutates only
//! the fields the form owns:
//!
//! ```text
//! let mut f = self.base.clone();   // everything, by value
//! f.name = self.name.clone();      // only what the form owns
//! ```
//!
//! Anything with no control on screen rides back out on that clone and cannot
//! be lost by omission. That is not a nicety. `Feature::new(name, kind)` plus a
//! push destroys, in one gesture the user reads as "rename":
//!
//! - **the qualifiers**, their order, their repeats, and — the sharp one — the
//!   difference between `None` and `Some("")`. `pl_core::Feature`'s own doc
//!   counts 11,716 valueless qualifiers against 4 empty-valued ones across this
//!   project's 328-file GenBank corpus. A form field that reads a qualifier into
//!   a `String` collapses `/pseudo` into `/pseudo=""`, and a pseudogene reopens
//!   as an ordinary protein-coding gene with a full-length ORF a cloner will
//!   trust. That is why [`QualRow`] carries a `has_value` **flag** and not an
//!   empty string: a text box has one empty state and the model has two.
//! - **per-segment colour and the `translated` flag.** `Segment::translated`
//!   decides whether the Sequence tab draws the amino-acid track under that
//!   segment — `bins/pl-gui/src/aa.rs` is its reader, and until that module
//!   existed nothing in this program read the bit at all — which is exactly
//!   why an author rebuilding a `Feature` from form fields forgets it.
//! - **`Segment::kind`**, the `.dna` `<Segment type=>`. One distinct value in
//!   the whole local corpus, so it gets no control; it is carried verbatim.
//! - **segment order**, which is load-bearing three times over: `Feature::extent`
//!   recognises an origin-crossing join only in the shape the writer emits,
//!   `map::bands` builds the intron connectors from `windows(2)` in file order,
//!   and `genbank::format_location` emits the parts of a `join(...)` verbatim —
//!   INSDC join order *is* the reading order. Nothing here sorts.
//!
//! # It goes through `OpKind`
//!
//! One [`pl_core::OpKind::SetFeature`] per Save, applied by `App::edit` through
//! `Document::apply`, so undo, the annotation remap and the `WouldCorrupt` gate
//! are all inherited. Nothing writes `Molecule::features`. One operation and not
//! one per field, for three reasons that agree: it is what the user did (one
//! Save, one Ctrl+Z); the engine has no per-field op and `OpKind::content`
//! already hashes every field this form can change; and the gate is all-or-
//! nothing against the *final* state, so splitting a Save would run it against
//! an intermediate the user never asked for.
//!
//! # SacB, and why "these two rows look the same" is not an invitation to merge
//!
//! The user's own pKoV carries `SacB` as two segments, `1976..3310` and
//! `3311..3397`. They **abut**: it is not a spliced CDS and not an origin
//! crossing, it is one contiguous reverse-strand CDS split so the N-terminal
//! signal peptide can be drawn separately — SnapGene names the second segment
//! "signal peptide", and `pl_core::Segment` has no `name` field, so that label
//! is already gone by the time this form sees it. The two rows therefore differ
//! in nothing the model holds. There is deliberately no "merge contiguous
//! segments" affordance: `1976..3397` renders identically, `extent` gives the
//! same answer, and the only biology those two rows encode would be gone.
//!
//! # It is not modal, and pays for it
//!
//! `egui::Window`, like the design panel, because coordinates are typed while
//! reading the map and the sequence. The cost is the same and is paid the same
//! way: `doc_at` is refreshed every frame, [`Panel::stale_reason`] refuses a
//! commit whose document has moved, and `App` suppresses the sequence keys and
//! the undo/redo shortcuts while this is up.

use std::collections::BTreeSet;

use eframe::egui::{self, RichText, Ui};
use pl_core::{Feature, Invalid, Molecule, Segment, Strand, Topology};

use crate::fmt_int;
use crate::theme::Palette;

/// The feature keys the Type combo offers.
///
/// A suggestion, not a vocabulary. `Feature::kind` is stored verbatim by both
/// writers and `pl-features` deliberately declined to close this list (its
/// `genbank_key` is a free `String` and not a Sequence Ontology term, for a
/// licence reason recorded there), so refusing an unknown key would make files
/// that open fine today unopenable-and-resaveable.
///
/// **The spellings are INSDC's, exactly.** `theme::by_kind` and
/// `pl_draw::colour_for` match the whole string, so `cds` gets the fallback grey
/// while `CDS` gets the CDS colour.
///
/// `source` is deliberately absent: `genbank::parse` drops it as whole-molecule
/// metadata, so a feature typed `source` vanishes on the next open — and until
/// then draws a full-length bar across the map.
pub const KINDS: &[&str] = &[
    "CDS",
    "gene",
    "promoter",
    "terminator",
    "RBS",
    "polyA_signal",
    "rep_origin",
    "primer_bind",
    "protein_bind",
    "misc_feature",
    "sig_peptide",
    "misc_recomb",
    "regulatory",
    "ncRNA",
    "tRNA",
    "rRNA",
];

/// What `genbank::write` truncates the feature key to.
const GENBANK_KEY_CHARS: usize = 15;

/// The two row buttons' labels, and they are constants because **the obvious
/// characters are tofu in this binary's own faces.**
///
/// Photographed, not predicted: the first cut of this window used `✕` and
/// `▾`/`▸`, and all three came out as empty boxes in the capture. The tofu
/// oracle then confirmed it — U+2715, U+25BE, U+25B8 and U+21C5 all render as
/// the replacement glyph in Proportional, while U+2191, U+2193 and U+00D7 do
/// not. Same trap as `menu_with_caret` (U+25BE, an empty box on all three
/// menus) and `strand_word` (U+2190 in the proportional face); the third time
/// this project has paid for asking a font for chrome.
///
/// `the_feature_editors_own_glyphs_are_in_the_face_that_draws_them` is what
/// keeps these honest, and the disclosure triangle is a `CollapsingHeader`
/// shape rather than a character at all.
pub const UP: &str = "↑";
/// U+00D7, the multiplication sign, not U+2715 `✕`.
pub const DELETE: &str = "×";

/// How tall the scrolling part of the window may get before it scrolls.
///
/// Chosen so the footer is still on a 768 pt screen with the window at its
/// default position. See `body` for what happened without it.
const BODY_MAX_H: f32 = 460.0;

/// How much room one qualifier value gets before it scrolls inside its own box.
///
/// SacB's `/translation` is 476 characters. Left to grow, it made one table row
/// taller than the screen. Left ELIDED, it would be a value the user cannot see
/// the whole of and will therefore clobber without noticing — which is the
/// failure this whole window exists to avoid. So: bounded, and scrollable.
const VALUE_W: f32 = 250.0;
const VALUE_H: f32 = 46.0;
/// Wide enough for `ribosomal_slippage`, the longest key this corpus carries.
const KEY_W: f32 = 150.0;
/// Wide enough for `#rrggbb` with room for the caret. `#993366` and `#9933ff`
/// are the same colour box until the last two characters are on screen.
const COLOR_W: f32 = 84.0;

/// One row of the segments table.
#[derive(Debug, Clone, PartialEq)]
pub struct SegRow {
    pub start: u64,
    pub end: u64,
    /// Whether this pair names the arc through base 1.
    ///
    /// **Derived, not stored.** The model has exactly one spelling for an
    /// origin crossing — `end < start` — so this is `circular && end < start`
    /// and [`Panel::normalise`] recomputes it every frame. It is a field only
    /// so `egui::Ui::checkbox` has somewhere to write; the click is turned into
    /// a swap of the pair, which is the only thing a crossing can mean.
    ///
    /// Always false on a linear molecule: a line has no origin to cross.
    pub wraps: bool,
    pub translated: bool,
    /// `#rrggbb`, or empty for "no colour of its own".
    pub color: String,
    /// The `.dna` `<Segment type=>`. No control, never lost — see the module doc.
    kind: String,
}

impl SegRow {
    fn of(s: &Segment, circular: bool) -> Self {
        SegRow {
            start: s.start,
            end: s.end,
            wraps: circular && s.end < s.start,
            translated: s.translated,
            color: s.color.clone().unwrap_or_default(),
            kind: s.kind.clone(),
        }
    }
}

/// One qualifier, with the two empty states kept apart.
#[derive(Debug, Clone, PartialEq)]
pub struct QualRow {
    pub key: String,
    /// `false` is `None` — a bare `/pseudo`. `true` with an empty `value` is
    /// `Some("")` — `/replace=""`. They are different qualifiers and the model
    /// says so; see the module doc for what happened when they were conflated.
    pub has_value: bool,
    pub value: String,
}

/// Where the feature's colour comes from.
///
/// `Segment::color` is per segment and `Feature::color()` returns the *first*
/// segment carrying one, which is what both renderers paint the whole feature
/// with. So one control writes every segment — except when the incoming
/// segments already disagree, which is the only case that gets the per-row
/// boxes. Flattening a file that arrived with disagreeing segment colours, as a
/// side effect of renaming, would be exactly the silent loss this form exists to
/// avoid.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorMode {
    /// `color: None` on every segment — what an ordinary GenBank record is.
    /// `theme::by_kind` then answers, and that is a real answer.
    FromKind,
    /// One colour on every segment.
    One(String),
    /// The segments disagree; each row owns its own.
    PerSegment,
}

/// Which colour control a feature's segments call for.
///
/// Read from the segments and not from `Feature::color()`, which is first-wins
/// and cannot tell "all the same" from "the first one is set". Used both to seed
/// the form and, in [`Panel::file_colours`], to tell a value the user typed from
/// one the file arrived with.
fn color_of(f: &Feature) -> ColorMode {
    let first = f.segments.first().and_then(|s| s.color.as_deref());
    let agreed = f.segments.iter().all(|s| s.color.as_deref() == first);
    if !agreed {
        ColorMode::PerSegment
    } else {
        match first {
            None => ColorMode::FromKind,
            Some(h) => ColorMode::One(h.to_string()),
        }
    }
}

/// The editor's state. `index` mirrors `OpKind::SetFeature`'s own shape exactly:
/// `None` appends, `Some(i)` replaces.
#[derive(Debug)]
pub struct Panel {
    pub index: Option<usize>,
    /// The feature exactly as the document holds it, cloned once on open.
    ///
    /// The read-modify-write base. Every field with no control on screen comes
    /// back out of here untouched.
    base: Feature,

    pub name: String,
    pub kind: String,
    /// The Type combo is on "other…", so the free-text box is showing.
    pub kind_other: bool,
    pub strand: Strand,
    pub segments: Vec<SegRow>,
    pub quals: Vec<QualRow>,
    pub color: ColorMode,

    /// `Molecule::annotation_span()`, which is what `Molecule::validate`
    /// measures against — not `len()`. An annotation-only GenBank has features
    /// and no bases.
    pub span: u64,
    pub circular: bool,

    /// Where the document stood when this panel opened.
    opened_at: Option<pl_core::oplog::OpId>,
    /// Where it stands now, refreshed by `App` every frame.
    pub doc_at: Option<pl_core::oplog::OpId>,

    /// Set by the footer buttons, read and cleared by `App` after the frame.
    pub save: bool,
    pub delete: bool,
    pub close: bool,
    /// Whether the qualifier table is unrolled.
    pub show_quals: bool,

    /// Why the last Save or Delete did not happen, drawn INSIDE this window.
    ///
    /// It used to go to `App::notice`, which `central` paints at the top-left of
    /// the map panel — which is where this window sits by default. Photographed:
    /// the banner rendered behind the editor with a sliver showing, so the user
    /// pressed a button, nothing happened, and the explanation was underneath
    /// the thing they were looking at. A refusal has to be readable from where
    /// the refused gesture was made.
    pub notice: Option<String>,

    /// Bumped by `App` when this panel is opened, and used to salt the body's
    /// scroll id.
    ///
    /// A fixed `egui::Window` id gives a fixed `ScrollArea` id, and a
    /// `ScrollArea` remembers its offset against that id — across closes. So
    /// scrolling to the bottom of SacB, cancelling, and opening CmR showed CmR
    /// already scrolled, with Name and Type above the viewport and nothing but a
    /// thin scrollbar to say so.
    pub generation: u64,
}

impl Panel {
    /// Read a feature into the form, or say why there is nothing to edit.
    ///
    /// `base` is the feature to modify; for an add it is the seed, and it is
    /// still the read-modify-write base so a seeded qualifier survives.
    pub fn open(
        index: Option<usize>,
        base: Feature,
        span: u64,
        circular: bool,
        at: Option<pl_core::oplog::OpId>,
    ) -> Result<Panel, String> {
        // `annotation_span()` falls back to `max(Feature::end())` when the file
        // has neither bases nor a declared length, and BOTH `PastEnd` guards in
        // `Molecule::validate` are behind `n > 0`. So on a molecule with no span
        // at all, a feature at `1..1_000_000` validates clean, the gate accepts
        // it, and it silently *redefines* the molecule's span to 1,000,000 —
        // after which every pre-existing coordinate validates clean forever.
        // There is no coordinate to check against, so there is no coordinate to
        // offer.
        if span == 0 {
            return Err(
                "This file declares no length and carries no bases, so there is \
                        nothing for a coordinate to mean. Nothing was changed."
                    .into(),
            );
        }

        let kind = base.kind.clone();
        let segments: Vec<SegRow> = base
            .segments
            .iter()
            .map(|s| SegRow::of(s, circular))
            .collect();
        let quals: Vec<QualRow> = base
            .qualifiers
            .iter()
            .map(|(k, v)| QualRow {
                key: k.clone(),
                has_value: v.is_some(),
                value: v.clone().unwrap_or_default(),
            })
            .collect();

        let color = color_of(&base);

        Ok(Panel {
            index,
            name: base.name.clone(),
            strand: base.strand,
            kind_other: !KINDS.contains(&kind.as_str()),
            kind,
            segments,
            quals,
            color,
            base,
            span,
            circular,
            opened_at: at,
            doc_at: at,
            save: false,
            delete: false,
            close: false,
            show_quals: true,
            notice: None,
            generation: 0,
        })
    }

    /// Make the "crosses origin" box agree with the numbers beside it.
    ///
    /// **The box is a reading of the pair, never an independent fact**, because
    /// the model has exactly one spelling for an origin crossing: `end < start`.
    /// Recomputing it every frame is what makes the two incapable of
    /// disagreeing, and it replaces two defects that the previous
    /// swap-on-untick rule had between them:
    ///
    /// - Unticking a genuine wrap transposed the pair and re-ticking could not
    ///   put it back — 381..40 (60 bp) became 40..381 (342 bp) and stayed
    ///   there — while the sentence on screen said the user had "typed it the
    ///   other way round", which they had not: they had clicked a checkbox.
    /// - Ticking the box on a row where `start <= end` was silently inert. It
    ///   stayed ticked, `to_feature` wrote a plain forward span, and neither a
    ///   refusal nor a warning mentioned the contradiction.
    ///
    /// Nothing here rewrites a coordinate. The only thing that swaps the pair is
    /// the user clicking the box, and clicking it twice puts it back.
    ///
    /// On a **line** the box is always off — a line has no origin to cross, the
    /// same rule `Selection::clamped` applies — and `end < start` is left
    /// standing so the form can refuse it by name as `Invalid::Inverted` rather
    /// than guess which end was meant.
    ///
    /// Called at the top of every frame and by the tests.
    pub fn normalise(&mut self) {
        let circular = self.circular;
        for r in &mut self.segments {
            r.wraps = circular && r.end < r.start;
        }
    }

    /// The linear pieces this pair really covers, in drawing order.
    ///
    /// A wrap is two pieces — `start..span` and `1..end` — which is what
    /// `pl_draw::ranges` paints and what `genbank::format_location` emits. The
    /// overlap check needs them expanded or it is blind by construction: a
    /// wrapping pair never satisfies `end >= start`, so a second span
    /// double-covering the wrap's tail drew those bases twice and nothing said
    /// so.
    ///
    /// Empty for an inverted pair on a line, which has no arc at all and is
    /// refused by name.
    fn pieces(&self, s: u64, e: u64) -> Vec<(u64, u64)> {
        if e >= s {
            vec![(s, e)]
        } else if self.circular {
            vec![(s, self.span), (1, e)]
        } else {
            Vec::new()
        }
    }

    /// The feature this form describes.
    ///
    /// **A clone of `base`, mutated.** Read the module doc before changing this
    /// to build a `Feature` from the fields.
    pub fn to_feature(&self) -> Feature {
        let mut f = self.base.clone();
        f.name = self.name.clone();
        f.kind = self.kind.clone();
        f.strand = self.strand;
        f.segments = self
            .segments
            .iter()
            .map(|r| {
                // Verbatim. `normalise` keeps `wraps` equal to `end < start`,
                // so there is no reading left to apply and nothing here can
                // move a coordinate the user did not move.
                Segment {
                    start: r.start,
                    end: r.end,
                    color: match &self.color {
                        ColorMode::FromKind => None,
                        ColorMode::One(h) => Some(h.clone()),
                        ColorMode::PerSegment => {
                            let c = r.color.trim();
                            (!c.is_empty()).then(|| c.to_string())
                        }
                    },
                    translated: r.translated,
                    // Verbatim. There is no control for it and there must not be
                    // a default here: `Segment::new` would write "standard" over
                    // whatever the file said.
                    kind: r.kind.clone(),
                }
            })
            .collect();
        f.qualifiers = self
            .quals
            .iter()
            .map(|q| {
                (
                    q.key.clone(),
                    // The whole point of `has_value`. `.then(...)` and not
                    // `(!value.is_empty()).then(...)`: an empty box under
                    // `= value` is `Some("")`, which is `/replace=""`, which is
                    // a different qualifier from a bare `/replace`.
                    q.has_value.then(|| q.value.clone()),
                )
            })
            .collect();
        f
    }

    /// Has the user changed anything at all?
    pub fn dirty(&self) -> bool {
        self.to_feature() != self.base
    }

    /// A Save that would record an operation changing nothing.
    ///
    /// `SetFeature` with a feature bit-identical to the one already there still
    /// derives an id, records an op, dirties the document and spends an undo
    /// step — and the title bar then claims unsaved changes that do not exist.
    /// `Feature: PartialEq`, so this is exact and covers qualifier order and the
    /// `Option` inside every value.
    pub fn is_noop(&self) -> bool {
        self.index.is_some() && !self.dirty()
    }

    /// The molecule the `WouldCorrupt` gate will validate, minus the bases.
    ///
    /// `Molecule::validate` measures annotations against `annotation_span()`,
    /// which for a molecule carrying no bases is the length it declares. So a
    /// one-feature molecule with the real span declared and the real topology
    /// gives *the gate's own answers* about this feature, without cloning 4.6 Mb
    /// of sequence on every frame.
    ///
    /// It exists because the gate is a counter, not a validator: it refuses only
    /// an *increase* per problem kind, so on a file that arrived with one
    /// `PastEnd` an edit that fixes that one and creates another is accepted.
    /// And its refusal reads "feature 9 'PhoP' segment 0: 9000 is past the
    /// 8,117 bp molecule" — for an add, naming a feature that does not exist
    /// after the refusal. The form must never let the user press Save into that.
    pub fn preflight(&self) -> Vec<Invalid> {
        let m = Molecule {
            declared_len: Some(self.span),
            topology: if self.circular {
                Topology::Circular
            } else {
                Topology::Linear
            },
            features: vec![self.to_feature()],
            ..Default::default()
        };
        m.validate()
    }

    /// Why Save is disabled, in the words the user needs, one line per reason.
    ///
    /// Empty means it is allowed.
    pub fn refusals(&self) -> Vec<String> {
        let mut out = Vec::new();

        if self.kind.is_empty() {
            // Not cosmetic. `genbank::write` takes 15 characters of the key and
            // writes `format!("     {key:<15} {loc}")`, so an empty key emits
            // fifteen spaces and a location — a line no parser reads, with
            // nothing on `unwritable` and exit 0. The `.dna` writer keeps the
            // empty string, so the two formats would disagree about the same
            // document.
            out.push(
                "Give this feature a type. GenBank has no line for a feature without one; \
                 misc_feature is the honest default."
                    .into(),
            );
        } else if self.kind.chars().any(char::is_whitespace) {
            // THE TOP OF THE SILENT-CORRUPTION LIST, because "signal peptide" is
            // what a biologist types. `genbank::write` puts it in the key column;
            // the reader splits the body at the first whitespace and takes `misc`
            // as the key and `peptide 3311..3397` as the location; `parse_location`
            // yields nothing; `flush` returns early on `segments.is_empty()`. The
            // feature is GONE after one save and reopen, with exit 0 and an empty
            // `unwritable` report, because the location code never ran.
            out.push(format!(
                "A feature type cannot contain a space: GenBank writes it in the key column, \
                 and \"{}\" would be read back as \"{}\" with the rest taken for coordinates — \
                 the feature disappears on the next open.",
                self.kind,
                self.kind.split_whitespace().next().unwrap_or_default()
            ));
        }

        // ONLY A COLOUR THE USER SET THIS SESSION CAN BLOCK A SAVE. `cyan`,
        // `#0f0` and `green` are all things `genbank::parse` stores verbatim out
        // of an ApE- or Benchling-authored plasmid (`/ApEinfo_fwdcolor="cyan"`
        // reads as `Some("cyan")`), and refusing them meant a file the user
        // never touched came up with Save permanently greyed and a hover blaming
        // a box they had not filled in — with exactly one way out, "from its
        // type", which discards the file's own colour. That is the loss this
        // window exists to prevent, dressed as a validation. A colour the file
        // carried is warned about below and kept; only something new must be
        // readable.
        if let ColorMode::One(h) = &self.color {
            if !hex_ok(h) && !self.came_from_file(h) {
                out.push(format!(
                    "\"{h}\" is not a colour. Colours are #rrggbb — six hex digits."
                ));
            }
        }
        if self.color == ColorMode::PerSegment {
            for (i, r) in self.segments.iter().enumerate() {
                let c = r.color.trim();
                if !c.is_empty() && !hex_ok(c) && !self.came_from_file(c) {
                    out.push(format!(
                        "Segment {}: \"{c}\" is not a colour. Colours are #rrggbb — six hex \
                         digits.",
                        i + 1
                    ));
                }
            }
        }

        out.extend(self.coordinate_refusals());
        out
    }

    /// The five `Invalid` kinds, said at the altitude of the box holding the
    /// number, with `Molecule::validate` itself as the backstop.
    fn coordinate_refusals(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut covered: BTreeSet<&'static str> = BTreeSet::new();

        if self.segments.is_empty() {
            out.push(
                "A feature has to annotate somewhere — give it at least one span, or delete \
                 the feature instead."
                    .into(),
            );
            covered.insert("feature without segments");
        }

        for (i, r) in self.segments.iter().enumerate() {
            let (s, e) = (r.start, r.end);
            let n = i + 1;
            if s == 0 || e == 0 {
                out.push(format!(
                    "Segment {n}: coordinates start at 1, so there is no base 0."
                ));
                covered.insert("zero start");
            }
            if e < s && !self.circular {
                out.push(format!(
                    "Segment {n} ends before it starts ({s}..{e}), and a linear molecule has \
                     no origin to cross."
                ));
                covered.insert("inverted");
            }
            if s > self.span || e > self.span {
                out.push(format!(
                    "Segment {n}: base {} is past the {} bp molecule.",
                    fmt_int(s.max(e)),
                    fmt_int(self.span)
                ));
                covered.insert("past the end");
            }
        }

        // THE BACKSTOP, and it is what makes the loop above a check that can
        // fail. If `Molecule::validate` ever objects to something this form does
        // not, the user still gets a refusal here rather than the gate's
        // whole-molecule sentence after pressing Save.
        for x in self.preflight() {
            if !covered.contains(x.kind()) {
                out.push(x.to_string());
            }
        }
        out
    }

    /// What will be lost or misread, without blocking the Save.
    ///
    /// The split is the one the codebase already makes: refuse what the ENGINE
    /// would refuse, warn about what a WRITER would silently drop.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();

        if self.name.trim().is_empty() {
            out.push(
                "This feature has no name; the list and the map will show only its type.".into(),
            );
        }
        if self.kind.chars().count() > GENBANK_KEY_CHARS {
            let short: String = self.kind.chars().take(GENBANK_KEY_CHARS).collect();
            out.push(format!(
                "GenBank truncates the feature key to {GENBANK_KEY_CHARS} characters and will \
                 save this as \"{short}\"."
            ));
        }

        // Colour is per segment in the model and in `.dna`; `genbank::write`
        // emits `f.color()` — the first colour ANY segment carries — once for
        // the whole feature, and the reader paints every segment with it. Said
        // where the swatches are, so the user does not discover it.
        if self.color == ColorMode::PerSegment && self.segments.len() > 1 {
            out.push(
                "GenBank keeps one colour per feature: a .gb save writes the first segment's \
                 colour and reads it back onto all of them. .dna keeps them apart."
                    .into(),
            );
        }
        // The colour the file carried that this program cannot read. It is NOT
        // refused (see `refusals`) — it is kept byte for byte and both writers
        // emit it — but the map draws the type colour, and a user who never
        // learns that will think the swatch is broken.
        for h in self.unreadable_file_colours() {
            out.push(format!(
                "This file's colour \"{h}\" is not one this program can read — colours are \
                 #rrggbb — so the map draws the type colour instead. It is kept exactly as it \
                 is and written back unchanged."
            ));
        }
        // `Segment::translated` is a `.dna` field with no GenBank spelling: the
        // format has no way to say "draw the amino-acid track here". Warned
        // because the tick is a control the user just interacted with, and
        // because SacB — the feature they are most likely to open — arrives with
        // it set on both segments.
        if self.segments.iter().any(|r| r.translated) {
            out.push(
                "GenBank has no way to record the amino-acid track, so a .gb save loses the \
                 \"aa\" ticks. .dna keeps them."
                    .into(),
            );
        }

        // Segment geometry: legal, drawn oddly, never silently rewritten.
        let spans: Vec<(u64, u64)> = self.segments.iter().map(|r| (r.start, r.end)).collect();
        // An origin crossing is only RECOGNISABLE as such in the shape the
        // writer emits — `Feature::extent` reads it off the last two parts of a
        // join — so a wrap sharing a feature with any other span has no
        // unambiguous GenBank spelling. Measured: `[(381,40),(100,150)]` on a
        // 400 bp circle writes as `join(381..400,1..40,100..150)` and reloads as
        // three plain segments whose `extent` is `1..400`, i.e. a 111 bp feature
        // that the Features list then reports as the whole plasmid.
        if self.segments.len() > 1 {
            if let Some(i) = self.segments.iter().position(|r| r.wraps) {
                out.push(format!(
                    "Segment {} crosses the origin and is not this feature's only span. GenBank \
                     can only spell a crossing as the last two parts of a join(...), so on the \
                     next .gb open this feature reads as one long span from base 1. .dna keeps \
                     it.",
                    i + 1
                ));
            }
        }
        for i in 1..spans.len() {
            if spans[i].0 < spans[i - 1].0 && !self.segments[i].wraps && !self.segments[i - 1].wraps
            {
                out.push(format!(
                    "Segments {} and {} are out of ascending order. That is legal — a GenBank \
                     join's order IS the reading order — and nothing here will sort them; the \
                     map draws the connector between them in file order.",
                    i,
                    i + 1
                ));
                break;
            }
        }
        // EXPANDED FIRST, and over every pair rather than neighbours only. The
        // old test was `a1 >= a0 && b0 <= a1 && a0 <= b1`, which a wrapping pair
        // can never satisfy: a wrap plus an ordinary exon covering the wrap's
        // tail double-drew those bases and nothing said so. And the partner in
        // an overlap need not be adjacent once a wrap has split the order.
        'outer: for i in 0..spans.len() {
            for j in (i + 1)..spans.len() {
                for (a0, a1) in self.pieces(spans[i].0, spans[i].1) {
                    for (b0, b1) in self.pieces(spans[j].0, spans[j].1) {
                        if b0 <= a1 && a0 <= b1 {
                            out.push(format!(
                                "Segments {} and {} overlap at {}..{}; the map will draw those \
                                 bases twice. Sometimes that is right — a -1 frameshift really \
                                 does read some bases twice.",
                                i + 1,
                                j + 1,
                                fmt_int(a0.max(b0)),
                                fmt_int(a1.min(b1))
                            ));
                            break 'outer;
                        }
                    }
                }
            }
        }

        // Qualifier keys `genbank::write` skips outright, and the one value it
        // re-reads as something else.
        let mut said_skip = false;
        for (i, q) in self.quals.iter().enumerate() {
            let n = i + 1;
            if !said_skip {
                if q.key == "label" {
                    out.push(format!(
                        "Qualifier {n}: GenBank writes the Name box as /label, so this one will \
                         not reach a .gb file. Rename the feature instead. (.dna keeps it.)"
                    ));
                    said_skip = true;
                } else if q.key.starts_with("ApEinfo") {
                    out.push(format!(
                        "Qualifier {n}: ApEinfo_* is how colour is written to GenBank; use the \
                         colour control. This one will not reach a .gb file."
                    ));
                    said_skip = true;
                } else if q.key.is_empty()
                    // `genbank::write`'s own guard, character for character.
                    || !q.key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    out.push(format!(
                        "Qualifier {n}: GenBank qualifier names are letters, digits and \
                         underscore. This one will not reach a .gb file."
                    ));
                    said_skip = true;
                }
            }
            // `genbank::parse` re-reads a `#rrggbb` inside a /note as the
            // feature's COLOUR, so `/note="clone #1a2b3c from the -80"` round-
            // trips as a recolour. A reader defect, but this is the first
            // surface that lets a user type such a string.
            if q.key == "note" && q.has_value {
                if let Some(h) = embedded_hex(&q.value) {
                    out.push(format!(
                        "Qualifier {n}: a note containing {h} is read back as this feature's \
                         colour on the next GenBank open."
                    ));
                    break;
                }
            }
        }

        // A LINE BREAK INSIDE A VALUE, which this window is the first surface in
        // the program that can produce: the value box is the only
        // `TextEdit::multiline` in the GUI, and one Enter — or a pasted note, or
        // a `/translation` copied out of another tool — puts one there. GenBank
        // has no spelling for it at all: the reader joins a continuation line to
        // the one before it with a space (`""` for `/translation`), so the break
        // comes back as a space at best. The `.dna` writer keeps the value byte
        // for byte, so the two formats disagree about the same document.
        //
        // `qualifier_lines_opt` now emits the pieces as proper continuation
        // lines and `write_reporting` reports the loss, so this no longer
        // destroys the qualifier that FOLLOWS it — but the break itself is still
        // gone, and that is the user's to know before they save.
        if let Some((i, _)) = self
            .quals
            .iter()
            .enumerate()
            .find(|(_, q)| q.has_value && q.value.contains(['\n', '\r']))
        {
            out.push(format!(
                "Qualifier {}: a GenBank value cannot contain a line break — the break comes \
                 back as a space on the next .gb open. .dna keeps it exactly.",
                i + 1
            ));
        }

        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for q in &self.quals {
            if !seen.insert(q.key.as_str()) {
                out.push(format!(
                    "\"{}\" appears more than once. That is legal and is kept in order; said \
                     only in case it was not meant.",
                    q.key
                ));
                break;
            }
        }

        // `Unoriented` is a real state SnapGene stores and GenBank cannot
        // express. pKoV has three such features.
        if matches!(self.strand, Strand::Unoriented | Strand::Both) {
            out.push(
                "GenBank has no way to write this strand: a location is either plain or \
                 wrapped in complement(), so a .gb save records it as forward. .dna keeps it."
                    .into(),
            );
        }
        out
    }

    /// Is this colour string one the FILE brought in, rather than one the user
    /// typed or clicked?
    ///
    /// A set membership and not an index comparison, deliberately: rows can be
    /// added, removed and reordered, and a `+ span` inherits its neighbour's
    /// colour, so "the colour that was at row 2" is not a stable question. Every
    /// colour the incoming feature carried is grandfathered; anything else has
    /// to be readable.
    fn came_from_file(&self, hex: &str) -> bool {
        self.base
            .segments
            .iter()
            .any(|s| s.color.as_deref() == Some(hex))
    }

    /// Every colour the file brought in that `theme::parse_hex` cannot read.
    fn unreadable_file_colours(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in &self.base.segments {
            if let Some(h) = &s.color {
                if !hex_ok(h) && !out.contains(h) {
                    out.push(h.clone());
                }
            }
        }
        out
    }

    /// Why this edit cannot be committed, if the document moved under it.
    ///
    /// `RemoveFeature` shifts every later index and `remap_annotations` drops a
    /// feature whose bases were all deleted, so a held `Some(i)` can come to
    /// name a different feature entirely — and Save would then destroy it with a
    /// perfectly valid operation. Content-addressed, the same identity autosave
    /// uses, so an edit and its undo land back on the same id and the panel is
    /// live again, which is right: it is live again.
    pub fn stale_reason(&self) -> Option<&'static str> {
        (self.doc_at != self.opened_at).then_some(
            "The document has changed since this feature was opened, so this form may no \
             longer describe the feature it was opened on. Nothing was changed — close the \
             editor and open it again.",
        )
    }

    /// The base count this row covers, reading the wrap answer.
    ///
    /// Not `Segment::len()`, which returns 0 for an inverted span and says why:
    /// the real length of a wrap needs the molecule.
    pub fn row_bases(&self, r: &SegRow) -> u64 {
        let (s, e) = (r.start, r.end);
        if e >= s {
            e.saturating_sub(s).saturating_add(1)
        } else if self.circular && s <= self.span {
            // The same expression `select_feature_under` uses.
            self.span - s + 1 + e
        } else {
            0
        }
    }

    fn title(&self) -> String {
        match self.index {
            Some(i) => format!("Feature {i}"),
            None => "New feature".into(),
        }
    }
}

/// `#rrggbb`, exactly, because that is all `theme::parse_hex` reads.
///
/// A colour it cannot read is stored faithfully in the model and written
/// faithfully to the file, then rendered as the *type* colour with nothing
/// anywhere saying the value was ignored. `#abc` is refused rather than
/// expanded: `parse_hex` does not expand the short form, so it would round-trip
/// as no colour at all.
pub fn hex_ok(s: &str) -> bool {
    match s.strip_prefix('#') {
        Some(h) => h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()),
        None => false,
    }
}

/// Add the `#` a paste usually loses. Anything else is returned untouched, so
/// the refusal above sees exactly what the user typed.
pub fn tidy_hex(s: &str) -> String {
    let t = s.trim();
    if t.len() == 6 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        format!("#{t}")
    } else {
        t.to_string()
    }
}

/// The colour `genbank::parse` would read out of this note, if it would read
/// one.
///
/// Deliberately `genbank::parse`'s own expression and not a general hex scan:
/// the reader splits at the **first** `#` and takes six hex characters, so
/// `/note="clone #1a2b3c from the -80"` comes back as a recolour and
/// `/note="lot 12#ab"` does not. A different rule here would warn about notes
/// that are fine and stay quiet about the ones that are not.
fn embedded_hex(s: &str) -> Option<String> {
    let after = s.split('#').nth(1)?;
    let hex: String = after
        .chars()
        .take(6)
        .take_while(char::is_ascii_hexdigit)
        .collect();
    (hex.len() == 6).then(|| format!("#{hex}"))
}

// ---------------------------------------------------------------------------
// the window
// ---------------------------------------------------------------------------

/// Draw the editor. Returns false when it should close.
///
/// `sel` is the sequence selection as a 1-based inclusive pair plus its wrap
/// bit, or `None`. It is passed in rather than read, so this module never
/// touches the document.
pub fn show(
    ctx: &egui::Context,
    panel: &mut Panel,
    sel: Option<(u64, u64, bool)>,
    dark: bool,
) -> bool {
    let mut open = true;
    egui::Window::new(panel.title())
        // Fixed, so the window does not jump when "New feature" becomes
        // "Feature 12" after the first Save.
        .id(egui::Id::new("pl-feature-editor"))
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .open(&mut open)
        .show(ctx, |ui| body(ui, panel, sel, dark));
    open && !panel.close
}

fn body(ui: &mut Ui, panel: &mut Panel, sel: Option<(u64, u64, bool)>, dark: bool) {
    // First, so the correction a transposed pair gets is on screen in the same
    // frame `to_feature` would apply it.
    panel.normalise();
    let pal = Palette::of(dark);

    ui.label(
        RichText::new(format!(
            "{} bp {} — coordinates are 1-based and inclusive",
            fmt_int(panel.span),
            if panel.circular { "circular" } else { "linear" }
        ))
        .monospace()
        .size(11.0)
        .color(pal.ink2),
    );
    if let Some(why) = panel.stale_reason() {
        ui.add_space(4.0);
        ui.label(RichText::new(why).color(pal.warn).size(11.0));
    }
    // In here, not in `App::notice`, which this window would be covering.
    if let Some(msg) = panel.notice.clone() {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(msg).color(pal.warn).size(11.0));
            if ui.small_button("Dismiss").clicked() {
                panel.notice = None;
            }
        });
    }
    ui.add_space(6.0);

    // THE FOOTER IS OUTSIDE THIS, AND THAT IS THE POINT. Photographed without
    // it: SacB carries a 476-character `/translation`, the qualifier table grew
    // past the bottom of the screen, and Save, Cancel and Delete were all
    // unreachable — on the very feature the user is most likely to open. A form
    // whose commit button can be pushed off the screen by its own content is a
    // form that cannot be used on a real CDS.
    let mut refusals: Vec<String> = Vec::new();
    egui::ScrollArea::vertical()
        // Salted with the open count, so each open gets a fresh scroll state.
        // See `Panel::generation` for what a remembered offset looked like.
        .id_salt(("pl-feature-body", panel.generation))
        .max_height(BODY_MAX_H)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            scalars(ui, panel, &pal);
            ui.add_space(6.0);
            colour(ui, panel, &pal);
            ui.add_space(8.0);
            segments(ui, panel, sel, &pal);
            ui.add_space(8.0);
            qualifiers(ui, panel, &pal);

            ui.add_space(6.0);
            let r = panel.refusals();
            for x in &r {
                ui.label(RichText::new(x).color(pal.warn).size(11.0));
            }
            for w in panel.warnings() {
                ui.label(RichText::new(w).color(pal.muted).size(11.0));
            }
            // Same frame, not last frame: a Save button that is still disabled
            // after the user has fixed the last refusal — or still enabled after
            // they have broken something — is a button lying about the state of
            // the form it belongs to.
            refusals = r;
        });

    ui.add_space(6.0);
    ui.separator();
    footer(ui, panel, &refusals, &pal);
}

fn scalars(ui: &mut Ui, panel: &mut Panel, pal: &Palette) {
    egui::Grid::new("pl-feature-scalars")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Name");
            ui.add(
                egui::TextEdit::singleline(&mut panel.name)
                    .desired_width(f32::INFINITY)
                    .hint_text("SacB"),
            );
            ui.end_row();

            ui.label("Type");
            ui.horizontal(|ui| {
                let selected = if panel.kind_other {
                    "other…".to_string()
                } else {
                    panel.kind.clone()
                };
                egui::ComboBox::from_id_salt("pl-feature-kind")
                    .selected_text(selected)
                    .width(170.0)
                    .show_ui(ui, |ui| {
                        for k in KINDS {
                            if ui
                                .selectable_label(!panel.kind_other && panel.kind == *k, *k)
                                .clicked()
                            {
                                panel.kind = (*k).to_string();
                                panel.kind_other = false;
                            }
                        }
                        if ui.selectable_label(panel.kind_other, "other…").clicked() {
                            panel.kind_other = true;
                        }
                    })
                    .response
                    .on_hover_text(
                        "The GenBank feature key. The list is a suggestion — any key a file \
                         carries is kept — but the spellings are INSDC's exactly: CDS upper, \
                         rep_origin lower, because the colour tables match the whole string.",
                    );
                if panel.kind_other {
                    ui.add(
                        egui::TextEdit::singleline(&mut panel.kind)
                            .desired_width(150.0)
                            .hint_text("any GenBank key"),
                    );
                }
            });
            ui.end_row();

            ui.label("Strand");
            ui.horizontal(|ui| {
                // Glyph AND word on every option. `strand_glyph` and
                // `strand_word` exist as a pair precisely so direction is never
                // carried by a glyph alone, and all FOUR states are offered:
                // a two-state toggle silently promotes `Unoriented` to
                // `Forward`, a directional claim the file never made. pKoV has
                // three such features.
                for s in [
                    Strand::Forward,
                    Strand::Reverse,
                    Strand::Both,
                    Strand::Unoriented,
                ] {
                    let label = format!("{} {}", crate::strand_glyph(s), crate::strand_word(s));
                    ui.selectable_value(&mut panel.strand, s, label);
                }
            });
            ui.end_row();
        });
    let _ = pal;
}

fn colour(ui: &mut Ui, panel: &mut Panel, pal: &Palette) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Colour").color(pal.ink2));

        let from_kind = panel.color == ColorMode::FromKind;
        if ui
            .selectable_label(from_kind, "from its type")
            .on_hover_text(
                "A feature with no colour of its own is what an ordinary GenBank record is, \
                 and the type colour is a real answer rather than a fallback for a broken one.",
            )
            .clicked()
        {
            panel.color = ColorMode::FromKind;
        }

        let per_seg = panel.color == ColorMode::PerSegment;
        if ui
            .selectable_label(per_seg, "per segment")
            .on_hover_text(
                "Only worth it when the segments really do disagree. The map paints the whole \
                 feature with the FIRST colour any segment carries.",
            )
            .clicked()
        {
            // Seed every row from the single colour, so switching to per-segment
            // does not blank them.
            if let ColorMode::One(h) = &panel.color {
                let h = h.clone();
                for r in &mut panel.segments {
                    r.color = h.clone();
                }
            }
            panel.color = ColorMode::PerSegment;
        }
    });

    if panel.color == ColorMode::PerSegment {
        ui.label(
            RichText::new("each row below carries its own")
                .size(11.0)
                .color(pal.muted),
        );
        return;
    }

    // The eight, each with its NAME as text. A row of eight coloured squares is
    // colour as the only channel, which is the one thing this project's
    // accessibility pass forbids — and the person most likely to need a CVD-safe
    // palette is the least able to pick from unlabelled swatches.
    ui.horizontal_wrapped(|ui| {
        for (name, hex) in pl_draw::contrast::OKABE_ITO {
            let on = matches!(&panel.color, ColorMode::One(h) if h.eq_ignore_ascii_case(hex));
            if swatch(ui, name, hex, on).clicked() {
                panel.color = ColorMode::One((*hex).to_string());
            }
        }
    });
    ui.horizontal(|ui| {
        let mut text = match &panel.color {
            ColorMode::One(h) => h.clone(),
            _ => String::new(),
        };
        let r = ui.add(
            egui::TextEdit::singleline(&mut text)
                .desired_width(90.0)
                .hint_text("#rrggbb"),
        );
        if r.changed() {
            let t = tidy_hex(&text);
            panel.color = if t.is_empty() {
                ColorMode::FromKind
            } else {
                ColorMode::One(t)
            };
        }
        if r.lost_focus() {
            if let ColorMode::One(h) = &panel.color {
                let t = tidy_hex(h);
                panel.color = ColorMode::One(t);
            }
        }
        ui.label(
            RichText::new("any of the 16.7 million; the file's own colour always wins")
                .size(11.0)
                .color(pal.muted),
        );
    });
}

/// One named swatch, with its name drawn OVER it in the ink `theme::on_color`
/// guarantees clears 4.58:1 for every colour in the RGB cube.
fn swatch(ui: &mut Ui, name: &str, hex: &str, on: bool) -> egui::Response {
    let c = crate::theme::parse_hex(hex).unwrap_or(egui::Color32::GRAY);
    let galley = ui.painter().layout_no_wrap(
        name.to_string(),
        egui::FontId::proportional(10.5),
        crate::theme::on_color(c),
    );
    let size = egui::vec2(galley.size().x + 12.0, 19.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, egui::CornerRadius::same(3), c);
    if on {
        p.rect_stroke(
            rect,
            egui::CornerRadius::same(3),
            egui::Stroke::new(2.0, crate::theme::on_color(c)),
            egui::StrokeKind::Inside,
        );
    }
    p.galley(
        rect.center() - galley.size() / 2.0,
        galley,
        crate::theme::on_color(c),
    );
    resp.on_hover_text(format!("{name}  {hex}"))
}

fn segments(ui: &mut Ui, panel: &mut Panel, sel: Option<(u64, u64, bool)>, pal: &Palette) {
    ui.label(RichText::new("Spans").color(pal.ink2));
    let n = panel.span;
    let circular = panel.circular;
    let mut remove = None;
    let mut move_up = None;
    let rows = panel.segments.len();

    // Read before the mutable loop: the length and the note need `panel`.
    let bases: Vec<u64> = panel.segments.iter().map(|r| panel.row_bases(r)).collect();
    let inverted: Vec<bool> = panel
        .segments
        .iter()
        .map(|r| !r.wraps && r.end < r.start)
        .collect();

    egui::Grid::new("pl-feature-segments")
        .num_columns(8)
        .spacing([8.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for (i, r) in panel.segments.iter_mut().enumerate() {
                ui.label(
                    RichText::new(format!("{}", i + 1))
                        .monospace()
                        .size(11.0)
                        .color(if inverted[i] { pal.warn } else { pal.muted }),
                );
                // `range(1..=n)` keeps `Invalid::ZeroStart` and
                // `Invalid::PastEnd` — two of the five kinds the gate refuses —
                // untypeable. `n` is `annotation_span()`, not `len()`, because
                // that is what `validate()` measures against.
                //
                // `clamp_existing_to_range(false)` IS LOAD-BEARING AND IS NOT A
                // TIDY-UP. egui's default is `true`, and `true` means the widget
                // rewrites a value it was merely asked to DRAW: one layout pass,
                // no input, `set()` called, the panel dirty. This repository's
                // own `tests/library-fixture/odd.gb` is a 7 bp record carrying
                // `CDS 1..9` and `misc_feature 10..15`; with the default, opening
                // "spacer" and typing a new NAME committed `7..7` — five of its
                // six bases gone and the feature moved three bases — with no
                // refusal, no warning, `notice == None`, and a status line that
                // said only "edit feature 1". The gate cannot help: the edit
                // *reduces* the `PastEnd` count, which `refuse_new_problems`
                // reads as an improvement. The refusal below is already written
                // and already says the right sentence; the widget must show the
                // user the number the file holds and let them decide.
                //
                // User input is still clamped — egui clamps the drag and the
                // typed value separately (drag_value.rs:661 and :548/:588) — so
                // 9,000 typed into a 400 bp molecule still cannot be entered.
                ui.add(
                    egui::DragValue::new(&mut r.start)
                        .range(1..=n)
                        .clamp_existing_to_range(false)
                        .speed(1.0),
                );
                ui.add(
                    egui::DragValue::new(&mut r.end)
                        .range(1..=n)
                        .clamp_existing_to_range(false)
                        .speed(1.0),
                );
                // Ticking or unticking is a SWAP and nothing else, because the
                // model has exactly one spelling for a crossing — `end < start`
                // — so the box is a reading of the pair rather than an
                // independent fact. That makes the two clicks an involution:
                // 381..40 unticks to 40..381 and re-ticks to 381..40. The
                // previous design swapped on untick and could not swap back,
                // so a 60 bp feature became a 342 bp one and re-ticking left a
                // box that was ticked and inert.
                //
                // Disabled when the two numbers are equal: one base names one
                // arc, so there is nothing to choose, and a box that silently
                // un-ticked itself on the next frame would be worse than one
                // that says why it is unavailable.
                ui.add_enabled_ui(circular && r.start != r.end, |ui| {
                    let resp = ui
                        .checkbox(&mut r.wraps, "crosses origin")
                        .on_hover_text(
                            "Two coordinates on a circle name two arcs, and no ordering of the \
                             pair says which. Ticked, this span runs from the first number \
                             forwards through base 1 to the second. Clicking swaps them, so \
                             clicking twice puts them back.",
                        )
                        .on_disabled_hover_text(if circular {
                            "Both numbers are the same, so there is only one arc to name."
                        } else {
                            "A line has no origin to cross."
                        });
                    if resp.changed() {
                        std::mem::swap(&mut r.start, &mut r.end);
                    }
                });
                ui.label(
                    RichText::new(format!("{} bp", fmt_int(bases[i])))
                        .monospace()
                        .size(11.0)
                        .color(pal.muted),
                );
                ui.checkbox(&mut r.translated, "aa").on_hover_text(
                    "Draws the amino-acid track under this segment in the Sequence tab \n                         (Show > from file), whatever the feature's kind — the feature \n                         needs a strand, because a reading has a direction. It is \n                         round-tripped through .dna; GenBank has no way to record it and \n                         a .gb save loses it silently.",
                );
                if panel.color == ColorMode::PerSegment {
                    // `add_sized`, NOT `desired_width` — the same trap, in the
                    // same file, that the qualifier key box documents 140 lines
                    // below: a `TextEdit` sizes to `min(desired, available)` and
                    // inside an `egui::Grid` `available` is last frame's column
                    // width, which never grows for a widget that only ever asks
                    // for what is available. Photographed: `#993366` rendered as
                    // `#993`, in the one control whose entire job is the exact
                    // colour, on the code path that exists so disagreeing
                    // segment colours are not silently flattened.
                    ui.add_sized(
                        [COLOR_W, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut r.color).hint_text("#rrggbb"),
                    );
                } else {
                    ui.label("");
                }
                ui.horizontal(|ui| {
                    if let Some((a, b, _w)) = sel {
                        if ui
                            .small_button("sel")
                            .on_hover_text("take the sequence selection")
                            .clicked()
                        {
                            // `wraps` is not taken from the selection: it is
                            // recomputed from the pair by `normalise`, and the
                            // selection already spells a through-origin arc the
                            // same way the model does, with `end < start`.
                            r.start = a;
                            r.end = b;
                        }
                    }
                    ui.add_enabled_ui(i > 0, |ui| {
                        if ui
                            .small_button(UP)
                            .on_hover_text(
                                "Order is output: a GenBank join's order is the reading order, \
                                 and `extent` recognises an origin crossing only in the shape \
                                 the writer emits. Nothing sorts these behind your back.",
                            )
                            .clicked()
                        {
                            move_up = Some(i);
                        }
                    });
                    // Disabled on the last remaining row: a feature with no
                    // segments is `Invalid::FeatureWithoutSegments` and the gate
                    // refuses the whole edit. Delete the FEATURE instead.
                    ui.add_enabled_ui(rows > 1, |ui| {
                        if ui
                            .small_button(DELETE)
                            .on_hover_text("remove this span")
                            .on_disabled_hover_text(
                                "A feature must cover at least one span — delete the feature \
                                 instead.",
                            )
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                });
                ui.end_row();
            }
        });

    // What a ticked box actually means, in bases and in numbers, because the
    // tick is the only thing on the row that says it and the bp figure beside it
    // is a number the user has no baseline for. It also states the way out: this
    // box swaps, so clicking it twice is a no-op.
    for (i, r) in panel.segments.iter().enumerate() {
        if r.wraps {
            ui.label(
                RichText::new(format!(
                    "Segment {} runs from {} forward through base 1 to {} — {} bp. Untick \
                     \"crosses origin\" to read the same two numbers the other way round \
                     ({}..{}).",
                    i + 1,
                    fmt_int(r.start),
                    fmt_int(r.end),
                    fmt_int(panel.row_bases(r)),
                    fmt_int(r.end),
                    fmt_int(r.start),
                ))
                .color(pal.muted)
                .size(11.0),
            );
        }
    }

    ui.horizontal(|ui| {
        if ui
            .button("+ span")
            .on_hover_text(
                "Multi-segment is the normal case, not an edge: a spliced CDS, a fusion, or a \
                 CDS whose signal peptide is drawn apart from the rest.",
            )
            .clicked()
        {
            // Colour and `kind` inherited from the neighbour. A new segment with
            // `color: None` inserted first would silently promote the NEXT
            // segment's colour to the whole feature's map colour, because
            // `Feature::color()` is first-wins.
            let last = panel.segments.last().cloned();
            let (color, kind) = match &last {
                Some(p) => (p.color.clone(), p.kind.clone()),
                None => (String::new(), "standard".to_string()),
            };
            // AFTER the previous span, not ON its last base. Seeding at `p.end`
            // made the default state of a freshly added row one the form itself
            // immediately warned about — "Segments 2 and 3 overlap at 260..260"
            // — on the button whose whole reason for existing is a spliced CDS.
            let start = last
                .as_ref()
                .map(|p| p.end.saturating_add(1).min(n))
                .unwrap_or(1)
                .max(1);
            panel.segments.push(SegRow {
                start,
                end: start,
                wraps: false,
                translated: last.as_ref().is_some_and(|p| p.translated),
                color,
                kind,
            });
        }
    });

    if let Some(i) = move_up {
        panel.segments.swap(i - 1, i);
    }
    if let Some(i) = remove {
        panel.segments.remove(i);
    }
}

fn qualifiers(ui: &mut Ui, panel: &mut Panel, pal: &Palette) {
    let n = panel.quals.len();
    ui.horizontal(|ui| {
        // `CollapsingHeader` and not a `▾`/`▸` label. Both of those are tofu
        // here — measured, U+25BE and U+25B8 are in none of the embedded faces,
        // which is the same trap `menu_with_caret` records and photographs. This
        // widget paints its triangle as a SHAPE, so it cannot ask a font for a
        // character the font has not got.
        let r = egui::CollapsingHeader::new(format!("Qualifiers ({n})"))
            .id_salt("pl-feature-quals-head")
            .open(Some(panel.show_quals))
            .show_unindented(ui, |_| {});
        if r.header_response.clicked() {
            panel.show_quals = !panel.show_quals;
        }
        ui.label(
            RichText::new("kept in file order, repeats and all")
                .size(11.0)
                .color(pal.muted),
        );
    });
    if !panel.show_quals {
        return;
    }

    let mut remove = None;
    let mut move_up = None;
    egui::Grid::new("pl-feature-quals")
        .num_columns(5)
        .spacing([8.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for (i, q) in panel.quals.iter_mut().enumerate() {
                // `add_sized`, NOT `desired_width`, and that is not a style
                // choice. A `TextEdit` sizes itself to `min(desired,
                // available)`, and inside an `egui::Grid` `available` is the
                // column width the grid measured LAST frame — which starts at
                // `min_col_width` and can only grow if something in the column
                // asks for more. A widget that asks for "whatever is available"
                // never does, so the column stays at its minimum forever.
                // Photographed twice: the key box read "codor" and "transl",
                // making `codon_start` and `transl_table` indistinguishable at a
                // glance in the one table whose job is naming them.
                ui.add_sized(
                    [KEY_W, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut q.key).hint_text("note"),
                );
                // TWO selectable values and NOT a text box, because a text box
                // has one empty state and the model has two. There is no way to
                // reach `None` by clearing the box and no way to reach `Some("")`
                // by picking the flag, so a slip cannot confuse them.
                ui.horizontal(|ui| {
                    let hover = "A flag qualifier is written bare — /pseudo. \"= value\" with \
                                 the box empty is written /replace=\"\", which is a different \
                                 thing.";
                    if ui
                        .selectable_label(q.has_value, "= value")
                        .on_hover_text(hover)
                        .clicked()
                    {
                        q.has_value = true;
                    }
                    if ui
                        .selectable_label(!q.has_value, "flag")
                        .on_hover_text(hover)
                        .clicked()
                    {
                        q.has_value = false;
                    }
                });
                if q.has_value {
                    // Multiline and SCROLLING IN ITS OWN BOX, not elided and not
                    // free to grow. `desired_width` is a wish, and inside a Grid
                    // cell it lost: photographed, SacB's 476-character
                    // `/translation` came out four characters wide and thirty
                    // rows tall, and pushed the Save button off the screen.
                    // `allocate_ui` fixes the width so the wrap is right, and the
                    // inner ScrollArea bounds the height so a long value is
                    // readable in full without owning the window.
                    ui.allocate_ui(egui::vec2(VALUE_W, VALUE_H), |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(("pl-feature-qual-value", i))
                            .max_height(VALUE_H)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut q.value)
                                        .desired_width(VALUE_W - 14.0)
                                        .desired_rows(1)
                                        .font(egui::TextStyle::Monospace),
                                );
                            });
                    });
                } else {
                    let key = if q.key.is_empty() { "pseudo" } else { &q.key };
                    ui.label(
                        RichText::new(format!("(no value — written /{key})"))
                            .size(11.0)
                            .color(pal.muted),
                    );
                }
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(i > 0, |ui| {
                        if ui
                            .small_button(UP)
                            .on_hover_text("qualifier order is file order, and it is kept")
                            .clicked()
                        {
                            move_up = Some(i);
                        }
                    });
                    if ui
                        .small_button(DELETE)
                        .on_hover_text("remove this qualifier")
                        .clicked()
                    {
                        remove = Some(i);
                    }
                });
                ui.end_row();
            }
        });
    if ui.button("+ qualifier").clicked() {
        // `= value` by default: the 11,716-to-4 ratio compares the two
        // EMPTY-LOOKING states, and someone reaching for this button is usually
        // adding a /note.
        panel.quals.push(QualRow {
            key: String::new(),
            has_value: true,
            value: String::new(),
        });
    }
    if let Some(i) = move_up {
        panel.quals.swap(i - 1, i);
    }
    if let Some(i) = remove {
        panel.quals.remove(i);
    }
}

fn footer(ui: &mut Ui, panel: &mut Panel, refusals: &[String], pal: &Palette) {
    ui.horizontal(|ui| {
        if panel.index.is_some() {
            // Gated on the SAME staleness Save is gated on. Drawn live beside a
            // greyed-out Save, the two buttons made contradictory claims about
            // one state — and the only one that was disabled was the safe one.
            // Pressing it did nothing (`App::feature_editor` re-checks), which
            // is worse than not offering it: the user got a refusal instead of
            // the action the button advertised.
            let live = panel.stale_reason().is_none();
            // THE COLOUR GOES ON THE WIDGET STATE AND NOT ON THE STRING, and
            // the difference is a state nobody could read.
            //
            // This was `Button::new(RichText::new(..).color(..))`, which pins
            // one colour through every state the button can be in. That was
            // free until the design-system port made `widgets.active.bg_fill`
            // the accent: a `warn` label on the pressed fill measures
            // **2.10:1** in dark mode and 2.21 in light, so for as long as the
            // mouse is held the destructive verb is the one thing on screen
            // that cannot be read. The light half is not new — egui's own
            // `gray(165)` gave 2.02 — but the dark half is: `gray(55)` gave
            // 4.56.
            //
            // Written into `inactive.fg_stroke` instead, the red says
            // "destructive" AT REST, which is when it is read and decided on,
            // and egui's own hovered and pressed label ink takes over the
            // moment the pointer arrives — `Palette::ink`, 9.68:1 on the hover
            // fill and 4.65:1 on the pressed one. The disabled colour moves to
            // `noninteractive.fg_stroke` by the same route, and
            // `a_stale_form_greys_the_delete_button_as_well_as_save` still
            // reads both off the galley, because a `Button`'s text colour is
            // exactly its state's `fg_stroke` when the string does not override
            // it.
            let del = ui
                .scope(|ui| {
                    ui.visuals_mut().widgets.inactive.fg_stroke.color = pal.warn;
                    ui.add_enabled(
                        live,
                        if live {
                            egui::Button::new("Delete feature")
                        } else {
                            // The DISABLED half keeps its colour in the string,
                            // exactly as it shipped, and the asymmetry is the
                            // argument rather than an oversight: a disabled
                            // control is never hovered and never pressed, so
                            // there is no state for a pinned colour to be wrong
                            // in. It is also the half egui would otherwise draw
                            // in `inactive.fg_stroke` — the red set above —
                            // faded to half alpha, which is a pale red and not
                            // the grey `Save` uses beside it.
                            egui::Button::new(RichText::new("Delete feature").color(pal.muted))
                        },
                    )
                })
                .inner;
            if del.clicked() {
                panel.delete = true;
            }
            if live {
                del.on_hover_text("removes the whole feature — Ctrl+Z puts it back");
            } else {
                del.on_disabled_hover_text(panel.stale_reason().unwrap_or_default());
            }
            ui.separator();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ok = refusals.is_empty() && panel.stale_reason().is_none();
            let save = ui.add_enabled(ok, egui::Button::new("Save"));
            if save.clicked() {
                panel.save = true;
            }
            if !ok {
                save.on_disabled_hover_text(
                    refusals
                        .first()
                        .cloned()
                        .or_else(|| panel.stale_reason().map(|s| s.to_string()))
                        .unwrap_or_default(),
                );
            }
            if ui.button("Cancel").clicked() {
                panel.close = true;
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A feature carrying every one of the things this form can silently
    /// destroy, so that a test built on it can actually detect the loss.
    ///
    /// Shaped on the user's own pKoV `SacB` — two abutting segments, both
    /// coloured, both translated, a reverse strand and repeated qualifiers —
    /// plus the one thing pKoV does not have: a **valueless** qualifier. There
    /// is no `/pseudo` anywhere in that file, and it is the sharpest of the set.
    fn fixture() -> Feature {
        let mut f = Feature::new("SacB", "CDS");
        f.strand = Strand::Reverse;
        f.segments = vec![
            Segment {
                start: 1976,
                end: 3310,
                color: Some("#993366".into()),
                translated: true,
                kind: "standard".into(),
            },
            Segment {
                start: 3311,
                end: 3397,
                // DELIBERATELY DIFFERENT from segment 1. `Feature::color()` is
                // first-wins, so a form that flattened the pair on a rename
                // would draw the same colour on the map and the loss would stay
                // invisible until a `.dna` was reopened.
                color: Some("#4477aa".into()),
                translated: true,
                kind: "standard".into(),
            },
        ];
        f.set_qualifier("codon_start", "1");
        f.set_flag_qualifier("pseudo");
        f.set_qualifier("transl_table", "11");
        // An empty VALUE, next to the valueless one above, because the whole
        // point is that these two are different qualifiers.
        f.set_qualifier("replace", "");
        f.set_qualifier("codon_start", "1");
        f
    }

    /// The fixture is worth nothing if it does not carry what the tests claim.
    fn assert_fixture_can_detect_the_loss(f: &Feature) {
        assert!(f.segments.len() >= 2, "no multi-segment feature");
        assert!(
            f.segments.iter().any(|s| s.color.is_some()),
            "no coloured segment"
        );
        assert!(
            f.segments.iter().any(|s| s.translated),
            "no translated segment"
        );
        assert!(
            f.qualifiers.iter().any(|(_, v)| v.is_none()),
            "no valueless qualifier — the fixture cannot detect the loss this \
             test exists to catch"
        );
        assert!(
            f.qualifiers.iter().any(|(_, v)| v.as_deref() == Some("")),
            "no empty-valued qualifier to tell the valueless one apart from"
        );
        assert!(
            f.qualifiers
                .iter()
                .filter(|(k, _)| k == "codon_start")
                .count()
                > 1,
            "no repeated qualifier"
        );
    }

    fn panel_on(f: Feature, span: u64, circular: bool) -> Panel {
        Panel::open(Some(0), f, span, circular, None).expect("the fixture has a span")
    }

    /// PROVEN TO FAIL at 04afbb6: there was no feature editor, so `Panel` does
    /// not exist and this does not compile.
    #[test]
    fn renaming_a_feature_changes_only_its_name() {
        let original = fixture();
        assert_fixture_can_detect_the_loss(&original);

        // The DIALOG'S OWN reader and writer. A test that hand-builds the edited
        // `Feature` proves only that `Vec::clone` works.
        let mut p = panel_on(original.clone(), 8117, true);
        p.name = "RENAMED".into();
        let edited = p.to_feature();

        assert_eq!(edited.name, "RENAMED");

        // Field by field, so a failure says which field.
        assert_eq!(edited.kind, original.kind, "kind");
        assert_eq!(edited.strand, original.strand, "strand");
        assert_eq!(
            edited.segments.len(),
            original.segments.len(),
            "a multi-segment feature must survive as multi-segment"
        );
        for (i, (a, b)) in edited.segments.iter().zip(&original.segments).enumerate() {
            assert_eq!(a.start, b.start, "segment {i} start");
            assert_eq!(a.end, b.end, "segment {i} end");
            assert_eq!(a.color, b.color, "segment {i} colour");
            assert_eq!(a.translated, b.translated, "segment {i} translated");
            assert_eq!(a.kind, b.kind, "segment {i} kind");
        }
        assert_eq!(
            edited.qualifiers, original.qualifiers,
            "qualifiers, their order, their repeats and the Option inside each"
        );

        // And the catch-all, which covers any field added to `Feature` after
        // this test was written.
        let mut back = edited;
        back.name = original.name.clone();
        assert_eq!(back, original, "renaming changed something else");
    }

    /// PROVEN TO FAIL at 04afbb6: no `Panel`.
    ///
    /// The sharpest single loss: `None` becoming `Some("")` writes `/pseudo=""`
    /// instead of a bare `/pseudo`, and a pseudogene reopens as an ordinary
    /// protein-coding gene with a full-length ORF a cloner will trust.
    #[test]
    fn a_valueless_qualifier_does_not_become_an_empty_one() {
        let p = panel_on(fixture(), 8117, true);
        let rows: Vec<&QualRow> = p.quals.iter().filter(|q| q.key == "pseudo").collect();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].has_value, "read as a flag");

        let f = p.to_feature();
        assert!(f.has_qualifier("pseudo"), "still present");
        assert_eq!(f.qualifier("pseudo"), None, "still valueless");
        assert_eq!(
            f.qualifiers.iter().find(|(k, _)| k == "replace"),
            Some(&("replace".to_string(), Some(String::new()))),
            "and the EMPTY-valued one is still empty-valued, not collapsed with it"
        );
    }

    /// PROVEN TO FAIL at 04afbb6: no `Panel`.
    #[test]
    fn a_flag_and_an_empty_value_cannot_be_confused_by_a_slip() {
        let mut p = panel_on(fixture(), 8117, true);
        // Clearing the box under `= value` gives `Some("")`, never `None`.
        let i = p.quals.iter().position(|q| q.key == "codon_start").unwrap();
        p.quals[i].value.clear();
        assert_eq!(
            p.to_feature().qualifiers[i],
            ("codon_start".into(), Some(String::new()))
        );
        // Only the mode control reaches `None`.
        p.quals[i].has_value = false;
        assert_eq!(p.to_feature().qualifiers[i], ("codon_start".into(), None));
    }

    /// PROVEN TO FAIL at 04afbb6: no `Panel`.
    #[test]
    fn an_origin_crossing_span_survives_the_form_on_a_circle() {
        let mut f = Feature::new("wrapper", "misc_feature");
        f.segments.push(Segment::new(8000, 120));
        let mut p = panel_on(f, 8117, true);

        assert!(p.segments[0].wraps, "read as a wrap, not as a typo");
        assert_eq!(p.row_bases(&p.segments[0]), 8117 - 8000 + 1 + 120);
        p.normalise();
        assert!(p.refusals().is_empty(), "{:?}", p.refusals());

        let out = p.to_feature();
        assert_eq!((out.segments[0].start, out.segments[0].end), (8000, 120));
        assert_eq!(
            out.extent(8117, true),
            Some((8000, 120)),
            "and it still reads as a wrap"
        );
    }

    /// PROVEN TO FAIL at 04afbb6: no `Panel`.
    ///
    /// The same two numbers on a line are `Invalid::Inverted`, which the gate
    /// refuses with a sentence pitched at the whole molecule. The form has to
    /// say which segment, because that is the box the user is looking at.
    #[test]
    fn the_same_numbers_on_a_line_are_refused_by_segment() {
        let mut f = Feature::new("wrapper", "misc_feature");
        f.segments.push(Segment::new(1, 10));
        f.segments.push(Segment::new(8000, 120));
        let mut p = panel_on(f, 8117, false);

        assert!(!p.segments[1].wraps, "a line has no origin to cross");
        p.normalise();
        assert!(!p.segments[1].wraps, "and normalising does not invent one");

        let refusals = p.refusals();
        let named = refusals
            .iter()
            .find(|r| r.contains("Segment 2"))
            .unwrap_or_else(|| panic!("nothing named the segment: {refusals:?}"));
        assert!(named.contains("ends before it starts"), "{named}");
        assert!(named.contains("no origin to cross"), "{named}");

        // And it really is what `validate()` would have said.
        assert!(p.preflight().iter().any(|x| x.kind() == "inverted"));
    }

    /// PROVEN TO FAIL against the WORKING code as delivered, which put the pair
    /// back in order behind the user and told them they had "typed it the other
    /// way round".
    ///
    /// A pair on a circle with the higher number first is a wrap — that is the
    /// model's only spelling for one — so the form says so and rewrites
    /// nothing. The tick is the statement; the way out is one click, and
    /// `unticking_a_wrap_and_re_ticking_it_is_the_identity` is what proves that
    /// click is reversible.
    #[test]
    fn a_transposed_pair_on_a_circle_reads_as_a_wrap_and_is_never_rewritten() {
        let mut f = Feature::new("typo", "misc_feature");
        f.segments.push(Segment::new(500, 3000));
        let mut p = panel_on(f, 8117, true);
        p.segments[0].start = 3000;
        p.segments[0].end = 500;
        p.segments[0].wraps = false;

        p.normalise();
        assert_eq!(
            (p.segments[0].start, p.segments[0].end),
            (3000, 500),
            "the two numbers the user put in the boxes are still the two numbers in the boxes"
        );
        assert!(
            p.segments[0].wraps,
            "and the box says what they mean: the arc through base 1"
        );
        // `to_feature` agrees, so a Save in the same frame as the edit commits
        // what is on screen.
        let out = p.to_feature();
        assert_eq!((out.segments[0].start, out.segments[0].end), (3000, 500));
        // 8,117 - 3,000 + 1 (bases 3,000..8,117) + 500 (bases 1..500).
        assert_eq!(p.row_bases(&p.segments[0]), 5618);
    }

    /// PROVEN TO FAIL against the working code as delivered: measured there,
    /// 381..40 (60 bp) unticked to 40..381 (342 bp) and re-ticking left the box
    /// ticked and inert at 342 bp, so the only way back was retyping both
    /// numbers from memory.
    ///
    /// The checkbox is a reading of the pair, so a click is a swap and two
    /// clicks are nothing. This is the one gesture the box exists for, on the
    /// one feature class it exists for.
    #[test]
    fn unticking_a_wrap_and_re_ticking_it_is_the_identity() {
        let mut f = Feature::new("wrapper", "misc_feature");
        f.segments.push(Segment::new(381, 40));
        let mut p = panel_on(f, 400, true);
        p.normalise();
        assert!(p.segments[0].wraps, "the premise: it arrives as a wrap");
        assert_eq!(p.row_bases(&p.segments[0]), 60, "381..400 plus 1..40");

        // What the checkbox does when it is clicked, which is all it does.
        let click = |p: &mut Panel| {
            let r = &mut p.segments[0];
            std::mem::swap(&mut r.start, &mut r.end);
            p.normalise();
        };

        click(&mut p);
        assert_eq!((p.segments[0].start, p.segments[0].end), (40, 381));
        assert!(!p.segments[0].wraps, "and the box follows the numbers");
        assert_eq!(p.row_bases(&p.segments[0]), 342);

        click(&mut p);
        assert_eq!(
            (p.segments[0].start, p.segments[0].end),
            (381, 40),
            "two clicks put the user back exactly where they started"
        );
        assert!(p.segments[0].wraps);
        assert_eq!(p.row_bases(&p.segments[0]), 60);
    }

    /// PROVEN TO FAIL against the working code as delivered: measured there, a
    /// 400 bp circle carrying the wrap `381..40` plus an ordinary exon `20..60`
    /// — 21 bases drawn twice — produced `warnings() == []` and `refusals() ==
    /// []`.
    ///
    /// Both loops were wrap-blind by construction. The overlap test required
    /// `a1 >= a0`, which a wrapping pair never satisfies, and it only ever
    /// compared NEIGHBOURS, which a wrap in the middle of the list breaks.
    #[test]
    fn a_span_overlapping_a_wraps_tail_is_not_invisible() {
        let mut f = Feature::new("x", "misc_feature");
        f.segments = vec![Segment::new(381, 40), Segment::new(20, 60)];
        let mut p = panel_on(f, 400, true);
        p.normalise();
        assert!(p.segments[0].wraps, "the premise");

        let w = p.warnings();
        let over = w
            .iter()
            .find(|w| w.contains("overlap"))
            .unwrap_or_else(|| panic!("nothing said the bases are drawn twice: {w:?}"));
        // 1..40 against 20..60 is 20..40.
        assert!(over.contains("20..40"), "{over}");

        // And the shape itself, which is the bigger loss: GenBank can only spell
        // a crossing as the LAST two parts of a join, so this feature comes back
        // from a .gb save as one span from base 1 — a 101 bp feature that the
        // Features list then reports as the whole plasmid.
        assert!(
            w.iter()
                .any(|x| x.contains("crosses the origin") && x.contains("join(")),
            "{w:?}"
        );

        // Non-vacuity in both directions: two disjoint spans on the same circle
        // say neither thing.
        let mut g = Feature::new("y", "misc_feature");
        g.segments = vec![Segment::new(100, 150), Segment::new(200, 250)];
        let q = panel_on(g, 400, true);
        assert!(
            !q.warnings().iter().any(|x| x.contains("overlap")),
            "{:?}",
            q.warnings()
        );
        assert!(!q
            .warnings()
            .iter()
            .any(|x| x.contains("crosses the origin")));
    }

    /// PROVEN TO FAIL against the working code as delivered: `+ span` seeded the
    /// new row at `previous.end`, so on the SacB-shaped fixture (…201..260) it
    /// produced `260..260` and the form immediately warned "Segments 2 and 3
    /// overlap at 260..260" — about a row it had just written itself, on the
    /// button whose whole reason for existing is a spliced CDS.
    ///
    /// The arithmetic and not the widget, because the widget needs a frame; the
    /// expression under test is the one line `segments` runs on the click.
    #[test]
    fn a_new_span_starts_after_the_previous_one_and_not_on_it() {
        let p = panel_on(fixture(), 8117, true);
        let last = p.segments.last().unwrap();
        let n = p.span;
        let seeded = last.end.saturating_add(1).min(n).max(1);
        assert_eq!(seeded, last.end + 1, "3,398, not 3,397");

        // And the seeded row does not warn about itself.
        let mut q = panel_on(fixture(), 8117, true);
        q.segments.push(SegRow {
            start: seeded,
            end: seeded,
            wraps: false,
            translated: true,
            color: q.segments[0].color.clone(),
            kind: "standard".into(),
        });
        assert!(
            !q.warnings().iter().any(|w| w.contains("overlap")),
            "{:?}",
            q.warnings()
        );
        // Non-vacuity: the old seed DID.
        let mut bad = panel_on(fixture(), 8117, true);
        let e = bad.segments.last().unwrap().end;
        bad.segments.push(SegRow {
            start: e,
            end: e,
            wraps: false,
            translated: true,
            color: String::new(),
            kind: "standard".into(),
        });
        assert!(
            bad.warnings().iter().any(|w| w.contains("overlap")),
            "the check cannot fail: {:?}",
            bad.warnings()
        );
    }

    /// PROVEN TO FAIL against the working code as delivered: measured there,
    /// `start=100 end=200 wraps=true` on a 400 bp circle stayed ticked, stored a
    /// plain 101 bp forward span, and neither `refusals()` nor `warnings()`
    /// mentioned the contradiction.
    ///
    /// It cannot be reached at all now: the box is recomputed from the pair
    /// every frame, so it and the stored segment are incapable of disagreeing.
    #[test]
    fn a_ticked_box_can_never_disagree_with_the_numbers_beside_it() {
        let mut f = Feature::new("x", "misc_feature");
        f.segments.push(Segment::new(100, 200));
        let mut p = panel_on(f, 400, true);
        p.segments[0].wraps = true;
        p.normalise();
        assert!(
            !p.segments[0].wraps,
            "100 comes before 200, so there is no crossing to tick"
        );
        let out = p.to_feature();
        assert_eq!((out.segments[0].start, out.segments[0].end), (100, 200));
        assert_eq!(p.row_bases(&p.segments[0]), 101);

        // And the other direction: a genuine crossing cannot be left unticked.
        p.segments[0].start = 381;
        p.segments[0].end = 40;
        p.segments[0].wraps = false;
        p.normalise();
        assert!(p.segments[0].wraps);
    }

    /// PROVEN TO FAIL at 04afbb6: no `Panel`.
    ///
    /// Every kind the `WouldCorrupt` gate counts, refused at the form first.
    #[test]
    fn the_form_refuses_what_validate_would_reject() {
        // The DragValues make these untypeable; the model does not, and a check
        // that cannot fail proves nothing.
        for (seg, kind) in [
            (Segment::new(0, 40), "zero start"),
            (Segment::new(1, 0), "zero start"),
            (Segment::new(9_000, 9_100), "past the end"),
            (Segment::new(40, 9_000), "past the end"),
        ] {
            let mut f = Feature::new("bad", "misc_feature");
            f.segments.push(seg);
            let p = panel_on(f, 8117, true);
            assert!(
                p.preflight().iter().any(|x| x.kind() == kind),
                "the premise: validate() objects with {kind}"
            );
            assert!(
                !p.refusals().is_empty(),
                "the form let {kind} through to the gate"
            );
        }

        // A feature with no segments is always +1 and so always refused.
        let mut p = panel_on(fixture(), 8117, true);
        p.segments.clear();
        assert!(p
            .preflight()
            .iter()
            .any(|x| x.kind() == "feature without segments"));
        assert!(p
            .refusals()
            .iter()
            .any(|r| r.contains("annotate somewhere")));
    }

    /// PROVEN TO FAIL at 04afbb6: no `Panel`.
    ///
    /// "signal peptide" is what a biologist types, and it makes the feature
    /// vanish on the next open with exit 0 and nothing on `unwritable`.
    #[test]
    fn a_type_with_a_space_is_refused_and_the_reason_names_what_survives() {
        let mut p = panel_on(fixture(), 8117, true);
        p.kind = "signal peptide".into();
        let r = p.refusals();
        let said = r
            .iter()
            .find(|x| x.contains("cannot contain a space"))
            .unwrap_or_else(|| panic!("{r:?}"));
        assert!(said.contains("\"signal\""), "names what comes back: {said}");

        p.kind = String::new();
        assert!(p.refusals().iter().any(|x| x.contains("misc_feature")));

        // Long is legal and lossy, so it warns rather than refuses.
        p.kind = "a_very_long_feature_key".into();
        assert!(p.refusals().is_empty(), "{:?}", p.refusals());
        // Fifteen characters exactly, which is what `genbank::write` keeps.
        assert!(p
            .warnings()
            .iter()
            .any(|w| w.contains("\"a_very_long_fea\"")));
    }

    /// PROVEN TO FAIL at 04afbb6: no `Panel`.
    #[test]
    fn a_colour_the_renderer_cannot_read_is_refused_at_the_box() {
        let mut p = panel_on(fixture(), 8117, true);
        p.color = ColorMode::One("#abc".into());
        assert!(p.refusals().iter().any(|r| r.contains("six hex digits")));
        // The common paste.
        assert_eq!(tidy_hex("993366"), "#993366");
        assert!(hex_ok("#993366"));
        assert!(!hex_ok("#abc"), "parse_hex does not expand the short form");
        // The user's own colours, neither of which is in Okabe-Ito: a picker
        // that could not express these would make a round trip through this
        // editor a silent recolouring of their file.
        assert!(hex_ok("#993366") && hex_ok("#ffff00"));
        assert!(crate::theme::parse_hex("#abc").is_none(), "the same answer");
    }

    /// PROVEN TO FAIL against the working code as delivered: `cyan`, `#0f0` and
    /// `#00FF00 ` were all measured locking Save on a feature the user had never
    /// touched.
    ///
    /// A colour the FILE carried can only ever be warned about. The refusal is
    /// for what the user typed or clicked, which is the only thing they can act
    /// on. The App-level half of this — that the rename lands and the colour is
    /// still `Some("cyan")` afterwards — is
    /// `a_colour_the_file_carried_does_not_block_a_rename_and_is_not_discarded`.
    #[test]
    fn only_a_colour_the_user_set_can_block_the_save() {
        for arrived_with in ["cyan", "#0f0", "#00FF00 ", "green"] {
            let mut f = fixture();
            for s in &mut f.segments {
                s.color = Some(arrived_with.to_string());
            }
            let mut p = panel_on(f, 8117, true);
            assert_eq!(p.color, ColorMode::One(arrived_with.into()));
            assert!(!hex_ok(arrived_with), "the premise: {arrived_with:?}");
            assert!(
                p.refusals().is_empty(),
                "{arrived_with:?} arrived in the file and must not disable Save: {:?}",
                p.refusals()
            );
            assert!(
                p.warnings().iter().any(|w| w.contains(arrived_with)),
                "but it is said: {:?}",
                p.warnings()
            );
            // Kept, byte for byte, which is the whole point.
            assert_eq!(
                p.to_feature().segments[0].color,
                Some(arrived_with.to_string())
            );

            // And the moment the user types something else, it is refused
            // again — otherwise this "fix" would have made the box inert.
            p.color = ColorMode::One("#gg".into());
            assert!(
                p.refusals().iter().any(|r| r.contains("six hex digits")),
                "a value the user typed is still checked"
            );
        }
    }

    /// PROVEN TO FAIL against the working code as delivered: `warnings()`
    /// returned `[]` for a segment with `translated: true`, while the `aa`
    /// checkbox's own tooltip promised the flag "is round-tripped".
    ///
    /// It round-trips through `.dna` and not through GenBank — measured, a
    /// `translated: true` segment through `genbank::write` + `genbank::parse`
    /// comes back `false` — and the same function already warns about two other
    /// asymmetries of exactly this shape.
    #[test]
    fn the_aa_tick_says_that_a_genbank_save_loses_it() {
        let p = panel_on(fixture(), 8117, true);
        assert!(
            p.segments.iter().any(|r| r.translated),
            "the premise: SacB arrives with it set on both segments"
        );
        assert!(
            p.warnings()
                .iter()
                .any(|w| w.contains("amino-acid track") && w.contains(".dna keeps them")),
            "{:?}",
            p.warnings()
        );

        // Non-vacuity: a feature without the flag is not warned about.
        let mut f = fixture();
        for s in &mut f.segments {
            s.translated = false;
        }
        let q = panel_on(f, 8117, true);
        assert!(!q.warnings().iter().any(|w| w.contains("amino-acid track")));
    }

    /// PROVEN TO FAIL against the working code as delivered: `warnings()`
    /// returned only "This feature has no name" for a value carrying a newline.
    ///
    /// This window's qualifier box is the only `TextEdit::multiline` in the GUI,
    /// so it is the first surface that can put one there — one Enter, one pasted
    /// note, one `/translation` copied out of another tool.
    #[test]
    fn a_line_break_in_a_value_is_said_before_the_save() {
        let mut p = panel_on(fixture(), 8117, true);
        p.quals.push(QualRow {
            key: "note".into(),
            has_value: true,
            value: "line one\nline two".into(),
        });
        assert!(
            p.warnings()
                .iter()
                .any(|w| w.contains("line break") && w.contains("comes back as a space")),
            "{:?}",
            p.warnings()
        );
        // It is a warning and NOT a refusal: `.dna` keeps the value exactly, and
        // refusing would make a value that is perfectly expressible unsaveable.
        assert!(p.refusals().is_empty(), "{:?}", p.refusals());
        assert_eq!(
            p.to_feature().qualifiers.last().unwrap().1.as_deref(),
            Some("line one\nline two"),
            "and the model keeps it verbatim"
        );
    }

    /// PROVEN TO FAIL at 04afbb6: no `Panel`.
    ///
    /// SacB's two segments differ in nothing `pl_core::Segment` holds —
    /// SnapGene's `<Segment name="signal peptide">` is already gone by the time
    /// the form sees it — so this is exactly the pair a "these look identical,
    /// merge?" affordance would destroy. There is no such affordance.
    #[test]
    fn sacbs_two_abutting_segments_are_not_merged() {
        let mut f = fixture();
        f.segments[1].color = f.segments[0].color.clone();
        let p = panel_on(f.clone(), 8117, true);
        assert_eq!(p.segments.len(), 2);
        assert_eq!(p.segments[0].end + 1, p.segments[1].start, "they abut");
        let out = p.to_feature();
        assert_eq!(out.segments.len(), 2, "still two");
        assert_eq!(out, f);
    }

    /// PROVEN TO FAIL at 04afbb6: no `Panel`.
    #[test]
    fn segment_order_is_never_touched_except_by_the_button() {
        let mut f = Feature::new("join", "CDS");
        f.segments = vec![
            Segment::new(400, 500),
            Segment::new(100, 200),
            Segment::new(700, 800),
        ];
        let p = panel_on(f, 8117, true);
        let out = p.to_feature();
        assert_eq!(
            out.segments.iter().map(|s| s.start).collect::<Vec<_>>(),
            vec![400, 100, 700],
            "file order, however untidy: a GenBank join's order IS the reading \
             order, and `extent` reads the writer's own shape"
        );
        // Said, though, because the map draws the connectors in this order.
        assert!(p
            .warnings()
            .iter()
            .any(|w| w.contains("out of ascending order")));
    }

    /// PROVEN TO FAIL at 04afbb6: no `Panel`.
    #[test]
    fn all_four_strands_are_reachable_and_the_unexpressible_ones_are_said() {
        let mut f = Feature::new("pSC101 ori", "rep_origin");
        f.segments.push(Segment::new(10, 40));
        f.strand = Strand::Unoriented;
        let p = panel_on(f, 8117, true);
        assert_eq!(
            p.strand,
            Strand::Unoriented,
            "read, not promoted to Forward"
        );
        assert_eq!(p.to_feature().strand, Strand::Unoriented);
        assert!(p.warnings().iter().any(|w| w.contains("complement()")));
    }

    /// PROVEN TO FAIL at 04afbb6: no `Panel`.
    #[test]
    fn a_note_that_would_come_back_as_a_colour_is_said() {
        let mut f = Feature::new("x", "misc_feature");
        f.segments.push(Segment::new(1, 10));
        f.set_qualifier("note", "clone #1a2b3c from the -80");
        let p = panel_on(f, 8117, true);
        assert!(p
            .warnings()
            .iter()
            .any(|w| w.contains("#1a2b3c") && w.contains("colour")));
        assert_eq!(
            embedded_hex("clone #1a2b3c from the -80").as_deref(),
            Some("#1a2b3c")
        );
        assert_eq!(embedded_hex("lot 12#ab"), None);
    }

    /// PROVEN TO FAIL at 04afbb6: no `Panel`.
    #[test]
    fn a_save_that_changes_nothing_is_a_no_op() {
        let p = panel_on(fixture(), 8117, true);
        assert!(p.is_noop(), "nothing was touched");
        assert!(!p.dirty());
        let mut p = panel_on(fixture(), 8117, true);
        p.segments[0].translated = false;
        assert!(
            !p.is_noop(),
            "a flag with no reader in this program is still an edit"
        );
    }

    /// PROVEN TO FAIL at 04afbb6: no `Panel`.
    ///
    /// `annotation_span()` falls back to `max(Feature::end())` and both
    /// `PastEnd` guards are behind `n > 0`, so a feature at `1..1_000_000` on a
    /// spanless molecule is accepted and silently redefines the span, after
    /// which every pre-existing coordinate validates clean forever.
    #[test]
    fn a_molecule_with_no_span_offers_no_coordinates_at_all() {
        let e = Panel::open(None, fixture(), 0, false, None).unwrap_err();
        assert!(e.contains("no length"), "{e}");
        assert!(e.contains("Nothing was changed"), "{e}");
    }

    /// PROVEN TO FAIL at 04afbb6: no `Panel`.
    #[test]
    fn a_moved_document_refuses_the_commit_rather_than_writing_through_the_index() {
        let mut p = panel_on(fixture(), 8117, true);
        assert!(p.stale_reason().is_none(), "the premise: it opened clean");
        // Any operation at all moves the cursor, and `RemoveFeature` moves every
        // later index with it — after which `Some(3)` names a different feature
        // and Save would destroy it with a perfectly valid operation.
        p.doc_at = Some(fake_id());
        let why = p
            .stale_reason()
            .expect("a moved cursor must refuse the commit");
        assert!(why.contains("Nothing was changed"), "{why}");
    }

    /// An `OpId` that is not `None`, which is all the staleness test needs.
    fn fake_id() -> pl_core::oplog::OpId {
        let mut log = pl_core::OpLog::new(Molecule {
            seq: b"ACGTACGTACGT".to_vec(),
            ..Default::default()
        });
        log.apply(
            pl_core::OpKind::InsertAt {
                at: 1,
                seq: "A".into(),
            },
            "t",
        )
        .unwrap();
        log.cursor().expect("one op recorded")
    }
}
