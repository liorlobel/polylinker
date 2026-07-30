//! Where everything *around* a plasmid ring goes: the reserve, the ruler's own
//! band, co-located cut sites, and the label ring itself.
//!
//! # Why this is here and not in the painter
//!
//! There were two independent layouts. `bins/pl-gui/src/map.rs` had
//! `LABEL_RESERVE = 132.0` and a greedy `label_slots`; [`crate::scene`] had a
//! measured margin and an exact isotonic packer. They shared only
//! [`crate::ranges`], so a label fix on the screen left `pl export`,
//! "Map SVG…" and "Map PDF…" untouched — and the exported figure is the one
//! that goes into a paper. The commit log's "PDF export: one Scene, two back
//! ends" is the precedent: put the *decision* in one place and let the painters
//! disagree only about ink.
//!
//! Everything here is arithmetic over `f64`. Nothing reads a font, a clock, a
//! locale or the filesystem, which is the crate-wide guarantee (see the module
//! doc on `lib.rs`) and also the reason a *width* is an argument rather than
//! something this module measures: egui hands over a galley width and the
//! exporters hand over [`crate::pdf::text_width_in`]. The two differ by a few
//! points, so screen and figure differ by a few points — which is a great deal
//! better than differing by the entire enzyme list.
//!
//! # The order the pieces are used in
//!
//! Both painters sequence them the same way, and the order matters:
//!
//! 1. [`reserve_for`] first, on the widest **unmerged** label that will land in
//!    a side column — a name in the twelve- or six-o'clock row costs vertical
//!    room, not radius — and then [`radius`], which spends the reserve on the
//!    horizontal axis and the row strip on the vertical one. Before merging,
//!    because merging may not buy itself radius: [`Site::label`] carries every
//!    name *and* every coordinate, so a folded label is always wider than either
//!    name in it.
//! 2. [`merge_sites`] next, at the radius that came out of 1, with a threshold
//!    from [`bases_per_arc`] — the tick's own stroke width, not a label height.
//!    A group is kept folded only where its label fits the room the individual
//!    names had already earned; where it does not, the sites stay separate and
//!    the packer moves them one line apart. Untidy and true, against an ellipsis
//!    through a merged label that can drop a whole enzyme name.
//! 3. [`centre_room`] to bound the lines written in the middle, then
//!    [`keep_clear_for`] over what is actually drawn, then [`inside_of`] to
//!    divide what is left of the inside between the inward feature lanes and the
//!    ruler. That order — geometry, then text — is load-bearing; run it the other
//!    way and a 69-character filename costs a genome its whole scale.
//! 4. [`place_ring`] last, once the radius is known, with every label — column
//!    and row alike — cut to [`label_room`]. One allowance, not one per run,
//!    because [`place_ring`] moves what a row cannot hold into a column.
//!
//! # This layer is outside the cross-implementation oracle
//!
//! `tests/agreement.rs` compares [`place_column`], [`crate::polar`],
//! [`crate::ranges`], [`crate::angle`], [`crate::nice_step`] and the string
//! helpers against `@polylinker/circular-map`'s TypeScript. It does **not** reach
//! anything in this file: the browser renderer has two columns and no L-ring, so
//! there is no second implementation of [`place_ring`], [`reserve_for`],
//! [`inside_of`] or [`merge_sites`] to disagree with. The primitives underneath
//! are covered and the arrangement built on them is not, and the guard on the
//! arrangement is the unit tests at the bottom of this file plus the frame tests
//! in `bins/pl-gui`. Said here so the next reader does not assume the oracle
//! covers the largest new piece of geometry in the project because it covers
//! everything around it.

use crate::labels::{place_column, LabelBox};
use crate::scene::Anchor;
use crate::{commas, TAU};

// ---------------------------------------------------------------------------
// the reserve
// ---------------------------------------------------------------------------

/// What [`reserve_for`] decided, and what a label actually gets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reserve {
    /// Distance to keep clear outside the backbone: `radius = pane/2 - reserve`.
    pub reserve: f64,
    /// How much of that the label text itself may use.
    ///
    /// This is the number the old constant got wrong. `map.rs` reserved a flat
    /// 132 pt and then spent 41 pt of it on the leader and 13 pt on every
    /// feature lane, so on a two-lane plasmid a label had 65 pt — 10.8
    /// characters — and `EcoRI 7,530` was drawn as `coRI 7,530`. A cut
    /// coordinate is a *wrong* coordinate, and the arithmetic that produced it
    /// has the opposite sign from the intuition: `room = reserve - outward`, so
    /// *reducing* the reserve to win back radius reduces the room as well.
    pub room: f64,
    /// The cap bound, so `room < widest` and the caller must shorten and say so.
    pub capped: bool,
}

/// Room to keep outside the backbone so the widest side-column label is whole.
///
/// `outward` is everything between the backbone and the first glyph of the
/// text: the feature lanes, the leader, and the gaps either side of it. The cap
/// at 30% of the smaller pane dimension is the one [`crate::scene`] has always
/// used, and it is what stops a single 60-character feature name from
/// collapsing the ring to the 40 pt floor.
pub fn reserve_for(widest: f64, outward: f64, pane_min: f64) -> Reserve {
    let want = widest.max(0.0) + outward.max(0.0);
    let cap = (pane_min * 0.30).max(0.0);
    let reserve = want.min(cap);
    Reserve {
        reserve,
        room: (reserve - outward).max(0.0),
        capped: want > cap,
    }
}

/// The radius, from the reserve and the vertical strip the rows need.
///
/// **Two axes, not one.** A side column spends *horizontal* room on the widest
/// name; the twelve- and six-o'clock rows spend a fixed *vertical* strip. The
/// obvious `pane_min / 2 - reserve` charges the widest name to both, which is
/// radius a pane wider than it is tall gives away for nothing: on
/// `pl export --width 1200 --height 600` it yields 173.6 against this rule's
/// 235, a quarter of the ring, while 347 pt of column room sits unused. `map.rs`
/// had this rule and [`crate::scene`] had the other one, so the shared layer was
/// half lifted and the publication path — the one a figure for a paper goes down
/// — kept the worse half.
///
/// The 40 pt floor is a token circle rather than nothing: a map that collapses
/// to zero looks like a renderer fault, and a small one looks like a small pane.
pub fn radius(pane_w: f64, pane_h: f64, reserve: f64, row_strip: f64) -> f64 {
    (pane_w * 0.5 - reserve)
        .min(pane_h * 0.5 - row_strip)
        .max(40.0)
}

// ---------------------------------------------------------------------------
// the inside of the ring
// ---------------------------------------------------------------------------

/// How the space inside the backbone is divided.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Inside {
    /// Radius of the ruler's numbers.
    pub ruler_text_r: f64,
    /// The ruler's tick mark, as `(inner, outer)` radii.
    pub ruler_tick: (f64, f64),
    /// How many inward feature lanes get a radius of their own.
    ///
    /// Never zero. A lane at or past this index is drawn at the last one
    /// instead: overprinted bands are ugly and finite, whereas the unclamped
    /// `r - band_w - lane * lane_step` went **negative** from lane 17 on a
    /// 218 pt ring, and `polar` then mirrored those arcs to the opposite side
    /// of the map, under the wrong coordinates, where hit-testing
    /// (`(d - base).abs() <= 7.5` against a negative `base`) could never reach
    /// them. Drawn and unreachable is worse than drawn twice.
    pub lanes_kept: usize,
    /// Whether the numbers fit at all. The tick marks are drawn either way.
    ///
    /// False when nothing inside the backbone clears `keep_clear`. **A caller
    /// that bounds its centre lines with [`centre_room`] and derives
    /// `keep_clear` with [`keep_clear_for`] can never see this**, which is the
    /// point of those two functions existing: the first thing this flag did in
    /// the field was cost a 4.6 Mb genome its whole scale, because
    /// `caption_of` left a 69-character filename as the caption, `keep_clear`
    /// came out at 265 and no radius on a 720 pt pane clears that. Dropping the
    /// annotation was the wrong end to give way at — the caption is the line
    /// with a hover and a `<title>` behind it, and the ruler is not.
    ///
    /// It stays as the last resort for a caller that passes its own
    /// `keep_clear`. Nothing is concealed by it: the tick marks stay, at every
    /// tenth of the molecule, so the scale is still drawn; it is the annotation
    /// on it that has nowhere to go, and a tick with no number makes no claim a
    /// reader could act on wrongly.
    pub numbers: bool,
}

/// The clearance a line written in the middle keeps from the ruler's numbers.
///
/// Held here rather than at the two call sites because [`centre_room`] has to
/// subtract exactly what [`keep_clear_for`] adds. When they disagreed by 3 pt the
/// numbers still vanished on a long caption, which is a defect that reads as the
/// bound not working rather than as an off-by-three.
pub const CENTRE_PAD: f64 = 6.0;

/// The radius the centre block occupies, from the width of its widest line.
///
/// The floor is there because the block is never nothing: `8,117 bp` alone still
/// wants the middle of the ring kept clear of feature bands.
///
/// `widest_number` is the widest ruler number the caller will draw, and it is
/// half of it that goes in — the number's **half-diagonal** rather than its
/// half-height. A number is about two and a half times as wide as it is tall, so
/// along a ray near the horizontal its box reaches inward well past its own
/// centre radius, and a centre line that cleared it radially still shares ink
/// with it: measured at 0.9 x 8.6 pt on NC_017320, the disclosure line against
/// `2,000`. It is the same trap the ruler's own hairline fell into one radius out.
pub fn keep_clear_for(widest_centre_line: f64, widest_number: f64) -> f64 {
    (widest_centre_line * 0.5).max(22.0) + CENTRE_PAD + widest_number.max(0.0) * 0.5
}

/// The widest a line written in the middle may be drawn.
///
/// **Elide the centre lines to this before deriving `keep_clear` from them.**
/// The dependency runs one way only: the geometry decides how much middle there
/// is, and then the text is cut to it. Computing `keep_clear` from an unbounded
/// caption and handing it to [`inside_of`] runs the dependency backwards, and
/// what gives way is whichever of the two the code happens to check second — it
/// was the ruler, on every `.dna` pulled from NCBI, which is precisely the
/// population the filename fallback exists to serve.
///
/// The caption has a hover in the app and a `<title>` in the SVG carrying the
/// whole string; the ruler has neither. That is the whole argument for cutting
/// this one and not that one.
pub fn centre_room(r: f64, band_w: f64, lane_step: f64, text_h: f64, widest_number: f64) -> f64 {
    // One lane, because that is the most middle the numbers could ever leave.
    let free = inside_of(r, band_w, lane_step, 1, text_h, 0.0).ruler_text_r;
    // Exactly what `keep_clear_for` adds, so the two cannot drift: when they
    // disagreed by 3 pt the numbers still vanished on a long caption, and that
    // reads as the bound not working rather than as an off-by-three.
    (2.0 * (free - text_h * 0.5 - CENTRE_PAD - widest_number.max(0.0) * 0.5)).max(0.0)
}

/// Give the ruler a radial band of its own, under every inward feature lane.
///
/// The "3,247" tick on the user's pKoV was unreadable because the ruler and the
/// reverse-strand lanes share radii and the features are painted second: the
/// number spans `r-21.5..r-11.5` and reverse lane 0 spans `r-15..r-3` when
/// emphasised. Which of the five labelled ticks breaks is decided by wherever
/// the features happen to fall, which is why exactly one of them was broken.
///
/// **Reordering the paint is the wrong fix** and was measured before being
/// rejected: drawing the ruler last puts `pal.muted` (`#849299`) on SacB's
/// `#993366` at 2.17:1, on CmR's `#ccffcc` at 2.86:1 and on f1 ori's `#ffff00`
/// at 2.98:1 — all under half of WCAG 2.2 AA — while `theme.rs`'s contrast test
/// measures `muted` against the *background* and therefore stays green. That
/// trades a legibility bug for an accessibility bug no gate can see. Separating
/// the radii makes the clearance a property of the geometry instead of a
/// property of which features a file happens to contain.
///
/// `keep_clear` is the radius the centre caption occupies — half its widest
/// line plus a little. It is not decoration: on a molecule with twenty mutually
/// overlapping reverse features the bands spiralled inward and painted over the
/// caption, and "8,117 bp" in `pal.muted` on the band beneath it measured
/// 2.9:1.
pub fn inside_of(
    r: f64,
    band_w: f64,
    lane_step: f64,
    inward_lanes: usize,
    text_h: f64,
    keep_clear: f64,
) -> Inside {
    // An emphasised band is 3 pt wider than a quiet one, and hover must not be
    // what decides whether the ruler is legible.
    let half = band_w * 0.5 + 1.5;
    let gap = 4.0;
    let tick = 5.0;
    // The radius the numbers take if `k` inward lanes each get their own.
    let text_at = |k: usize| {
        r - band_w - (k.max(1) - 1) as f64 * lane_step - half - gap - tick - 2.0 - text_h * 0.5
    };
    // Bounded by the outermost place the numbers could sit, because a caption
    // wider than the ring would otherwise push them *outside* the backbone and
    // into the label column — measured on the user's file at the app's own
    // 880 x 560 minimum, where "6,494" and "3,247" ended up beyond the ring with
    // the enzyme names. Dodging a centre line the ring cannot hold is not
    // possible; the caller shortens or drops that line instead.
    let floor = (keep_clear + text_h * 0.5).min(text_at(1).max(0.0));

    let want = inward_lanes.max(1);
    let mut kept = want;
    while kept > 1 && text_at(kept) < floor {
        kept -= 1;
    }
    // With one lane and still no room the pane is a token circle; put the
    // numbers where they least overlap and let the caller's own floor decide
    // the rest. Nothing here may return a negative radius: `polar` mirrors it.
    let text_r = text_at(kept).max(floor).max(0.0);
    Inside {
        ruler_text_r: text_r,
        ruler_tick: (
            text_r + text_h * 0.5 + 2.0,
            text_r + text_h * 0.5 + 2.0 + tick,
        ),
        numbers: text_r >= keep_clear + text_h * 0.5,
        lanes_kept: kept,
    }
}

/// The radius of inward feature lane `lane`, floored by [`Inside::lanes_kept`].
pub fn inward_radius(r: f64, band_w: f64, lane_step: f64, lane: usize, inside: &Inside) -> f64 {
    let lane = lane.min(inside.lanes_kept.saturating_sub(1));
    r - band_w - lane as f64 * lane_step
}

// ---------------------------------------------------------------------------
// co-located sites
// ---------------------------------------------------------------------------

/// One tick on the ring, and every enzyme that cuts within a label height of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// Every enzyme at this tick, in coordinate order.
    pub names: Vec<String>,
    /// Their cut positions, one per name, in the same order.
    pub positions: Vec<u64>,
}

impl Site {
    /// The base the tick and its leader point at.
    pub fn anchor(&self) -> u64 {
        self.positions.first().copied().unwrap_or(1)
    }

    /// The text drawn beside the tick.
    ///
    /// **Every name, and every coordinate.** The tidy form — `XmaI/SmaI  6,917`
    /// — is the change that most improves a screenshot and it is wrong at the
    /// bench: XmaI is `C^CCGGG`, a 4-nt 5' overhang, and SmaI is `CCC^GGG`,
    /// blunt. They are isoschizomers of the same recognition site cut at
    /// different bonds, which is the entire reason a cloner picks between them,
    /// and one coordinate for the pair shows the wrong one for SmaI. A reader
    /// plans a blunt ligation against a sticky end and the map looks calmer than
    /// it did before.
    ///
    /// A **range** — `SacI/KpnI/XmaI/SmaI/BamHI  281-292` — was the first
    /// attempt and is the same failure one step further out. Five names against
    /// two numbers: the mapping is not recoverable, and the two numbers printed
    /// are the ones belonging to the outermost pair, so KpnI's 287, XmaI's 287
    /// and SmaI's 289 appear nowhere on a figure that names all three. It also
    /// scaled the wrong way — the threshold was a label *height* in bases, so
    /// shrinking the window widened it, and pKoV folded sites 126 bp apart at a
    /// 704 pt pane. Resizing a window changed what the map claimed about the
    /// molecule.
    ///
    /// So: one coordinate when they genuinely share a base, and otherwise every
    /// name carrying its own. `XmaI  6,917 / SmaI  6,919` is wider than either
    /// name and narrower than two lines, and every input position is readable
    /// back out of it — which is the invariant
    /// `every_position_in_a_fold_is_readable_back_out_of_the_label` asserts.
    pub fn label(&self) -> String {
        let lo = self.positions.iter().copied().min().unwrap_or(1);
        let hi = self.positions.iter().copied().max().unwrap_or(1);
        if lo == hi {
            format!("{}  {}", self.names.join("/"), commas(lo))
        } else {
            self.names
                .iter()
                .zip(&self.positions)
                .map(|(n, p)| format!("{n}  {}", commas(*p)))
                .collect::<Vec<_>>()
                .join(" / ")
        }
    }
}

/// Fold sites whose ticks are the same tick, `within` bases of each other.
///
/// **`within` is a tick separation, not a label height.** Two sites are one tick
/// when the arc between their cut positions is under the tick's own stroke width
/// — see [`bases_per_arc`], which is how a caller turns that width into bases.
/// The first version asked instead whether their *labels* would collide in the
/// column, which conflates "these two names cannot both be written here" with
/// "these two cuts are in the same place". Those are different claims and only
/// the second one is about the molecule: the label-height rule grew as the ring
/// shrank — 10 bp at a maximised window, 28 at the default, 126 at 704 pt — so
/// pKoV folded `SphI/NsiI/BglII` across 128 bp, which is 21 pt of arc and
/// plainly three ticks, and NsiI's real 4,760 appeared nowhere. Colliding labels
/// are the packer's problem and it already has an answer for them: move them a
/// line apart.
///
/// On the user's pKoV this is about 2 bp and it catches SalI/XbaI at 6, SphI/NsiI
/// at 2 and XmaI/SmaI at 2 — the pairs that genuinely share a tick — and nothing
/// else, at any window size.
///
/// A group never spans more than `within` **from its own first member**, so a
/// long chain of sites cannot zip itself into one label the width of the pane.
///
/// Sites either side of the origin are deliberately **not** merged: the group
/// would have to claim a range that runs backwards, and two ordinary labels one
/// line apart tell no lie — they are only untidy.
pub fn merge_sites(sites: &[(String, u64)], within: u64) -> Vec<Site> {
    let mut order: Vec<usize> = (0..sites.len()).collect();
    order.sort_by_key(|&i| (sites[i].1, sites[i].0.clone()));

    let mut out: Vec<Site> = Vec::new();
    for &i in &order {
        let (name, pos) = (&sites[i].0, sites[i].1);
        match out.last_mut() {
            Some(g) if pos.saturating_sub(g.anchor()) <= within => {
                g.names.push(name.clone());
                g.positions.push(pos);
            }
            _ => out.push(Site {
                names: vec![name.clone()],
                positions: vec![pos],
            }),
        }
    }
    out
}

/// How many bases an arc of `arc` points subtends at radius `tick_r`.
///
/// The threshold [`merge_sites`] wants, in the units a cut position is in. Pass
/// the width of the tick's own stroke: two cuts closer than that are drawn as
/// one mark whatever anyone would prefer, so calling them one tick states a fact
/// about the picture. Pass a *label* height instead and the threshold grows as
/// the ring shrinks, which is how a window resize came to change what the map
/// claimed about the molecule.
///
/// At least 1, so a fold always means "the same mark" and never "the same base".
pub fn bases_per_arc(arc: f64, tick_r: f64, span: u64) -> u64 {
    if tick_r <= 0.0 || span == 0 {
        return 1;
    }
    let frac = (arc / tick_r) / TAU;
    ((frac * span as f64).floor() as u64).max(1)
}

// ---------------------------------------------------------------------------
// what the ring is not showing
// ---------------------------------------------------------------------------

/// The counts behind the line that says what a map is *not* showing.
///
/// One wording and one piece of arithmetic for both painters. The screen had it
/// and the figure had nothing — not in the SVG, not in the PDF, not on `pl
/// export`'s stderr — while [`crate::Options::sites`] defaults to unique cutters,
/// so every default export dropped a plasmid's dual and multi cutters in silence.
/// `docs/PLAN.md` item 33 calls a silent filter "the one documented case of this
/// software category costing a user a month of bench time", and of the two
/// artefacts the figure is the one that leaves the machine.
///
/// # Enzymes, never labels
///
/// [`Disclosure::labelled`] counts **enzymes**. Counting labels is the mistake
/// the first version made, and it is invisible until a tick folds: pET28a said
/// `14 of 31 cutters labelled · 7 dual, 1 multi not drawn` when 23 unique cutters
/// were on the map in 14 labels — understating by nine, and 14 + 7 + 1 = 22
/// against a stated 31, telling the reader nine enzymes were unaccounted for when
/// none were. [`Disclosure::closes`] is that arithmetic as a question, and it is
/// asserted in the tests rather than trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Disclosure {
    /// Enzymes cutting the molecule at least once.
    pub cutters: usize,
    /// Enzymes named somewhere on the map.
    pub labelled: usize,
    /// Enzymes cutting exactly twice, excluded by the filter.
    pub dual: usize,
    /// Enzymes cutting more than twice, excluded by the filter.
    pub multi: usize,
    /// Enzymes the filter admitted that the ring then could not fit.
    pub hidden: usize,
    /// Labels drawn with their text cut short.
    pub shortened: usize,
}

impl Disclosure {
    /// Whether every cutting enzyme is accounted for by exactly one of the four
    /// buckets. A line that fails this is worse than no line: it tells the reader
    /// a number of enzymes went missing that did not.
    pub fn closes(&self) -> bool {
        self.labelled + self.hidden + self.dual + self.multi == self.cutters
    }

    /// The full sentence.
    pub fn long(&self) -> String {
        let mut s = format!("{} of {} cutters labelled", self.labelled, self.cutters);
        if self.dual + self.multi > 0 {
            s.push_str(&format!(
                " · {} dual, {} multi not drawn",
                self.dual, self.multi
            ));
        }
        if self.hidden > 0 {
            s.push_str(&format!(" · {} would not fit", self.hidden));
        }
        if self.shortened > 0 {
            s.push_str(&format!(" · {} shortened", self.shortened));
        }
        s
    }

    /// The form for a ring too narrow to hold [`Disclosure::long`].
    pub fn short(&self) -> String {
        let mut s = format!("{}/{} cutters", self.labelled, self.cutters);
        if self.hidden > 0 {
            s.push_str(&format!(" · {} hidden", self.hidden));
        }
        if self.shortened > 0 {
            s.push_str(&format!(" · {} short", self.shortened));
        }
        s
    }

    /// The last form before the line goes altogether: the fraction alone.
    ///
    /// At the desktop app's own 880 x 560 minimum the ring comes out at 72 pt and
    /// the middle holds about 36 pt of text — not enough for [`Disclosure::short`],
    /// and what was on screen then was a map with every coordinate dropped and
    /// nothing saying a filter had been applied at all. `22/40` fits, and a reader
    /// who can see that 18 enzymes are unaccounted for knows to look at the
    /// Enzymes tab. It does not mention shortening: three characters of "short"
    /// would cost the fraction, and the fraction is the part that says a filter
    /// exists.
    ///
    /// Never an ellipsis anywhere in these three. Half a count is a number a
    /// reader would act on.
    pub fn tiny(&self) -> String {
        format!("{}/{}", self.labelled, self.cutters)
    }
}

// ---------------------------------------------------------------------------
// the label ring
// ---------------------------------------------------------------------------

/// Which of the four runs a label was put in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

/// A label wanting to point at one angle on the ring.
#[derive(Debug, Clone, Copy)]
pub struct RingLabel {
    /// Clockwise from twelve o'clock, radians — [`crate::angle`]'s convention.
    ///
    /// egui's map works in `-PI/2 + frac * TAU` with `x = cos`, `y = sin`;
    /// that is this angle less a quarter turn, so `map.rs` adds `FRAC_PI_2` on
    /// the way in and gets screen coordinates straight back out.
    pub angle: f64,
    /// The drawn width of the text, measured by the caller in its own font.
    pub width: f64,
    /// The drawn height of one line.
    pub height: f64,
    /// Resistance to displacement. Heavier labels hold their position.
    pub weight: f64,
}

/// The frame the labels are placed in.
#[derive(Debug, Clone, Copy)]
pub struct RingGeom {
    pub cx: f64,
    pub cy: f64,
    /// Radius the leaders leave the ring at: outside every feature band.
    pub tick_r: f64,
    /// Distance from `tick_r` out to the columns and rows.
    pub gap: f64,
    /// Half-width of the twelve- and six-o'clock rows, in radians.
    ///
    /// 30 degrees, measured. On pKoV it takes the longest leader from 249 pt to
    /// 144 and the median from 68 to 34, and the two horizontal rows come out
    /// 63% full so they need no dropping of their own. 40 degrees looks better
    /// on paper and fills them to 97%: one longer enzyme name and they overflow.
    pub row_half: f64,
    /// Gutter between two labels sharing a row.
    pub row_gap: f64,
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

/// Where one label and its leader ended up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingPlacement {
    pub side: Side,
    /// Where the leader leaves the ring — the label's own tick, and nobody
    /// else's.
    pub tip: (f64, f64),
    /// Where the leader bends, just outside the ring.
    pub bend: (f64, f64),
    /// The text's anchor point.
    pub at: (f64, f64),
    pub anchor: Anchor,
}

/// What [`place_ring`] managed to place.
#[derive(Debug, Clone, Default)]
pub struct Ring {
    /// One entry per input, in input order. `None` for a label with nowhere
    /// to go.
    pub placed: Vec<Option<RingPlacement>>,
    /// Indices dropped because a run could not hold them all, lightest first.
    pub dropped: Vec<usize>,
}

/// Which run a label at this angle belongs to.
pub fn side_of(angle: f64, row_half: f64) -> Side {
    let a = angle.rem_euclid(TAU);
    let half = row_half.clamp(0.0, TAU / 8.0);
    if a <= half || a >= TAU - half {
        Side::Top
    } else if (a - TAU / 2.0).abs() <= half {
        Side::Bottom
    } else if a < TAU / 2.0 {
        Side::Right
    } else {
        Side::Left
    }
}

/// How far the rows may reach either side of centre: to the columns' inner edge.
///
/// The rows used to be given the whole canvas, and it cost two things at once.
///
/// **Overlap.** The four runs are packed independently, so nothing bounded a
/// row's `x` against a column's `x`. On `pl export J01749.gb` at the default
/// 720 x 720, `misc_binding` in the top row landed across `BamHI  376` at the
/// top of the right-hand column and typeset as `misc_bindiRgmHlis3f6inding` —
/// the cut coordinate destroyed, and neither `labels_hidden` nor
/// `labels_truncated` said a word, because from each run's own point of view
/// nothing had gone wrong. Twenty files exported clean at e087e27 and two did
/// not after the L-ring landed.
///
/// **Long leaders, which is what the L-ring was for.** A row member's tick is
/// within `tick_r * sin(row_half)` of centre by construction, so a row that
/// reaches the canvas edge produces exactly the near-horizontal run the two
/// columns produced: 290 pt on pGhost9ISS1 and 243 on NC_017320, against the
/// 241 pt leader on pKoV that started this. Bounded here, the horizontal run is
/// at most `gap + tick_r(1 - sin(row_half))`, which is 86 pt on pKoV.
///
/// The capacity this gives up is handed back by [`place_ring`]'s spill, so the
/// bound costs labels nothing — only their run.
fn row_span(g: &RingGeom) -> (f64, f64) {
    let inner = g.tick_r + g.gap;
    ((g.cx - inner).max(g.left), (g.cx + inner).min(g.right))
}

/// The widest any label may be drawn — one number for all four runs.
///
/// Two bounds, and a label has to satisfy both because it can end up in any run:
///
/// * a column's room, from its anchor at `cx ± (tick_r + gap)` out to the pane
///   edge `pane_half` from centre. That edge is where the `viewBox`, the
///   `/MediaBox`, the `%%BoundingBox` and egui's clip rect all crop, so a label
///   wider than this is not shortened, it is *cut*, by the typesetter and in
///   silence. `pane_half` is passed rather than read off [`RingGeom::right`]
///   because `right` carries the runs' own padding and a column's last glyph is
///   allowed to reach the edge — charging it the padding shortened
///   `AmpR-promoter` on a plasmid where it had always fitted.
/// * half a row, because a run holding one label that fills it is a run holding
///   one label. On stock pET28a that is exactly what happened — the
///   137-character `Multiple Cloning Site (MCS); contains unique sites for ...`
///   typeset 641 pt wide, took 651 of the top row's 704, and evicted all nine of
///   the MCS enzyme labels whose coordinates it was describing.
///
/// **One number, not one per run**, because [`place_ring`] moves what a row
/// cannot hold into a column. Measuring a label against the row it started in
/// and then drawing it in a column with less room is how `sacB promoter` ran 4 pt
/// off a 340 pt canvas with `Report::labels_truncated` empty — the same
/// deciding-in-one-unit-drawing-in-another shape as the estimate-versus-Helvetica
/// defect, one axis over.
///
/// A name this cuts keeps its whole text in the SVG `<title>`, in the PDF's own
/// annotation and in the app's Features tab.
pub fn label_room(g: &RingGeom, pane_half: f64) -> f64 {
    let (lo, hi) = row_span(g);
    (pane_half - (g.tick_r + g.gap))
        .min((hi - lo) * 0.5 - g.row_gap)
        .max(0.0)
}

/// Place labels in an L-shaped ring: two columns and two rows.
///
/// One column per side was the shipped answer and it is what produces the
/// user's complaint. The leader from a label at twelve o'clock has to run
/// `22 + tick_r(1 - |cos a|)` horizontally to reach its column — 249 pt at
/// 1.4 degrees off horizontal on pKoV — because `lx` is pinned to
/// `cx ± (tick_r + 22)` whatever the angle. Rebalancing the two columns makes
/// that *worse*, not better: BbsI's tick is 31 pt left of centre, so moving it
/// to the right-hand column lengthens its leader to 312 pt and drags it across
/// the top of the ring. The count is the thing to reduce, and the axis is the
/// thing to change.
///
/// Columns pack in `y` and rows pack in `x`, both through [`place_column`], so
/// the ordering within a run is the ordering of the ideal positions and is
/// therefore identical for two callers measuring text in different fonts.
///
/// # The rows go first, and what they cannot hold spills into a column
///
/// [`row_span`] bounds a row to the columns' inner edge, which is what stops a
/// row label being written across the top of a column. On a molecule with a
/// polylinker that bound is binding: twelve of pET28a's cutters fall within five
/// degrees of each other, and no horizontal run 344 pt wide holds twelve names.
/// So a label a row drops is re-offered to the column on the side its own tick
/// is on — `sin(angle) >= 0` is the right-hand half — and the columns are packed
/// afterwards, holding their own members and the spill together.
///
/// That ordering is what makes the bound free. Without the spill, bounding the
/// rows traded one defect for another: no overlap, and eight fewer cutters on a
/// pET28a figure. With it, the L-ring is strictly better than the two columns it
/// replaced rather than better on one file and worse on another — a spilled
/// label is exactly where e087e27 would have put it, and everything else is
/// nearer its tick.
pub fn place_ring(labels: &[RingLabel], g: &RingGeom) -> Ring {
    let mut out = Ring {
        placed: vec![None; labels.len()],
        dropped: Vec::new(),
    };
    if labels.is_empty() {
        return out;
    }

    let at_of = |i: usize, side: Side, v: f64| -> RingPlacement {
        let a = labels[i].angle;
        let bend_r = g.tick_r + g.gap * 0.35;
        let (at, anchor) = match side {
            Side::Right => ((g.cx + g.tick_r + g.gap, v), Anchor::Start),
            Side::Left => ((g.cx - g.tick_r - g.gap, v), Anchor::End),
            Side::Top => ((v, g.cy - g.tick_r - g.gap), Anchor::Middle),
            Side::Bottom => ((v, g.cy + g.tick_r + g.gap), Anchor::Middle),
        };
        RingPlacement {
            side,
            tip: (g.cx + g.tick_r * a.sin(), g.cy - g.tick_r * a.cos()),
            bend: (g.cx + bend_r * a.sin(), g.cy - bend_r * a.cos()),
            at,
            anchor,
        }
    };

    let mut side: Vec<Side> = labels
        .iter()
        .map(|l| side_of(l.angle, g.row_half))
        .collect();

    // The rows first, so a label a row cannot hold is still in play.
    let (row_lo, row_hi) = row_span(g);
    for row in [Side::Top, Side::Bottom] {
        let idx: Vec<usize> = (0..labels.len()).filter(|&i| side[i] == row).collect();
        if idx.is_empty() {
            continue;
        }
        let boxes: Vec<LabelBox> = idx
            .iter()
            .map(|&i| LabelBox {
                ideal: g.cx + g.tick_r * labels[i].angle.sin(),
                height: labels[i].width + g.row_gap,
                weight: labels[i].weight,
            })
            .collect();
        let placed = place_column(&boxes, row_lo, row_hi);
        for (k, &i) in idx.iter().enumerate() {
            match placed.positions[k] {
                Some(v) => out.placed[i] = Some(at_of(i, row, v)),
                // The side its own tick is on, so the leader still runs the
                // short way. Twelve o'clock exactly goes right, arbitrarily but
                // deterministically: two painters must agree.
                None => {
                    side[i] = if labels[i].angle.sin() >= 0.0 {
                        Side::Right
                    } else {
                        Side::Left
                    }
                }
            }
        }
    }

    for col in [Side::Right, Side::Left] {
        let idx: Vec<usize> = (0..labels.len())
            .filter(|&i| side[i] == col && out.placed[i].is_none())
            .collect();
        if idx.is_empty() {
            continue;
        }
        let boxes: Vec<LabelBox> = idx
            .iter()
            .map(|&i| LabelBox {
                ideal: g.cy - g.tick_r * labels[i].angle.cos(),
                height: labels[i].height,
                weight: labels[i].weight,
            })
            .collect();
        let placed = place_column(&boxes, g.top, g.bottom);
        for d in &placed.dropped {
            out.dropped.push(idx[*d]);
        }
        for (k, &i) in idx.iter().enumerate() {
            if let Some(v) = placed.positions[k] {
                out.placed[i] = Some(at_of(i, col, v));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_room_a_label_gets_is_the_reserve_less_what_the_leader_spends() {
        // The arithmetic `LABEL_RESERVE = 132.0` did not do. Two feature lanes
        // and a 41 pt leader leave 65 pt, which is 10.8 characters at the map's
        // 6 pt monospace advance -- `EcoRI 7,530` is 12 and came out `coRI`.
        let r = reserve_for(84.0, 41.0 + 13.0 * 2.0, 700.0);
        assert!(!r.capped);
        assert_eq!(r.room, 84.0, "the widest name, whole");
        assert_eq!(r.reserve, 151.0);
    }

    #[test]
    fn a_pathological_name_is_capped_rather_than_allowed_to_eat_the_ring() {
        let r = reserve_for(600.0, 67.0, 700.0);
        assert!(r.capped);
        assert_eq!(r.reserve, 210.0, "30% of the pane");
        assert!(r.room < 600.0, "so the caller must shorten and say so");
    }

    #[test]
    fn reducing_the_reserve_reduces_the_room_which_is_the_trap() {
        // The sign nobody expects: a smaller reserve buys radius and *costs*
        // legibility. Worth an assertion because the intuition is the reverse.
        let wide = reserve_for(84.0, 67.0, 700.0);
        let narrow = reserve_for(40.0, 67.0, 700.0);
        assert!(narrow.reserve < wide.reserve);
        assert!(narrow.room < wide.room);
    }

    #[test]
    fn the_ruler_sits_clear_of_every_inward_lane() {
        // The pKoV geometry: one inward lane, 9 pt bands, 13 pt pitch.
        let inside = inside_of(199.0, 9.0, 13.0, 1, 9.0, 110.0);
        let lane0 = inward_radius(199.0, 9.0, 13.0, 0, &inside);
        // The band's inner edge, at its emphasised width.
        let edge = lane0 - 9.0 * 0.5 - 1.5;
        assert!(
            inside.ruler_tick.1 < edge,
            "ruler reaches {} against a band edge at {edge}",
            inside.ruler_tick.1
        );
        assert!(inside.ruler_text_r + 4.5 < edge);
    }

    #[test]
    fn a_deep_inward_stack_is_floored_instead_of_reaching_the_caption() {
        // Twenty mutually overlapping reverse features. Unclamped, lane 17 put
        // the radius at -12 and `polar` mirrored those arcs to the opposite
        // side of the map, where nothing could hover them.
        let inside = inside_of(199.0, 9.0, 13.0, 20, 9.0, 110.0);
        assert!(inside.lanes_kept >= 1);
        assert!(inside.lanes_kept < 20, "something had to give");
        for lane in 0..20 {
            let rr = inward_radius(199.0, 9.0, 13.0, lane, &inside);
            assert!(rr > 0.0, "lane {lane} at radius {rr}");
            assert!(
                rr - 9.0 * 0.5 - 1.5 > inside.ruler_tick.1,
                "lane {lane} reaches the ruler"
            );
        }
        assert!(
            inside.ruler_text_r - 4.5 >= 110.0,
            "and clear of the caption"
        );
    }

    #[test]
    fn two_enzymes_at_one_tick_keep_both_names_and_both_coordinates() {
        let sites = vec![
            ("XmaI".to_string(), 6_917),
            ("SmaI".to_string(), 6_919),
            ("EcoRI".to_string(), 7_530),
        ];
        let m = merge_sites(&sites, 3);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].names, vec!["XmaI", "SmaI"]);
        assert_eq!(m[0].label(), "XmaI  6,917 / SmaI  6,919");
        assert_eq!(m[1].label(), "EcoRI  7,530");
    }

    #[test]
    fn enzymes_that_cut_the_same_base_share_one_coordinate() {
        // BspQI and SapI are the same recognition site and the same bond, so one
        // number is the whole truth and repeating it reads as two cuts.
        let m = merge_sites(
            &[("BspQI".to_string(), 2_639), ("SapI".to_string(), 2_639)],
            3,
        );
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].label(), "BspQI/SapI  2,639");
    }

    #[test]
    fn every_position_in_a_fold_is_readable_back_out_of_the_label() {
        // The invariant a *range* broke. `SacI/KpnI/XmaI/SmaI/BamHI  281-292`
        // names five enzymes and prints two numbers, so three of the five cut
        // positions appear nowhere on a figure that claims to show them.
        let groups: Vec<Vec<(String, u64)>> = vec![
            vec![("XmaI".into(), 6_917), ("SmaI".into(), 6_919)],
            vec![
                ("SacI".into(), 281),
                ("KpnI".into(), 287),
                ("XmaI".into(), 287),
                ("SmaI".into(), 289),
                ("BamHI".into(), 292),
            ],
            vec![("BspQI".into(), 2_639), ("SapI".into(), 2_639)],
        ];
        for g in &groups {
            let span =
                g.iter().map(|(_, p)| *p).max().unwrap() - g.iter().map(|(_, p)| *p).min().unwrap();
            for s in merge_sites(g, span + 1) {
                let text = s.label();
                for (name, pos) in s.names.iter().zip(&s.positions) {
                    assert!(text.contains(name), "{text} does not name {name}");
                    assert!(
                        text.contains(&commas(*pos)),
                        "{text} does not carry {name}'s own {}",
                        commas(*pos)
                    );
                }
            }
        }
    }

    #[test]
    fn a_chain_of_sites_cannot_zip_itself_into_one_label() {
        // Each 60 bp from the last, but 300 from the first. Chaining on the
        // previous member instead of the group's first would make one label
        // out of all six and claim a 300 bp range for one tick.
        let sites: Vec<(String, u64)> = (0..6).map(|i| (format!("E{i}"), 1_000 + i * 60)).collect();
        let m = merge_sites(&sites, 65);
        assert!(
            m.len() >= 3,
            "{:?}",
            m.iter().map(|s| s.label()).collect::<Vec<_>>()
        );
        for s in &m {
            let lo = s.positions.iter().min().unwrap();
            let hi = s.positions.iter().max().unwrap();
            assert!(hi - lo <= 65, "{} spans more than one label", s.label());
        }
    }

    #[test]
    fn one_enzyme_still_gets_one_coordinate() {
        let m = merge_sites(&[("HindIII".to_string(), 2_059)], 65);
        assert_eq!(m[0].label(), "HindIII  2,059");
    }

    #[test]
    fn a_fold_never_claims_more_arc_than_the_mark_that_draws_it() {
        // The scaling defect, as the invariant that actually holds.
        //
        // The threshold *does* grow as the ring shrinks, and that is correct: at
        // a smaller radius the same 1.5 pt of stroke genuinely covers more bases,
        // so more cuts genuinely are one mark. What must not grow is the arc the
        // fold claims, and that is what a *label*-height threshold got wrong —
        // it claimed a label's worth of arc, eight times the mark, which is how
        // `SphI/NsiI/BglII` came to span 128 bp on the user's own plasmid at a
        // 704 pt pane with NsiI's real 4,760 printed nowhere.
        let span = 8_117;
        for tick_r in [88.0, 140.0, 226.0, 520.0] {
            let bases = bases_per_arc(1.5, tick_r, span);
            // What those bases subtend, back in points.
            let arc = bases as f64 / span as f64 * TAU * tick_r;
            assert!(
                arc <= 1.5 + 1e-9,
                "r={tick_r}: folding {bases} bp claims {arc} pt of arc for a 1.5 pt mark"
            );
            let by_label = bases_per_arc(13.0, tick_r, span);
            assert!(
                by_label >= bases * 5,
                "r={tick_r}: the label rule gave {by_label} against the mark's {bases}"
            );
        }
    }

    #[test]
    fn the_four_runs_are_split_at_thirty_degrees() {
        let h = 30f64.to_radians();
        assert_eq!(side_of(0.0, h), Side::Top);
        assert_eq!(side_of(29f64.to_radians(), h), Side::Top);
        assert_eq!(side_of((360.0f64 - 29.0).to_radians(), h), Side::Top);
        assert_eq!(side_of(31f64.to_radians(), h), Side::Right);
        assert_eq!(side_of(149f64.to_radians(), h), Side::Right);
        assert_eq!(side_of(151f64.to_radians(), h), Side::Bottom);
        assert_eq!(side_of(180f64.to_radians(), h), Side::Bottom);
        assert_eq!(side_of(209f64.to_radians(), h), Side::Bottom);
        assert_eq!(side_of(211f64.to_radians(), h), Side::Left);
        assert_eq!(side_of(329f64.to_radians(), h), Side::Left);
    }

    fn geom() -> RingGeom {
        RingGeom {
            cx: 350.0,
            cy: 400.0,
            tick_r: 210.0,
            gap: 22.0,
            row_half: 30f64.to_radians(),
            row_gap: 10.0,
            left: 6.0,
            right: 694.0,
            top: 12.0,
            bottom: 788.0,
        }
    }

    /// Every placed label's drawn box, so overlap can be asked ACROSS runs and
    /// not only within one.
    ///
    /// The only overlap test this module had was named
    /// `no_two_labels_in_A_RUN_overlap`, so the corner where two runs meet was
    /// untested by construction — and that corner is exactly where
    /// `misc_bindiRgmHlis3f6inding` came from.
    fn rects(labels: &[RingLabel], ring: &Ring) -> Vec<(usize, [f64; 4])> {
        ring.placed
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                p.map(|p| {
                    let (w, h) = (labels[i].width, labels[i].height);
                    let x0 = match p.anchor {
                        Anchor::Start => p.at.0,
                        Anchor::End => p.at.0 - w,
                        Anchor::Middle => p.at.0 - w / 2.0,
                    };
                    (i, [x0, x0 + w, p.at.1 - h / 2.0, p.at.1 + h / 2.0])
                })
            })
            .collect()
    }

    fn overlapping(labels: &[RingLabel], ring: &Ring) -> Vec<(usize, usize)> {
        let rs = rects(labels, ring);
        let mut out = Vec::new();
        for a in 0..rs.len() {
            for b in a + 1..rs.len() {
                let (p, q) = (rs[a].1, rs[b].1);
                if p[1].min(q[1]) - p[0].max(q[0]) > 0.5 && p[3].min(q[3]) - p[2].max(q[2]) > 0.5 {
                    out.push((rs[a].0, rs[b].0));
                }
            }
        }
        out
    }

    #[test]
    fn no_two_labels_in_a_run_overlap_and_each_leader_ends_at_its_own_tick() {
        let labels: Vec<RingLabel> = (0..22)
            .map(|i| RingLabel {
                angle: (i as f64 * 13.0).to_radians(),
                width: 70.0,
                height: 13.0,
                weight: 1.0,
            })
            .collect();
        let ring = place_ring(&labels, &geom());
        assert!(ring.dropped.is_empty());
        for (i, p) in ring.placed.iter().enumerate() {
            let p = p.expect("placed");
            let a = labels[i].angle;
            let want = (350.0 + 210.0 * a.sin(), 400.0 - 210.0 * a.cos());
            assert!(
                (p.tip.0 - want.0).abs() < 1e-9 && (p.tip.1 - want.1).abs() < 1e-9,
                "label {i} points at {:?}, not its own tick {want:?}",
                p.tip
            );
        }
        for side in [Side::Top, Side::Right, Side::Bottom, Side::Left] {
            let mut run: Vec<(f64, f64)> = ring
                .placed
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    p.filter(|p| p.side == side).map(|p| {
                        if matches!(side, Side::Left | Side::Right) {
                            (p.at.1, labels[i].height)
                        } else {
                            (p.at.0, labels[i].width + 10.0)
                        }
                    })
                })
                .collect();
            run.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            for w in run.windows(2) {
                let need = (w[0].1 + w[1].1) / 2.0;
                assert!(
                    w[1].0 - w[0].0 >= need - 1e-6,
                    "{side:?}: {} and {} are {} apart, needing {need}",
                    w[0].0,
                    w[1].0,
                    w[1].0 - w[0].0
                );
            }
        }
    }

    #[test]
    fn a_row_label_is_never_written_across_a_column() {
        // The J01749 shape: several wide row labels and a full side column at a
        // small ring. Each run's own packer is content — the collision exists
        // only between them, which is why the one overlap test this module had,
        // `no_two_labels_in_A_RUN_overlap`, could not see it. On that file
        // `misc_binding` typeset across `BamHI  376` as
        // `misc_bindiRgmHlis3f6inding`, destroying the cut coordinate, with both
        // `labels_hidden` and `labels_truncated` empty.
        let g = RingGeom {
            tick_r: 146.0,
            ..geom()
        };
        // Every width at the allowance a real caller cuts to, so this is data the
        // painters could actually produce. Unbounded rows spread four of these
        // across the whole canvas and the rightmost lands on the column.
        let wide = label_room(&g, 350.0);
        let mut labels: Vec<RingLabel> = (0..4)
            .map(|i| RingLabel {
                angle: (i as f64 * 7.0 - 10.0).to_radians(),
                width: wide,
                height: 12.0,
                weight: 9.0,
            })
            .collect();
        labels.extend((0..26).map(|i| RingLabel {
            angle: (33.0 + i as f64 * 4.0).to_radians(),
            width: 90.0,
            height: 12.0,
            weight: 1.0,
        }));
        let ring = place_ring(&labels, &g);
        let bad = overlapping(&labels, &ring);
        assert!(bad.is_empty(), "{bad:?} overlap across runs");
        for (i, r) in rects(&labels, &ring) {
            assert!(
                r[0] >= g.left - 1e-6 && r[1] <= g.right + 1e-6,
                "label {i} spans {}..{} outside {}..{}",
                r[0],
                r[1],
                g.left,
                g.right
            );
        }
        // And the bound bit: not all four of those rows fit, so the spill ran.
        assert!(ring
            .placed
            .iter()
            .take(4)
            .any(|p| p.is_some_and(|p| p.side != Side::Top)));
    }

    #[test]
    fn a_row_that_overflows_spills_into_the_column_its_own_tick_is_on() {
        // pET28a's polylinker: twelve cutters inside five degrees of each other.
        // No horizontal run holds twelve names, and dropping eight of them was
        // the price of bounding the row until the spill existed.
        let labels: Vec<RingLabel> = (0..12)
            .map(|i| RingLabel {
                angle: (22.0 + i as f64 * 0.45).to_radians(),
                width: 62.0,
                height: 13.0,
                weight: 1.0,
            })
            .collect();
        let g = RingGeom {
            tick_r: 146.0,
            ..geom()
        };
        let ring = place_ring(&labels, &g);
        assert!(
            ring.dropped.is_empty(),
            "{} of 12 cutters dropped; the column had room",
            ring.dropped.len()
        );
        let rows = ring
            .placed
            .iter()
            .filter(|p| p.is_some_and(|p| p.side == Side::Top))
            .count();
        assert!((1..12).contains(&rows), "{rows} in the row");
        // Every spilled one went RIGHT, because every tick is right of centre.
        assert!(ring
            .placed
            .iter()
            .all(|p| p.is_some_and(|p| matches!(p.side, Side::Top | Side::Right))));
        assert!(overlapping(&labels, &ring).is_empty());
    }

    #[test]
    fn a_wide_pane_is_not_charged_the_widest_name_twice() {
        // `pl export --width 1200 --height 600`: 600 pt of horizontal half-width
        // against a 126 pt column reserve, and `pane_min / 2 - reserve` handed
        // back 173.6.
        let r = reserve_for(92.0, 34.0, 600.0);
        let two = radius(1200.0, 600.0, r.reserve, 65.0);
        let one = (600.0 / 2.0 - r.reserve).max(40.0);
        assert!(two > one + 50.0, "two-axis {two} against one-axis {one}");
        // And the square default is unaffected, so no existing figure moves.
        assert_eq!(
            radius(720.0, 720.0, r.reserve, 65.0),
            360.0 - r.reserve,
            "the height binds on a square pane"
        );
    }

    #[test]
    fn a_long_caption_never_costs_the_ruler_its_numbers() {
        // The 4.6 Mb genome: `caption_of` leaves a 69-character filename, which
        // typesets about 517 pt wide, and `keep_clear` from that was 265 against
        // a ruler radius of 169. The numbers vanished from the whole map.
        for r in [88.0, 140.0, 200.0, 320.0] {
            for lanes in [1usize, 2, 6, 20] {
                let room = centre_room(r, 9.0, 13.0, 9.0, 24.0);
                assert!(room > 0.0, "r={r} leaves no middle at all");
                // What the caller may draw, at its widest.
                let inside = inside_of(r, 9.0, 13.0, lanes, 9.0, keep_clear_for(room, 24.0));
                assert!(
                    inside.numbers,
                    "r={r} lanes={lanes}: a caption elided to {room} still cost the numbers"
                );
                // And the number's own box clears the centre block, which asking
                // radially about its half-height does not establish: `2,000` and
                // the disclosure line shared 9 pt of ink on NC_017320 that way.
                let reach = inside.ruler_text_r - (12.0f64 * 12.0 + 4.5 * 4.5).sqrt();
                assert!(
                    reach >= room * 0.5,
                    "r={r} lanes={lanes}: a number reaches in to {reach} against a {} half-line",
                    room * 0.5
                );
            }
        }
        // And an unbounded caption is what used to do it, so the guard is real.
        assert!(!inside_of(200.0, 9.0, 13.0, 1, 9.0, keep_clear_for(517.0, 24.0)).numbers);
    }

    #[test]
    fn the_run_order_does_not_depend_on_how_the_text_was_measured() {
        // The property that lets one layout serve two painters with different
        // font metrics: `place_column` sorts by the ideal position, and the
        // ideal is a function of the angle alone.
        let angles: Vec<f64> = (0..22).map(|i| (i as f64 * 17.0).to_radians()).collect();
        let by = |w: f64, h: f64| -> Vec<(Side, usize)> {
            let labels: Vec<RingLabel> = angles
                .iter()
                .map(|&angle| RingLabel {
                    angle,
                    width: w,
                    height: h,
                    weight: 1.0,
                })
                .collect();
            let ring = place_ring(&labels, &geom());
            let mut runs: Vec<(Side, usize, f64)> = ring
                .placed
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    p.map(|p| {
                        let key = if matches!(p.side, Side::Left | Side::Right) {
                            p.at.1
                        } else {
                            p.at.0
                        };
                        (p.side, i, key)
                    })
                })
                .collect();
            runs.sort_by(|a, b| {
                format!("{:?}", a.0)
                    .cmp(&format!("{:?}", b.0))
                    .then(a.2.partial_cmp(&b.2).unwrap())
            });
            runs.into_iter().map(|(s, i, _)| (s, i)).collect()
        };
        assert_eq!(by(70.0, 13.0), by(84.0, 15.0));
        assert_eq!(by(70.0, 13.0), by(52.0, 12.0));
    }

    #[test]
    fn what_a_run_cannot_hold_is_dropped_and_named() {
        let labels: Vec<RingLabel> = (0..40)
            .map(|i| RingLabel {
                angle: (90.0 + i as f64 * 0.1).to_radians(),
                width: 70.0,
                height: 60.0,
                weight: 1.0 + i as f64,
            })
            .collect();
        let ring = place_ring(&labels, &geom());
        assert!(!ring.dropped.is_empty(), "40 x 60 pt cannot fit in 776");
        assert_eq!(
            ring.placed.iter().filter(|p| p.is_none()).count(),
            ring.dropped.len()
        );
    }
}
