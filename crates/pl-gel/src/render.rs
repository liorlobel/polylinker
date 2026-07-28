//! A gel as a [`Scene`], and so as SVG or PDF.
//!
//! # Bands are drawn where the model can place them, and nowhere else
//!
//! A fragment outside the gel's resolving range gets no rectangle. It is not
//! drawn at the well "approximately", and it is not omitted in silence either:
//! [`Options::note_unplaced`] puts it in the caption under its lane, because a
//! picture that is missing a fragment the digest really makes is worse than one
//! that says it cannot place it.
//!
//! # Co-migrating fragments are one band, labelled with all of them
//!
//! This is the point of the whole exercise. Two fragments 100 bp apart at 2 kb
//! are one band on a 1% gel, and the label says `2000/2100` on that single band
//! rather than drawing two lines a fraction of a millimetre apart.

use crate::{Group, Simulation};
use pl_draw::scene::{Anchor, Item, Scene, Seg};

/// One lane of the picture.
#[derive(Debug, Clone)]
pub struct Lane {
    pub label: String,
    pub sim: Simulation,
    /// Draw as a ladder: every band labelled, thinner, in the reference colour.
    pub is_ladder: bool,
}

/// Layout.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub lane_width: f64,
    pub lane_gap: f64,
    /// Points per millimetre of gel.
    pub scale: f64,
    /// List fragments the gel cannot place under their lane.
    pub note_unplaced: bool,
    /// Dark background with light bands, the way a stained gel photographs.
    pub inverted: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            lane_width: 74.0,
            lane_gap: 16.0,
            scale: 4.0,
            note_unplaced: true,
            inverted: true,
        }
    }
}

const TOP: f64 = 34.0; // room for the lane labels and the wells
const PAD: f64 = 18.0;
/// How far outside the lane a band's size label is set.
const LABEL_OFFSET: f64 = 4.0;
const BAND_LABEL_SIZE: f64 = 9.0;
const NOTE_SIZE: f64 = 8.5;

/// Width of a string in points, from Helvetica's advance widths.
///
/// pl-gel takes no dependencies, so there is no font metric table to consult —
/// but a band label is only ever digits and `/`, and Helvetica's digits are all
/// 0.556 em (they are tabular by design, so a size label cannot surprise us)
/// with `/` at 0.278 em. Anything else in a caption is charged 0.6 em, which
/// over-estimates for the lower case that dominates English prose; the estimate
/// erring wide is the direction that keeps text inside the page.
fn text_width(s: &str, size: f64) -> f64 {
    size * s
        .chars()
        .map(|c| match c {
            '0'..='9' => 0.556,
            '/' | ' ' | ',' | '.' | 'i' | 'l' | 'j' | 't' | 'f' => 0.278,
            _ => 0.6,
        })
        .sum::<f64>()
}

/// Where the lanes sit and how wide the picture has to be to hold them.
///
/// Split out because the geometry has to be computed once and *used* twice —
/// by the renderer and by the tests. `lanes_do_not_overlap` re-derived the lane
/// positions from a formula copied out of `to_scene`, checked the lane
/// rectangles and nothing else, and so stayed green while every band label ran
/// off the edge of the viewBox.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    /// Left edge of lane 0.
    left: f64,
    /// Centre-to-centre spacing minus the lane width. At least
    /// `Options::lane_gap`, wider when the size labels need the room.
    gap: f64,
    width: f64,
}

impl Layout {
    fn x_of(&self, i: usize, o: &Options) -> f64 {
        self.left + i as f64 * (o.lane_width + self.gap)
    }
}

/// Reserve room for the text, not just for the rectangles.
///
/// Band size labels are drawn *outside* the lane: to the right for a sample
/// lane, to the left for a ladder. Nothing used to add their extents to the
/// scene, so the two outermost columns of labels were cut off by the SVG
/// viewBox and the PDF MediaBox alike — `pl gel demo.gb --cut EcoRI --svg`
/// emitted `viewBox="0 0 200 424"` with `<text x="186" …>3180</text>`, which
/// needs about 20 pt and had 14. The label was not missing but *truncated*, so
/// the reader saw a partial, entirely plausible fragment size. The same 4 pt
/// offset put a `2000/2100` label about 29 pt into the neighbouring lane
/// whenever more than one lane was drawn.
fn layout(lanes: &[Lane], o: &Options) -> Layout {
    let widest = |ladder: bool| -> f64 {
        lanes
            .iter()
            .filter(|l| l.is_ladder == ladder)
            .flat_map(|l| l.sim.groups.iter())
            .map(|g| text_width(&label_of(g), BAND_LABEL_SIZE))
            .fold(0.0f64, f64::max)
    };
    let sample = widest(false);
    let ladder = widest(true);

    // The unplaced-fragment captions are centred under their lane, so only the
    // half that hangs past the lane edge needs reserving.
    let note_over = if o.note_unplaced {
        lanes
            .iter()
            .flat_map(notes_for)
            .map(|n| (text_width(&n, NOTE_SIZE) - o.lane_width) / 2.0)
            .fold(0.0f64, f64::max)
            .max(0.0)
    } else {
        0.0
    };

    let left = (LABEL_OFFSET + ladder + 2.0).max(PAD).max(PAD + note_over);
    let right = (LABEL_OFFSET + sample + 2.0).max(PAD).max(PAD + note_over);
    // A ladder in the middle of a gel puts its labels in the gap to its left,
    // so the gap has to hold whichever kind of label is wider.
    let gap = o.lane_gap.max(LABEL_OFFSET * 2.0 + sample.max(ladder));
    let width = left
        + right
        + lanes.len() as f64 * o.lane_width
        + (lanes.len().saturating_sub(1)) as f64 * gap;
    Layout { left, gap, width }
}

/// The captions for the fragments this lane's gel cannot place.
fn notes_for(lane: &Lane) -> Vec<String> {
    let mut notes = Vec::new();
    let big = lane.sim.too_large();
    let small = lane.sim.too_small();
    if !big.is_empty() {
        notes.push(format!("{} too large to place", join(&big)));
    }
    if !small.is_empty() {
        notes.push(format!("{} too small to place", join(&small)));
    }
    notes
}

/// Draw a set of lanes.
pub fn to_scene(lanes: &[Lane], o: &Options, title: &str) -> Scene {
    let run_mm = lanes
        .iter()
        .filter_map(|l| {
            l.sim
                .groups
                .iter()
                .map(|g| g.mm)
                .fold(None, |a: Option<f64>, m| Some(a.map_or(m, |a| a.max(m))))
        })
        .fold(80.0f64, f64::max);
    let gel_h = run_mm * o.scale + 24.0;
    let lay = layout(lanes, o);
    let width = lay.width;
    let note_h = if o.note_unplaced { 46.0 } else { 14.0 };
    let height = TOP + gel_h + note_h;

    let (bg, band, text, dim) = if o.inverted {
        ("#15181c", "#f2f4f7", "#e6e9ee", "#9aa4b0")
    } else {
        ("#ffffff", "#1c2026", "#1c2026", "#5d6774")
    };

    let mut items = vec![Item::Path {
        segs: rect(0.0, 0.0, width, height),
        fill: Some(bg.into()),
        stroke: None,
        stroke_width: 0.0,
        title: None,
    }];

    for (i, lane) in lanes.iter().enumerate() {
        let x = lay.x_of(i, o);
        // The well.
        items.push(Item::Path {
            segs: rect(x, TOP - 8.0, o.lane_width, 6.0),
            fill: Some(dim.into()),
            stroke: None,
            stroke_width: 0.0,
            title: Some(format!("{} well", lane.label)),
        });
        items.push(Item::Text {
            x: x + o.lane_width / 2.0,
            y: TOP - 18.0,
            size: 11.0,
            anchor: Anchor::Middle,
            color: text.into(),
            bold: true,
            text: lane.label.clone(),
        });

        for g in &lane.sim.groups {
            let y = TOP + g.mm * o.scale;
            let h = if lane.is_ladder { 2.4 } else { 3.4 };
            items.push(Item::Path {
                segs: rect(x, y - h / 2.0, o.lane_width, h),
                fill: Some(band.into()),
                stroke: None,
                stroke_width: 0.0,
                title: Some(label_of(g)),
            });
            // Ladder bands are labelled inside the picture; sample bands get
            // their sizes to the right of the lane so they do not sit on top of
            // the band itself.
            let (lx, anchor) = if lane.is_ladder {
                (x - LABEL_OFFSET, Anchor::End)
            } else {
                (x + o.lane_width + LABEL_OFFSET, Anchor::Start)
            };
            items.push(Item::Text {
                x: lx,
                y,
                size: BAND_LABEL_SIZE,
                anchor,
                color: if g.is_merged() { text } else { dim }.into(),
                bold: g.is_merged(),
                text: label_of(g),
            });
        }

        if o.note_unplaced {
            let notes = notes_for(lane);
            for (k, n) in notes.iter().enumerate() {
                items.push(Item::Text {
                    x: x + o.lane_width / 2.0,
                    y: TOP + gel_h + 12.0 + k as f64 * 12.0,
                    size: NOTE_SIZE,
                    anchor: Anchor::Middle,
                    color: dim.into(),
                    bold: false,
                    text: n.clone(),
                });
            }
        }
    }

    Scene {
        width,
        height,
        title: title.to_string(),
        items,
    }
}

/// `2000` for one fragment, `2000/2100` for a band holding several.
fn label_of(g: &Group) -> String {
    g.sizes
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join("/")
}

fn join(v: &[u64]) -> String {
    v.iter().map(u64::to_string).collect::<Vec<_>>().join(", ")
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Vec<Seg> {
    vec![
        Seg::Move(x, y),
        Seg::Line(x + w, y),
        Seg::Line(x + w, y + h),
        Seg::Line(x, y + h),
        Seg::Close,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Conditions, Gel};

    fn lane(fragments: &[u64], label: &str, is_ladder: bool) -> Lane {
        Lane {
            label: label.into(),
            sim: Gel::modelled(Conditions::default()).run(fragments),
            is_ladder,
        }
    }

    fn texts(sc: &Scene) -> Vec<String> {
        sc.items
            .iter()
            .filter_map(|i| match i {
                Item::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn one_band_per_visible_group_not_one_per_fragment() {
        // The picture must not show two lines where a person sees one, which is
        // the failure that makes a non-diagnostic digest look diagnostic.
        let l = lane(&[2_000, 2_100, 6_000], "digest", false);
        assert_eq!(l.sim.groups.len(), 2);
        let sc = to_scene(&[l], &Options::default(), "t");
        // One background + one well + two bands.
        let rects = sc
            .items
            .iter()
            .filter(|i| matches!(i, Item::Path { .. }))
            .count();
        assert_eq!(rects, 4, "background, well, and two bands");
        assert!(texts(&sc).contains(&"2000/2100".to_string()));
    }

    #[test]
    fn a_fragment_that_cannot_be_placed_is_named_in_the_caption() {
        // Not drawn at a made-up position, and not silently missing either.
        let g = Gel::modelled(Conditions {
            agarose_percent: 2.0,
            ..Default::default()
        });
        let l = Lane {
            label: "d".into(),
            sim: g.run(&[20, 800, 40_000]),
            is_ladder: false,
        };
        let sc = to_scene(&[l], &Options::default(), "t");
        let t = texts(&sc).join(" | ");
        assert!(t.contains("40000 too large"), "{t}");
        assert!(t.contains("20 too small"), "{t}");
        assert!(!t.contains("40000\n"), "and it has no band label");
    }

    #[test]
    fn a_bigger_fragment_is_drawn_nearer_the_well() {
        // If the picture inverts this it is not a gel.
        let l = lane(&[1_000, 8_000], "d", false);
        let sc = to_scene(&[l], &Options::default(), "t");
        let ys: Vec<(f64, String)> = sc
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Text { y, text, size, .. } if *size < 10.0 => Some((*y, text.clone())),
                _ => None,
            })
            .collect();
        let big = ys.iter().find(|(_, t)| t == "8000").expect("8000").0;
        let small = ys.iter().find(|(_, t)| t == "1000").expect("1000").0;
        assert!(big < small, "8 kb at {big} must sit above 1 kb at {small}");
    }

    /// Left and right edge of a piece of text, in scene coordinates.
    fn extent(x: f64, size: f64, anchor: Anchor, text: &str) -> (f64, f64) {
        let w = text_width(text, size);
        match anchor {
            Anchor::Start => (x, x + w),
            Anchor::Middle => (x - w / 2.0, x + w / 2.0),
            Anchor::End => (x - w, x),
        }
    }

    #[test]
    fn lanes_do_not_overlap() {
        let lanes = vec![
            lane(&[500, 1_000, 3_000], "L", true),
            lane(&[2_000], "a", false),
            lane(&[900, 4_000], "b", false),
        ];
        let o = Options::default();
        let sc = to_scene(&lanes, &o, "t");
        // The lane positions come from `layout`, the same function the
        // renderer uses. They used to be re-derived here from a copy of the
        // width formula, which is how this test kept passing while the picture
        // it describes had its labels cut off.
        let lay = layout(&lanes, &o);
        for i in 0..lanes.len() {
            let x = lay.x_of(i, &o);
            assert!(x + o.lane_width <= sc.width - PAD + 1e-9);
            if i + 1 < lanes.len() {
                assert!(
                    x + o.lane_width <= lay.x_of(i + 1, &o) + 1e-9,
                    "lane {i} runs into lane {}",
                    i + 1
                );
            }
        }
        assert!(sc.width > 0.0 && sc.height > 0.0);
    }

    #[test]
    fn every_label_is_inside_the_picture_it_is_drawn_on() {
        // pl_draw emits `viewBox="0 0 width height"` and an SVG clips at its
        // viewport, so anything past `sc.width` is not merely close to the
        // edge, it is gone. The shipped CLI reproduced this exactly: a gel of
        // demo-construct.gb cut with EcoRI came out `viewBox="0 0 200 424"`
        // with `<text x="186" … text-anchor="start">3180</text>` — 14 pt of
        // room for a label needing 20. The trailing digit fell off, so the
        // reader saw "318", a perfectly plausible fragment size that is not
        // the one on the gel.
        let cases = vec![
            vec![lane(&[2_000, 2_100, 6_000], "digest", false)],
            vec![
                lane(&[500, 1_000, 3_000, 10_000], "ladder", true),
                lane(&[3_180, 1_120], "EcoRI", false),
            ],
            // A gel with unplaceable fragments, whose caption is the widest
            // text on the picture.
            vec![Lane {
                label: "d".into(),
                sim: Gel::modelled(Conditions {
                    agarose_percent: 2.0,
                    ..Default::default()
                })
                .run(&[20, 800, 40_000]),
                is_ladder: false,
            }],
        ];
        for lanes in cases {
            let sc = to_scene(&lanes, &Options::default(), "t");
            for item in &sc.items {
                let Item::Text {
                    x,
                    size,
                    anchor,
                    text,
                    ..
                } = item
                else {
                    continue;
                };
                let (l, r) = extent(*x, *size, *anchor, text);
                assert!(l >= -1e-9, "{text:?} starts at {l}, off the left edge");
                assert!(
                    r <= sc.width + 1e-9,
                    "{text:?} ends at {r}, past the {} pt edge",
                    sc.width
                );
            }
        }
    }

    #[test]
    fn a_band_label_does_not_sit_over_the_next_lane() {
        // The other half of the same 4 pt offset: with the default 16 pt gap a
        // "2000/2100" label is about 45 pt wide and runs 29 pt into whatever
        // is drawn beside it.
        let lanes = vec![
            lane(&[2_000, 2_100], "a", false),
            lane(&[900, 4_000], "b", false),
            lane(&[6_000], "c", false),
        ];
        let o = Options::default();
        let sc = to_scene(&lanes, &o, "t");
        let lay = layout(&lanes, &o);
        let band_labels: Vec<(f64, f64, String)> = sc
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Text {
                    x,
                    size,
                    anchor,
                    text,
                    ..
                } if *size == BAND_LABEL_SIZE => {
                    let (l, r) = extent(*x, *size, *anchor, text);
                    Some((l, r, text.clone()))
                }
                _ => None,
            })
            .collect();
        assert!(!band_labels.is_empty(), "the fixture must draw labels");
        for (l, r, text) in band_labels {
            let next = (0..lanes.len())
                .map(|i| lay.x_of(i, &o))
                .find(|&x| x > l + 1e-9);
            if let Some(next) = next {
                assert!(
                    r <= next + 1e-9,
                    "{text:?} runs to {r}, over the lane starting at {next}"
                );
            }
        }
    }

    #[test]
    fn a_gel_with_no_labels_to_place_is_not_padded_for_them() {
        // The control. Reserving room for text must be driven by the text that
        // is actually there: an empty gel keeps the plain PAD margins, so the
        // fix cannot be "make everything wider and hope".
        let o = Options::default();
        let sc = to_scene(&[lane(&[], "empty", false)], &o, "t");
        assert_eq!(sc.width, PAD * 2.0 + o.lane_width);
        // And a one-lane gel whose only label is narrow gets a narrow margin,
        // not the widest one any gel might need.
        let narrow = to_scene(&[lane(&[2_000], "a", false)], &o, "t");
        let wide = to_scene(&[lane(&[2_000, 2_100], "a", false)], &o, "t");
        assert!(
            narrow.width < wide.width,
            "{} vs {}",
            narrow.width,
            wide.width
        );
    }

    #[test]
    fn drawing_the_same_gel_twice_gives_the_same_picture() {
        let lanes = vec![lane(&[500, 1_000, 1_010], "a", false)];
        let o = Options::default();
        assert_eq!(to_scene(&lanes, &o, "t"), to_scene(&lanes, &o, "t"));
    }

    #[test]
    fn an_empty_gel_still_draws_its_lanes() {
        let sc = to_scene(&[lane(&[], "empty", false)], &Options::default(), "t");
        assert!(texts(&sc).contains(&"empty".to_string()));
        assert!(sc.height > 0.0);
    }
}
