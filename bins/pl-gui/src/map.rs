//! Plasmid map painting: circular for circular molecules, a track for linear.
//!
//! Layout is separated from painting on purpose. `lanes` and `label_slots` are
//! ordinary functions over numbers with no egui in sight, so the fiddly parts —
//! overlap packing and label collision — are unit-testable without a window.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use pl_core::{Molecule, Topology};
use pl_enzymes::Digest;

use crate::theme::{self, Palette};

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
    // Take the panel's own rect, and a painter clipped to it.
    //
    // `available_size()` reported far more than this panel actually owns, so a
    // map sized from it grew until it covered the side panel and ran off the
    // bottom of the window. The map is painted last, so it simply hid them.
    let rect = ui.max_rect();
    let response = ui.allocate_rect(rect, Sense::click());
    let painter = ui.painter_at(rect);
    let mut out = MapResponse {
        hovered: None,
        clicked: None,
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
    out
}

// ---------------------------------------------------------------------------
// circular
// ---------------------------------------------------------------------------

/// Space kept clear outside the backbone for enzyme labels.
const LABEL_RESERVE: f32 = 132.0;
/// How close a label may come to the edge of the panel.
const LABEL_PAD: f32 = 6.0;

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
    // Leave room outside the backbone for ticks, leader lines and the enzyme
    // labels themselves. An enzyme name plus a formatted position runs to about
    // 110 px at this font, and the labels sit outside the leaders.
    let r = (rect.width().min(rect.height()) * 0.5 - LABEL_RESERVE).max(40.0);
    let lane_step = 13.0;
    let band_w = 9.0;

    // backbone
    p.circle_stroke(center, r, Stroke::new(1.5, pal.line));

    // Ticks every 10% of the molecule, labelled in bp.
    for i in 0..10 {
        let pos = tick_pos(span, i);
        let a = angle_of(pos, span);
        p.line_segment(
            [polar(center, r - 5.0, a), polar(center, r + 5.0, a)],
            Stroke::new(1.0, pal.line),
        );
        if i % 2 == 0 {
            p.text(
                polar(center, r - 16.0, a),
                Align2::CENTER_CENTER,
                crate::doc::fmt_int(pos),
                FontId::monospace(9.0),
                pal.muted,
            );
        }
    }

    // features
    for b in bands {
        // Reverse-strand features sit inside the backbone, forward outside:
        // the convention that lets you read direction without a legend.
        let base = if b.reverse {
            r - band_w - b.lane as f32 * lane_step
        } else {
            r + band_w + b.lane as f32 * lane_step
        };
        let emphasised = selected == Some(b.index) || hot == Some(b.index);
        let w = if emphasised { band_w + 3.0 } else { band_w };

        // Each part as its own arc, always the short way round: `bands` has
        // already split anything that crosses the origin, so `s <= e` here and
        // `arc_points` never interpolates backwards across the whole ring.
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
                    let (lo, hi) = (angle_of(s, span), angle_of(e, span));
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

    // Unique cutters, outside everything, with labels fanned to avoid overlap.
    let uniq: Vec<(&str, u64)> = digest
        .iter()
        .filter(|d| d.is_unique_cutter())
        .map(|d| (d.enzyme.name, d.positions[0]))
        .collect();

    let outer =
        r + band_w + (bands.iter().map(|b| b.lane).max().unwrap_or(0) as f32 + 1.0) * lane_step;
    let tick_r = outer + 6.0;

    // Split left and right so labels stack vertically on each side.
    let mut left: Vec<(usize, f32)> = Vec::new();
    let mut right: Vec<(usize, f32)> = Vec::new();
    for (i, (_, pos)) in uniq.iter().enumerate() {
        let a = angle_of(*pos, span);
        let y = center.y + (tick_r + 14.0) * a.sin();
        if a.cos() < 0.0 {
            left.push((i, y));
        } else {
            right.push((i, y));
        }
    }

    for (side, sign) in [(&left, -1.0f32), (&right, 1.0f32)] {
        let anchors: Vec<f32> = side.iter().map(|(_, y)| *y).collect();
        let placed = label_slots(&anchors, 13.0, rect.top() + 12.0, rect.bottom() - 12.0);
        for (k, (i, _)) in side.iter().enumerate() {
            let (name, pos) = uniq[*i];
            let a = angle_of(pos, span);
            let from = polar(center, outer, a);
            let to = polar(center, tick_r + 8.0, a);
            p.line_segment([from, to], Stroke::new(1.0, pal.muted));

            // Keep the label inside the panel. Without this the text runs under
            // the side panel and enzyme names on the right are cut in half.
            let lx = (center.x + sign * (tick_r + 22.0))
                .clamp(rect.left() + LABEL_PAD, rect.right() - LABEL_PAD);
            let ly = placed[k];
            p.line_segment([to, Pos2::new(lx, ly)], Stroke::new(0.8, pal.faint));
            p.text(
                Pos2::new(lx + sign * 4.0, ly),
                if sign < 0.0 {
                    Align2::RIGHT_CENTER
                } else {
                    Align2::LEFT_CENTER
                },
                format!("{name}  {}", crate::doc::fmt_int(pos)),
                FontId::monospace(10.0),
                pal.ink2,
            );
        }
    }

    // Centre caption. The .dna container carries no molecule name at all, so
    // fall back to what the user actually called the file.
    p.text(
        center - Vec2::new(0.0, 9.0),
        Align2::CENTER_CENTER,
        caption,
        FontId::proportional(15.0),
        pal.ink,
    );
    p.text(
        center + Vec2::new(0.0, 11.0),
        Align2::CENTER_CENTER,
        format!("{} bp", crate::doc::fmt_int(mol.span())),
        FontId::monospace(11.0),
        pal.muted,
    );
}

/// Sample an arc into a polyline. Enough segments that curvature is smooth at
/// any size, few enough that a genome with hundreds of features stays cheap.
fn arc_points(center: Pos2, radius: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let sweep = (a1 - a0).abs().max(0.004);
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
    let sweep = (tip_angle - back_angle).abs();
    let head = (w * 1.6 / radius.max(1.0)).min(sweep * 0.9);
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
