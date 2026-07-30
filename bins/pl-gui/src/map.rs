//! Plasmid map painting: circular for circular molecules, a track for linear.
//!
//! Layout is separated from painting on purpose. `lanes` is an ordinary function
//! over numbers with no egui in sight, and everything the circular map decides
//! about *where* — the radius, the ruler's own band, which cut sites share a
//! tick, and the label ring itself — is now [`pl_draw::ring`], which the SVG and
//! PDF exporters call as well. That is the point of it being there: this file and
//! `pl_draw::scene` had two independent layouts sharing only `pl_draw::ranges`,
//! so a label fix on the screen left the figure that goes into a paper untouched.
//!
//! What is left here is ink, measurement and hit-testing. `label_slots` still
//! serves the *linear* track, which has no ring and no columns.
//!
//! The picture is verified as a picture: the frame tests in `main.rs` paint a map
//! and assert on the shapes that came back, because this file can be
//! self-consistent about its arithmetic and still draw a label off the pane —
//! which is exactly what `LABEL_RESERVE = 132.0` did.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use pl_core::{Molecule, Topology};
use pl_draw::ring::{self, RingGeom, RingLabel, Side};
use pl_enzymes::Digest;

use crate::theme::{self, Palette};

/// The label font, in one place so measuring and drawing cannot disagree.
///
/// The whole `LABEL_RESERVE` defect was a decision made in one unit and a
/// drawing made in another; a second literal at the paint site is how that
/// comes back.
fn label_font() -> FontId {
    FontId::monospace(10.0)
}

/// `(enzymes a reader can read off the map, enzymes admitted that appear nowhere)`.
///
/// `admitted` is `(enzyme, position)` PAIRS as the filter produced them;
/// `on_labels` is the enzyme names carried by each label that was placed AND
/// drawn, one slice per label, several when a tick folded.
///
/// A function, and not four lines inline in `draw_circular`, because it is the
/// arithmetic behind a sentence a reader trusts and inline arithmetic inside a
/// painting routine cannot be asserted. That is not hypothetical here: the same
/// two numbers in `pl_draw::scene` were a sum of per-label tallies and a
/// subtraction of counts, both in the wrong unit, and they put "71 of 40 cutters
/// labelled" into an exported figure while every test in the corpus passed.
///
/// **Sets, in both halves.**
///  * `labelled` is a distinct count, because one enzyme cutting five times is
///    named in five labels and a sum over labels counts MENTIONS.
///  * `unnamed` is a set DIFFERENCE and not `admitted.len() - labelled`, because
///    an enzyme dropped at eight of its nine ticks is named, not hidden, and
///    "8/9 hidden" is not a state an enzyme can be in.
///
/// Both halves are unreachable through this file's own caller today, which draws
/// only unique cutters — one pair per enzyme, so a mention, a label and an enzyme
/// are the same integer. That accident is exactly what made the identical bug in
/// `pl_draw` invisible until `--sites dual` was asked for, and a "show dual
/// cutters" toggle is the obvious next feature.
fn cutters_shown(admitted: &[(String, u64)], on_labels: &[&[String]]) -> (usize, usize) {
    use std::collections::BTreeSet;
    let named: BTreeSet<&str> = on_labels
        .iter()
        .flat_map(|ns| ns.iter().map(String::as_str))
        .collect();
    let asked: BTreeSet<&str> = admitted.iter().map(|(n, _)| n.as_str()).collect();
    (named.len(), asked.difference(&named).count())
}

/// One drawable feature, resolved to positions on the molecule.
pub struct Band {
    pub index: usize,
    /// Every drawable part, in coordinate order and never running backwards.
    ///
    /// A feature with more than one segment is a join — an intron-split CDS —
    /// and drawing it as a single bar from start to end would silently claim
    /// the gaps are part of the feature. A *single* segment can need splitting
    /// too: `end < start` on a circle is the ordinary origin-crossing form that
    /// `Molecule::validate` accepts and that `Edit > Set origin at selected
    /// feature` produces. Interpolating straight from `angle_of(start)` to
    /// `angle_of(end)` then painted the complement arc — a 2,499 bp band, in
    /// the feature's own colour under its own name, for a 187 bp feature — so
    /// the split is done here, with `pl_draw::ranges`, which is the same
    /// function the SVG/PDF exporter uses. The map and `pl export` now agree by
    /// construction.
    ///
    /// May be empty: a segment lying wholly outside the molecule names no base,
    /// and `ranges` drops it rather than collapsing it onto base 1.
    pub segs: Vec<(u64, u64)>,
    /// The gaps between consecutive segments, as drawable parts.
    ///
    /// Taken in the segments' *file* order, not sorted, because that is the only
    /// thing that distinguishes an intron from the rest of the plasmid:
    /// `join(2600..2686, 1..100)` on a 2,686 bp circle is contiguous across the
    /// origin and has no gap at all, while `join(1..100, 2600..2686)` has one of
    /// 2,499 bp. Sorted, the two are the same set of coordinates.
    pub joins: Vec<(u64, u64)>,
    /// The one arrowhead: which of [`Band::segs`] carries it.
    ///
    /// `None` for `Strand::Unoriented` and `Strand::Both`, matching
    /// `pl_draw::scene`'s `arrow_on = -1`. It used to test only
    /// `f.strand.is_reverse()`, so an unoriented feature got a FORWARD head: the
    /// screen painted 9 arrowheads on the user's pKoV where `pl export` painted 6,
    /// and the yellow `pSC101 ori` at one o'clock claimed a direction the file
    /// never states. GenBank cannot express these strands and `pl convert` already
    /// says so out loud for exactly these three features.
    ///
    /// An INDEX, and not the `(tip, back)` coordinates it carried. The part is
    /// chosen in *biological* order — the forward feature 2587..87 ends at base 87,
    /// not at the end of its highest-numbered part, and reading the head off the
    /// sorted parts put it at 2,686 pointing counter-clockwise, a forward feature
    /// drawn as a reverse one. But `segs` is then SORTED, so the head's part is not
    /// `segs.last()`: for 8100..50 forward on 8117 it is `segs[0]`. Matching by
    /// coordinates at the paint site works and flips with the strand
    /// (`(s,e) == (back,tip)` forward, `(tip,back)` reverse), which is precisely
    /// how a sign gets inverted — and it would fail silently on reverse features,
    /// which on pKoV are five of nine and all on the inward lanes where the head
    /// is largest. `bands` knows the sorted position, so it records it.
    pub head: Option<usize>,
    pub start: u64,
    pub end: u64,
    pub reverse: bool,
    pub lane: usize,
    pub color: Color32,
    pub name: String,
}

/// Pack overlapping intervals into lanes so nothing is drawn on top of anything.
///
/// Greedy by start position: each interval takes the lowest lane whose last
/// occupant ends before it begins. Returns a lane per input, in input order.
pub fn lanes(spans: &[(u64, u64)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by_key(|&i| (spans[i].0, spans[i].1));

    let mut lane_end: Vec<u64> = Vec::new();
    let mut out = vec![0usize; spans.len()];
    for &i in &order {
        let (s, e) = spans[i];
        // A one-base gap keeps adjacent features visually distinct.
        //
        // `saturating_add`, because `end` is a coordinate straight out of a
        // file and nothing on the open path has validated it. A `.dna` carrying
        // `range="1-18446744073709551615"` panicked here in debug with "attempt
        // to add with overflow", and in release wrapped to 0 so every feature
        // silently stacked into a single lane.
        let lane = lane_end.iter().position(|&end| end.saturating_add(1) < s);
        match lane {
            Some(l) => {
                lane_end[l] = e;
                out[i] = l;
            }
            None => {
                lane_end.push(e);
                out[i] = lane_end.len() - 1;
            }
        }
    }
    out
}

/// Push labels apart so they do not overlap, keeping them near their anchors.
///
/// `anchors` are desired positions along one axis, `min_gap` the smallest
/// acceptable separation, `(lo, hi)` the range labels must stay inside. Works in
/// one pass forward then one back, which is enough for the modest counts here
/// and, unlike an iterative relaxation, always terminates.
pub fn label_slots(anchors: &[f32], min_gap: f32, lo: f32, hi: f32) -> Vec<f32> {
    let mut idx: Vec<usize> = (0..anchors.len()).collect();
    idx.sort_by(|&a, &b| {
        anchors[a]
            .partial_cmp(&anchors[b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut placed = vec![0.0f32; anchors.len()];
    let mut last = lo - min_gap;
    for &i in &idx {
        let want = anchors[i].max(last + min_gap);
        placed[i] = want;
        last = want;
    }
    // If that pushed past the end, pull back from the top.
    if let Some(&top) = idx.last() {
        if placed[top] > hi {
            let mut next = hi;
            for &i in idx.iter().rev() {
                placed[i] = placed[i].min(next);
                next = placed[i] - min_gap;
            }
        }
    }
    placed
}

/// Resolve every feature to the parts a painter can draw.
///
/// Splitting is done against the same denominator `angle_of` and `x_of` use, so
/// a part that survives here is one those functions can place.
fn bands(mol: &Molecule) -> Vec<Band> {
    let span = mol.annotation_span().max(1);
    let circular = mol.topology.is_circular();
    let spans: Vec<(u64, u64)> = mol.features.iter().map(|f| (f.start(), f.end())).collect();
    let lane = lanes(&spans);
    // A part is worth drawing only if it covers something; `ranges` emits a
    // degenerate `(n, n)` for the zero-length hop from the last base to the
    // first, which is a dot in the middle of a contiguous feature.
    let split = |a: u64, b: u64| -> Vec<(u64, u64)> {
        pl_draw::ranges(a, b, span, circular)
            .into_iter()
            .filter(|(s, e)| e > s)
            .collect()
    };
    mol.features
        .iter()
        .enumerate()
        .map(|(i, f)| {
            // File order, not sorted: see `Band::joins`.
            let mut raw: Vec<(u64, u64)> = f.segments.iter().map(|s| (s.start, s.end)).collect();
            if raw.is_empty() {
                raw.push((f.start(), f.end()));
            }
            // Biological order: each segment in the order the file gives them,
            // and a wrapped segment as its pre-origin part followed by its
            // post-origin one, which is `ranges`' own order.
            let flow: Vec<(u64, u64)> = raw
                .iter()
                .flat_map(|&(a, b)| pl_draw::ranges(a, b, span, circular))
                .collect();
            let joins: Vec<(u64, u64)> =
                raw.windows(2).flat_map(|w| split(w[0].1, w[1].0)).collect();
            let reverse = f.strand.is_reverse();
            // The terminal part in BIOLOGICAL order, then its position in the
            // SORTED list the painter walks. `Strand::Unoriented`/`Both` get no
            // head, matching `pl_draw::scene`'s `arrow_on = -1`: a directional
            // point is a claim, and these strands are the file declining to make
            // it.
            let terminal = match f.strand {
                pl_core::Strand::Forward => flow.last().copied(),
                pl_core::Strand::Reverse => flow.first().copied(),
                _ => None,
            };
            let mut segs = flow.clone();
            segs.sort_unstable();
            let head = terminal.and_then(|t| segs.iter().position(|&s| s == t));
            Band {
                index: i,
                segs,
                joins,
                head,
                start: f.start(),
                end: f.end(),
                reverse,
                lane: lane[i],
                color: theme::feature_color(f),
                name: f.name.clone(),
            }
        })
        .collect()
}

pub struct MapResponse {
    /// Feature index under the pointer, if any.
    pub hovered: Option<usize>,
    /// Feature index clicked this frame.
    pub clicked: Option<usize>,
    /// Feature index double-clicked this frame.
    ///
    /// The map is where a plasmid biologist looks first, so it is where a
    /// double-click has to open the feature editor. Taken from the same hit
    /// answer `clicked` uses — no geometry, lane, arrowhead, leader or ruler code
    /// is touched.
    pub double_clicked: Option<usize>,
    /// Where the centre caption was drawn and what it says in full, when the
    /// ring was too narrow to hold the whole name.
    ///
    /// The caption gives way to the ruler rather than the reverse — see
    /// `ring::centre_room` — and this is what makes that trade honest: on a
    /// 4.6 Mb genome the caption is a 69-character filename, and truncating it
    /// with the whole string a hover away costs nothing, while dropping the
    /// scale annotation cost the map its only statement of size.
    pub caption_full: Option<(Rect, String)>,
}

/// Draw the molecule. `selected` highlights one feature; `hot` is the one the
/// pointer is over elsewhere in the UI.
pub fn show(
    ui: &mut Ui,
    mol: &Molecule,
    caption: &str,
    digest: &[Digest],
    selected: Option<usize>,
    hot: Option<usize>,
) -> MapResponse {
    let pal = Palette::of(ui.visuals().dark_mode);
    // Take what is LEFT of the panel, and a painter clipped to it.
    //
    // `available_size()` reported far more than this panel actually owns, so a
    // map sized from it grew until it covered the side panel and ran off the
    // bottom of the window. The map is painted last, so it simply hid them.
    //
    // `max_rect()` was the whole CentralPanel, which was harmless only while
    // every label was in one of two side columns: the top of the pane was empty,
    // so nothing was drawn over the crash-recovery banner laid out above the map
    // in the same panel. `ring::place_ring` now puts a row there, and
    // `EcoRI  7,530   BbsI  7,963   AflII  271   SpeI  562` was painted across
    // the banner's path with SpeI's leader drawn down through the `Discard`
    // button. That banner is the recover-or-discard decision for an unsaved
    // draft; map ink over its buttons is worse than cosmetic. So the vertical
    // extent comes from what is left after the banner.
    //
    // And both extents are intersected with the CLIP rect, because the layout
    // rect and the clip rect do not agree once the splitter has been dragged
    // inwards. Measured with `PL_GUI_DEBUG_GEOMETRY=1` on the user's own file
    // after dragging the splitter from 716 pt to 376: the details panel really is
    // 911.2 pt of a 1,280 pt window, the CentralPanel's clip is `0..368.8`
    // exactly as it should be — and its layout rect is `8..663.6`, 295 pt wider,
    // on every frame and not only the drag frame. So the map centred itself on
    // 336, put its whole right-hand column behind the panel, cut the caption
    // mid-word at `pKoV with His`, and counted none of it as hidden — because
    // `shortened_to`, `place_ring`'s bounds and the disclosure line were all
    // computed against a rect the user cannot see.
    //
    // The clip rect is the one that cannot be wrong: it is by definition what
    // reaches the screen. Taking the smaller of the two is not a patch over
    // egui's bookkeeping, it is the correct question — how much of this can be
    // seen — asked of the only thing that knows the answer.
    let rect = {
        let a = ui.available_rect_before_wrap();
        let c = ui.clip_rect();
        // A pane with nothing left in it is still a rect: `from_min_max` with
        // `max < min` gives negative extents, and the 40 pt radius floor cannot
        // save that because the centre itself would be outside the window.
        Rect::from_min_max(
            a.min,
            Pos2::new(
                a.max.x.min(c.max.x).max(a.min.x + 1.0),
                a.max.y.min(c.max.y).max(a.min.y + 1.0),
            ),
        )
    };
    let response = ui.allocate_rect(rect, Sense::click());
    let painter = ui.painter_at(rect);
    let mut out = MapResponse {
        hovered: None,
        clicked: None,
        double_clicked: None,
        caption_full: None,
    };

    let span = mol.annotation_span().max(1);
    let bands = bands(mol);
    let pointer = response.hover_pos();

    if mol.topology == Topology::Circular {
        draw_circular(
            &painter, rect, span, &bands, caption, digest, mol, &pal, selected, hot, pointer,
            &mut out,
        );
    } else {
        draw_linear(
            &painter, rect, span, &bands, digest, mol, &pal, selected, hot, pointer, &mut out,
        );
    }

    if response.clicked() {
        out.clicked = out.hovered;
    }
    if response.double_clicked() {
        out.double_clicked = out.hovered;
    }
    if let Some((at, full)) = &out.caption_full {
        ui.interact(*at, ui.id().with("caption"), Sense::hover())
            .on_hover_text(full);
    }
    out
}

// ---------------------------------------------------------------------------
// circular
// ---------------------------------------------------------------------------

/// How close a label may come to the edge of the panel.
const LABEL_PAD: f32 = 6.0;
/// Air between two labels stacked in the same column.
///
/// The gap Hack happened to leave under the pinned `LINE_H = 13.0` this replaces
/// was 1.36 pt. This is that number made deliberate, so the clearance is a
/// property of the layout instead of a leftover of one face's line box.
const LINE_GAP: f32 = 1.5;

/// The size the ruler's numbers are drawn at, on both the circular and the
/// linear map, in one place because it is passed to `pl_draw` as a clearance and
/// drawn as text and the two must be the same number.
const RULER_PT: f32 = 9.0;

/// The vertical pitch of a label column: the label font's own drawn height,
/// plus [`LINE_GAP`].
///
/// **THIS WAS `const LINE_H: f32 = 13.0` AND THE FONT SWAP IS WHAT PROVED IT
/// COULD NOT STAY ONE.** `place_column` packs labels by this height and treats
/// the positions it returns as centres, so tightly stacked labels end up exactly
/// this far apart; `texts_in` then measures each drawn label as
/// `Rect::from_min_size(t.pos, t.galley.size())`, the face's REAL row height.
/// The two numbers were 13.0 and 11.64 under Hack — 1.36 pt of accidental
/// clearance — and `epaint` takes a family's line box from the FIRST face in the
/// chain, so installing IBM Plex Mono made the drawn height 1.300 em, which at
/// the 10 pt label size is exactly 13.00. Zero gap, and `Rect::intersects`
/// counts touching as intersecting, so
/// `every_enzyme_label_is_whole_inside_the_pane_and_points_at_its_own_tick`
/// went red on its no-two-labels-overlap assertion. A pinned pitch beside a
/// measured height is a defect waiting for whoever changes the face next.
///
/// Note what is NOT the fix: loosening that assertion to `expand(-0.5)`, which
/// would have made the suite green and the labels touching.
///
/// The advance band cannot see any of this. It models the painter horizontally,
/// and a line box is vertical.
fn line_h(p: &egui::Painter) -> f32 {
    // Through the same `layout_no_wrap` the labels are measured and drawn with,
    // rather than `row_height`, so the packer's pitch and the drawn rect come
    // from one measurement and cannot disagree about the face.
    p.layout_no_wrap("Ag".to_owned(), label_font(), Color32::WHITE)
        .size()
        .y
        + LINE_GAP
}
/// From the outermost feature lane to where a leader starts.
const TICK_GAP: f32 = 6.0;
/// From where a leader starts to the label's own anchor.
const LEADER_GAP: f32 = 26.0;
/// Radial thickness of a feature band, and the pitch between lanes.
const BAND_W: f32 = 9.0;
const LANE_STEP: f32 = 13.0;
/// Stroke of the mark a cut site puts on the ring, and the arc below which two
/// sites are one mark. See `pl_draw::ring::bases_per_arc`.
const TICK_STROKE: f32 = 1.5;

/// An arrowhead's length along the arc, in band widths.
///
/// Today's `w * 1.6`, kept: proportional to the band is the right invariant for a
/// 9 pt screen band, and this pass is about the overlap and not about the size.
/// Worth recording that it leaves the two renderers unequal — `pl_draw` uses a
/// fixed 8.0 scene units against an 18-unit band, so the same feature's head
/// subtends about half as much in the exported figure (2.04 degrees against 4.06
/// for `Rep101(Ts)` on pKoV). That is a parity gap to close on purpose, not as a
/// side effect of fixing an overlap.
const ARROW_LEN: f32 = 1.6;

/// How far the body's end is drawn UNDER the head, in points of arc.
///
/// The arrow cannot be one path here: it is concave at the barbs and egui only
/// fills convex polygons (`Shape::convex_polygon`), so unlike
/// `pl_draw::arc_segs` — one closed path, one fill — the body stays a stroked
/// polyline and the head a separate filled triangle. Two antialiased shapes
/// sharing an edge do not composite to full coverage: each contributes about half
/// and the result reads as a lighter hairline straight down the feature at the
/// head's base. Two sub-pixel effects push the same way: `arc_points` samples the
/// arc as chords, so the body's butt cap is up to 0.75 degrees off radial and
/// misses the head's radial base edge by about 0.06 pt at the band's outer edge,
/// and a chord-stroked outer edge dips 0.017 pt inside the true radius mid-step.
///
/// Invisible, because the head is radially wider than the body — `w * 0.8` against
/// `w * 0.5` — and it cannot lengthen the feature, because it points inward, at the
/// tip, under opaque fill. The geometry assertions therefore read
/// `body_end <= head_base + seam` and not equality: exact equality would either
/// fail on float noise or force an implementation that draws a crack.
const SEAM_PT: f32 = 0.5;

/// Below this many degrees a feature is drawn as a radial mark, not an arc.
///
/// `pl_draw::Options::min_feature_degrees`' default, so the screen and the figure
/// make the same call about the same feature. Two reasons it has to exist here
/// too, and the second is the serious one.
///
/// An arrowhead smaller than a pixel reads as dirt on the figure — that is the
/// exporter's reason. And a band whose arc is a few bases long *tessellates into
/// garbage*: `arc_points` floors only the `sweep` it counts steps with, and
/// interpolates with the raw `a1 - a0`, so pET28a's single-base `rep_origin`
/// (2,464..2,464, a real GenBank location) yielded three coincident points, and
/// `Shape::line` over a zero-length path with a 9 pt stroke painted a translucent
/// wedge across half the map pane. `draw_arrowhead` did the same with three
/// collinear vertices. It appeared on four of nine real files — a 3 bp CDS on
/// pACYC184, a 9 bp `-10` box on the E. coli genome, two more on a Borrelia
/// plasmid — as feature-coloured straight lines drawn through the backbone, the
/// centre caption and the note. It is *radius-dependent*, absent at a maximised
/// window and present at the default, so it is not something this pass could
/// change every radius on the map and leave alone.
///
/// A mark rather than a floored arc: a 1 bp feature drawn as a 4 pt arc overstates
/// its extent, and the exporter already chose the mark for exactly that reason.
const MIN_FEATURE_DEGREES: f32 = 1.2;

/// Everything between the backbone and the first glyph of a side-column label.
///
/// This is the number `LABEL_RESERVE = 132.0` left out. The reserve was a flat
/// constant and the leader spent 54 pt of it before a lane was charged, so on
/// the user's own pKoV — two feature lanes — a label had 65 pt, which is 10.8
/// characters at IBM Plex Mono's 6.000 pt advance (this was written against
/// Hack's 6.021 and rounded to "6 pt"; it is now exact). `EcoRI 7,530` is 12 and was drawn
/// `coRI 7,530`; `HindIII 2,059` came out `HindIII 2,`. A cut coordinate is a
/// *wrong* coordinate, and it is not rare: of five ordinary plasmids measured
/// at the shipped pane size, four clipped every label they drew.
///
/// Note what the arithmetic does to the obvious remedy. `room = reserve -
/// outward`, so *reducing* the reserve to win back radius reduces the room as
/// well — the sign is the opposite of the intuition, which is why the
/// `DEF_PANEL` doc comment could diagnose the symptom exactly and still ship
/// it: it computed the room as 132.
///
/// Only **forward** lanes are charged. Reverse lanes stack inward and cost the
/// outward budget nothing; the old `outer` took the maximum lane over all
/// bands, so a reverse-heavy plasmid spent its whole label reserve on lanes
/// drawn on the other side of the backbone.
fn outward_of(forward_lanes: usize) -> f32 {
    BAND_W + forward_lanes as f32 * LANE_STEP + TICK_GAP + LEADER_GAP
}

/// The 1-based coordinate of the `i`th tenth-of-the-molecule ruler tick.
///
/// Multiplied in `u128`, because `span` is a coordinate straight out of a file
/// and nothing on the open path has validated it. An annotation-only `.dna`
/// carrying `range="1-18446744073709551615"` — the same 185-byte hostile file
/// `lanes` and `arc_points` were hardened against — makes `annotation_span()`
/// return `u64::MAX`, and `Molecule::validate` does not even flag it: its
/// past-the-end check is gated on a non-empty sequence and there are no bases
/// here, so the status bar stays silent too.
///
/// `span * i` then overflowed from `i = 2` on. Debug builds panicked with
/// "attempt to multiply with overflow" and the window died on open; release
/// ships with overflow-checks off, so it wrapped and every labelled tick
/// printed 1,844,674,407,370,955,16x instead of 3.69, 7.38, 11.07 and 14.76
/// x 10^18 — four near-identical fabricated coordinates that `angle_of` then
/// placed on the same spoke, collapsing the whole ruler.
fn tick_pos(span: u64, i: u64) -> u64 {
    // The product needs 68 bits at most (u64::MAX x 10), and the quotient is
    // never larger than `span`, so the narrowing back is exact. The final
    // increment saturates for the one input that reaches the top: i = 10 on a
    // span of u64::MAX, in `draw_linear`'s inclusive loop.
    (((span as u128 * i as u128) / 10) as u64).saturating_add(1)
}

/// Position on the molecule to angle, starting at twelve o'clock and running
/// clockwise, which is how every plasmid map in the literature is drawn.
fn angle_of(pos: u64, span: u64) -> f32 {
    let frac = (pos.saturating_sub(1)) as f32 / span as f32;
    -std::f32::consts::FRAC_PI_2 + frac * std::f32::consts::TAU
}

/// Where an arc that ENDS at `pos` closes: one base further on.
///
/// `pl_draw::angle_past`, in this file's convention. The two renderers disagreed
/// about this: `pl_draw::scene` closes a feature arc at `angle_past(b)` and this
/// file closed it at `angle_of(e)`, so every band on the screen covered one base
/// fewer than the same band in the exported figure — 0.044 degrees on pKoV, but
/// 3.6 on a 100 bp molecule.
///
/// It is not only a parity point. `angle_of(e) - angle_of(s)` is ZERO for a 1 bp
/// part, and a part is not covered by the `MIN_FEATURE_DEGREES` gate, which sums
/// the whole feature's bases: `join(1..1000, 2000..2000)` reached
/// `draw_arrowhead` with a sweep of 0 and got a needle spike off the `.max(0.002)`
/// floor — the same three-collinear-vertices hazard that once tessellated a wedge
/// across half the pane. With this, a part covers the bases it names and the
/// invariant "the body's extent plus the head's equals the feature's" is literally
/// true rather than true up to one base.
///
/// `pos % span` and not `pos + 1`, for the reason `angle_past` gives: `span` comes
/// off a LOCUS line and can be `u64::MAX`, where the addition overflows before the
/// division is reached.
fn angle_end(pos: u64, span: u64) -> f32 {
    if span == 0 {
        return -std::f32::consts::FRAC_PI_2;
    }
    let frac = (pos % span) as f32 / span as f32;
    -std::f32::consts::FRAC_PI_2 + frac * std::f32::consts::TAU
}

/// How much of a part's arc its arrowhead takes, in radians.
///
/// Zero means "no room for a head — draw the part as a plain arc". The single
/// number that decides both where the body STOPS and where the head's base sits:
/// they were two expressions in two functions, which is how the body came to be
/// drawn full length with the head painted on top of its last few degrees. That
/// is the same class of defect this file's own header names for `LABEL_RESERVE` —
/// a decision made in one unit and a drawing made in another.
///
/// Arc length is `r * theta`, so a head `w * ARROW_LEN` points long at the band's
/// own radius subtends `w * ARROW_LEN / radius`.
///
/// **Clamped to HALF the sweep, and the clamp is last.** Past half, the head's
/// base lands before the part's own start and the outline crosses itself — the
/// classic artefact where a short feature renders as a bow tie. It was
/// `.min(sweep * 0.9).max(0.002)`, and the trailing `max` is the trap: applied
/// after the clamp it can return a head LONGER than half the arc, reintroducing
/// the inversion for exactly the smallest inputs, where nobody looks. There is no
/// floor here; where a head would be sub-pixel the caller draws no head, which is
/// honest, rather than a floored one, which is a degenerate polygon.
fn head_angle(sweep: f32, radius: f32, w: f32) -> f32 {
    // `is_finite` before the comparison, and not `!(sweep > 0.0)`: NaN has to be
    // rejected explicitly rather than by relying on a negated comparison being
    // true for it. `min` propagates a NaN's operand silently, and a NaN head
    // reaches `polar` as a vertex at nowhere.
    if !sweep.is_finite() || !radius.is_finite() || !w.is_finite() || sweep <= 0.0 {
        return 0.0;
    }
    (w * ARROW_LEN / radius.max(1.0)).min(sweep * 0.5)
}

fn polar(center: Pos2, radius: f32, angle: f32) -> Pos2 {
    Pos2::new(
        center.x + radius * angle.cos(),
        center.y + radius * angle.sin(),
    )
}

#[allow(clippy::too_many_arguments)]
fn draw_circular(
    p: &egui::Painter,
    rect: Rect,
    span: u64,
    bands: &[Band],
    caption: &str,
    digest: &[Digest],
    mol: &Molecule,
    pal: &Palette,
    selected: Option<usize>,
    hot: Option<usize>,
    pointer: Option<Pos2>,
    out: &mut MapResponse,
) {
    let center = rect.center();
    let lane_step = LANE_STEP;
    let band_w = BAND_W;
    let pane_min = rect.width().min(rect.height());

    // Lanes counted per side. Reverse bands stack *inward* and cost the outward
    // label budget nothing, which the old single `max` over all bands did not
    // know.
    let lanes_of = |reverse: bool| {
        bands
            .iter()
            .filter(|b| b.reverse == reverse && !b.segs.is_empty())
            .map(|b| b.lane + 1)
            .max()
            .unwrap_or(0)
    };
    let (fwd_lanes, rev_lanes) = (lanes_of(false), lanes_of(true));
    let outward = outward_of(fwd_lanes);

    // What the map labels, and what it is therefore not showing.
    let unique: Vec<(String, u64)> = digest
        .iter()
        .filter(|d| d.is_unique_cutter())
        .map(|d| (d.enzyme.name.to_string(), d.positions[0]))
        .collect();
    let cutters = digest.iter().filter(|d| d.count() > 0).count();
    let dual = digest.iter().filter(|d| d.count() == 2).count();
    let multi = digest.iter().filter(|d| d.count() > 2).count();

    let measure = |s: &str| {
        p.layout_no_wrap(s.to_string(), label_font(), pal.ink2)
            .size()
            .x
    };
    // Measured once, beside the width that is measured the same way, because the
    // reserve, the column pitch and the leader's attach point must all be talking
    // about the same face. See `line_h`.
    let line_h = line_h(p);
    // A label's angle in `pl_draw`'s convention: zero at twelve o'clock, `x`
    // from the sine. egui's map runs `-PI/2 + frac * TAU` off the cosine, which
    // is the same circle a quarter turn back, so adding it here means every
    // point `place_ring` returns is already a screen position.
    let ring_angle = |pos: u64| (angle_of(pos, span) + std::f32::consts::FRAC_PI_2) as f64;
    let row_half = 30f64.to_radians();
    // Every site label, whatever run it starts in.
    //
    // This filtered to `Side::Left | Side::Right` — the same filter, and the same
    // defect, as `pl_draw::scene`'s `widest_of`, which is what makes fixing only one
    // of them a screen/figure divergence rather than a fix. The argument for the
    // filter is that a twelve- or six-o'clock label costs vertical room and not
    // radius; the reason it is wrong is that `ring::label_room` cuts every label to
    // ONE allowance whatever its run, because `place_ring` spills what a row cannot
    // hold into a column, and on a large ring the binding term of that allowance is
    // the column's. A molecule whose only unique cutter sits near 50% therefore
    // reserved nothing, and `EcoRI  402` was drawn `Ec...` — a destroyed enzyme
    // coordinate, at every canvas size, which is exactly what the computed reserve
    // replaced `LABEL_RESERVE = 132.0` to stop.
    //
    // Every entry here is a site label already — `one_each` is built from the unique
    // cutters and nothing else — so this reserves for enzyme names only, which is
    // the same rule the exporter applies. Feature names are not in this list and do
    // not move the radius, in either renderer.
    let widest_site_label = |labels: &[(String, u64)]| -> f32 {
        labels
            .iter()
            .map(|(text, _)| measure(text))
            .fold(0.0_f32, f32::max)
    };
    // The pane is wider than it is tall as often as not, and the two axes do
    // not need the same clearance: a side column spends horizontal room on the
    // widest name, while the twelve- and six-o'clock rows spend a fixed
    // vertical strip. `min(w, h) * 0.5 - reserve` charged the widest name to
    // both. The rule lives in `pl_draw::ring::radius` so the exporter has it
    // too — it did not, and gave away 26% of the radius on a wide figure.
    let radius_for = |widest: f32| -> f32 {
        // The one point of cushion is not decoration. The reserve is decided in
        // f64 inside `pl-draw` and the room is measured back in f32 here, and
        // when the two are exactly equal — the common case, since the reserve
        // is *derived* from the widest label — the round trip can land a
        // fraction the wrong side and shorten a name that fits.
        let reserve =
            ring::reserve_for(widest as f64, outward as f64, pane_min as f64).reserve as f64 + 1.0;
        let vertical = (outward + line_h + LABEL_PAD * 2.0) as f64;
        ring::radius(rect.width() as f64, rect.height() as f64, reserve, vertical) as f32
    };

    // The radius is decided by the widest **unmerged** label, before anything is
    // folded together, because merging may not buy itself room.
    //
    // An honest merged label carries every name and the range — see
    // `ring::Site::label` for why it must — so it is always wider than either of
    // the names in it: `XmaI/SmaI  6,917-6,919` is 22 characters against
    // `HindIII  2,059`'s 14. Sizing the ring to hold it costs 47 pt of radius on
    // the user's own file, a quarter of the ring, to gain tidiness.
    let one_each: Vec<(String, u64)> = unique
        .iter()
        .map(|(n, pos)| (format!("{n}  {}", crate::doc::fmt_int(*pos)), *pos))
        .collect();
    let r = radius_for(widest_site_label(&one_each));
    let outer = r + band_w + fwd_lanes as f32 * lane_step;
    let tick_r = outer + TICK_GAP;
    let geom = RingGeom {
        cx: center.x as f64,
        cy: center.y as f64,
        tick_r: tick_r as f64,
        gap: LEADER_GAP as f64,
        row_half,
        row_gap: 10.0,
        left: (rect.left() + LABEL_PAD) as f64,
        right: (rect.right() - LABEL_PAD) as f64,
        top: (rect.top() + LABEL_PAD + line_h) as f64,
        bottom: (rect.bottom() - LABEL_PAD) as f64,
    };
    // What a label may actually use, read off the geometry it is placed with
    // rather than rebuilt from `r`, and one number for all four runs because a
    // label the rows cannot hold ends up in a column. See `ring::label_room`.
    let room = ring::label_room(&geom, (rect.width() * 0.5) as f64) as f32;

    // Sites whose ticks are the same tick become one label — but only if the
    // honest form of that label fits the room the individual names had already
    // earned.
    //
    // When it does not, the sites stay separate and the packer moves them one
    // line apart. Untidy and true: the alternative is a merged label with an
    // ellipsis through it, and shortening `XmaI  6,917 / SmaI  6,919` can drop a
    // whole enzyme, which is the failure `Site::label` exists to prevent. Two
    // ordinary labels a line apart tell no lie.
    //
    // The threshold is the TICK's stroke, not a label height. A label height in
    // bases grows as the ring shrinks, so the same molecule folded sites 10 bp
    // apart at a maximised window and 126 bp apart at 704 pt — resizing the
    // window changed what the map claimed about the plasmid, and NsiI's own
    // 4,760 appeared nowhere.
    let within = ring::bases_per_arc(TICK_STROKE as f64, tick_r as f64, span);
    let folded = ring::merge_sites(&unique, within);
    // The enzyme NAMES alongside each label, because the line under the caption
    // counts ENZYMES and a folded tick names several. Counting labels is
    // invisible until a fold fires and then understates itself: pET28a claimed
    // `14 of 31 cutters labelled` with 23 on the map, and 14 + 7 + 1 did not
    // reach the 31 it had just stated.
    //
    // The names and not a per-label tally, which is the shape this carried and
    // which is only right by accident here. `unique` holds one pair per enzyme,
    // so summing the tally happens to equal the distinct count — the same
    // accidental immunity that made `pl export --sites unique` right and
    // `--sites dual` print "46 of 40". Two things break it, neither
    // hypothetical: a "show dual cutters" toggle, which `--sites dual` already
    // does in the CLI; and a molecule large enough that `merge_sites`' `within`
    // (which scales with the span — thousands of bases on a 4.6 Mb genome) folds
    // two cuts of ONE enzyme into a single tick, where `s.names.len()` counts it
    // once per cut. Correct by construction beats correct by input.
    let mut labels: Vec<(String, u64)> = Vec::new();
    let mut names_in: Vec<Vec<String>> = Vec::new();
    for s in folded {
        if s.names.len() == 1 || measure(&s.label()) <= room {
            labels.push((s.label(), s.anchor()));
            names_in.push(s.names.clone());
        } else {
            for (n, p) in s.names.iter().zip(&s.positions) {
                labels.push((format!("{n}  {}", crate::doc::fmt_int(*p)), *p));
                names_in.push(vec![n.clone()]);
            }
        }
    }

    // Cut sites, outside everything, in an L-shaped ring.
    //
    // One column per side was the shipped answer and it is what produces the
    // complaint: 16 labels stacked down the left against 6 on the right, with
    // leaders running most of the radius at a degree or two off horizontal.
    // `label_slots` was not the cause — measured on this file it leaves 13 of
    // the 16 exactly at their anchors and the column is 27% full — and
    // rebalancing the sides makes it worse, not better: BbsI's tick is 31 pt
    // left of centre, so moving it to the right-hand column lengthens its
    // leader from 249 pt to 312 and drags it across the top of the ring.
    // Labels near twelve and six o'clock take a horizontal run instead, which
    // on this molecule halves the median leader and cuts the longest by 42%.
    //
    // Placed here, before anything is painted, because one of the lines written
    // in the middle of the ring reports the outcome — and the middle is what the
    // ruler and the inward lanes have to be kept off.
    let shortened_to = |text: &str| -> Option<String> {
        if measure(text) <= room {
            return Some(text.to_string());
        }
        // The coordinate goes whole or not at all. `EcoRI  7,5...` reads as a cut
        // position and is not one, and a wrong coordinate on a plasmid map is the
        // failure this whole pass is about — `EcoRI` with nothing after it claims
        // nothing, and the line under the caption counts it as shortened either
        // way. Two spaces, so a name with one space in it is left alone.
        if let Some((head, _)) = text.rsplit_once("  ") {
            if !head.is_empty() && measure(head) <= room {
                return Some(head.to_string());
            }
        }
        // Three ASCII full stops, not U+2026: three dots are three dots in
        // every encoding and the real character is not.
        let mut kept = String::new();
        for c in text.chars() {
            let trial = format!("{kept}{c}...");
            if measure(&trial) > room {
                break;
            }
            kept.push(c);
        }
        (!kept.is_empty()).then(|| kept + "...")
    };
    // Every run, including the rows. Exempting a row label from shortening did
    // not remove the clipping this pass is about, it moved it into the run the
    // fix created: on stock pET28a the top row ran off the pane edge mid-glyph at
    // `Hind`, which is a name one letter from reading as a different enzyme, with
    // no ellipsis and nothing in the note about it.
    let drawn: Vec<Option<String>> = labels.iter().map(|(text, _)| shortened_to(text)).collect();
    let boxes: Vec<RingLabel> = labels
        .iter()
        .zip(&drawn)
        .map(|((_, pos), text)| RingLabel {
            angle: ring_angle(*pos),
            width: text.as_deref().map_or(0.0, |t| measure(t) as f64),
            height: line_h as f64,
            weight: 1.0,
        })
        .collect();
    let placed = ring::place_ring(&boxes, &geom);

    // What the map is *not* showing, said in the map.
    //
    // 58 enzymes are digested, 40 cut this plasmid and 22 cut it exactly once.
    // The other 18 — twelve dual cutters and six multi — were drawn nowhere and
    // mentioned nowhere, and a dual cutter is exactly what you want for an
    // excision. `docs/PLAN.md` item 33 calls a silent filter "the one documented
    // case of this software category costing a user a month of bench time";
    // `pl_enzymes::Visibility` exists so that what a filter hides can never be
    // silent, and this map was hiding 18 enzymes without it.
    //
    // The Enzymes tab's own `EnzymeSet` is deliberately *not* what the map
    // draws. Its default is "All cutters", which on this file is 40 enzymes and
    // about 100 ticks — a map nobody can read, arrived at without the user
    // asking for it. The map keeps its own rule, and states it.
    // Enzymes, never labels and never mentions — see `cutters_shown`, which is
    // where the arithmetic lives so that it can be asserted without a painter.
    // Only the names a reader can still READ, which is not the same as the labels
    // that were placed. `shortened_to` drops the coordinate before it reaches for
    // an ellipsis, so `EcoRI  7,530` becomes `EcoRI` and the enzyme is still
    // named — but at a small enough `room` it becomes `Ec...`, and counting that
    // as a labelled cutter says the figure names an enzyme it does not. Same rule
    // as `pl_draw`'s paint loop, so screen and figure keep agreeing; the names
    // that fail it fall into `unnamed` through `cutters_shown`'s set difference.
    let legible: Vec<Vec<String>> = (0..labels.len())
        .filter(|&i| placed.placed[i].is_some())
        .filter_map(|i| drawn[i].as_deref().map(|t| (i, t)))
        .map(|(i, t)| {
            names_in[i]
                .iter()
                .filter(|n| t.contains(n.as_str()))
                .cloned()
                .collect()
        })
        .collect();
    let shown: Vec<&[String]> = legible.iter().map(Vec::as_slice).collect();
    let (labelled, unnamed) = cutters_shown(&unique, &shown);
    let told = ring::Disclosure {
        cutters,
        labelled,
        dual,
        multi,
        hidden: unnamed,
        shortened: (0..labels.len())
            .filter(|&i| placed.placed[i].is_some())
            .filter(|&i| drawn[i].as_deref().is_some_and(|s| s != labels[i].0))
            .count(),
        // Zero because `unique` is `filter(is_unique_cutter)`: a single cutter is
        // never turned away here. It is a term the moment that filter widens,
        // and the assertion below is what will say so.
        single: 0,
    };
    // The same guard both export paths have (`bins/pl/src/main.rs` and
    // `bins/pl-gui/src/main.rs`'s `figure_options`), which this producer did not.
    // It is correct today only because the unique filter hands it one pair per
    // enzyme, so a mention, a label and an enzyme are the same integer — which is
    // precisely the accident that hid the mention-counting bug in `pl-draw` until
    // `--sites dual` was asked for. `cutters_shown`'s own doc names a "show dual
    // cutters" toggle as the obvious next feature; this is what catches it.
    debug_assert!(told.closes(), "{told:?} does not account for every cutter");
    let bp = format!("{} bp", crate::doc::fmt_int(mol.span()));
    let width_of = |s: &str, f: FontId| p.layout_no_wrap(s.to_string(), f, pal.ink).size().x;
    // The ruler number's DRAWN height, which is not its font size.
    //
    // `ring::centre_room` and `ring::inside_of` both use `text_h * 0.5` as a
    // half-height, and both were handed the literal `9.0` — the point size. Under
    // Hack the real drawn height was 10.48 pt, so the clearance was already 0.74 pt
    // short; IBM Plex Mono's line box is 1.300 em against Hack's 1.164, making it
    // 11.70 pt and the shortfall 1.35 pt. That number is spent at the 40 pt radius
    // floor, where `nothing_written_in_the_middle_leaves_the_ring` and
    // `a_ruler_number_is_clear_of_every_feature_band` are exercised, and it is the
    // margin between a legible scale and `3,247` sitting on a feature band.
    let ruler_h = p
        .layout_no_wrap("0".to_owned(), FontId::monospace(RULER_PT), pal.ink)
        .size()
        .y;

    // Everything written in the middle is cut to the ring BEFORE the ruler is
    // placed, and never the other way round.
    //
    // Deriving the ruler's clearance from an unbounded caption and then dropping
    // whichever of the two was checked second is what cost a 4.6 Mb genome its
    // whole scale: `caption_of` leaves a 69-character filename, which is 517 pt
    // of proportional 15, and no radius on this pane clears that. The caption is
    // the line with a hover behind it and the ruler is not, so the caption is the
    // one that gives way. See `ring::centre_room`.
    let widest_number = width_of(&crate::doc::fmt_int(span), FontId::monospace(RULER_PT));
    let centre_room = ring::centre_room(
        r as f64,
        band_w as f64,
        lane_step as f64,
        ruler_h as f64,
        widest_number as f64,
    ) as f32;
    let cut_to = |text: &str, font: FontId| -> Option<String> {
        if width_of(text, font.clone()) <= centre_room {
            return Some(text.to_string());
        }
        let mut kept = String::new();
        for c in text.chars() {
            if width_of(&format!("{kept}{c}..."), font.clone()) > centre_room {
                break;
            }
            kept.push(c);
        }
        (!kept.is_empty()).then(|| kept + "...")
    };
    let caption_drawn = cut_to(caption, FontId::proportional(15.0));
    let bp_drawn = cut_to(&bp, FontId::monospace(11.0));
    // Two forms, and the longer one only when the middle is wide enough to hold
    // it. A sentence wider than the ring is drawn across the backbone and the
    // features, in `pal.muted` over whatever colour the file chose, and that is
    // a worse answer than the short form. At a pane too small for even the short
    // form the line goes: the Enzymes tab lists every cutter with its count, so
    // this is a signpost and not the only record. Never an ellipsis — half a
    // count is a number the reader would act on.
    let note = (cutters > 0)
        .then(|| [told.long(), told.short(), told.tiny()])
        .and_then(|forms| {
            forms
                .into_iter()
                .find(|f| width_of(f, FontId::proportional(10.0)) <= centre_room)
        });

    // The footprint of what was actually drawn, so the ruler and the inward
    // lanes can be kept off it. Twenty mutually overlapping reverse features used
    // to spiral inward until "8,117 bp" was `pal.muted` on a coloured band at
    // 2.9:1.
    //
    // The *note* counts as well, and on a plasmid with plenty of dual cutters it
    // is the widest of the three lines. Measuring only the caption would keep
    // the bands off the plasmid's name and let them cross the sentence saying
    // what the map is not showing.
    let centre_w = [
        caption_drawn
            .as_deref()
            .map_or(0.0, |t| width_of(t, FontId::proportional(15.0))),
        bp_drawn
            .as_deref()
            .map_or(0.0, |t| width_of(t, FontId::monospace(11.0))),
        note.as_deref()
            .map_or(0.0, |n| width_of(n, FontId::proportional(10.0))),
    ]
    .into_iter()
    .fold(0.0_f32, f32::max);
    let inside = ring::inside_of(
        r as f64,
        band_w as f64,
        lane_step as f64,
        rev_lanes,
        ruler_h as f64,
        ring::keep_clear_for(centre_w as f64, widest_number as f64),
    );

    // backbone
    p.circle_stroke(center, r, Stroke::new(1.5, pal.line));

    // Ticks every 10% of the molecule, labelled in bp, in a radial band of
    // their own under every inward lane.
    //
    // The "3,247" tick on the user's pKoV was painted over by SacB, a reverse
    // feature: the number spanned `r-21.5..r-11.5` and reverse lane 0 spans
    // `r-15..r-3` when emphasised, so the ruler and the lanes shared radii and
    // the features — drawn second — always won. Exactly one of the five
    // labelled ticks broke, which is not luck: it is the one tick that happened
    // to fall inside a reverse feature.
    //
    // Painting the ruler *last* instead was measured and rejected. It puts
    // `pal.muted` on SacB's `#993366` at 2.17:1, on CmR's `#ccffcc` at 2.86:1
    // and on f1 ori's `#ffff00` at 2.98:1 — under half of AA — while
    // `theme.rs`'s contrast test measures `muted` against the *background* and
    // stays green. That is a legibility bug traded for an accessibility bug no
    // gate can see.
    // A number OR a mark at each tick, never both.
    //
    // The mark is offset radially from the number's centre by a little over half
    // the text's HEIGHT, and a number is two and a half times as wide as it is
    // tall: along a ray near the horizontal — 72 degrees on this plasmid, the
    // "1,624" tick — the box reaches further outward than the mark begins and the
    // hairline was drawn through the last digit. Clearing it in every direction
    // would mean reserving the box's half-diagonal instead of its half-height,
    // which costs the inward lanes 9 pt for a mark that says nothing the number
    // does not: a number centred on the ray already locates the tick.
    let (tick_in, tick_out) = (inside.ruler_tick.0 as f32, inside.ruler_tick.1 as f32);
    for i in 0..10 {
        let pos = tick_pos(span, i);
        let a = angle_of(pos, span);
        // Every other tick is numbered, and on a pane too small for numbers at
        // all every tick gets a mark instead. See `Inside::numbers`.
        if i % 2 == 0 && inside.numbers {
            p.text(
                polar(center, inside.ruler_text_r as f32, a),
                Align2::CENTER_CENTER,
                crate::doc::fmt_int(pos),
                FontId::monospace(RULER_PT),
                pal.muted,
            );
        } else {
            p.line_segment(
                [polar(center, tick_in, a), polar(center, tick_out, a)],
                Stroke::new(1.0, pal.line),
            );
        }
    }

    // features
    for b in bands {
        // Reverse-strand features sit inside the backbone, forward outside:
        // the convention that lets you read direction without a legend. The
        // inward radius is floored: unclamped it went negative from lane 17 on
        // a 218 pt ring, and `polar` then mirrored those arcs to the opposite
        // side of the map under the wrong coordinates, where the hit test
        // `(d - base).abs() <= 7.5` against a negative `base` could never reach
        // them. Drawn and unreachable is worse than drawn twice.
        let base = if b.reverse {
            ring::inward_radius(r as f64, band_w as f64, lane_step as f64, b.lane, &inside) as f32
        } else {
            r + band_w + b.lane as f32 * lane_step
        };
        let emphasised = selected == Some(b.index) || hot == Some(b.index);
        let w = if emphasised { band_w + 3.0 } else { band_w };

        // A feature too short to draw to scale is a mark across the band, the
        // same call `pl_draw` makes at the same threshold. See
        // `MIN_FEATURE_DEGREES` for the wedge that came out of drawing it as an
        // arc instead.
        let bases: u64 = b.segs.iter().map(|&(s, e)| e - s + 1).sum();
        let tiny = (bases as f64 / span as f64) * 360.0 < MIN_FEATURE_DEGREES as f64;
        if tiny {
            for &(s, _) in &b.segs {
                let a = angle_of(s, span);
                p.line_segment(
                    [
                        polar(center, base - w * 0.5, a),
                        polar(center, base + w * 0.5, a),
                    ],
                    Stroke::new(1.75, b.color),
                );
            }
        } else {
            // Each part as its own arc, always the short way round: `bands` has
            // already split anything that crosses the origin, so `s <= e` here
            // and `arc_points` never interpolates backwards across the whole
            // ring.
            //
            // The part carrying the arrowhead stops where the head BEGINS. The
            // body used to be drawn full length and the head painted over its
            // last few degrees, in the band's own colour, which is why a feature
            // read as a flat-ended bar with a chevron embossed in it and two
            // shoulders sticking out — 4.06 degrees of double-painted arc on
            // pKoV's forward lane 0, 90% of the whole arc for the 103 bp
            // `cat promoter`. One number decides both ends of the seam, and it is
            // `head_angle`.
            for (i, &(s, e)) in b.segs.iter().enumerate() {
                let a0 = angle_of(s, span);
                // `angle_end` wraps: a part ending on the LAST base closes at the
                // origin's own angle, so `a1 == a0` for a whole-molecule feature
                // and `a1 < a0` for the pre-origin part of a wrapped one. Both
                // gave a non-positive sweep, which skipped the arc entirely — a
                // 287 bp part of an origin-crossing feature painted nothing at
                // all. `pl_draw::arc_segs` normalises the same way and for the
                // same reason.
                let mut a1 = angle_end(e, span);
                if a1 <= a0 {
                    a1 += std::f32::consts::TAU;
                }
                let head = if b.head == Some(i) {
                    head_angle(a1 - a0, base, w)
                } else {
                    0.0
                };
                // Under the head, not abutting it: see `SEAM_PT`. Clamped to half
                // the head so a fully-clamped head cannot be overrun by its own
                // seam.
                let seam = (SEAM_PT / base.max(1.0)).min(head * 0.5);
                let (body0, body1) = if b.reverse {
                    (a0 + head - seam, a1)
                } else {
                    (a0, a1 - head + seam)
                };
                // `arc_points` inflates any sweep below 0.004 rad back up to
                // 0.004 in the direction of travel, so a shortened body whose
                // remainder falls under that would be silently redrawn PAST the
                // head's base and the overlap would come back for the smallest
                // parts. Below the floor the head alone is the feature.
                if body1 - body0 >= 0.004 {
                    let pts = arc_points(center, base, body0, body1);
                    if pts.len() >= 2 {
                        p.add(Shape::line(pts, Stroke::new(w, b.color)));
                    }
                }
                if head > 0.0 {
                    let (tip_a, base_a) = if b.reverse {
                        (a0, a0 + head)
                    } else {
                        (a1, a1 - head)
                    };
                    draw_arrowhead(p, center, base, tip_a, base_a, barb_half(w), b.color);
                }
            }
            // Thin connectors show the joins, split the same way.
            for &(s, e) in &b.joins {
                let (h0, mut h1) = (angle_of(s, span), angle_end(e, span));
                if h1 <= h0 {
                    h1 += std::f32::consts::TAU;
                }
                let hair = arc_points(center, base, h0, h1);
                if hair.len() >= 2 {
                    p.add(Shape::line(hair, Stroke::new(1.0, b.color)));
                }
            }
        }

        // Hit-testing in polar space: on the band's radius, within any segment.
        if let Some(ptr) = pointer {
            let d = (ptr - center).length();
            if (d - base).abs() <= band_w * 0.5 + 3.0 {
                let a = (ptr.y - center.y).atan2(ptr.x - center.x);
                for &(s, e) in &b.segs {
                    // No lo/hi swap: `bands` guarantees `s <= e` on every part,
                    // so the interval is the arc that was drawn. Swapping used
                    // to be how an origin-crossing segment was handled here,
                    // and it gave a 258-degree hover region for a 25-degree
                    // feature — matching neither the band on screen nor the
                    // bases the feature describes.
                    // Widened by the same floor the drawing uses. A one-base
                    // feature subtends 0.04 degrees, so the unwidened interval
                    // was a window no pointer could land in: the mark was drawn
                    // and could not be hovered, which is the same
                    // drawn-and-unreachable failure `inward_radius` was floored
                    // for one radius in.
                    let (lo, hi) = (angle_of(s, span), angle_of(e, span));
                    let hi = hi.max(lo + MIN_FEATURE_DEGREES.to_radians());
                    // Normalise by arithmetic, not by accumulation.
                    //
                    // `while a < lo - PI { a += TAU }` terminates only while
                    // TAU is large enough to change `a`. `angle_of` scales a
                    // raw file coordinate by 1/span with no clamp, so `lo` is
                    // unbounded; past about 2^27 the f32 step size exceeds TAU,
                    // the addition stops moving the value, and the loop spins
                    // forever with the window frozen. One `.dna` with an
                    // out-of-range coordinate hung the app.
                    let mut a = a;
                    if lo.is_finite() && a.is_finite() {
                        let turns = ((lo - std::f32::consts::PI - a) / std::f32::consts::TAU)
                            .ceil()
                            .max(0.0);
                        if turns.is_finite() && turns < 1.0e6 {
                            a += turns * std::f32::consts::TAU;
                        }
                    }
                    if a >= lo && a <= hi {
                        out.hovered = Some(b.index);
                        break;
                    }
                }
            }
        }
    }

    // Cut sites: one leader each, ending at its own tick.
    for (i, (_, pos)) in labels.iter().enumerate() {
        let Some(pl) = placed.placed[i] else { continue };
        let Some(shown) = drawn[i].clone() else {
            continue;
        };
        let a = angle_of(*pos, span);
        p.line_segment(
            [polar(center, outer, a), polar(center, tick_r, a)],
            Stroke::new(1.0, pal.muted),
        );
        let at = Pos2::new(pl.at.0 as f32, pl.at.1 as f32);
        let stop = match pl.side {
            Side::Right => Pos2::new(at.x - 4.0, at.y),
            Side::Left => Pos2::new(at.x + 4.0, at.y),
            Side::Top => Pos2::new(at.x, at.y + line_h * 0.5),
            Side::Bottom => Pos2::new(at.x, at.y - line_h * 0.5),
        };
        p.add(Shape::line(
            vec![
                Pos2::new(pl.tip.0 as f32, pl.tip.1 as f32),
                Pos2::new(pl.bend.0 as f32, pl.bend.1 as f32),
                stop,
            ],
            Stroke::new(0.8, pal.faint),
        ));
        p.text(
            at,
            match pl.side {
                Side::Right => Align2::LEFT_CENTER,
                Side::Left => Align2::RIGHT_CENTER,
                Side::Top | Side::Bottom => Align2::CENTER_CENTER,
            },
            shown,
            label_font(),
            pal.ink2,
        );
    }

    // Centre caption. The .dna container carries no molecule name at all, so
    // fall back to what the user actually called the file.
    if let Some(text) = caption_drawn {
        let cut = text != caption;
        let at = p.text(
            center - Vec2::new(0.0, 9.0),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(15.0),
            pal.ink,
        );
        // Whose whole form is one hover away, which is the reason this is the
        // line that gives way to the ruler rather than the other way round.
        if cut {
            out.caption_full = Some((at, caption.to_string()));
        }
    }
    if let Some(text) = bp_drawn {
        p.text(
            center + Vec2::new(0.0, 11.0),
            Align2::CENTER_CENTER,
            text,
            FontId::monospace(11.0),
            pal.muted,
        );
    }
    if let Some(note) = note {
        p.text(
            center + Vec2::new(0.0, 28.0),
            Align2::CENTER_CENTER,
            note,
            FontId::proportional(10.0),
            pal.muted,
        );
    }
}

/// Sample an arc into a polyline. Enough segments that curvature is smooth at
/// any size, few enough that a genome with hundreds of features stays cheap.
fn arc_points(center: Pos2, radius: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    // Floor the sweep that is INTERPOLATED, not only the one that counts steps.
    // The two were different numbers: `sweep` was floored at 0.004 for `steps`
    // while the interpolation below used the raw `(a1 - a0)`, so a one-base
    // feature produced three coincident points and `Shape::line` with a 9 pt
    // stroke tessellated them into a wedge covering half the pane. The floor
    // belongs here as well as at the caller, because a degenerate polyline is a
    // hazard to whoever calls this next and not only to today's caller.
    let raw = a1 - a0;
    let sweep = raw.abs().max(0.004);
    let a1 = a0 + if raw < 0.0 { -sweep } else { sweep };
    // Capped, because `a0`/`a1` derive from file coordinates. A feature ending
    // at u64::MAX produced a sweep large enough that `steps` reached usize::MAX
    // and the `collect` below panicked with "capacity overflow", killing the
    // app on open. A full turn needs 240 points, so 720 is three times more
    // than any valid sweep can ask for and the cap never fires on a real file.
    let steps = (((sweep / std::f32::consts::TAU) * 240.0).ceil().max(2.0) as usize).min(720);
    (0..=steps)
        .map(|i| polar(center, radius, a0 + (a1 - a0) * (i as f32 / steps as f32)))
        .collect()
}

/// How far an arrowhead's barb may sit either side of the band's own radius.
///
/// **Half the lane pitch, never more, and that bound is the whole point.** The
/// barbs used to be at `radius ± w * 0.8` with no bound at all, and `w` carries the
/// emphasis bump, so an emphasised head reached `0.8 * 12 = 9.60` pt out of its
/// band while the next lane's quiet band begins `LANE_STEP - BAND_W * 0.5 = 8.50`
/// pt out — measured in the painter with two stacked features, an inner barb at
/// r=376.60 against an outer band's inner edge at r=375.50: **1.10 pt of one
/// feature's arrowhead drawn over another feature's rectangle**. That is the user's
/// sentence — "arrows overlap with the feature rectangles" — surviving the pass
/// that was supposed to close it, one lane over. Quiet it was clear by 1.30 pt,
/// which is exactly why it only appeared when the pointer was on the feature.
///
/// Half the pitch is chosen over "clear of a quiet neighbour" because the two
/// neighbouring bands can BOTH be emphasised — `selected` and `hot` are separate
/// fields and may name adjacent lanes — and an emphasised band reaches
/// `(BAND_W + 3) * 0.5 = 6.0` pt, so a 7.2 pt barb collides with it too. Giving
/// each lane its own half of `LANE_STEP` makes the clearance a property of the
/// geometry rather than of which feature the pointer happens to be over. It is the
/// same argument `ring::inside_of` makes one radius in, where the ruler is kept off
/// the bands at their emphasised width so that "hover must not be what decides
/// whether the ruler is legible".
///
/// The cost is honest: an emphasised head keeps only 0.5 pt of shoulder past its
/// own 6.0 pt shaft, so emphasis is carried by the band's width and by the head's
/// LENGTH — `head_angle` grows with `w`, 14.4 pt of arc to 19.2 — rather than by a
/// wider barb. `pl_draw::arc_segs` bounds its own barb the same way, absolutely
/// (`((ro - ri) * 0.35).min(2.5)`) and not as a fraction of the band, and it has no
/// lanes to collide with at all.
fn barb_half(w: f32) -> f32 {
    (w * 0.8).min(LANE_STEP * 0.5)
}

/// The arrowhead alone: a tip on the band's own radius and two barbs at
/// `base_angle`.
///
/// **No angle arithmetic, and no radial arithmetic either.** It used to compute its
/// own head length from `w`, `radius` and the part's sweep while the caller drew the
/// body from the part's full extent, so the body's end and the head's base were two
/// expressions in two functions and nothing made them the same number — the body
/// ran the whole way and the head sat on top of it. The caller now owns the one
/// number (`head_angle`) and hands in the angle it decided, which is what makes
/// "the body stops where the head begins" true by construction rather than by
/// coincidence. Recomputing it here is how the two drift apart again: one guard
/// differing (`radius.max(1.0)` in one place and not the other, `w` before or after
/// the emphasis bump) turns the seam into a gap or an overlap that depends on the
/// lane.
///
/// `barb` arrived here for the same reason one step later. It was `w * 0.8` inline,
/// which no test could reach and which therefore could not be asserted against
/// `LANE_STEP` — see [`barb_half`] for the 1.10 pt of a neighbouring feature's band
/// that bought.
fn draw_arrowhead(
    p: &egui::Painter,
    center: Pos2,
    radius: f32,
    tip_angle: f32,
    base_angle: f32,
    barb: f32,
    color: Color32,
) {
    let tip = polar(center, radius, tip_angle);
    let a = polar(center, radius + barb, base_angle);
    let b = polar(center, radius - barb, base_angle);
    p.add(Shape::convex_polygon(vec![tip, a, b], color, Stroke::NONE));
}

// ---------------------------------------------------------------------------
// linear
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn draw_linear(
    p: &egui::Painter,
    rect: Rect,
    span: u64,
    bands: &[Band],
    digest: &[Digest],
    mol: &Molecule,
    pal: &Palette,
    selected: Option<usize>,
    hot: Option<usize>,
    pointer: Option<Pos2>,
    out: &mut MapResponse,
) {
    let pad = 60.0;
    let x0 = rect.left() + pad;
    let x1 = rect.right() - pad;
    let y = rect.center().y;
    let width = (x1 - x0).max(1.0);
    let x_of = |pos: u64| x0 + (pos.saturating_sub(1)) as f32 / span as f32 * width;

    p.line_segment(
        [Pos2::new(x0, y), Pos2::new(x1, y)],
        Stroke::new(1.5, pal.line),
    );

    for i in 0..=10 {
        let pos = tick_pos(span, i);
        let x = x_of(pos);
        p.line_segment(
            [Pos2::new(x, y - 5.0), Pos2::new(x, y + 5.0)],
            Stroke::new(1.0, pal.line),
        );
        if i % 2 == 0 {
            p.text(
                Pos2::new(x, y + 18.0),
                Align2::CENTER_CENTER,
                crate::doc::fmt_int(pos),
                FontId::monospace(RULER_PT),
                pal.muted,
            );
        }
    }

    let lane_step = 15.0;
    let h = 11.0;
    for b in bands {
        let by = if b.reverse {
            y + 24.0 + b.lane as f32 * lane_step
        } else {
            y - 24.0 - b.lane as f32 * lane_step
        };
        let bx0 = x_of(b.start);
        let bx1 = x_of(b.end).max(bx0 + 2.0);
        let emphasised = selected == Some(b.index) || hot == Some(b.index);
        let hh = if emphasised { h * 0.65 } else { h * 0.5 };
        // Half the width, then 7 pt — the same rule the circular track keeps, and
        // for the same reason one step down in severity. There is no overlap defect
        // here (the pentagon subtracts the point from the body by construction), but
        // `.min(bx1 - bx0)` alone lets a 6 pt feature become 100% arrowhead with two
        // duplicated vertices, which is a degenerate polygon handed to
        // `convex_polygon` — the shape that once tessellated a wedge across the
        // circular pane. One rule for both tracks is also one thing to remember.
        let head = ((bx1 - bx0) * 0.5).min(7.0);

        // A pentagon rather than a rectangle: the point carries the strand. An
        // unoriented feature gets the rectangle, because a point is a directional
        // claim and `Strand::Unoriented`/`Both` is the file declining to make it —
        // the same rule as the circular track and as `pl_draw::scene`.
        let pts = if b.head.is_none() {
            vec![
                Pos2::new(bx0, by - hh),
                Pos2::new(bx1, by - hh),
                Pos2::new(bx1, by + hh),
                Pos2::new(bx0, by + hh),
            ]
        } else if b.reverse {
            vec![
                Pos2::new(bx1, by - hh),
                Pos2::new(bx0 + head, by - hh),
                Pos2::new(bx0, by),
                Pos2::new(bx0 + head, by + hh),
                Pos2::new(bx1, by + hh),
            ]
        } else {
            vec![
                Pos2::new(bx0, by - hh),
                Pos2::new(bx1 - head, by - hh),
                Pos2::new(bx1, by),
                Pos2::new(bx1 - head, by + hh),
                Pos2::new(bx0, by + hh),
            ]
        };
        p.add(Shape::convex_polygon(pts, b.color, Stroke::NONE));

        let hit = Rect::from_min_max(Pos2::new(bx0, by - hh - 2.0), Pos2::new(bx1, by + hh + 2.0));
        if pointer.is_some_and(|ptr| hit.contains(ptr)) {
            out.hovered = Some(b.index);
        }

        if bx1 - bx0 > 34.0 {
            p.text(
                Pos2::new((bx0 + bx1) * 0.5, by),
                Align2::CENTER_CENTER,
                &b.name,
                FontId::proportional(10.0),
                theme::on_color(b.color),
            );
        }
    }

    let uniq: Vec<(&str, u64)> = digest
        .iter()
        .filter(|d| d.is_unique_cutter())
        .map(|d| (d.enzyme.name, d.positions[0]))
        .collect();
    let anchors: Vec<f32> = uniq.iter().map(|(_, pos)| x_of(*pos)).collect();
    let placed = label_slots(&anchors, 58.0, rect.left() + 30.0, rect.right() - 30.0);
    let ly = rect.top() + 22.0;
    for (i, (name, pos)) in uniq.iter().enumerate() {
        let x = x_of(*pos);
        p.line_segment(
            [Pos2::new(x, y - 12.0), Pos2::new(x, ly + 8.0)],
            Stroke::new(0.8, pal.faint),
        );
        p.line_segment(
            [Pos2::new(x, ly + 8.0), Pos2::new(placed[i], ly + 4.0)],
            Stroke::new(0.8, pal.faint),
        );
        p.text(
            Pos2::new(placed[i], ly),
            Align2::CENTER_CENTER,
            format!("{name} {}", crate::doc::fmt_int(*pos)),
            FontId::monospace(9.5),
            pal.ink2,
        );
    }

    let _ = mol;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_overlapping_features_share_one_lane() {
        assert_eq!(lanes(&[(1, 10), (20, 30), (40, 50)]), vec![0, 0, 0]);
    }

    /// FAILS ONLY TO COMPILE at 0ebaa41 — `cutters_shown` did not exist, and
    /// saying so plainly matters more than the test does.
    ///
    /// The on-map line at 0ebaa41 was arithmetically the same defect as
    /// `pl_draw::scene`'s (a sum of per-label tallies, and `unique.len()` minus
    /// it) and could not be made to produce a wrong number, because
    /// `draw_circular` feeds it `filter(is_unique_cutter)` — one pair per enzyme,
    /// so mentions, labels and enzymes coincide. Screenshotted on the user's own
    /// pKoV before the change: "22 of 40 cutters labelled · 12 dual, 6 multi not
    /// drawn", and 22 + 0 + 12 + 6 = 40 closes. It was right by INPUT.
    ///
    /// So there is no failing frame test to offer and none is claimed. What this
    /// pins is the arithmetic itself, on the input the filter is currently keeping
    /// away from it: the day someone adds a "show dual cutters" toggle — which
    /// `pl export --sites dual` already does one layer over, and got wrong — the
    /// contract is written down and asserted rather than rediscovered.
    #[test]
    fn the_map_counts_enzymes_not_the_times_they_are_mentioned() {
        let pairs = |v: &[(&str, u64)]| -> Vec<(String, u64)> {
            v.iter().map(|(n, p)| (n.to_string(), *p)).collect()
        };
        let one = |n: &str| vec![n.to_string()];

        // One enzyme, five ticks, five labels. A tally sums to 5.
        let dra = pairs(&[
            ("DraI", 1_182),
            ("DraI", 1_226),
            ("DraI", 1_750),
            ("DraI", 2_357),
            ("DraI", 2_969),
        ]);
        let five: Vec<Vec<String>> = (0..5).map(|_| one("DraI")).collect();
        let refs: Vec<&[String]> = five.iter().map(Vec::as_slice).collect();
        assert_eq!(cutters_shown(&dra, &refs), (1, 0));

        // A fold of two DIFFERENT names is two enzymes in one label — the case a
        // distinct count must NOT collapse, and the reason `pkov_cutter_names`
        // could never catch the mention bug.
        let xs = pairs(&[("XmaI", 6_917), ("SmaI", 6_919)]);
        let folded = vec!["XmaI".to_string(), "SmaI".to_string()];
        assert_eq!(cutters_shown(&xs, &[folded.as_slice()]), (2, 0));

        // Dropped at some ticks is NAMED, not hidden: subtraction of counts gives
        // 5 - 2 = 3 "hidden" for an enzyme that is plainly on the map.
        let kept: Vec<Vec<String>> = (0..2).map(|_| one("DraI")).collect();
        let refs: Vec<&[String]> = kept.iter().map(Vec::as_slice).collect();
        assert_eq!(cutters_shown(&dra, &refs), (1, 0));

        // And an enzyme on no label at all is the one thing `hidden` means.
        let two = pairs(&[("DraI", 1_182), ("EcoRI", 7_530)]);
        assert_eq!(cutters_shown(&two, &[one("EcoRI").as_slice()]), (1, 1));
        assert_eq!(cutters_shown(&two, &[]), (0, 2));
    }

    /// The screen reserves radius for a site label in a ROW, exactly as the figure
    /// does.
    ///
    /// PROVEN TO FAIL before `widest_site_label` dropped its `Side::Left |
    /// Side::Right` filter: the on-screen text was `Ec...`, at every window size.
    /// This is the same defect as `pl_draw::scene`'s and the reason fixing one
    /// without the other would have been a divergence rather than a fix — the map on
    /// screen and the figure in the paper would have disagreed about whether this
    /// plasmid has an EcoRI site anyone can name.
    ///
    /// `GATTACA` holds no palindromic 6-mer, so the filler contributes no site of its
    /// own and the molecule really has one cutter; the site sits at 49.7% of it, which
    /// is the six-o'clock row.
    #[test]
    fn a_site_label_in_a_row_is_drawn_whole_on_screen_too() {
        let filler = "GATTACA".repeat(17);
        let seq = format!("{filler}GAATTC{filler}");
        let mol = Molecule {
            seq: seq.into_bytes(),
            topology: Topology::Circular,
            ..Default::default()
        };
        let digest: Vec<Digest> = pl_enzymes::digest_all(&mol)
            .into_iter()
            .filter(|d| d.count() > 0)
            .collect();
        let unique: Vec<&Digest> = digest.iter().filter(|d| d.is_unique_cutter()).collect();
        assert_eq!(
            unique.len(),
            1,
            "the fixture must have exactly one unique cutter: {:?}",
            unique.iter().map(|d| d.enzyme.name).collect::<Vec<_>>()
        );
        let cut = unique[0].positions[0];
        let frac = cut as f64 / mol.span() as f64;
        assert!(
            (0.42..0.58).contains(&frac),
            "the site is at {frac:.3} of the molecule, which is not the six-o'clock row"
        );

        for (w, h) in [(706.0f32, 756.0f32), (880.0, 560.0), (1296.0, 879.0)] {
            let ctx = crate::test_ctx();
            let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h));
            let input = egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            };
            let mut texts: Vec<String> = Vec::new();
            for _ in 0..2 {
                let frame = ctx.run_ui(input.clone(), |ui| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ui, |ui| {
                            show(ui, &mol, "fixture", &digest, None, None);
                        });
                });
                fn walk(s: &Shape, acc: &mut Vec<String>) {
                    match s {
                        Shape::Vec(v) => v.iter().for_each(|s| walk(s, acc)),
                        Shape::Text(t) => acc.push(t.galley.text().to_string()),
                        _ => {}
                    }
                }
                texts = Vec::new();
                for cs in &frame.shapes {
                    walk(&cs.shape, &mut texts);
                }
            }
            let want = format!("{}  {}", unique[0].enzyme.name, crate::doc::fmt_int(cut));
            assert!(
                texts.contains(&want),
                "{w}x{h}: the map draws no {want:?}; its enzyme text is {:?}",
                texts
                    .iter()
                    .filter(|t| t.contains(unique[0].enzyme.name) || t.contains('…'))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn overlapping_features_are_stacked() {
        assert_eq!(lanes(&[(1, 100), (50, 150), (60, 70)]), vec![0, 1, 2]);
    }

    #[test]
    fn a_lane_is_reused_once_it_is_free() {
        // Third interval starts after the first ends, so it drops back to lane 0.
        let l = lanes(&[(1, 10), (5, 20), (30, 40)]);
        assert_eq!(l[0], 0);
        assert_eq!(l[1], 1);
        assert_eq!(l[2], 0);
    }

    #[test]
    fn adjacent_features_do_not_share_a_lane() {
        // Touching intervals would render as one continuous band.
        assert_eq!(lanes(&[(1, 10), (11, 20)]), vec![0, 1]);
    }

    #[test]
    fn lane_assignment_is_returned_in_input_order() {
        // Deliberately unsorted input: output must line up with the features.
        let l = lanes(&[(100, 200), (1, 50), (150, 250)]);
        assert_eq!(l.len(), 3);
        assert_eq!(l[1], 0, "the earliest interval takes the first lane");
        assert_ne!(l[0], l[2], "these two overlap and must be separated");
    }

    #[test]
    fn labels_that_fit_are_left_where_they_want_to_be() {
        let out = label_slots(&[10.0, 50.0, 90.0], 12.0, 0.0, 100.0);
        assert_eq!(out, vec![10.0, 50.0, 90.0]);
    }

    #[test]
    fn crowded_labels_are_pushed_apart_by_at_least_the_gap() {
        let out = label_slots(&[50.0, 51.0, 52.0], 12.0, 0.0, 200.0);
        let mut sorted = out.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for w in sorted.windows(2) {
            assert!(w[1] - w[0] >= 12.0 - 1e-3, "{sorted:?}");
        }
    }

    #[test]
    fn labels_stay_inside_the_allowed_range() {
        let anchors: Vec<f32> = (0..12).map(|i| 95.0 + i as f32).collect();
        let out = label_slots(&anchors, 12.0, 0.0, 100.0);
        for v in &out {
            assert!(*v <= 100.0 + 1e-3, "{out:?}");
        }
    }

    #[test]
    fn label_output_keeps_input_order() {
        let out = label_slots(&[90.0, 10.0, 50.0], 12.0, 0.0, 100.0);
        assert!(out[1] < out[2] && out[2] < out[0], "{out:?}");
    }

    #[test]
    fn twelve_oclock_is_position_one_and_it_runs_clockwise() {
        let span = 1000;
        let top = angle_of(1, span);
        assert!((top + std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        // A quarter of the way round should be at three o'clock (angle 0).
        let quarter = angle_of(251, span);
        assert!(quarter.abs() < 0.01, "{quarter}");
    }

    #[test]
    fn an_arc_is_sampled_finely_enough_to_look_curved() {
        let pts = arc_points(Pos2::ZERO, 100.0, 0.0, std::f32::consts::TAU);
        assert!(pts.len() >= 240, "{} points for a full circle", pts.len());
        // ...and a hairline feature still yields a drawable segment.
        assert!(arc_points(Pos2::ZERO, 100.0, 0.0, 0.0001).len() >= 2);
    }

    #[test]
    fn a_hostile_coordinate_does_not_take_the_window_down() {
        // A 185-byte `.dna` declaring `range="1-18446744073709551615"` used to
        // kill the app on open: `lanes` panicked on `end + 1` in debug and
        // wrapped to 0 in release, and `arc_points` then asked for a vector of
        // usize::MAX points and panicked with "capacity overflow".
        let spans = [(1u64, u64::MAX), (2, 3), (u64::MAX, u64::MAX), (0, 0)];
        let out = lanes(&spans);
        assert_eq!(out.len(), spans.len());

        // A sweep no valid file can produce must still terminate cheaply.
        for sweep in [1e9f32, f32::MAX, std::f32::consts::TAU * 1e6] {
            let pts = arc_points(Pos2::ZERO, 100.0, 0.0, sweep);
            assert!(pts.len() <= 721, "{} points", pts.len());
            assert!(pts.len() >= 2);
        }
        // Non-finite input must not produce a NaN-sized allocation either.
        assert!(arc_points(Pos2::ZERO, 100.0, 0.0, f32::INFINITY).len() <= 721);
    }

    #[test]
    fn a_ruler_tick_on_a_hostile_span_is_computed_rather_than_wrapped() {
        // The same 185-byte `.dna`, at the two sites the earlier hardening
        // missed. An annotation-only record has no bases, so `Molecule::span`
        // is 0, `annotation_span` falls through to the largest feature end, and
        // `validate` cannot even warn about it — its past-the-end check is
        // gated on a non-empty sequence.
        //
        // `span * i` then overflowed from i = 2 on: a panic in debug that took
        // the window down on open, and in release four labelled ticks all
        // reading 1,844,674,407,370,955,16x instead of 3.69, 7.38, 11.07 and
        // 14.76 x 10^18, collapsed by `angle_of` onto a single spoke.
        let span = u64::MAX;
        assert_eq!(tick_pos(span, 0), 1);
        assert_eq!(tick_pos(span, 2), 3_689_348_814_741_910_324);
        assert_eq!(tick_pos(span, 4), 7_378_697_629_483_820_647);
        assert_eq!(tick_pos(span, 6), 11_068_046_444_225_730_970);
        assert_eq!(tick_pos(span, 8), 14_757_395_258_967_641_293);
        // `draw_linear`'s loop is inclusive, so the last tick is the one where
        // the trailing +1 would carry past the end of the type.
        assert_eq!(tick_pos(span, 10), u64::MAX);
        // Distinct and increasing is the property that keeps it a ruler.
        for i in 1..=10 {
            assert!(
                tick_pos(span, i) > tick_pos(span, i - 1),
                "tick {i} did not advance"
            );
        }
    }

    #[test]
    fn ruler_ticks_on_an_ordinary_plasmid_are_where_they_have_always_been() {
        // The control. pUC19 at 2,686 bp: hardening the arithmetic must not
        // move a single tick on a file anyone actually opens.
        let want = [1, 269, 538, 806, 1075, 1344, 1612, 1881, 2149, 2418, 2687];
        for (i, w) in want.iter().enumerate() {
            assert_eq!(tick_pos(2686, i as u64), *w, "tick {i}");
        }
    }

    /// A 2,686 bp circle carrying one feature, on the given strand.
    fn circle(segs: &[(u64, u64)], reverse: bool) -> Molecule {
        let mut mol = Molecule {
            seq: vec![b'A'; 2_686],
            topology: Topology::Circular,
            ..Default::default()
        };
        let mut f = pl_core::Feature::new("bla", "CDS");
        f.strand = if reverse {
            pl_core::Strand::Reverse
        } else {
            pl_core::Strand::Forward
        };
        for &(s, e) in segs {
            f.segments.push(pl_core::Segment::new(s, e));
        }
        mol.features.push(f);
        mol
    }

    /// Degrees of arc the parts of a band actually paint.
    fn drawn_degrees(b: &Band, span: u64) -> f32 {
        b.segs
            .iter()
            .map(|&(s, e)| (angle_of(e, span) - angle_of(s, span)).abs())
            .sum::<f32>()
            .to_degrees()
    }

    /// PROVEN TO FAIL at dfd6ac9: `bands` kept `(2587, 87)` as one pair — the
    /// assertion below reports `left: [(2587, 87)]` there — and `draw_circular`
    /// interpolated straight from `angle_of(2587)` down to `angle_of(87)`,
    /// painting 335.07 degrees, a 2,499 bp band in bla's colour under bla's
    /// name, for a 187 bp feature that covers 25.1.
    #[test]
    fn an_origin_crossing_feature_is_drawn_as_the_arc_it_names() {
        // What `Edit > Set origin at selected feature` produces: `Molecule::rotate`
        // remaps each endpoint independently, and `validate()` deliberately
        // accepts `end < start` on a circle as a wrap, so nothing warns.
        let mol = circle(&[(2_587, 87)], false);
        assert!(mol.validate().is_empty(), "the premise: a legal wrap");
        let span = mol.annotation_span().max(1);
        assert_eq!(span, 2_686);

        let b = &bands(&mol)[0];
        assert_eq!(
            b.segs,
            vec![(1, 87), (2_587, 2_686)],
            "the two arcs `pl_draw::ranges` gives the exporter"
        );
        let mut exported = pl_draw::ranges(2_587, 87, span, true);
        exported.sort_unstable();
        assert_eq!(
            b.segs, exported,
            "the map and `pl export` describe the same molecule"
        );
        let deg = drawn_degrees(b, span);
        assert!(
            (20.0..30.0).contains(&deg),
            "a 187 bp feature covers 25.1 degrees; this drew {deg}"
        );

        // The control: an ordinary feature is untouched.
        let plain = circle(&[(2_400, 2_586)], false);
        let pb = &bands(&plain)[0];
        assert_eq!(pb.segs, vec![(2_400, 2_586)]);
        let pdeg = drawn_degrees(pb, span);
        assert!((20.0..30.0).contains(&pdeg), "{pdeg}");
    }

    /// The arrowhead and the join connectors, which the split also decides.
    ///
    /// Not runnable against dfd6ac9 — `Band` had neither field there — so this
    /// covers the two consequences rather than reproducing the original defect;
    /// `an_origin_crossing_feature_is_drawn_as_the_arc_it_names` is the one that
    /// fails at HEAD.
    #[test]
    fn a_wrapped_feature_points_forwards_and_a_contiguous_join_has_no_connector() {
        let span = 2_686;

        // Forward 2587..87: the feature ends at base 87, so the head sits there
        // and points clockwise. Read off the sorted parts it sat at 2,686 and
        // pointed counter-clockwise — a forward feature drawn as a reverse one.
        //
        // `head` is now an index into the SORTED parts, which is the point of the
        // change: `segs` for this feature is `[(1, 87), (2_587, 2_686)]`, so the
        // head's part is `segs[0]` and `segs.last()` would be the wrong one.
        let fwd = &bands(&circle(&[(2_587, 87)], false))[0];
        let hi = fwd.head.expect("a terminal part");
        assert_eq!(fwd.segs, vec![(1, 87), (2_587, 2_686)]);
        assert_eq!(
            hi, 0,
            "the head is on the post-origin part, not on segs.last()"
        );
        let (s, e) = fwd.segs[hi];
        assert_eq!(e, 87, "a forward feature's head is at the base it ends on");
        assert!(
            angle_end(e, span) > angle_of(s, span),
            "the head must point the way the feature reads"
        );

        // Reverse 2587..87 reads the other way and ends at base 2,587 — the
        // PRE-origin part, which sorts last.
        let rev = &bands(&circle(&[(2_587, 87)], true))[0];
        let ri = rev.head.expect("a terminal part");
        assert_eq!(ri, 1, "a reverse feature's head is on the pre-origin part");
        assert_eq!(rev.segs[ri].0, 2_587);

        // An unoriented feature makes no directional claim, so it gets no head:
        // `pl_draw::scene` sets `arrow_on = -1` for it and the screen painted a
        // forward arrowhead anyway, on three of pKoV's nine features.
        let mut un = circle(&[(2_400, 2_586)], false);
        un.features[0].strand = pl_core::Strand::Unoriented;
        assert!(
            bands(&un)[0].head.is_none(),
            "an unoriented feature has no direction to draw"
        );
        let mut both = circle(&[(2_400, 2_586)], false);
        both.features[0].strand = pl_core::Strand::Both;
        assert!(bands(&both)[0].head.is_none());

        // `join(2600..2686, 1..100)` is contiguous across the origin: no gap,
        // so no connector. Sorted, its parts look exactly like the next case.
        let contiguous = &bands(&circle(&[(2_600, 2_686), (1, 100)], false))[0];
        assert_eq!(contiguous.segs, vec![(1, 100), (2_600, 2_686)]);
        assert!(
            contiguous.joins.is_empty(),
            "a 335-degree hairline across the rest of the plasmid: {:?}",
            contiguous.joins
        );

        // `join(1..100, 2600..2686)` is an intron-split feature with a real
        // 2,499 bp gap, and the connector belongs there.
        let split = &bands(&circle(&[(1, 100), (2_600, 2_686)], false))[0];
        assert_eq!(split.segs, vec![(1, 100), (2_600, 2_686)]);
        assert_eq!(split.joins, vec![(100, 2_600)]);
    }

    /// A segment naming no base of the molecule must not be drawn at all.
    ///
    /// `pl_draw::ranges` calls collapsing it onto base 1 "fabrication, which is
    /// worse than loss"; the map used to do exactly that by way of `angle_of`.
    #[test]
    fn a_segment_wholly_outside_the_molecule_paints_nothing() {
        let mut mol = circle(&[(1, 10)], false);
        mol.features[0].segments = vec![pl_core::Segment::new(9_000, 9_100)];
        let b = &bands(&mol)[0];
        assert!(b.segs.is_empty(), "{:?}", b.segs);
        assert!(b.head.is_none(), "and nothing to point at");
    }

    // -----------------------------------------------------------------------
    // the arrowhead, on geometry
    // -----------------------------------------------------------------------
    //
    // Never on a screenshot, and that is not fastidiousness. The head and the body
    // are the SAME COLOUR, so a full-length body under an opaque triangle and a
    // correctly notched body are pixel-identical over the whole interior — the only
    // differences are a sub-pixel seam and the barbs. A before/after pair of
    // screenshots of this fix looks near-identical and would be read as proof
    // either way. Quantifying the defect from the picture took a radial-thickness
    // profile: the pure-#ffff00 pixels of `pSC101 ori` binned by angle came back a
    // flat 7.9 pt from the head's base to the tip, where a correct render tapers
    // 14.4 -> 0. The house rule that a check which cannot fail proves nothing
    // applies to the screenshot; the picture is still worth looking at for the
    // arrow SHAPE and for the three heads that should have vanished.

    /// One frame of the circular map, and the shapes it painted.
    fn paint(mol: &Molecule, w: f32, h: f32) -> (Vec<Shape>, Rect) {
        let ctx = crate::test_ctx();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h));
        let input = egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        };
        let mut out = Vec::new();
        // Two passes: egui's first frame has no galley cache and the map measures
        // its own labels to decide the radius.
        for _ in 0..2 {
            let frame = ctx.run_ui(input.clone(), |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        show(ui, mol, "fixture", &[], None, None);
                    });
            });
            fn walk(s: &Shape, acc: &mut Vec<Shape>) {
                match s {
                    Shape::Vec(v) => v.iter().for_each(|s| walk(s, acc)),
                    other => acc.push(other.clone()),
                }
            }
            out = Vec::new();
            for cs in &frame.shapes {
                walk(&cs.shape, &mut out);
            }
        }
        (out, rect)
    }

    /// Every arrowhead, as its three vertices.
    ///
    /// `Shape::convex_polygon` gives a closed `Path` with a filled interior and no
    /// stroke, which nothing else on the circular map produces: bands are open
    /// polylines with a 9 pt stroke and no fill, and the ruler and leaders are
    /// hairlines.
    fn arrowheads(shapes: &[Shape]) -> Vec<[Pos2; 3]> {
        shapes
            .iter()
            .filter_map(|s| match s {
                Shape::Path(p)
                    if p.closed
                        && p.stroke.width == 0.0
                        && p.points.len() == 3
                        && p.fill != Color32::TRANSPARENT =>
                {
                    Some([p.points[0], p.points[1], p.points[2]])
                }
                _ => None,
            })
            .collect()
    }

    /// `d`, folded by whole turns into `(-PI, PI]`.
    ///
    /// `atan2` returns a FOLDED angle and `angle_of`/`angle_end` return an
    /// UNFOLDED one in `[-PI/2, 3PI/2)`. Subtracting one from the other is
    /// meaningless, and it is meaningless silently: the difference comes back a
    /// plausible number that is a whole turn wrong. Every angle read out of a
    /// painted shape below goes through this or through [`band_arc`]'s
    /// accumulation before it meets a number the painter computed.
    fn wrap(d: f32) -> f32 {
        let mut d = d % std::f32::consts::TAU;
        if d > std::f32::consts::PI {
            d -= std::f32::consts::TAU;
        }
        if d <= -std::f32::consts::PI {
            d += std::f32::consts::TAU;
        }
        d
    }

    /// `a`, shifted by whole turns to lie within half a turn of `near`.
    fn near(a: f32, near: f32) -> f32 {
        near + wrap(a - near)
    }

    /// A band polyline's `(mean radius, first angle, last angle)` about `centre`,
    /// with the last angle CONTINUOUS from the first rather than folded.
    ///
    /// The fold is fatal for exactly the two cases this file has to get right. A
    /// feature spanning the whole molecule ends one head-length *before* its own
    /// start once `atan2` has folded both, so `last - first` came back negative
    /// and "the head laps the band's start" fired on a render that does not lap
    /// it. And the pre-origin part of a wrapped feature starts at a folded angle
    /// two turns away from the `angle_of` value naming the same base, so matching
    /// arcs to parts by nearest start angle picked the same arc twice.
    ///
    /// `arc_points` samples at worst about 3 degrees per step, far inside half a
    /// turn, so accumulating the folded step-to-step deltas recovers the true
    /// sweep exactly.
    fn band_arc(pts: &[Pos2], c: Pos2) -> (f32, f32, f32) {
        let r = pts.iter().map(|p| (*p - c).length()).sum::<f32>() / pts.len() as f32;
        let ang = |p: &Pos2| (p.y - c.y).atan2(p.x - c.x);
        let a0 = ang(pts.first().unwrap());
        let (mut acc, mut prev) = (a0, a0);
        for p in &pts[1..] {
            let cur = ang(p);
            acc += wrap(cur - prev);
            prev = cur;
        }
        (r, a0, acc)
    }

    /// The sweep `draw_circular` draws a part over, normalised the way it
    /// normalises it: a part ending on the molecule's last base closes at the
    /// origin's own angle, which is not greater than where it started.
    fn part_sweep(s: u64, e: u64, span: u64) -> f32 {
        let a0 = angle_of(s, span);
        let mut a1 = angle_end(e, span);
        if a1 <= a0 {
            a1 += std::f32::consts::TAU;
        }
        a1 - a0
    }

    /// The band with the longest arc, which is not a join hairline.
    fn widest_band(shapes: &[Shape], c: Pos2) -> (Vec<Pos2>, f32) {
        let mut best: Option<(Vec<Pos2>, f32)> = None;
        for s in shapes {
            if let Shape::Path(p) = s {
                if p.stroke.width >= 6.0 && p.points.len() >= 2 {
                    let (_, a0, a1) = band_arc(&p.points, c);
                    let span = (a1 - a0).abs();
                    if best.as_ref().is_none_or(|(_, b)| span > *b) {
                        best = Some((p.points.clone(), span));
                    }
                }
            }
        }
        best.expect("a feature band was painted")
    }

    /// A single-part forward feature: its body arc and its head, in polar terms.
    ///
    /// Returns `(radius, body_start, body_end, head_base, head_tip)`, monotonic
    /// and continuous — `body_start` is folded, everything after it is carried
    /// forward from there, so `body_end - body_start` is the arc actually drawn
    /// even when it is a whole turn. The head's base is placed nearest the body's
    /// end (they are a seam apart by construction) and the tip a head-length on
    /// from the base, so a head that straddles twelve o'clock stays ordered.
    fn one_arrow(mol: &Molecule, w: f32, h: f32) -> (f32, f32, f32, f32, f32) {
        let (shapes, rect) = paint(mol, w, h);
        let c = rect.center();
        let heads = arrowheads(&shapes);
        assert_eq!(heads.len(), 1, "expected exactly one arrowhead");
        let (pts, _) = widest_band(&shapes, c);
        let (radius, a_s, a_e) = band_arc(&pts, c);
        let ang = |p: &Pos2| (p.y - c.y).atan2(p.x - c.x);
        // The tip is the vertex on the band's own radius; the barbs are the two at
        // `radius ± w*0.8` and share one angle.
        let h3 = heads[0];
        let tip_i = (0..3)
            .min_by(|&i, &j| {
                let d = |k: usize| ((h3[k] - c).length() - radius).abs();
                d(i).partial_cmp(&d(j)).unwrap()
            })
            .unwrap();
        let barb = (0..3).find(|&i| i != tip_i).unwrap();
        let a_base = near(ang(&h3[barb]), a_e);
        let a_tip = a_base + wrap(ang(&h3[tip_i]) - ang(&h3[barb]));
        (radius, a_s, a_e, a_base, a_tip)
    }

    fn forward(segs: &[(u64, u64)]) -> Molecule {
        circle(segs, false)
    }

    /// A4 — the clamp is half the sweep, and it binds only where it must.
    ///
    /// PROVEN TO FAIL at 0ebaa41 on the first case: the clamp there was
    /// `sweep * 0.9`, so a short feature came back 0.009 and not 0.005. It fails to
    /// COMPILE as well, because there was no such function to call — the number
    /// lived inside a painting routine and could not be asserted at all, which is
    /// the finding and not an inconvenience.
    #[test]
    fn the_arrowhead_takes_at_most_half_the_arc_it_sits_on() {
        // Short: the head takes as much as it may and the shaft keeps the rest.
        assert_eq!(head_angle(0.01, 89.0, 9.0), 0.005);
        // Long: the clamp must NOT bind — the full 1.6 * w / r, not the midpoint.
        assert!((head_angle(1.0, 90.0, 9.0) - 14.4 / 90.0).abs() < 1e-6);
        // Emphasis reaches it, so the body is shortened by the larger amount.
        assert!((head_angle(1.0, 90.0, 12.0) - 19.2 / 90.0).abs() < 1e-6);
        // No room is no head, never a floored one: three collinear vertices handed
        // to `convex_polygon` is what tessellated a wedge across half the pane.
        assert_eq!(head_angle(0.0, 200.0, 9.0), 0.0);
        assert_eq!(head_angle(-1.0, 200.0, 9.0), 0.0);
        assert_eq!(head_angle(f32::NAN, 200.0, 9.0), 0.0);
        // And it never exceeds half, at any radius or width.
        for sweep in [0.001f32, 0.004, 0.01, 0.05, 0.2, 1.0, std::f32::consts::TAU] {
            for r in [1.0f32, 40.0, 172.0, 203.0, 900.0] {
                for w in [9.0f32, 12.0] {
                    let head = head_angle(sweep, r, w);
                    assert!(
                        head <= sweep * 0.5 + 1e-7,
                        "sweep {sweep} r {r} w {w}: head {head} inverts the outline"
                    );
                }
            }
        }
    }

    /// A1/A2/A3 — the body stops where the head begins, the two extents sum to the
    /// feature's span, and they do not overlap.
    ///
    /// PROVEN TO FAIL at 0ebaa41: the body was drawn from the part's full extent and
    /// the head painted over its last `head` radians, so `body_end - head_base` was
    /// 0.0709 rad against a seam of 0.0025 — off by 28x — and body + head overshot
    /// the span by the whole head.
    #[test]
    fn the_body_stops_where_the_arrowhead_begins() {
        let span = 2_686u64;
        for (w, h) in [(706.0f32, 756.0f32), (880.0, 620.0), (1400.0, 950.0)] {
            let mol = forward(&[(400, 1_400)]);
            let (r, a_s, a_e, a_base, a_tip) = one_arrow(&mol, w, h);
            let seam = (SEAM_PT / r.max(1.0)).max(1e-4);
            let want = head_angle(angle_end(1_400, span) - angle_of(400, span), r, 9.0);

            // The head points at the base the feature ends on.
            let tip_want = angle_end(1_400, span);
            assert!(
                (a_tip - tip_want).abs() < 1e-3,
                "{w}x{h}: the tip is at {a_tip}, the feature ends at {tip_want}"
            );
            // A1: the body does not run past the head's base, and does not stop
            // short of it either.
            let over = a_e - a_base;
            assert!(
                over <= seam + 1e-6,
                "{w}x{h}: the body runs {over:.5} rad ({:.2} pt of arc at r={r:.1}) past the \
                 head's base; the seam allows {seam:.5}",
                over * r
            );
            // A LOWER bound as well as an upper one, so `SEAM_PT` is a constant a
            // test can fail on. With only the upper bound, setting `seam = 0.0` in
            // the painter turned NOTHING red across all 241 tests here — zero
            // overlap satisfies "at most SEAM_PT" perfectly — while `SEAM_PT`'s own
            // doc claims a concrete visual failure it would bring back: two
            // antialiased shapes abutting each contribute about half coverage, and
            // the result reads as a lighter hairline straight down the feature at
            // the head's base. The tolerance is 1e-4 rad, 0.03 pt of arc at these
            // radii, against a seam of 0.00163 — a 16x margin, so this fails on the
            // seam going away and not on float noise.
            assert!(
                over >= seam - 1e-4,
                "{w}x{h}: the body stops {:.3} pt of arc short of the head's base, where \
                 SEAM_PT asks it to run {SEAM_PT} pt UNDER it; an abutting edge is the \
                 antialiased hairline that constant exists to hide",
                -over * r
            );
            // A2: the two extents sum to the feature's span.
            let (body, head) = (a_e - a_s, a_tip - a_base);
            let feature = angle_end(1_400, span) - angle_of(400, span);
            assert!(
                (body + head - feature).abs() < seam + 1e-3,
                "{w}x{h}: body {body:.5} + head {head:.5} = {:.5}, the feature spans {feature:.5}",
                body + head
            );
            assert!(
                (head - want).abs() < 1e-3,
                "{w}x{h}: head {head:.5}, head_angle says {want:.5}"
            );
            // A3: as an interval test. The head's base must not precede the
            // feature's own start — that is the bow tie.
            assert!(
                a_base >= a_s - 1e-4,
                "{w}x{h}: the head's base is before the feature's start: a bow tie"
            );
            assert!(a_base <= a_tip, "{w}x{h}: the head points backwards");
        }
    }

    /// A5 — a feature shorter than a full arrowhead is half body, half head.
    ///
    /// PROVEN TO FAIL at 0ebaa41: the body was the full sweep and the head 90% of
    /// it, so a 103 bp feature was 90% double-painted. Above
    /// `MIN_FEATURE_DEGREES` (1.2 deg = 9 bases here) so it is an arc and not a
    /// mark, and below `2 * ARROW_LEN * w` of arc so the clamp binds.
    #[test]
    fn a_feature_shorter_than_its_arrowhead_becomes_a_short_arrow_not_a_bow_tie() {
        let span = 2_686u64;
        // 30 bases = 4.02 degrees, and at r ~ 200 the unclamped head wants 4.13.
        let mol = forward(&[(400, 429)]);
        let (r, a_s, a_e, a_base, a_tip) = one_arrow(&mol, 706.0, 756.0);
        let sweep = angle_end(429, span) - angle_of(400, span);
        assert!(
            head_angle(sweep, r, 9.0) >= sweep * 0.5 - 1e-6,
            "at r={r:.1} this feature is not short enough for the clamp to bind; \
             pick a shorter one"
        );
        let (body, head) = (a_e - a_s, a_tip - a_base);
        let seam = SEAM_PT / r.max(1.0);
        assert!(
            (body - head).abs() < 2.0 * seam + 1e-3,
            "the clamp splits the arc in half: body {body:.5} against head {head:.5}"
        );
        assert!(
            a_base >= a_s - 1e-4,
            "the head's base is before the start: a bow tie"
        );
    }

    /// A6 — an origin-crossing feature: one head, on the right arc, and only that
    /// arc shortened.
    ///
    /// PROVEN TO FAIL at 0ebaa41: one head on the correct part, pointing the right
    /// way — that was fixed once already — but NEITHER body shortened. The trap the
    /// fix had to avoid is that `segs` is sorted, so the head's part is `segs[0]`
    /// here and a `segs.last()` rule would shorten the wrong arc.
    #[test]
    fn an_origin_crossing_feature_gets_one_head_and_only_that_arc_is_shortened() {
        let span = 2_686u64;
        let mol = forward(&[(2_400, 200)]);
        let (shapes, rect) = paint(&mol, 706.0, 756.0);
        let c = rect.center();
        let heads = arrowheads(&shapes);
        assert_eq!(heads.len(), 1, "one feature, one arrowhead");

        let bands_seen: Vec<(f32, f32, f32)> = shapes
            .iter()
            .filter_map(|s| match s {
                Shape::Path(p) if p.stroke.width >= 6.0 && p.points.len() >= 2 => {
                    Some(band_arc(&p.points, c))
                }
                _ => None,
            })
            .collect();
        assert_eq!(bands_seen.len(), 2, "two parts, two arcs");
        let r = bands_seen[0].0;

        // `ranges` gives [(2400, 2686), (1, 200)]; sorted that is
        // [(1, 200), (2400, 2686)] and the head is on (1, 200) — segs[0].
        let head_start = angle_of(1, span);
        let head_end = angle_end(200, span);
        let head_span = part_sweep(1, 200, span);
        let plain_start = angle_of(2_400, span);
        let plain_span = part_sweep(2_400, 2_686, span);
        let head = head_angle(head_span, r, 9.0);
        assert!(head > 0.0);

        let tip = heads[0]
            .iter()
            .map(|p| (p.y - c.y).atan2(p.x - c.x))
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        assert!(
            wrap(tip - head_end).abs() < 1e-2,
            "the tip is at {tip}, base 200 is at {head_end}; reading the head off \
             segs.last() would put it at base 2,686"
        );

        // Match each painted arc to the part it belongs to by START ANGLE, folded —
        // the pre-origin part starts at 4.04 rad where `atan2` reports -2.24, and
        // comparing those two unfolded picked the SAME arc for both parts, which
        // then asserted the head's own (correctly shortened) arc against the plain
        // part's span and read as a rendering defect.
        let arc_at = |want: f32| -> (f32, f32) {
            let (_, a0, a1) = bands_seen
                .iter()
                .copied()
                .min_by(|a, b| {
                    let d = |x: &(f32, f32, f32)| wrap(x.1 - want).abs();
                    d(a).partial_cmp(&d(b)).unwrap()
                })
                .unwrap();
            (a0, a1)
        };
        let seam = SEAM_PT / r.max(1.0);
        let (hs, he) = arc_at(head_start);
        let (ps, pe) = arc_at(plain_start);
        assert!(
            wrap(hs - ps).abs() > 0.1,
            "both parts matched the same arc; the match is not discriminating"
        );
        assert!(
            ((he - hs) - (head_span - head)).abs() < seam + 1e-3,
            "the head's own arc was not shortened: drew {:.5}, wanted {:.5}",
            he - hs,
            head_span - head
        );
        assert!(
            ((pe - ps) - plain_span).abs() < 1e-2,
            "the PRE-origin arc must not be shortened: drew {:.5}, its part spans {plain_span:.5}",
            pe - ps
        );
    }

    /// A7 — a feature spanning the whole molecule must not lap its own start.
    ///
    /// PROVEN TO FAIL at 0ebaa41: the body ran the full turn and the head was
    /// painted on top of the band's own beginning, for 364.0 degrees of ink on a
    /// molecule that is 360 round.
    ///
    /// The one case where the fold in `atan2` decides the verdict: the tip is at
    /// twelve o'clock and so is the start, so a folded `head_base` reads as
    /// *before* the start when it is a whole turn after it. [`band_arc`] and
    /// [`one_arrow`] carry the turn, which is why this can be an ordinary
    /// comparison.
    #[test]
    fn a_feature_spanning_the_whole_molecule_does_not_lap_its_own_start() {
        let mol = forward(&[(1, 2_686)]);
        let (r, a_s, a_e, a_base, a_tip) = one_arrow(&mol, 706.0, 756.0);
        let seam = SEAM_PT / r.max(1.0);
        let head = head_angle(part_sweep(1, 2_686, 2_686), r, 9.0);
        assert!(head > 0.0, "a full turn has room for a head");
        assert!(
            a_base > a_s,
            "the head's base at {a_base} is at or before the band's start at {a_s}"
        );
        // Not merely "after the start": the body has to have given up the head's
        // length, which is what makes the total a turn and not a turn plus a head.
        assert!(
            (a_e - a_s - (std::f32::consts::TAU - head)).abs() < seam + 1e-3,
            "the body drew {:.5} rad; a full turn less the head is {:.5}",
            a_e - a_s,
            std::f32::consts::TAU - head
        );
        let ink = (a_e - a_s) + (a_tip - a_base);
        assert!(
            ink <= std::f32::consts::TAU + seam + 1e-3,
            "{ink:.5} rad of ink on a molecule that is {:.5} round",
            std::f32::consts::TAU
        );
    }

    /// A9 — a sub-threshold feature stays a mark, with no arrowhead at all.
    ///
    /// PASSES at 0ebaa41, and this says so rather than dressing it up: it is a
    /// regression guard on the branch whose own reason is that a 9 pt stroke over
    /// coincident points tessellated a translucent wedge across half the pane. A
    /// change that shortens every body must not reach into it.
    #[test]
    fn a_one_base_feature_is_a_mark_with_no_arrowhead() {
        // 1, 3 and 8 bases of 2,686: 0.134, 0.402 and 1.072 degrees, all under the
        // 1.2 the gate tests. NINE bases is 1.2062 and is over it — the fixture said
        // 9 and the test failed on its own arithmetic, not on the renderer. The
        // boundary is pinned below rather than left as a comment nobody rechecks.
        for segs in [(2_464u64, 2_464u64), (100, 102), (191, 198)] {
            let mol = forward(&[segs]);
            let (shapes, _) = paint(&mol, 706.0, 756.0);
            let bases = segs.1 - segs.0 + 1;
            assert!(
                (bases as f64 / 2_686.0) * 360.0 < MIN_FEATURE_DEGREES as f64,
                "{segs:?} is {bases} bases, which is NOT below the gate"
            );
            assert!(
                arrowheads(&shapes).is_empty(),
                "{segs:?} is below MIN_FEATURE_DEGREES and must draw no head"
            );
            // A mark is a 1.75 pt line segment, not a 9 pt band.
            assert!(
                !shapes.iter().any(
                    |s| matches!(s, Shape::Path(p) if p.stroke.width >= 6.0 && p.points.len() >= 2)
                ),
                "{segs:?} was drawn as a band"
            );
        }
        // And one base over the gate IS an arc with a head, so the three above are
        // evidence about the gate and not about a threshold that swallowed
        // everything. Without this the test passes just as well if arrowheads stop
        // being drawn at all.
        let (shapes, _) = paint(&forward(&[(191, 199)]), 706.0, 756.0);
        assert_eq!(
            arrowheads(&shapes).len(),
            1,
            "9 bases is 1.2062 degrees, over the gate, and must still get its head"
        );
    }

    /// A8 — the screen and the exported figure agree about which features have a
    /// direction.
    ///
    /// PROVEN TO FAIL at 0ebaa41: **9 against 6** on the user's own pKoV. `bands`
    /// tested only `f.strand.is_reverse()`, so `pSC101 ori`, `decR` and `decR his`
    /// — all `Strand::Unoriented` — were given a FORWARD arrowhead on screen while
    /// `pl_draw::scene` sets `arrow_on = -1` for them and drew none. The screen
    /// claimed a direction the file does not state, and the printable figure
    /// disagreed with the screen.
    ///
    /// An arrow path in a `Scene` carries four `Seg::Line`s (barb out, tip, barb in,
    /// and the step to the inner radius); a plain sector carries one. Verified
    /// independently by counting `L` commands in the exported SVG.
    #[test]
    fn the_screen_and_the_figure_agree_on_which_features_have_a_direction() {
        let mol = pkov_with_strands();
        let (shapes, _) = paint(&mol, 706.0, 756.0);
        let on_screen = arrowheads(&shapes).len();

        let (sc, _) = pl_draw::scene(&mol, pl_draw::Options::default());
        let in_figure = sc
            .items
            .iter()
            .filter(|i| {
                matches!(i, pl_draw::Item::Path { segs, .. }
                    if segs.iter().filter(|s| matches!(s, pl_draw::Seg::Line(..))).count() >= 4)
            })
            .count();
        assert_eq!(
            on_screen, in_figure,
            "the screen claims {on_screen} directional features, the figure {in_figure}"
        );
        assert_eq!(
            on_screen, 6,
            "pKoV has 6 oriented features and 3 unoriented"
        );
    }

    /// pKoV's nine features with their real strands — three of them unoriented,
    /// which is what `pl convert` warns about on this file.
    fn pkov_with_strands() -> Molecule {
        let mut mol = Molecule {
            seq: vec![b'A'; 8_117],
            topology: Topology::Circular,
            ..Default::default()
        };
        use pl_core::Strand::{Forward, Reverse, Unoriented};
        for (name, start, end, strand) in [
            ("cat promoter", 7_748u64, 7_850u64, Reverse),
            ("CmR", 7_088, 7_747, Reverse),
            ("sacB promoter", 3_398, 3_843, Reverse),
            ("SacB", 1_976, 3_397, Reverse),
            ("f1 ori", 3_945, 4_399, Reverse),
            ("pSC101 ori", 363, 585, Unoriented),
            ("Rep101(Ts)", 633, 1_583, Forward),
            ("decR", 5_423, 5_878, Unoriented),
            ("decR his", 5_423, 5_905, Unoriented),
        ] {
            let mut f = pl_core::Feature::new(name, "misc_feature");
            f.strand = strand;
            f.segments = vec![pl_core::Segment::new(start, end)];
            mol.features.push(f);
        }
        mol
    }

    /// A10 — emphasis. A hovered feature's head grows and its body must give way by
    /// the same larger amount.
    ///
    /// PROVEN TO FAIL at 0ebaa41, where the overlap grew from 14.4 pt of arc to 19.2
    /// — the artefact got worse exactly when the user pointed at it. This is the
    /// assertion that catches a fix hard-coding `9.0` or reading `w` in the wrong
    /// place.
    #[test]
    fn emphasis_lengthens_the_head_and_shortens_the_body_by_the_same_amount() {
        let span = 2_686u64;
        let mol = forward(&[(400, 1_400)]);
        let ctx = crate::test_ctx();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(706.0, 756.0));
        let input = egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        };
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let frame = ctx.run_ui(input.clone(), |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        // `hot`, so `w` becomes BAND_W + 3.
                        show(ui, &mol, "fixture", &[], None, Some(0));
                    });
            });
            fn walk(s: &Shape, acc: &mut Vec<Shape>) {
                match s {
                    Shape::Vec(v) => v.iter().for_each(|s| walk(s, acc)),
                    other => acc.push(other.clone()),
                }
            }
            shapes = Vec::new();
            for cs in &frame.shapes {
                walk(&cs.shape, &mut shapes);
            }
        }
        let c = rect.center();
        let heads = arrowheads(&shapes);
        assert_eq!(heads.len(), 1);
        let (pts, _) = widest_band(&shapes, c);
        let (r, a_s, a_e) = band_arc(&pts, c);
        let ang = |p: &Pos2| (p.y - c.y).atan2(p.x - c.x);
        let a_tip = heads[0].iter().map(ang).fold(f32::MIN, f32::max);
        let a_base = heads[0].iter().map(ang).fold(f32::MAX, f32::min);
        let feature = angle_end(1_400, span) - angle_of(400, span);
        let want = head_angle(feature, r, BAND_W + 3.0);
        assert!(
            (want - head_angle(feature, r, BAND_W)).abs() > 1e-3,
            "the emphasised head must differ from the plain one or this proves nothing"
        );
        let seam = SEAM_PT / r.max(1.0);
        assert!(
            ((a_tip - a_base) - want).abs() < 1e-3,
            "emphasised head {:.5}, wanted {want:.5}",
            a_tip - a_base
        );
        assert!(
            a_e - a_base <= seam,
            "the emphasised body runs {:.2} pt of arc past the head's base",
            (a_e - a_base) * r
        );
        // And the seam is still THERE under emphasis: see the same pair in
        // `the_body_stops_where_the_arrowhead_begins`. Bounded-above only, a seam of
        // zero passes every assertion in this file.
        assert!(
            a_e - a_base >= seam - 1e-4,
            "the emphasised body abuts the head's base instead of running {SEAM_PT} pt under it"
        );
        assert!(
            ((a_e - a_s) + (a_tip - a_base) - feature).abs() < seam + 1e-3,
            "body + head must still be the feature's span under emphasis"
        );
    }

    /// A11 — an arrowhead's barbs stay in their own lane.
    ///
    /// PROVEN TO FAIL before `barb_half` existed, in the painter: with two stacked
    /// features and the pointer on the inner one, its head's outer barb was drawn at
    /// r=376.60 while the outer lane's band runs from r=375.50 — **1.10 pt of one
    /// feature's arrowhead over another feature's rectangle**, which is the user's
    /// own sentence about this map, one lane over from the overlap the pass fixed.
    /// It fails to COMPILE at 0ebaa41 as well, because the number was `w * 0.8`
    /// inline inside `draw_arrowhead` and nothing could ask it anything.
    ///
    /// Quiet it was clear by 1.30 pt, so this is precisely a defect that appears
    /// when the user points at a feature and not otherwise — the reason it survived
    /// every screenshot.
    ///
    /// Two forms, and both matter. The arithmetic form fails at `9.60 > 6.50`
    /// without reading a pixel; the painter form is what says the arithmetic is the
    /// arithmetic the map actually uses.
    #[test]
    fn an_arrowheads_barbs_stay_inside_their_own_lane() {
        // The arithmetic. Half the pitch, so two adjacent lanes each own their half
        // and nothing can cross whatever either one's emphasis is doing.
        for w in [BAND_W, BAND_W + 3.0] {
            assert!(
                barb_half(w) <= LANE_STEP * 0.5 + 1e-6,
                "a barb of {} reaches past the midpoint between two lanes ({})",
                barb_half(w),
                LANE_STEP * 0.5
            );
            // Clear of a neighbouring band at its own EMPHASISED width, which is the
            // width `ring::inside_of` reserves against one radius in for the same
            // reason: hover must not decide whether something else is legible.
            assert!(
                barb_half(w) + (BAND_W + 3.0) * 0.5 <= LANE_STEP,
                "an emphasised neighbour's band starts {} pt out and the barb reaches {}",
                LANE_STEP - (BAND_W + 3.0) * 0.5,
                barb_half(w)
            );
            // And it is still a head and not a butt end: it has to reach past its own
            // shaft or there is no shoulder to read a direction from.
            assert!(
                barb_half(w) > w * 0.5,
                "at w={w} the barb {} does not clear the shaft's own {}",
                barb_half(w),
                w * 0.5
            );
        }
        // Emphasis must not be free: it lengthens the head even where it can no
        // longer widen it, or pointing at a feature would change nothing.
        assert!(
            head_angle(1.0, 90.0, BAND_W + 3.0) > head_angle(1.0, 90.0, BAND_W),
            "with the barb clamped, the head's LENGTH is all emphasis has left"
        );

        // The painter. Two overlapping forward features, so `lanes` puts them in
        // lanes 0 and 1, with the pointer on the inner one.
        let mut mol = Molecule {
            seq: vec![b'A'; 2_686],
            topology: Topology::Circular,
            ..Default::default()
        };
        for (name, s, e) in [("inner", 400u64, 1_400u64), ("outer", 500, 1_500)] {
            let mut f = pl_core::Feature::new(name, "CDS");
            f.strand = pl_core::Strand::Forward;
            f.segments.push(pl_core::Segment::new(s, e));
            mol.features.push(f);
        }
        assert_eq!(
            bands(&mol).iter().map(|b| b.lane).collect::<Vec<_>>(),
            vec![0, 1],
            "the fixture must occupy two lanes or it cannot show a cross-lane overlap"
        );

        for emphasis in [None, Some(0), Some(1)] {
            let ctx = crate::test_ctx();
            let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(706.0, 756.0));
            let input = egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            };
            let mut shapes = Vec::new();
            for _ in 0..2 {
                let frame = ctx.run_ui(input.clone(), |ui| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ui, |ui| {
                            show(ui, &mol, "fixture", &[], None, emphasis);
                        });
                });
                fn walk(s: &Shape, acc: &mut Vec<Shape>) {
                    match s {
                        Shape::Vec(v) => v.iter().for_each(|s| walk(s, acc)),
                        other => acc.push(other.clone()),
                    }
                }
                shapes = Vec::new();
                for cs in &frame.shapes {
                    walk(&cs.shape, &mut shapes);
                }
            }
            let c = rect.center();
            let heads = arrowheads(&shapes);
            assert_eq!(
                heads.len(),
                2,
                "emphasis {emphasis:?}: two features, two heads"
            );

            // Every band's own radius, off the polyline it was painted as.
            let band_radii: Vec<f32> = shapes
                .iter()
                .filter_map(|s| match s {
                    Shape::Path(p) if p.stroke.width >= 6.0 && p.points.len() >= 2 => {
                        Some(band_arc(&p.points, c).0)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(band_radii.len(), 2, "emphasis {emphasis:?}: {band_radii:?}");

            for h in &heads {
                let rs: Vec<f32> = h.iter().map(|p| (*p - c).length()).collect();
                // The tip sits on the band's own radius; the barbs either side of it.
                let own = rs
                    .iter()
                    .copied()
                    .min_by(|a, b| {
                        let d = |x: f32| {
                            band_radii
                                .iter()
                                .map(|r| (x - r).abs())
                                .fold(f32::MAX, f32::min)
                        };
                        d(*a).partial_cmp(&d(*b)).unwrap()
                    })
                    .unwrap();
                for &other in &band_radii {
                    if (other - own).abs() <= 1.0 {
                        continue; // its own band
                    }
                    // A neighbour's band is `(BAND_W + 3) / 2` either side of its
                    // radius at the widest it is ever drawn. No vertex of this head
                    // may be inside that.
                    for &v in &rs {
                        assert!(
                            (v - other).abs() >= (BAND_W + 3.0) * 0.5,
                            "emphasis {emphasis:?}: a head on the band at r={own:.2} puts a \
                             vertex at r={v:.2}, which is inside the band at r={other:.2} \
                             (it runs r={:.2}..{:.2})",
                            other - (BAND_W + 3.0) * 0.5,
                            other + (BAND_W + 3.0) * 0.5
                        );
                    }
                }
            }
        }
    }
}
