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
    /// `(tip, back)` for the one arrowhead, in the direction the feature reads.
    ///
    /// Taken from the terminal part in *biological* order rather than in
    /// coordinate order: the forward feature 2587..87 ends at base 87, not at
    /// the end of its highest-numbered part, and reading the head off the sorted
    /// parts put it at 2,686 pointing counter-clockwise — a forward feature
    /// drawn as a reverse one.
    pub head: Option<(u64, u64)>,
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
            let head = if reverse {
                flow.first().map(|&(s, e)| (s, e))
            } else {
                flow.last().map(|&(s, e)| (e, s))
            };
            let mut segs = flow.clone();
            segs.sort_unstable();
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
/// Height of one line in a label column.
const LINE_H: f32 = 13.0;
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
/// characters at this font's 6 pt advance. `EcoRI 7,530` is 12 and was drawn
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
    // A label's angle in `pl_draw`'s convention: zero at twelve o'clock, `x`
    // from the sine. egui's map runs `-PI/2 + frac * TAU` off the cosine, which
    // is the same circle a quarter turn back, so adding it here means every
    // point `place_ring` returns is already a screen position.
    let ring_angle = |pos: u64| (angle_of(pos, span) + std::f32::consts::FRAC_PI_2) as f64;
    let row_half = 30f64.to_radians();
    let widest_column = |labels: &[(String, u64)]| -> f32 {
        labels
            .iter()
            .filter(|(_, pos)| {
                matches!(
                    ring::side_of(ring_angle(*pos), row_half),
                    Side::Left | Side::Right
                )
            })
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
        let vertical = (outward + LINE_H + LABEL_PAD * 2.0) as f64;
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
    let r = radius_for(widest_column(&one_each));
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
        top: (rect.top() + LABEL_PAD + LINE_H) as f64,
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
    // Name counts alongside the labels, because the line under the caption
    // counts ENZYMES and a folded tick names several. Counting labels is
    // invisible until a fold fires and then understates itself: pET28a claimed
    // `14 of 31 cutters labelled` with 23 on the map, and 14 + 7 + 1 did not
    // reach the 31 it had just stated.
    let mut labels: Vec<(String, u64)> = Vec::new();
    let mut names_in: Vec<usize> = Vec::new();
    for s in folded {
        if s.names.len() == 1 || measure(&s.label()) <= room {
            labels.push((s.label(), s.anchor()));
            names_in.push(s.names.len());
        } else {
            for (n, p) in s.names.iter().zip(&s.positions) {
                labels.push((format!("{n}  {}", crate::doc::fmt_int(*p)), *p));
                names_in.push(1);
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
            height: LINE_H as f64,
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
    // Enzymes, never labels. `names_in` is what makes the difference visible.
    let labelled: usize = (0..labels.len())
        .filter(|&i| placed.placed[i].is_some() && drawn[i].is_some())
        .map(|i| names_in[i])
        .sum();
    let told = ring::Disclosure {
        cutters,
        labelled,
        dual,
        multi,
        hidden: unique.len().saturating_sub(labelled),
        shortened: (0..labels.len())
            .filter(|&i| placed.placed[i].is_some())
            .filter(|&i| drawn[i].as_deref().is_some_and(|s| s != labels[i].0))
            .count(),
    };
    let bp = format!("{} bp", crate::doc::fmt_int(mol.span()));
    let width_of = |s: &str, f: FontId| p.layout_no_wrap(s.to_string(), f, pal.ink).size().x;

    // Everything written in the middle is cut to the ring BEFORE the ruler is
    // placed, and never the other way round.
    //
    // Deriving the ruler's clearance from an unbounded caption and then dropping
    // whichever of the two was checked second is what cost a 4.6 Mb genome its
    // whole scale: `caption_of` leaves a 69-character filename, which is 517 pt
    // of proportional 15, and no radius on this pane clears that. The caption is
    // the line with a hover behind it and the ruler is not, so the caption is the
    // one that gives way. See `ring::centre_room`.
    let widest_number = width_of(&crate::doc::fmt_int(span), FontId::monospace(9.0));
    let centre_room = ring::centre_room(
        r as f64,
        band_w as f64,
        lane_step as f64,
        9.0,
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
        9.0,
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
                FontId::monospace(9.0),
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
            for &(s, e) in &b.segs {
                let pts = arc_points(center, base, angle_of(s, span), angle_of(e, span));
                if pts.len() >= 2 {
                    p.add(Shape::line(pts, Stroke::new(w, b.color)));
                }
            }
            // Thin connectors show the joins, split the same way.
            for &(s, e) in &b.joins {
                let hair = arc_points(center, base, angle_of(s, span), angle_of(e, span));
                if hair.len() >= 2 {
                    p.add(Shape::line(hair, Stroke::new(1.0, b.color)));
                }
            }

            // One arrowhead, on the terminal part, pointing the way it reads.
            if let Some((tip_pos, back_pos)) = b.head {
                draw_arrowhead(
                    p,
                    center,
                    base,
                    angle_of(tip_pos, span),
                    angle_of(back_pos, span),
                    w,
                    b.color,
                );
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
            Side::Top => Pos2::new(at.x, at.y + LINE_H * 0.5),
            Side::Bottom => Pos2::new(at.x, at.y - LINE_H * 0.5),
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

fn draw_arrowhead(
    p: &egui::Painter,
    center: Pos2,
    radius: f32,
    tip_angle: f32,
    back_angle: f32,
    w: f32,
    color: Color32,
) {
    // Point the head along the direction of travel, shrinking it for very short
    // features so it never overshoots the feature it belongs to.
    //
    // Floored, because `sweep == 0` gave `head == 0`, which put all three
    // vertices on one ray and handed `Shape::convex_polygon` a degenerate
    // triangle: a black wedge over half the map pane on pET28a's single-base
    // `rep_origin`. The caller now draws a mark instead of an arc below
    // `MIN_FEATURE_DEGREES` and never reaches this, and the floor stays anyway
    // because a degenerate polygon is a hazard to the next caller too.
    let sweep = (tip_angle - back_angle).abs().max(0.004);
    let head = (w * 1.6 / radius.max(1.0)).min(sweep * 0.9).max(0.002);
    let dir = if tip_angle >= back_angle { 1.0 } else { -1.0 };
    let base_a = tip_angle - dir * head;
    let tip = polar(center, radius, tip_angle);
    let a = polar(center, radius + w * 0.8, base_a);
    let b = polar(center, radius - w * 0.8, base_a);
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
                FontId::monospace(9.0),
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
        let head = (bx1 - bx0).min(7.0);

        // A pentagon rather than a rectangle: the point carries the strand.
        let pts = if b.reverse {
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
        let fwd = &bands(&circle(&[(2_587, 87)], false))[0];
        let (tip, back) = fwd.head.expect("a terminal part");
        assert_eq!(tip, 87);
        assert!(
            angle_of(tip, span) > angle_of(back, span),
            "the head must point the way the feature reads"
        );

        // Reverse 2587..87 reads the other way and ends at base 2,587.
        let rev = &bands(&circle(&[(2_587, 87)], true))[0];
        let (rtip, rback) = rev.head.expect("a terminal part");
        assert_eq!(rtip, 2_587);
        assert!(angle_of(rtip, span) < angle_of(rback, span));

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
}
