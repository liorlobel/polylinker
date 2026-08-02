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
///
/// NOT `Copy` since [`Options::note`] arrived. That is deliberate: a caveat is
/// a `String` because [`crate::Simulation::caveat`] composes one, and the whole
/// reason it is a method rather than a UI string is that no caller may be
/// trusted to remember it.
#[derive(Debug, Clone)]
pub struct Options {
    pub lane_width: f64,
    pub lane_gap: f64,
    /// Points per millimetre of gel.
    pub scale: f64,
    /// List fragments the gel cannot place under their lane.
    pub note_unplaced: bool,
    /// Dark background with light bands, the way a stained gel photographs.
    pub inverted: bool,
    /// A sentence laid out under the picture — normally
    /// [`crate::Simulation::caveat`].
    ///
    /// The exported SVG used to carry no calibration statement at all: the CLI
    /// `println!`s the caveat beside the table and the file goes out bare, so a
    /// modelled gel reached a reader with nothing saying it was modelled. The
    /// map crate solved the same problem by putting its disclosure INTO the
    /// scene rather than beside it, and this is that fix for the gel.
    pub note: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            lane_width: 74.0,
            lane_gap: 16.0,
            scale: 4.0,
            note_unplaced: true,
            inverted: true,
            note: None,
        }
    }
}

impl Options {
    /// The background this picture is drawn on.
    ///
    /// Exposed because `pl_draw::contrast::audit` takes the background as a
    /// parameter, and a caller that hard-coded `"#15181c"` to satisfy it would
    /// be a second source of truth for a colour chosen here.
    pub fn background(&self) -> &'static str {
        self.palette().0
    }

    /// `(background, band, text, dim)`.
    fn palette(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        if self.inverted {
            ("#15181c", "#f2f4f7", "#e6e9ee", "#9aa4b0")
        } else {
            ("#ffffff", "#1c2026", "#1c2026", "#5d6774")
        }
    }
}

const TOP: f64 = 34.0; // room for the lane labels and the wells
const PAD: f64 = 18.0;
/// How far outside the lane a band's size label is set.
const LABEL_OFFSET: f64 = 4.0;
const BAND_LABEL_SIZE: f64 = 9.0;
const LANE_LABEL_SIZE: f64 = 11.0;
const NOTE_SIZE: f64 = 8.5;
const CAVEAT_SIZE: f64 = 8.0;
const CAVEAT_LEADING: f64 = 10.0;
/// The narrowest a picture carrying a caveat is allowed to be.
///
/// A two-lane gel is 238 pt wide, and wrapping 250 characters of prose into
/// 202 pt of text column produces fifteen lines of caption under a picture 424
/// pt tall. The sentence is not optional, so the picture widens for it.
const CAVEAT_MIN_WIDTH: f64 = 380.0;

/// Width of a string in points, from Helvetica's advance widths.
///
/// pl-gel takes no dependencies, so there is no font metric table to consult.
/// Each number below is the larger of that glyph's Helvetica and Helvetica-Bold
/// advance, rounded up, so the estimate errs wide whichever weight the text is
/// set in — and erring wide is the direction that keeps text inside the page.
///
/// This used to charge a flat 0.6 em for everything that is not a digit or `/`.
/// That is wide enough for the lower case that dominates a caption but *narrow*
/// for capitals — Helvetica-Bold `E` is 0.667 em and `W` is 0.944 — which did
/// not matter while the only measured text was band labels (digits and `/`,
/// tabular by design, so a size label cannot surprise us) and unplaced-fragment
/// captions (English prose). It started to matter when the bold, capital-heavy
/// lane labels were folded into [`layout`]: an under-estimate of
/// `EcoRI+BamHI+HindIII+XhoI` is not a picture a few points too wide, it is a
/// caption the viewBox cuts in half.
///
/// A character outside the table is charged 0.611 em, which bounds Latin script
/// and nothing else; a lane label in another script can still overrun.
fn text_width(s: &str, size: f64) -> f64 {
    size * s
        .chars()
        .map(|c| match c {
            '0'..='9' => 0.556,
            '/' | ' ' | ',' | '.' | 'i' | 'l' | 'j' | 'I' => 0.278,
            't' | 'f' => 0.333,
            'M' | 'W' => 0.944,
            'm' | 'w' => 0.889,
            'A'..='Z' => 0.778,
            _ => 0.611,
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

    // The lane label is centred over its lane exactly as the notes are, so
    // again only the overhang needs reserving — and it needs reserving at both
    // edges, because nothing here knows which lane is outermost. Reserving
    // nothing for it is what let `pl gel f.gb --cut EcoRI --cut BamHI --cut
    // HindIII --cut XhoI` emit `viewBox="0 0 240.99 424"` around a 142.7 pt
    // caption centred at x=175.04; an SVG clips at its viewport, so the reader
    // saw `EcoRI+BamHI+HindIII+Xho`. The dangerous ones are the digests that
    // clip to another real enzyme — `AarI+BamHI+BsiWI+SacII` came out
    // `AarI+BamHI+BsiWI+SacI`, naming a digest nobody ran.
    let label_over = lanes
        .iter()
        .map(|l| (text_width(&l.label, LANE_LABEL_SIZE) - o.lane_width) / 2.0)
        .fold(0.0f64, f64::max)
        .max(0.0);
    let centred_over = note_over.max(label_over);

    let left = (LABEL_OFFSET + ladder + 2.0)
        .max(PAD)
        .max(PAD + centred_over);
    let right = (LABEL_OFFSET + sample + 2.0)
        .max(PAD)
        .max(PAD + centred_over);
    // A sample lane sets its band labels in the gap to its *right* and a ladder
    // sets its own in the gap to its *left*, so a gap between a sample and the
    // ladder after it carries both columns at once and has to hold both widths.
    // Reserving room for whichever kind is wider left them overlapping by
    // exactly `min(sample, ladder)`: a `2000/2100` sample beside a 1 kb ladder
    // put both that label and the ladder's `2000` ending at x = 151.55, their
    // baselines 2 pt apart at 9 pt type, printing one on top of the other.
    let ladder_after_sample = lanes.windows(2).any(|w| !w[0].is_ladder && w[1].is_ladder);
    let in_gap = if ladder_after_sample {
        sample + ladder
    } else {
        sample.max(ladder)
    };
    let gap = o.lane_gap.max(LABEL_OFFSET * 2.0 + in_gap);
    let mut width = left
        + right
        + lanes.len() as f64 * o.lane_width
        + (lanes.len().saturating_sub(1)) as f64 * gap;
    // The caveat widens the picture rather than wrapping into a column the
    // lanes happen to have produced, and the lanes stay centred in it: a
    // two-lane gel pushed hard left under a full-width paragraph reads as a
    // layout bug, which is not what anyone should be looking at while deciding
    // whether to trust the positions.
    let mut left = left;
    if o.note.is_some() && width < CAVEAT_MIN_WIDTH {
        left += (CAVEAT_MIN_WIDTH - width) / 2.0;
        width = CAVEAT_MIN_WIDTH;
    }
    Layout { left, gap, width }
}

/// Break a sentence into lines no wider than `width`, using the same advance
/// table the margins are reserved from.
///
/// Greedy, on spaces. A word longer than the column is left long rather than
/// hyphenated or cut: `every_label_is_inside_the_picture_it_is_drawn_on` would
/// then be the thing that catches it, which is the right place for that to
/// surface. Nothing in `caveat()` is one.
fn wrap(s: &str, size: f64, width: f64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let candidate = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if !cur.is_empty() && text_width(&candidate, size) > width {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = candidate;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
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
    let caveat: Vec<String> = match &o.note {
        Some(n) => wrap(n, CAVEAT_SIZE, width - PAD * 2.0),
        None => Vec::new(),
    };
    let caveat_h = if caveat.is_empty() {
        0.0
    } else {
        caveat.len() as f64 * CAVEAT_LEADING + 8.0
    };
    let height = TOP + gel_h + note_h + caveat_h;

    let (bg, band, text, dim) = o.palette();

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
            size: LANE_LABEL_SIZE,
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

    for (k, line) in caveat.iter().enumerate() {
        items.push(Item::Text {
            x: PAD,
            y: TOP + gel_h + note_h + 4.0 + k as f64 * CAVEAT_LEADING,
            size: CAVEAT_SIZE,
            anchor: Anchor::Start,
            color: dim.into(),
            bold: false,
            text: line.clone(),
        });
    }

    Scene {
        width,
        height,
        title: title.to_string(),
        items,
    }
}

/// `2000` for one fragment, `2000/2100` for a band holding several — and a span
/// with a count once there are more of them than anyone would read.
///
/// See [`crate::MAX_LISTED`] for what an uncapped list did to the picture.
fn label_of(g: &Group) -> String {
    g.label()
}

fn join(v: &[u64]) -> String {
    crate::name_sizes(v, ", ")
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
            // A multi-enzyme digest, whose *lane* label is the widest text on
            // the picture. Every other fixture here labels its lanes with one
            // to six characters, which is why this guard stayed green while the
            // caption named the digest ran off the right edge.
            vec![
                lane(&[500, 1_000, 3_000, 10_000], "ladder", true),
                lane(&[3_180, 1_120], "EcoRI+BamHI+HindIII+XhoI", false),
            ],
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
    fn a_lane_label_wider_than_its_lane_stays_on_the_picture() {
        // `pl gel f.gb --cut EcoRI --cut BamHI --cut HindIII --cut XhoI` joins
        // the enzyme names with `+` into one lane caption. `layout` reserved
        // margins for the band labels and the unplaced-fragment captions and
        // nothing at all for this one, so the shipped binary emitted
        // `viewBox="0 0 240.99 424"` around a caption centred at x=175.04 that
        // needs 142.7 pt of Helvetica-Bold — 5.4 pt past the right edge, and an
        // SVG clips at its viewport, so the reader saw
        // `EcoRI+BamHI+HindIII+Xho`. The dangerous ones are the quadruple
        // digests that clip to another *real* enzyme: `AarI+BamHI+BsiWI+SacII`
        // came out `AarI+BamHI+BsiWI+SacI`, which is a different digest.
        let wide = "EcoRI+BamHI+HindIII+XhoI";
        let cases = vec![
            // The right edge, in the arrangement `pl gel` emits: ladder first,
            // the multi-enzyme lane outermost.
            vec![
                lane(&[500, 1_000, 3_000, 10_000], "ladder", true),
                lane(&[3_180, 1_120], wide, false),
            ],
            // And the left edge. `pl gel` cannot reach this — it always puts
            // the ladder in lane 0 — but `Lane` and `to_scene` are public API
            // and neither documents an ordering.
            vec![
                lane(&[3_180, 1_120], wide, false),
                lane(&[500, 1_000, 3_000, 10_000], "ladder", true),
            ],
        ];
        for lanes in cases {
            let sc = to_scene(&lanes, &Options::default(), "t");
            let (x, size, anchor) = sc
                .items
                .iter()
                .find_map(|i| match i {
                    Item::Text {
                        x,
                        size,
                        anchor,
                        text,
                        ..
                    } if text == wide => Some((*x, *size, *anchor)),
                    _ => None,
                })
                .expect("the lane label is drawn");
            let (l, r) = extent(x, size, anchor, wide);
            assert!(l >= -1e-9, "{wide:?} starts at {l}, off the left edge");
            assert!(
                r <= sc.width + 1e-9,
                "{wide:?} ends at {r}, past the {} pt edge",
                sc.width
            );
        }
    }

    #[test]
    fn a_sample_lane_beside_a_ladder_does_not_stack_two_band_labels() {
        // The gap between two lanes carries a sample lane's labels rightwards
        // and a ladder's leftwards, so a sample immediately left of a ladder
        // needs room for both columns; `layout` reserved the wider of the two.
        // Result on this fixture: `2000/2100` ran [109.02, 151.55] at y 207.85
        // and the ladder's `2000` ran [131.54, 151.55] at y 209.93 — both
        // ending at the same x, 2 pt apart at 9 pt type, so the two numbers
        // were printed one on top of the other and neither was legible.
        //
        // `a_band_label_does_not_sit_over_the_next_lane` cannot catch this: it
        // asks only whether a label crosses the next lane's left edge, and both
        // offenders stop 4 pt short of it.
        let lanes = vec![
            lane(&[2_000, 2_100], "a", false),
            lane(
                &[
                    500, 1_000, 1_500, 2_000, 3_000, 4_000, 5_000, 6_000, 8_000, 10_000,
                ],
                "L",
                true,
            ),
        ];
        let sc = to_scene(&lanes, &Options::default(), "t");
        // With exactly one lane of each kind the anchor identifies the lane:
        // sample band labels are set with `Anchor::Start`, ladder ones with
        // `Anchor::End`.
        let column = |want: Anchor| -> Vec<(f64, f64, f64, String)> {
            sc.items
                .iter()
                .filter_map(|i| match i {
                    Item::Text {
                        x,
                        y,
                        size,
                        anchor,
                        text,
                        ..
                    } if *size == BAND_LABEL_SIZE && *anchor == want => {
                        let (l, r) = extent(*x, *size, *anchor, text);
                        Some((l, r, *y, text.clone()))
                    }
                    _ => None,
                })
                .collect()
        };
        let sample = column(Anchor::Start);
        let ladder = column(Anchor::End);
        assert!(!sample.is_empty() && !ladder.is_empty(), "both kinds drawn");
        // The two columns must not share horizontal space at all. Report the
        // vertically closest offending pair, because that is the one whose two
        // numbers are actually printed over each other.
        let mut clash: Option<(f64, String)> = None;
        for (sl, sr, sy, st) in &sample {
            for (ll, lr, ly, lt) in &ladder {
                let shared = sr.min(*lr) - sl.max(*ll);
                if shared <= 1e-9 {
                    continue;
                }
                let dy = (sy - ly).abs();
                let closest = match &clash {
                    Some((best, _)) => dy < *best,
                    None => true,
                };
                if closest {
                    clash = Some((
                        dy,
                        format!(
                            "{st:?} [{sl}, {sr}] and {lt:?} [{ll}, {lr}] share {shared} pt of \
                             the same gap, {dy} pt apart vertically"
                        ),
                    ));
                }
            }
        }
        if let Some((_, why)) = clash {
            panic!("{why}");
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

    /// PROVEN TO FAIL at 78a46f2: `Options` had no `note` field, so no gel
    /// picture anywhere carried a calibration statement.
    ///
    /// `pl gel demo.gb --cut EcoRI --svg out.svg` printed the caveat to stdout
    /// and wrote an SVG with nothing of the kind in it. That file is what
    /// reaches a reader, and a modelled gel that does not say it is modelled is
    /// a picture somebody will size an unknown band off.
    #[test]
    fn a_caveat_reaches_the_picture_and_not_only_the_terminal() {
        let l = lane(&[2_000, 6_000], "EcoRI", false);
        let caveat = l.sim.caveat();
        let o = Options {
            note: Some(caveat.clone()),
            ..Default::default()
        };
        let sc = to_scene(&[l], &o, "t");
        let drawn = texts(&sc).join(" ");
        // Wrapped, so compare on the words rather than on the whole string.
        for word in caveat.split_whitespace() {
            assert!(drawn.contains(word), "{word:?} is missing from the picture");
        }
        assert!(
            drawn.contains("not good enough"),
            "the sentence that says what it cannot be used for: {drawn}"
        );

        // And the picture grew to hold it rather than drawing over the gel.
        let bare = to_scene(
            &[lane(&[2_000, 6_000], "EcoRI", false)],
            &Options::default(),
            "t",
        );
        assert!(sc.height > bare.height, "{} vs {}", sc.height, bare.height);
        // Every line of it is inside the viewBox: an SVG clips at its viewport,
        // so a caption past the edge is not close to the edge, it is gone.
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
            let (lo, hi) = extent(*x, *size, *anchor, text);
            assert!(lo >= -1e-9 && hi <= sc.width + 1e-9, "{text:?}: {lo}..{hi}");
        }
    }

    /// PROVEN TO FAIL before [`crate::MAX_LISTED`]: seven lanes of a genome
    /// digest produced a `Scene` 280,947 pt wide.
    ///
    /// A BbsI digest of E. coli K-12 makes 2,309 cuts, and single-linkage
    /// merging puts 1,769 of the fragments in ONE band. `label_of` joined every
    /// one of their sizes with `/` — 8,780 characters — and `layout` reserves
    /// half a band label at both margins and the whole of it in every
    /// inter-lane gap. `pl gel NC_000913.3.gb --cut BbsI --svg` came out
    /// `viewBox="0 0 80731.06 442"`; the GUI, which floors its fit scale so the
    /// 8.5 pt captions stay legible, painted the first lane 4,283 px into a
    /// 238,805 px canvas inside a 950 px pane, so almost every scrollbar
    /// position showed an empty dark field.
    ///
    /// The bound is stated rather than relative: every other test in this file
    /// uses a 3.2 kb plasmid, so none of them can see this.
    #[test]
    fn a_band_holding_a_thousand_fragments_does_not_widen_the_picture_off_the_screen() {
        // 1,769 sizes one base apart: adjacent ones are far closer than a band
        // width, so single linkage chains the whole run into one group — which
        // is exactly what a genome digest does.
        let sizes: Vec<u64> = (500..500 + 1_769).collect();
        let lanes = vec![
            lane(&[500, 1_000, 3_000, 10_000], "1kb ladder", true),
            lane(&sizes, "BbsI", false),
        ];
        let sim = &lanes[1].sim;
        assert_eq!(sim.groups.len(), 1, "the fixture must produce one band");
        assert_eq!(sim.groups[0].sizes.len(), 1_769);

        let o = Options {
            note: Some(sim.caveat()),
            ..Default::default()
        };
        let sc = to_scene(&lanes, &o, "t");
        assert!(
            sc.width < 1_200.0,
            "a two-lane gel came out {} pt wide",
            sc.width
        );
        let longest = texts(&sc).iter().map(|t| t.chars().count()).max().unwrap();
        assert!(
            longest < 200,
            "the longest text item is {longest} characters"
        );

        // AND THE COUNT IS STILL SAID. Capping the label must not turn 1,769
        // co-migrating fragments into a band that looks like one fragment.
        let drawn = texts(&sc).join(" | ");
        assert!(drawn.contains("1769 fragments"), "{drawn}");
        assert!(
            drawn.contains("500-2268"),
            "the span, not three of the sizes"
        );
    }

    /// The control: a band with four fragments in it still names all four.
    #[test]
    fn a_band_small_enough_to_read_is_still_named_fragment_by_fragment() {
        let g = Group {
            mm: 40.0,
            sizes: vec![2_000, 2_050, 2_100, 2_150],
        };
        assert_eq!(g.label(), "2000/2050/2100/2150");
        let mut five = g.sizes.clone();
        five.push(2_200);
        assert_eq!(
            crate::name_sizes(&five, "/"),
            "2000-2200 (5 fragments)",
            "one more, and it counts instead"
        );
    }

    /// The background is a fact about the picture, and a caller auditing its
    /// contrast must not have to re-type the hex.
    #[test]
    fn the_background_colour_is_the_one_the_picture_is_drawn_on() {
        for inverted in [true, false] {
            let o = Options {
                inverted,
                ..Default::default()
            };
            let sc = to_scene(&[lane(&[2_000], "a", false)], &o, "t");
            let first = match &sc.items[0] {
                Item::Path { fill, .. } => fill.clone().expect("the background is filled"),
                other => panic!("the first item is the background, got {other:?}"),
            };
            assert_eq!(first, o.background());
        }
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
