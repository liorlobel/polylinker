//! The map a linear molecule actually wants: a horizontal track.
//!
//! # What this exists for
//!
//! Until this module, `pl-draw` had one figure. A PCR product, a linearised
//! vector, a gene fragment, a gBlock, every FASTA and every assembly exported
//! as a **C-shaped ring with a notch in it** — because [`crate::scene`] read
//! the topology only to decide whether to close the backbone, and a ring with a
//! gap is an honest statement about topology and the wrong picture for a
//! construct laid out end to end. Topology was the only thing the two cases
//! disagreed about; everything else — the radius, the ruler around the inside,
//! the label ring, the caption in the middle — assumed a circle.
//!
//! # What is *not* new here
//!
//! Deliberately, almost nothing. This module adds a coordinate map, a packer
//! that stacks rows, and a builder that puts the two together. It adds:
//!
//! * **no new [`Scene`] primitive.** A track is `Move`, `Line`, `Close` and
//!   `Text`. There is no arc anywhere in a linear figure, so the SVG, PDF, EPS
//!   and PNG writers are untouched and the app's on-screen `Scene` painter gets
//!   this figure for free. If a linear figure had needed a writer change, that
//!   would have been the signal that the `Scene` layer was being bypassed.
//! * **no second collision solver.** [`place_rows`] is a loop around
//!   [`crate::place_column`] — the same isotonic regression the ring packs its
//!   columns and rows with, which is exact, order-independent and identical on
//!   every platform. The alternative, nudging labels apart until nothing moves,
//!   is the thing `labels.rs`'s header rejects.
//! * **no second feature resolver.** `crate::resolve_features` runs `ranges`,
//!   the `partly_drawn` bookkeeping and `mid_base` once, for both figures.
//! * **no second coordinate function.** [`crate::frac`] decides where a base
//!   sits, and the ring multiplies it by a turn while the track multiplies it
//!   by its width.
//!
//! # The one thing that is genuinely different
//!
//! A ring has a fixed circumference and two side columns that stack labels
//! vertically; a track has a fixed width and **one direction to grow in**. So
//! labels go in a band of rows above the track, filled from the row nearest the
//! track outward, and a label a row cannot hold spills to the next row out.
//! That is the same spill [`crate::ring::place_ring`] uses when a row hands a
//! label to a column, and it is what stops the pET28a case — twelve cutters
//! inside five degrees of each other — from costing eleven of them.
//!
//! Because `place_column` drops the *lightest* first, the spill sorts itself:
//! cut coordinates (`crate::SITE_WEIGHT`, above any feature weight) take the
//! row nearest the track, where the leader is shortest and the reading is
//! easiest, and feature names take the rows behind them.

use crate::labels::{place_column, LabelBox};
use crate::scene::{Anchor, Item, Scene, Seg};
use crate::{
    commas, drawn_width, fit_label, frac, frac_end, ink, nice_step, resolve_features, Arrow, Label,
    Options, Report, SITE_WEIGHT,
};
use pl_core::{Molecule, Strand};
use std::collections::BTreeSet;

// ---------------------------------------------------------------------------
// the track
// ---------------------------------------------------------------------------

/// Where the molecule's axis sits on the canvas, and how a base maps onto it.
///
/// The linear twin of [`crate::ring::RingGeom`]: the small bundle of numbers
/// every other piece of the figure is derived from, held in one place so that
/// the feature boxes, the site ticks, the ruler and the leaders cannot each
/// arrive at a slightly different answer for "where is base 4,102". The ring
/// learned that the hard way — `map.rs` rebuilt `tick_r` from `ro` and drifted
/// by the 2 units the leader starts outside the ring, which put a name past the
/// canvas edge with nothing reported.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Track {
    /// `x` of the first base's left edge.
    pub x0: f64,
    /// `x` of the last base's right edge.
    pub x1: f64,
    /// `y` of the backbone line — the middle of the feature band.
    pub y: f64,
    /// Thickness of the feature band, centred on `y`.
    ///
    /// The line runs *through* it, exactly as the ring's backbone circle runs
    /// through the annulus its features are drawn in.
    pub band: f64,
    /// The molecule's length in bases. Never zero — the caller passes
    /// `span().max(1)`.
    pub len: u64,
}

impl Track {
    /// How wide the drawable axis is.
    pub fn width(&self) -> f64 {
        (self.x1 - self.x0).max(0.0)
    }

    /// `x` of the **left** edge of a 1-based base.
    pub fn x_of(&self, base: u64) -> f64 {
        self.x0 + frac(base, self.len) * self.width()
    }

    /// `x` of the **right** edge of a 1-based base — where a feature ending
    /// there stops.
    ///
    /// [`crate::frac_end`] and not [`crate::frac`] of the next base, for the
    /// reason recorded there: on a line the molecule's last base ends at the
    /// far end of the track, and the ring's wrap-to-zero would draw a
    /// full-length feature as a sliver at the origin.
    pub fn x_end(&self, base: u64) -> f64 {
        self.x0 + frac_end(base, self.len) * self.width()
    }

    /// `y` of the band's upper edge — where leaders and site ticks leave it.
    pub fn top(&self) -> f64 {
        self.y - self.band * 0.5
    }

    /// `y` of the band's lower edge — where the ruler hangs from.
    pub fn bottom(&self) -> f64 {
        self.y + self.band * 0.5
    }
}

/// How many bases a mark of `mark` units covers on a track `track` units wide.
///
/// The linear twin of [`crate::ring::bases_per_arc`], and the same argument
/// applies unchanged: pass the width of a **tick's own stroke**, so that
/// "these two cuts fold into one label" states a fact about the picture — the
/// two marks are the same mark — rather than a fact about the window size. A
/// label height passed here would grow as the figure shrank, and resizing a
/// figure would change what it claims about the molecule.
///
/// At least 1, so a fold always means "the same mark" and never "the same base".
pub fn bases_per_unit(mark: f64, track: f64, span: u64) -> u64 {
    if track <= 0.0 || span == 0 {
        return 1;
    }
    ((mark / track * span as f64).floor() as u64).max(1)
}

// ---------------------------------------------------------------------------
// the label band
// ---------------------------------------------------------------------------

/// A label wanting to sit above one place on the track.
#[derive(Debug, Clone, Copy)]
pub struct RowLabel {
    /// The `x` it would like to be centred on.
    pub ideal: f64,
    /// The drawn width of the text, measured by the caller in its own font.
    pub width: f64,
    /// Resistance to displacement, and to being pushed to a further row.
    pub weight: f64,
}

/// Where one label ended up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowSlot {
    /// 0 is the row nearest the track; larger is further away.
    pub row: usize,
    /// The centre of the text.
    pub x: f64,
}

/// What [`place_rows`] managed to place.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rows {
    /// One entry per input, in input order. `None` for a label with nowhere to
    /// go.
    pub placed: Vec<Option<RowSlot>>,
    /// Indices that did not fit in any row, in input order.
    pub dropped: Vec<usize>,
}

impl Rows {
    /// How many rows were used — 0 when nothing was placed.
    pub fn used(&self) -> usize {
        self.placed
            .iter()
            .flatten()
            .map(|s| s.row + 1)
            .max()
            .unwrap_or(0)
    }
}

/// Pack labels into a band of horizontal rows, nearest row first.
///
/// One [`place_column`] per row, packing in `x`, exactly as
/// [`crate::ring::place_ring`] packs its twelve- and six-o'clock rows; what a
/// row drops is re-offered to the next row out. Nothing else here decides
/// anything — the ordering inside a row, the separation, the clamping to
/// `lo..hi` and the choice of what yields are all the isotonic regression's,
/// which is exact and identical on every platform.
///
/// # Termination
///
/// A row that places **nothing** ends the loop. Without that, a label wider
/// than the whole band would be offered to row after row and dropped by each of
/// them, and the loop would run to `rows` doing no work — or forever, if the
/// row count were derived from the labels rather than from the canvas. The
/// caller keeps the invariant that makes this unreachable in practice:
/// `crate::fit_label` shortens every label to `hi - lo - gap` first, so a label
/// that reaches the packer always fits a row on its own, so every row places at
/// least one label, so the band's capacity is `rows` labels at worst.
///
/// # Why the lightest spill
///
/// `place_column` drops by weight, and this reoffers what it dropped. A cut
/// coordinate outweighs every feature name ([`crate::SITE_WEIGHT`]), so
/// enzymes take the row against the track — the shortest leader and the
/// clearest reading — and feature names, whose full text survives in the SVG
/// `<title>`, in the app's Features tab and in [`crate::Report`], take the rows
/// behind them.
///
/// Not "and in the PDF annotation", which is what this sentence was copied
/// saying. [`crate::pdf`] emits no `/Annots` array and its own module doc says
/// why — an annotation would be furniture in a figure. Checked one writer at a
/// time rather than repaired by guesswork: [`crate::eps`] does keep the text,
/// as a PostScript comment nothing renders, and PDF and PNG keep no copy of it
/// whatsoever.
pub fn place_rows(labels: &[RowLabel], lo: f64, hi: f64, rows: usize, gap: f64) -> Rows {
    let mut out = Rows {
        placed: vec![None; labels.len()],
        dropped: Vec::new(),
    };
    let mut pending: Vec<usize> = (0..labels.len()).collect();
    for row in 0..rows {
        if pending.is_empty() {
            break;
        }
        let boxes: Vec<LabelBox> = pending
            .iter()
            .map(|&i| LabelBox {
                ideal: labels[i].ideal,
                height: labels[i].width + gap,
                weight: labels[i].weight,
            })
            .collect();
        let placed = place_column(&boxes, lo, hi);
        let mut spill: Vec<usize> = Vec::new();
        for (k, &i) in pending.iter().enumerate() {
            match placed.positions[k] {
                Some(x) => out.placed[i] = Some(RowSlot { row, x }),
                None => spill.push(i),
            }
        }
        if spill.len() == pending.len() {
            break;
        }
        pending = spill;
    }
    // In input order, because `pending` is only ever rebuilt by walking itself
    // in order and `pl-draw` promises byte-identical output for identical input
    // — a caller that prints this list is part of that promise.
    out.dropped = pending;
    out
}

// ---------------------------------------------------------------------------
// the figure
// ---------------------------------------------------------------------------

/// Everything between the band and the first row of labels.
///
/// The same 26 units the ring leaves between its outer radius and a label's own
/// anchor, so the two figures put a leader in the same place relative to the
/// thing it points at.
const LEADER_GAP: f64 = 26.0;

/// The stroke of the mark a cut site puts on the band, and the threshold two
/// sites fold into one tick at. `crate::SITE_TICK_STROKE`'s twin — held here
/// rather than imported so the two numbers are visibly one decision, and equal.
const SITE_TICK_STROKE: f64 = 1.25;

/// Gutter between two labels sharing a row.
const ROW_GAP: f64 = 10.0;

/// Distance from the canvas edge to anything drawn.
const PAD: f64 = 8.0;

/// Build the linear figure. See [`crate::scene`], which chooses this or the ring.
///
/// # [`Options::height`] is a budget here, not the canvas
///
/// The ring grows to fill whatever pane it is given; a track cannot. Its height
/// is the caption, the rows of labels it turned out to need, the band and the
/// ruler, and nothing stretches. So `height` bounds **how many rows of labels
/// there may be**, and [`Scene::height`] comes back as what the figure actually
/// used — usually much less.
///
/// The alternative, padding out to `height`, was drawn and rejected: at the
/// 720 x 720 default a PCR product is a 190 pt drawing centred in 530 pt of
/// white. That is not a stylistic preference. `page::Fit::to_width_mm` reads
/// the scene's own aspect, so the padded figure prints as an 89 x 89 mm block
/// with a 23 mm drawing in it; a raster export allocates — and `MAX_PIXELS`
/// charges for — 3.8x the pixels, all of them white; and a journal's layout
/// takes the white as part of the figure. A caller who wants the white can add
/// it; a caller who does not could not remove it.
///
/// A `height` so small that even the caption, the band and the ruler do not fit
/// yields a scene taller than it — deliberately. The other reading is cropping
/// the ruler off the bottom of a figure, in silence, which is the one thing
/// this crate refuses to do anywhere else.
pub fn scene(mol: &Molecule, opts: Options) -> (Scene, Report) {
    let mut report = Report::default();
    let mut items: Vec<Item> = Vec::new();
    let mut overlay: Vec<Item> = Vec::new();
    let len = mol.span().max(1);
    // The molecule's topology, not the figure's. It decides two different
    // things: whether `ranges` may split a feature across the origin (it may,
    // and on a cut map that feature genuinely appears at both ends), and
    // whether this figure has to say it cut something open.
    let circular = mol.topology.is_circular();
    report.cut_open = circular;

    let cx = opts.width * 0.5;
    let line_h = opts.font_size + 3.0;
    let ruler_size = opts.font_size * 0.72;

    // --- how wide anything may be ---
    //
    // A row runs the whole canvas less its padding, and `place_column` clamps a
    // label's own EDGES into that span, so this is exactly where the `viewBox`,
    // the `/MediaBox`, the `%%BoundingBox` and the raster canvas crop. Deciding
    // in one unit and cropping in another is the defect `crate::fit_label`'s
    // doc records twice; here they are the same number by construction.
    //
    // Less the gutter, so a label this admits always fits a row **on its own**.
    // That is what makes `place_rows` terminate: every row places at least one
    // label, so the spill always shrinks.
    let (row_lo, row_hi) = (PAD, (opts.width - PAD).max(PAD));
    let room = (row_hi - row_lo - ROW_GAP).max(0.0);

    let (drawn, mut anchors) = resolve_features(mol, len, circular, &mut report);

    // --- the caption, at the top ---
    //
    // At the top and not in the middle, because on a track the middle is where
    // the molecule is. `mol.name` first for the reason `crate::scene` records:
    // a GenBank LOCUS name is a real name, a filename is a guess, and
    // `"unnamed"` is not a name at all.
    let title = if !mol.name.is_empty() {
        mol.name.clone()
    } else if let Some(t) = opts
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        t.to_string()
    } else {
        "unnamed".to_string()
    };
    let caption_room = (opts.width - 2.0 * PAD).max(0.0);
    let title_drawn = fit_label(&title, caption_room, opts.font_size * 1.25);

    // A CIRCULAR molecule drawn here has been cut open, and the figure has to
    // say so: a track and a track are the same picture, and nothing in the
    // geometry distinguishes a linearised plasmid from a molecule that really
    // is a line. The three forms are picked widest-first for the same reason
    // `ring::Disclosure` has three — a sentence wider than the canvas is
    // cropped by the typesetter, in silence — and none of them is an ellipsis
    // through the claim, because half a disclosure is worse than none.
    // `Report::cut_open` carries the fact whatever fits.
    let bp_size = opts.font_size * 0.9;
    let bp = if circular {
        [
            format!("{} bp · circular, shown cut open at base 1", commas(len)),
            format!("{} bp · cut circle", commas(len)),
            format!("{} bp", commas(len)),
        ]
        .into_iter()
        .find(|f| drawn_width(f, bp_size) <= caption_room)
        .unwrap_or_else(|| format!("{} bp", commas(len)))
    } else {
        format!("{} bp", commas(len))
    };
    let bp_drawn = fit_label(&bp, caption_room, bp_size);
    let note_size = opts.font_size * 0.8;
    let note_drawn = opts.note.as_ref().and_then(|d| {
        [d.long(), d.short(), d.tiny()]
            .into_iter()
            .find(|f| drawn_width(f, note_size) <= caption_room)
    });
    // One line, whichever of the three forms is drawn and whether or not one is.
    let note_strip = note_size + 3.0;
    let note_shown = note_drawn.is_some();

    // Laid out with a cursor rather than at fixed offsets, so a figure with no
    // note does not carry the hole where the note would have been.
    let mut cursor = PAD;
    let mut caption: Vec<Item> = Vec::new();
    let mut line = |cursor: &mut f64, text: String, size: f64, colour: &str, bold: bool| {
        *cursor += size * 0.5;
        caption.push(Item::Text {
            x: cx,
            y: *cursor,
            size,
            anchor: Anchor::Middle,
            color: colour.to_string(),
            bold,
            text,
        });
        *cursor += size * 0.5 + 3.0;
    };
    if let Some(text) = title_drawn {
        report.title_truncated = text != title;
        line(
            &mut cursor,
            text,
            opts.font_size * 1.25,
            ink::TITLE_FILL,
            true,
        );
    }
    if let Some(text) = bp_drawn {
        line(&mut cursor, text, bp_size, ink::SUBTITLE_FILL, false);
    }
    if let Some(text) = note_drawn {
        line(&mut cursor, text, note_size, ink::SUBTITLE_FILL, false);
    }
    let caption_bottom = cursor;

    // --- the axis, across the canvas ---
    //
    // Inset by half the widest ruler number, because the numbers are centred on
    // their ticks and a tick can land on the very last base: `nice_step` divides
    // the length, so on a 1,000 bp molecule the last tick IS 1,000 and its label
    // would hang half its width past the canvas edge, cropped by every back end
    // in silence. This is the linear form of `ring::reserve_for`, and it is
    // capped for the same reason that one is — a 20-digit declared length off a
    // hostile LOCUS line must not collapse the track to nothing.
    let widest_number = drawn_width(&commas(len), ruler_size);
    let inset = if opts.ruler {
        (widest_number * 0.5).min(opts.width * 0.15)
    } else {
        0.0
    };
    let tick_len = 5.0;
    // What the ruler needs below the band: the tick, a gap, and the number,
    // whose baseline is the middle of its glyphs.
    let ruler_strip = if opts.ruler {
        tick_len + 2.0 + ruler_size
    } else {
        0.0
    };

    // --- how many rows of labels there is room for ---
    //
    // Computed against the band pushed as LOW as it can go, so this is the most
    // rows the canvas could ever hold; the block is then centred in what is left
    // once the rows that were actually used are known. The dependency runs one
    // way — the canvas decides the capacity, the capacity decides the height,
    // the height decides the position — which is the ordering `ring::centre_room`
    // records getting backwards once, at the cost of a genome's whole scale.
    // The note's line is RESERVED here whether or not there is a note, which is
    // the only thing in this figure that is deliberately not measured from what
    // is drawn.
    //
    // `bins/pl` and `bins/pl-gui` both build the disclosure line in two passes:
    // render once to learn how many cut labels the figure could fit, then render
    // again with a line saying so. That is only honest if adding the line cannot
    // change what it is counting. On the ring it cannot — `note` reaches
    // `centre_room` -> `keep_clear` -> the ruler's radius and stops. On a track
    // it reached `caption_bottom`, which is one of the four terms in `rows_room`,
    // so the note stole a row from the label band and the counts printed in it
    // described the figure from BEFORE it was added. Measured on a 6 kb track
    // with 40 cut sites at 720 x 180: pass one names 33 enzymes and hides 7, and
    // the figure that then went out named 24 and hid 16. `debug_assert!(closes)`
    // passes on that, because 24 + 16 and 33 + 7 both reach 40 — the arithmetic
    // closes over numbers taken from the wrong picture, which is the exact shape
    // of the defect the conservation law was added to catch and cannot see.
    //
    // Reserving costs at most one row of labels on a figure whose `height` is
    // actually binding, and every label that costs is NAMED in
    // `Report::labels_hidden`. A wrong count in the disclosure line is named
    // nowhere and reaches a reader who has no Enzymes tab to check it against.
    // Of the two, the loud one is the one to keep.
    let band_top_max = opts.height - PAD - ruler_strip - opts.ring_width;
    let caption_for_capacity = caption_bottom + if note_shown { 0.0 } else { note_strip };
    let rows_room = band_top_max - caption_for_capacity - PAD - LEADER_GAP - line_h * 0.5;
    let max_rows = if rows_room >= 0.0 {
        (rows_room / line_h).floor() as usize + 1
    } else {
        0
    };

    // --- the labels wanting a row ---
    //
    // Sites are folded onto shared ticks first, on the same rule the ring uses:
    // two cuts closer together than the mark that draws them ARE one mark. The
    // honest merged label carries every name and every coordinate, so it is
    // always wider than either name alone; where it would not fit the room a
    // single name has, the sites stay separate and the packer moves them a row
    // apart. Untidy and true — the alternative is an ellipsis through a merged
    // label, which drops a whole enzyme name.
    let site_label = |name: &str, pos: u64| Label {
        text: format!("{name}  {}", commas(pos)),
        frac: frac(pos, len),
        weight: SITE_WEIGHT,
        site: true,
        names: vec![name.to_string()],
    };
    let track = Track {
        x0: PAD + inset,
        x1: (opts.width - PAD - inset).max(PAD + inset),
        // Provisional: only `width()` is read before the band's `y` is known,
        // and `width()` does not depend on it. Fixed up below.
        y: 0.0,
        band: opts.ring_width,
        len,
    };
    if !opts.sites.is_empty() {
        let within = bases_per_unit(SITE_TICK_STROKE, track.width(), len);
        for s in crate::ring::merge_sites(&opts.sites, within) {
            let folded = s.label();
            if s.names.len() == 1 || drawn_width(&folded, opts.font_size) <= room {
                anchors.push(Label {
                    text: folded,
                    frac: frac(s.anchor(), len),
                    weight: SITE_WEIGHT,
                    site: true,
                    names: s.names.clone(),
                });
            } else {
                anchors.extend(
                    s.names
                        .iter()
                        .zip(&s.positions)
                        .map(|(n, p)| site_label(n, *p)),
                );
            }
        }
    }

    let texts: Vec<Option<String>> = anchors
        .iter()
        .map(|l| fit_label(&l.text, room, opts.font_size))
        .collect();
    let boxes: Vec<RowLabel> = anchors
        .iter()
        .zip(&texts)
        .map(|(l, t)| RowLabel {
            ideal: track.x0 + l.frac * track.width(),
            width: t.as_deref().map_or(0.0, |t| drawn_width(t, opts.font_size)),
            weight: l.weight,
        })
        .collect();
    let placed = place_rows(&boxes, row_lo, row_hi, max_rows, ROW_GAP);
    for &d in &placed.dropped {
        report.labels_hidden.push(anchors[d].text.clone());
    }

    // --- and now the band can be positioned, and the figure measured ---
    //
    // `above` is what the rows that were actually used occupy: the leader gap,
    // then one line per row, measured from the topmost row's upper edge because
    // that is what must clear the caption.
    let rows_used = placed.used();
    let above = if rows_used == 0 {
        0.0
    } else {
        LEADER_GAP + (rows_used as f64 - 1.0) * line_h + line_h * 0.5
    };
    let track = Track {
        y: caption_bottom + PAD + above + opts.ring_width * 0.5,
        ..track
    };
    let row_y = |row: usize| track.top() - LEADER_GAP - row as f64 * line_h;
    let height = track.bottom() + ruler_strip + PAD;

    // --- backbone ---
    //
    // A plain line. There is no gap and no notch: a track already claims exactly
    // what a linear molecule is, and the caption above says when a circle was
    // cut to make one. (The mirror case — `Shape::Circular` on a linear
    // molecule — keeps the gapped ring, which is the disclosure there.)
    items.push(Item::Path {
        segs: vec![Seg::Move(track.x0, track.y), Seg::Line(track.x1, track.y)],
        fill: None,
        stroke: Some(ink::BACKBONE_STROKE.into()),
        stroke_width: 1.25,
        title: None,
    });

    // --- ruler ---
    //
    // The same step and the same walk as the ring's, including the checked
    // addition: `len` comes straight off a GenBank LOCUS line, and for a
    // declared 18446744073709551615 the tenth tick overflows — a debug panic,
    // and in release an endless loop pushing two items a turn.
    if opts.ruler {
        // How many ticks the track can carry without its own numbers touching.
        //
        // The ring divides by a flat 12, and can: twelve numbers around a
        // circumference are spread over `2πr`, which at any radius this crate
        // draws is far more room than they need. A track has only its width, and
        // at 300 pt — the app's own narrow pane, and `--width 300` on the command
        // line — twelve numbers of `5,386` come out 26 pt apart with 21 pt of
        // glyphs, so `4,500` and `5,000` touch. Overprinted digits on a scale are
        // the same class of defect as a cut coordinate: what a reader takes off
        // the figure is not a number the molecule has.
        //
        // Never MORE than twelve, so a wide figure gets the ruler it always had
        // rather than forty ticks, and never fewer than one.
        let per = (widest_number + ROW_GAP).max(1.0);
        let want = (track.width() / per).floor().clamp(1.0, 12.0);
        // `nice_step` rounds up, so the tick count comes out at or below `want`.
        let step = nice_step(len as f64 / want);
        let mut base = step;
        while base <= len {
            let x = track.x_of(base);
            items.push(Item::Path {
                segs: vec![
                    Seg::Move(x, track.bottom()),
                    Seg::Line(x, track.bottom() + tick_len),
                ],
                fill: None,
                stroke: Some(ink::SUBTITLE_FILL.into()),
                stroke_width: 1.0,
                title: None,
            });
            items.push(Item::Text {
                x,
                y: track.bottom() + tick_len + 2.0 + ruler_size * 0.5,
                size: ruler_size,
                anchor: Anchor::Middle,
                color: ink::SUBTITLE_FILL.into(),
                bold: false,
                text: commas(base),
            });
            match base.checked_add(step) {
                Some(next) => base = next,
                None => break,
            }
        }
    }

    // --- features ---
    //
    // One band, with overlapping features overprinting, which is what the ring
    // does with its annulus. Lanes are a change to BOTH figures or to neither:
    // giving the track lanes the ring does not have would put the same molecule
    // in two different arrangements depending on its topology, and the divergence
    // this whole layer exists to close is exactly that.
    for d in &drawn {
        for (i, &(a, b)) in d.parts.iter().enumerate() {
            if d.degrees < opts.min_feature_degrees {
                // The same threshold the ring uses, and the same number: a share
                // of the molecule times 360. Below it an arrowhead is smaller
                // than the stroke around it and reads as dirt on the figure, so
                // the feature is a mark across the band instead.
                let x = track.x_of(a);
                items.push(Item::Path {
                    segs: vec![Seg::Move(x, track.top()), Seg::Line(x, track.bottom())],
                    fill: None,
                    stroke: Some(d.colour.clone()),
                    stroke_width: 1.75,
                    title: Some(d.name.clone()),
                });
            } else {
                let arrow = if i as isize == d.arrow_on {
                    match d.strand {
                        Strand::Reverse => Arrow::Start,
                        _ => Arrow::End,
                    }
                } else {
                    Arrow::None
                };
                items.push(Item::Path {
                    segs: box_segs(
                        track.x_of(a),
                        track.x_end(b),
                        track.top(),
                        track.bottom(),
                        arrow,
                    ),
                    fill: Some(d.colour.clone()),
                    stroke: Some(ink::FEATURE_STROKE.into()),
                    stroke_width: 0.6,
                    title: Some(d.name.clone()),
                });
            }
        }
    }

    // --- labels, their ticks and their leaders ---
    let mut named: BTreeSet<&str> = BTreeSet::new();
    for (i, l) in anchors.iter().enumerate() {
        let Some(slot) = placed.placed[i] else {
            continue;
        };
        let Some(text) = texts[i].clone() else {
            // Not even one character and an ellipsis fit. A leader with nothing
            // on the end of it reads as a renderer fault rather than as a canvas
            // too narrow for the name, so the label goes — and is named.
            report.labels_hidden.push(l.text.clone());
            continue;
        };
        if text != l.text {
            report.labels_truncated.push(l.text.clone());
            if l.site {
                report.sites_shortened += 1;
            }
        }
        let x = track.x0 + l.frac * track.width();
        if l.site {
            // Only the names a reader can still READ: `fit_label` may have cut
            // this to `Ec...`, and counting that enzyme as labelled is the same
            // lie as counting a label that was never drawn. `EcoRI  7,5...`
            // still names its enzyme and does count.
            named.extend(
                l.names
                    .iter()
                    .filter(|n| text.contains(n.as_str()))
                    .map(String::as_str),
            );
            // A leader alone points at a place; a tick says a cut happens there.
            items.push(Item::Path {
                segs: vec![Seg::Move(x, track.top()), Seg::Line(x, track.top() - 6.0)],
                fill: None,
                stroke: Some(ink::BACKBONE_STROKE.into()),
                stroke_width: SITE_TICK_STROKE,
                title: Some(l.text.clone()),
            });
        }
        // Vertical at the label's own base, then a short jog to the text. The
        // long run is the vertical one deliberately: it crosses the fewest rows,
        // and `place_column` has already kept the jog short by placing the label
        // as near its ideal `x` as the row allowed.
        let ty = row_y(slot.row);
        overlay.push(Item::Path {
            segs: vec![
                Seg::Move(x, track.top() - 2.0),
                Seg::Line(x, ty + line_h * 0.5 + 2.0),
                Seg::Line(slot.x, ty + line_h * 0.5),
            ],
            fill: None,
            stroke: Some(ink::LEADER_STROKE.into()),
            stroke_width: 0.9,
            title: None,
        });
        overlay.push(Item::Text {
            x: slot.x,
            y: ty,
            size: opts.font_size,
            anchor: Anchor::Middle,
            color: ink::LABEL_FILL.into(),
            bold: false,
            text,
        });
        report.labels_placed += 1;
    }
    // A SET DIFFERENCE against what was asked for, so a name lost anywhere
    // between the fold and the paint is counted whichever drop site loses it.
    // See `Report::sites_hidden` for the unit mismatch this replaced.
    let admitted: BTreeSet<&str> = opts.sites.iter().map(|(n, _)| n.as_str()).collect();
    report.sites_named = named.len();
    report.sites_hidden = admitted
        .difference(&named)
        .map(|n| (*n).to_string())
        .collect();
    report.sites_dropped = report.sites_hidden.len();

    items.extend(overlay);
    items.extend(caption);
    (
        Scene {
            width: opts.width,
            height,
            title,
            items,
        },
        report,
    )
}

/// One feature box, with its arrowhead, as device-independent segments.
///
/// `crate::arc_segs` with the arc taken out, vertex for vertex: the body runs
/// to `base`, the barbs step outside the band by the same `min(0.35 · band,
/// 2.5)`, the tip sits on the axis at the feature's own end, and the head is
/// clamped to half the feature so a short one degrades to a triangle instead of
/// inverting into a bow tie.
///
/// The head is **8 units**, which is what the ring's `8.0 / mid` radians works
/// out to as an arc length at the mid radius. Same figure, same arrowhead.
fn box_segs(lo: f64, hi: f64, top: f64, bot: f64, arrow: Arrow) -> Vec<Seg> {
    let mid = (top + bot) * 0.5;
    let head = if arrow == Arrow::None {
        0.0
    } else {
        8.0_f64.min((hi - lo) * 0.5)
    };
    let barb = ((bot - top) * 0.35).min(2.5);
    let mut segs = Vec::new();
    match arrow {
        Arrow::End => {
            let base = hi - head;
            segs.push(Seg::Move(lo, top));
            segs.push(Seg::Line(base, top));
            if head > 0.0 {
                segs.push(Seg::Line(base, top - barb));
                segs.push(Seg::Line(hi, mid));
                segs.push(Seg::Line(base, bot + barb));
            }
            segs.push(Seg::Line(base, bot));
            segs.push(Seg::Line(lo, bot));
        }
        Arrow::Start => {
            let base = lo + head;
            segs.push(Seg::Move(hi, top));
            segs.push(Seg::Line(base, top));
            if head > 0.0 {
                segs.push(Seg::Line(base, top - barb));
                segs.push(Seg::Line(lo, mid));
                segs.push(Seg::Line(base, bot + barb));
            }
            segs.push(Seg::Line(base, bot));
            segs.push(Seg::Line(hi, bot));
        }
        Arrow::None => {
            segs.push(Seg::Move(lo, top));
            segs.push(Seg::Line(hi, top));
            segs.push(Seg::Line(hi, bot));
            segs.push(Seg::Line(lo, bot));
        }
    }
    segs.push(Seg::Close);
    segs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(w: f64, len: u64) -> Track {
        Track {
            x0: 10.0,
            x1: 10.0 + w,
            y: 100.0,
            band: 18.0,
            len,
        }
    }

    #[test]
    fn base_one_starts_the_track_and_the_last_base_ends_it() {
        // The whole reason `frac_end` is not `frac` of the next base. On a ring
        // the last base's far edge is the origin, one turn on; on a line it is
        // the far end, and the ring's modulo would draw a feature covering the
        // whole molecule as a zero-width sliver back at base 1.
        for len in [1u64, 2, 999, 5_386, u64::MAX] {
            let t = track(700.0, len);
            assert_eq!(t.x_of(1), t.x0, "len {len}");
            assert_eq!(t.x_end(len), t.x1, "len {len}");
        }
    }

    #[test]
    fn a_base_sits_at_the_same_share_of_the_track_as_of_the_ring() {
        // One coordinate function, asserted rather than claimed: the ring
        // multiplies `frac` by a turn and the track by its width, so a feature
        // is the same distance along the molecule in both figures.
        let (len, w) = (5_386u64, 700.0);
        let t = track(w, len);
        for base in [1u64, 2, 1_000, 2_693, 5_385, 5_386] {
            let by_angle = crate::angle(base, len) / crate::TAU;
            assert!(
                ((t.x_of(base) - t.x0) / w - by_angle).abs() < 1e-12,
                "base {base}"
            );
        }
    }

    #[test]
    fn the_band_is_centred_on_the_backbone() {
        let t = track(100.0, 10);
        assert_eq!(t.bottom() - t.top(), t.band);
        assert_eq!((t.top() + t.bottom()) * 0.5, t.y);
    }

    #[test]
    fn a_mark_covers_at_least_one_base_however_long_the_track() {
        // The same floor `bases_per_arc` keeps, and for the same reason: a fold
        // must mean "one mark", never "one base".
        assert_eq!(bases_per_unit(1.25, 700.0, 5_386), 9);
        assert_eq!(bases_per_unit(1.25, 700.0, 100), 1);
        assert_eq!(bases_per_unit(1.25, 1e9, 5_386), 1);
        assert_eq!(bases_per_unit(1.25, 0.0, 5_386), 1);
        assert_eq!(bases_per_unit(1.25, 700.0, 0), 1);
    }

    fn row_labels(n: usize, width: f64) -> Vec<RowLabel> {
        (0..n)
            .map(|i| RowLabel {
                ideal: 100.0 + i as f64 * 3.0,
                width,
                weight: 1.0,
            })
            .collect()
    }

    #[test]
    fn what_one_row_cannot_hold_goes_to_the_next_row_out() {
        // Ten 60 pt labels all wanting the same 100 pt of a 200 pt band. A row
        // costs 70 apiece, so it holds two; the band holds all ten, in five
        // rows, and drops nothing. Without the spill this is `place_column`
        // alone: two names on the figure and eight in `labels_hidden`.
        let b = row_labels(10, 60.0);
        let r = place_rows(&b, 0.0, 200.0, 8, 10.0);
        assert!(r.dropped.is_empty(), "dropped {:?}", r.dropped);
        assert_eq!(r.used(), 5);
        for row in 0..r.used() {
            let n = r.placed.iter().flatten().filter(|s| s.row == row).count();
            assert!(n >= 1, "row {row} is empty but a further one is not");
        }
    }

    #[test]
    fn the_heaviest_labels_take_the_row_against_the_track() {
        // A cut coordinate outweighs every feature name, so the enzymes get the
        // short leaders and the names go behind them. `place_column` drops the
        // lightest and `place_rows` re-offers exactly what it dropped, so this
        // is a property of the two together and worth pinning: with the spill
        // written the other way round the figure puts the coordinates furthest
        // from the axis they belong to.
        let mut b = row_labels(8, 60.0);
        for (i, l) in b.iter_mut().enumerate() {
            l.weight = if i % 2 == 0 { crate::SITE_WEIGHT } else { 1.0 };
        }
        let r = place_rows(&b, 0.0, 200.0, 8, 10.0);
        let row_of = |i: usize| r.placed[i].expect("all placed").row;
        let deepest_site = (0..8)
            .filter(|i| i % 2 == 0)
            .map(row_of)
            .max()
            .expect("4 sites");
        let nearest_name = (0..8)
            .filter(|i| i % 2 == 1)
            .map(row_of)
            .min()
            .expect("4 names");
        assert!(
            deepest_site < nearest_name,
            "a feature name (row {nearest_name}) is nearer the track than a cut              coordinate (row {deepest_site})"
        );
    }

    #[test]
    fn a_label_no_row_can_hold_ends_the_loop_instead_of_running_it() {
        // The termination argument, as a test. A label wider than the band is
        // dropped by every row it is offered to, so without the no-progress
        // guard this walks all `rows` of them doing nothing -- and a row count
        // derived from the labels rather than from the canvas would not
        // terminate at all. A billion rows, and it must come straight back.
        let b = vec![RowLabel {
            ideal: 100.0,
            width: 500.0,
            weight: 1.0,
        }];
        let r = place_rows(&b, 0.0, 200.0, 1_000_000_000, 10.0);
        assert_eq!(r.dropped, vec![0]);
        assert_eq!(r.used(), 0);
        assert_eq!(r.placed, vec![None]);
    }

    #[test]
    fn no_label_may_reach_past_the_band_it_was_given() {
        // Where the viewBox, the /MediaBox, the %%BoundingBox and the raster
        // canvas all crop. A label past this is not shortened, it is cut, by the
        // typesetter and in silence.
        let mut b = row_labels(12, 70.0);
        b[0].ideal = -400.0;
        b[11].ideal = 900.0;
        let r = place_rows(&b, 20.0, 260.0, 6, 10.0);
        for (i, slot) in r.placed.iter().enumerate() {
            let Some(slot) = slot else { continue };
            let half = b[i].width * 0.5;
            assert!(slot.x - half >= 20.0 - 1e-6, "{i} runs off the left");
            assert!(slot.x + half <= 260.0 + 1e-6, "{i} runs off the right");
        }
    }

    #[test]
    fn two_labels_in_one_row_never_overlap() {
        let b = row_labels(6, 55.0);
        let r = place_rows(&b, 0.0, 400.0, 4, 10.0);
        for row in 0..r.used() {
            let mut xs: Vec<(f64, f64)> = r
                .placed
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.filter(|s| s.row == row).map(|s| (s.x, b[i].width)))
                .collect();
            xs.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("no NaN"));
            for w in xs.windows(2) {
                let gap = (w[1].0 - w[1].1 * 0.5) - (w[0].0 + w[0].1 * 0.5);
                assert!(gap >= -1e-6, "row {row}: labels overlap by {}", -gap);
            }
        }
    }

    #[test]
    fn packing_rows_is_deterministic() {
        let b: Vec<RowLabel> = (0..60)
            .map(|i| RowLabel {
                ideal: ((i * 7919) % 700) as f64,
                width: 30.0 + ((i * 13) % 40) as f64,
                weight: 1.0 + (i % 5) as f64,
            })
            .collect();
        let first = place_rows(&b, 0.0, 700.0, 9, 10.0);
        for _ in 0..8 {
            assert_eq!(place_rows(&b, 0.0, 700.0, 9, 10.0), first);
        }
    }

    #[test]
    fn an_arrowhead_never_eats_more_than_half_its_own_feature() {
        // `arc_segs`'s rule, one geometry over: a feature shorter than the head
        // degrades to a triangle rather than inverting into a bow tie, so every
        // vertex stays inside the box the feature's coordinates name.
        for w in [0.4_f64, 1.0, 4.0, 15.9, 16.0, 16.1, 400.0] {
            for arrow in [Arrow::End, Arrow::Start] {
                let segs = box_segs(100.0, 100.0 + w, 90.0, 108.0, arrow);
                for s in &segs {
                    if let Seg::Line(x, _) | Seg::Move(x, _) = *s {
                        assert!(
                            (100.0 - 1e-9..=100.0 + w + 1e-9).contains(&x),
                            "w={w}: x={x} outside the feature"
                        );
                    }
                }
                // And the tip is on the axis at the feature's own end.
                let tip = if arrow == Arrow::End {
                    Seg::Line(100.0 + w, 99.0)
                } else {
                    Seg::Line(100.0, 99.0)
                };
                assert!(segs.contains(&tip), "w={w}: no tip at {tip:?}");
            }
        }
    }

    #[test]
    fn an_unoriented_feature_is_a_rectangle_because_a_point_is_a_claim() {
        // The same rule the ring keeps: `Strand::Unoriented` is the file
        // declining to say which way the feature reads, and an arrowhead would
        // say it anyway.
        assert_eq!(
            box_segs(10.0, 50.0, 0.0, 18.0, Arrow::None),
            vec![
                Seg::Move(10.0, 0.0),
                Seg::Line(50.0, 0.0),
                Seg::Line(50.0, 18.0),
                Seg::Line(10.0, 18.0),
                Seg::Close,
            ]
        );
    }
}
