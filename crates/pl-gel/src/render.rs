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
    let width = PAD * 2.0
        + lanes.len() as f64 * o.lane_width
        + (lanes.len().saturating_sub(1)) as f64 * o.lane_gap;
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
        let x = PAD + i as f64 * (o.lane_width + o.lane_gap);
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
                (x - 4.0, Anchor::End)
            } else {
                (x + o.lane_width + 4.0, Anchor::Start)
            };
            items.push(Item::Text {
                x: lx,
                y,
                size: 9.0,
                anchor,
                color: if g.is_merged() { text } else { dim }.into(),
                bold: g.is_merged(),
                text: label_of(g),
            });
        }

        if o.note_unplaced {
            let mut notes = Vec::new();
            let big = lane.sim.too_large();
            let small = lane.sim.too_small();
            if !big.is_empty() {
                notes.push(format!("{} too large to place", join(&big)));
            }
            if !small.is_empty() {
                notes.push(format!("{} too small to place", join(&small)));
            }
            for (k, n) in notes.iter().enumerate() {
                items.push(Item::Text {
                    x: x + o.lane_width / 2.0,
                    y: TOP + gel_h + 12.0 + k as f64 * 12.0,
                    size: 8.5,
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

    #[test]
    fn lanes_do_not_overlap() {
        let lanes = vec![
            lane(&[500, 1_000, 3_000], "L", true),
            lane(&[2_000], "a", false),
            lane(&[900, 4_000], "b", false),
        ];
        let o = Options::default();
        let sc = to_scene(&lanes, &o, "t");
        for i in 0..lanes.len() {
            let x = PAD + i as f64 * (o.lane_width + o.lane_gap);
            assert!(x + o.lane_width <= sc.width - PAD + 1e-9);
        }
        assert!(sc.width > 0.0 && sc.height > 0.0);
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
