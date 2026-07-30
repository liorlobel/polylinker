//! Plasmid maps as SVG, from Rust.
//!
//! `docs/PLAN.md` v0.1: "SVG export via serialized DOM → `resvg`/`svg2pdf` in
//! Rust. Engine-independent, CI-diffable, strictly better than Chromium
//! `printToPDF`. Never use `html2canvas`."
//!
//! # Why this exists alongside `@polylinker/circular-map`
//!
//! ADR-1 assumed a Tauri shell, in which the desktop app and the browser tool
//! would share one TypeScript renderer. The shell is egui instead — a
//! deliberate departure — so the desktop app cannot call the TypeScript one and
//! a second implementation is unavoidable.
//!
//! That is a cost, and the way to make it pay is to treat the two as
//! independent implementations of one specification rather than as a copy:
//! `crates/pl-draw/tests/agreement.rs` replays a fixture generated from the
//! TypeScript through this crate's helpers — `angle`, `polar`, `ranges`,
//! `place_column`, `isotonic`, `safe_color`, `nice_step`, `commas`, `esc`, `n`
//! — and checks that the two compute the same numbers. Two renderers that agree
//! about their arithmetic are better evidence than one that nobody checks.
//!
//! **It is a helper-level check and not a picture-level one.** It never builds a
//! [`Molecule`], never calls [`scene`], and never compares an arc, a radius, an
//! arrowhead or a label column, so a swapped `Arrow::Start`/`Arrow::End` or a
//! moved label anchor passes it untouched. Until 2026-07 this paragraph said the
//! harness rendered the same molecule through both renderers, which is how the
//! origin-spanning label anchor in `mid_base` survived: there was no oracle.
//!
//! # What is guaranteed
//!
//! **Byte-identical output for identical input**, on every platform. Nothing
//! here reads a clock, a font, a locale or the filesystem: text width is
//! estimated from a constant, floats are rounded before formatting, and
//! iteration order is fixed. That is what makes the output diffable in CI and
//! usable as a figure that does not move between machines.
//!
//! Conventions match the TypeScript renderer and the field: base 1 at twelve
//! o'clock, coordinates increasing **clockwise**, 1-based inclusive.

use pl_core::{Molecule, Strand};

pub mod contrast;
pub mod eps;
mod labels;
pub mod page;
pub mod pdf;
pub mod ring;
pub mod scene;
pub mod trace;
pub use labels::{isotonic, place_column, LabelBox, Placement};
pub use ring::{
    bases_per_arc, centre_room, inside_of, inward_radius, keep_clear_for, label_room, merge_sites,
    place_ring, radius, reserve_for, side_of, Disclosure, Inside, Reserve, Ring, RingGeom,
    RingLabel, RingPlacement, Side, Site,
};
pub use scene::{Anchor, Item, Scene, Seg};

#[cfg(test)]
mod tests;

const TAU: f64 = std::f64::consts::TAU;

/// Rendering knobs. Defaults produce a figure-sized map.
///
/// No longer `Copy`: [`Options::title`] and [`Options::sites`] carry owned
/// strings, and the alternative — a lifetime on every call site — buys nothing
/// for a struct that is built once per file.
#[derive(Debug, Clone)]
pub struct Options {
    pub width: f64,
    pub height: f64,
    pub ring_width: f64,
    pub font_size: f64,
    /// Draw the base-position ruler.
    pub ruler: bool,
    /// Below this many degrees a feature is a tick, not an arrow — an
    /// arrowhead smaller than a pixel reads as dirt on the figure.
    pub min_feature_degrees: f64,
    /// What to call the molecule when the file did not name it.
    ///
    /// The `.dna` container carries no molecule name at all, so `mol.name` is
    /// empty for every SnapGene file and the centre caption fell through to the
    /// literal string `"unnamed"` — in the SVG `<title>` as well. The map on
    /// screen said "pKoV with His decR.dna" and the figure that goes into the
    /// paper said "unnamed", and neither `bins/pl` nor `bins/pl-gui` passed the
    /// filename in for it to say anything else.
    ///
    /// `mol.name` still wins when there is one: a GenBank LOCUS name is a real
    /// name and a filename is a guess.
    pub title: Option<String>,
    /// Restriction sites to label, as `(enzyme, cut position)`.
    ///
    /// Pairs rather than a `Digest`, so this crate stays enzyme-agnostic and
    /// needs no dependency on `pl-enzymes` to be tested. Empty by default,
    /// which is what every exported figure used to be: `pl-draw` has no
    /// reference to an enzyme anywhere, so "Map SVG…" on a plasmid the user had
    /// just read 22 unique cutters off produced a map with **no restriction
    /// sites on it at all**.
    pub sites: Vec<(String, u64)>,
    /// One line under the bp count saying what the figure is *not* showing.
    ///
    /// The desktop map says `22 of 40 cutters labelled · 12 dual, 6 multi not
    /// drawn`; the figure said nothing at all, in the SVG or on stderr, and
    /// [`Options::sites`] defaults to unique cutters — so every default export
    /// dropped 18 of the user's 40 enzymes silently. `docs/PLAN.md` item 33 calls
    /// a silent filter "the one documented case of this software category costing
    /// a user a month of bench time", and the figure is the artefact that leaves
    /// the machine: on screen the Enzymes tab is one click away, on a printed
    /// page nothing is.
    ///
    /// The counts and not a string, so this crate picks the widest of
    /// [`ring::Disclosure`]'s three forms that the ring can hold, and drops the
    /// line when none of them fits — a sentence wider than the ring is written
    /// across the backbone and the feature bands, which is worse than not
    /// written. **Never shortened with an ellipsis**: `22 of 40 cutters labelled
    /// · 12 du...` puts a cut through a count, and half a count is a number a
    /// reader would act on. Picking a form is the only honest way to narrow it.
    ///
    /// Passing the counts rather than the sentence also means the wording and the
    /// arithmetic live in one place for both painters, which is the divergence
    /// this whole layer exists to close.
    pub note: Option<ring::Disclosure>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            width: 720.0,
            height: 720.0,
            ring_width: 18.0,
            font_size: 12.0,
            ruler: true,
            min_feature_degrees: 1.2,
            title: None,
            sites: Vec::new(),
            note: None,
        }
    }
}

/// The ink this crate chooses itself, as opposed to a colour out of a file.
///
/// Named rather than spelled out at each use site because they are audited:
/// `contrast::tests::every_colour_this_crate_chooses_meets_wcag_aa_on_white`
/// measures these constants, and a literal repeated at the use site is a
/// literal the audit cannot see. Changing the feature-label fill to `#a0a0a0`
/// (2.61:1 on white) used to leave every gate green.
///
/// Each is the twin of a key in the TypeScript renderer's `DEFAULT_THEME`;
/// `contrast::tests::the_typescript_palette_matches_ours_and_also_passes` pins
/// the pairs together so one side cannot be fixed without the other.
pub mod ink {
    /// `labelFill` — feature label text, the most numerous text in a map.
    pub const LABEL_FILL: &str = "#22262a";
    /// `titleFill` — the molecule name at the centre.
    pub const TITLE_FILL: &str = "#16191c";
    /// `subtitleFill`/`tickStroke` — the bp count and the ruler numbers.
    pub const SUBTITLE_FILL: &str = "#6b7280";
    /// `backboneStroke` — the ring itself.
    pub const BACKBONE_STROKE: &str = "#33383d";
    /// `featureStroke` — the outline around a feature arrow.
    pub const FEATURE_STROKE: &str = "#2b2f34";
    /// `leaderStroke` — the line from the ring to a label.
    pub const LEADER_STROKE: &str = "#868d95";
}

/// What was drawn, and what could not be.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub labels_placed: usize,
    /// Labels that would have overlapped, or that had no room for even one
    /// character, and were dropped.
    ///
    /// Returned rather than silently omitted: a map missing three labels looks
    /// exactly like a plasmid with three fewer features.
    pub labels_hidden: Vec<String>,
    /// Features whose coordinates describe nothing drawable.
    pub malformed: Vec<String>,
    /// Features drawn from only *some* of their segments, by full name.
    ///
    /// A joined feature copied out of a larger parent record —
    /// `CDS join(100..200,5000..6000)` on a 1000 bp plasmid — keeps one segment
    /// that describes something and one that does not. The map can only draw
    /// the first, and a 101 bp single-exon arrow labelled `orfX` is
    /// indistinguishable from a real 101 bp `orfX`. Half a feature is a worse
    /// lie than no feature, so it is named here.
    pub partly_drawn: Vec<String>,
    /// Labels shortened with a trailing `...` because the canvas was too narrow
    /// for the whole name, by full name.
    ///
    /// The ring's radius reserves room for the widest label with a
    /// 0.55 em/character estimate, and two things can make the real glyphs
    /// outgrow it: the reservation is capped at 30% of the canvas so that one
    /// 60-character name cannot collapse the map to nothing, and Helvetica's
    /// own advances run up to ~26% above the estimate for capital-heavy names.
    /// Either way the choice is a clipped label or a shortened one. A clipped
    /// label is cropped by the `viewBox`, the `/MediaBox` and the
    /// `%%BoundingBox` alike — silently, in the typesetter's hands — so it is
    /// shortened here and said so. The feature's own `<title>` still carries
    /// the whole name, so nothing is lost from the SVG itself — only from what
    /// a reader of the printed figure can see, which is why it is reported.
    ///
    /// `pl export` prints this beside `labels_hidden`, and so does
    /// `partly_drawn`. Neither had a print site anywhere in `bins/` until
    /// 2026-07-29, so "shortened here and said so" was true of the `Report` and
    /// not of anything a user could see — a name could be cut short in a figure
    /// on its way to a journal in silence, and `pCMV-WP...` is a different
    /// plasmid's name from `pCMV-WPRE`. `bins/pl-gui` still surfaces only
    /// `labels_hidden`: there the whole name is one hover away, which is not
    /// true of a printed page.
    pub labels_truncated: Vec<String>,
    /// How many of [`Options::sites`] ended up named on the figure.
    ///
    /// **Enzymes, not labels.** A folded tick carries several names in one label,
    /// so `labels_placed` understates what the reader can see and the difference
    /// is exactly the number a disclosure line gets wrong: on pET28a the map said
    /// `14 of 31 cutters labelled` when 23 were, and `14 + 7 + 1` visibly failed
    /// to reach the 31 it claimed. See [`ring::Disclosure`], which will not
    /// compose a line whose arithmetic does not close.
    pub sites_named: usize,
    /// How many of [`Options::sites`] the ring could not fit, by the same count.
    pub sites_dropped: usize,
    /// How many site labels were drawn shortened.
    pub sites_shortened: usize,
    /// The centre caption was too wide for the ring and was drawn cut short.
    ///
    /// Its own field rather than an entry in [`Report::labels_truncated`], which
    /// is a list of *label* names: a caption is not a label, and folding the two
    /// together makes "no name was shortened" unaskable. Reported because a
    /// truncated molecule name on a printed figure is the same class of wrongness
    /// as a truncated enzyme name — `NC_000913.3 Escherichia coli str. K-12...`
    /// is at least recognisable, and `Scene::title` and the SVG `<title>` still
    /// carry the whole string, but a reader of the page has neither.
    pub title_truncated: bool,
}

/// Round to two decimals. Float noise triples an SVG's size and destroys
/// byte-identity between platforms for no visible gain.
pub fn n(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    // `-0` and `0` must format the same or two identical pictures differ.
    let r = if r == 0.0 { 0.0 } else { r };
    format!("{r}")
}

/// XML-escape text destined for a text node or an attribute value.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // XML 1.0 forbids most control characters outright, so escaping
            // them would still produce a document that will not parse. A
            // feature name out of a binary `.dna` payload can contain one.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// A colour safe to interpolate into an attribute, or the fallback.
///
/// Colours arrive inside `.dna` files from Addgene and from collaborators, so
/// they are not author-controlled. The same check the TypeScript renderer
/// makes, for the same reason.
pub fn safe_color(v: Option<&str>, fallback: &str) -> String {
    let Some(v) = v.map(str::trim).filter(|s| !s.is_empty()) else {
        return fallback.to_string();
    };

    // #rgb, #rgba, #rrggbb, #rrggbbaa
    let hex = v.strip_prefix('#').unwrap_or("");
    let hex_ok = matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());

    // rgb()/rgba()/hsl()/hsla() carrying only numbers, %, commas, slashes and
    // spaces. Real files use these, and refusing them would silently grey out
    // a map that another tool coloured correctly.
    let func_ok = ["rgb(", "rgba(", "hsl(", "hsla("].iter().any(|p| {
        v.strip_prefix(p)
            .and_then(|rest| rest.strip_suffix(')'))
            .is_some_and(|inner| {
                inner
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b"eE+-.,%/ \t\n\x0b\x0c\r".contains(&b))
            })
    });

    // A bare CSS colour keyword, plus the two SVG paint keywords.
    let word_ok = (1..=32).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_alphabetic());

    if hex_ok || func_ok || word_ok {
        v.to_string()
    } else {
        fallback.to_string()
    }
}

pub fn colour_for(kind: &str) -> &'static str {
    match kind {
        "CDS" | "gene" => "#4f7fd0",
        "promoter" | "RBS" => "#4aa564",
        "terminator" | "polyA_signal" => "#c05c5c",
        "rep_origin" | "origin" => "#c07e2e",
        "primer_bind" => "#7e8a97",
        "protein_bind" => "#b87bb0",
        "misc_feature" => "#8b7bb8",
        _ => "#7f8a95",
    }
}

pub fn polar(cx: f64, cy: f64, r: f64, a: f64) -> (f64, f64) {
    (cx + r * a.sin(), cy - r * a.cos())
}

/// Angle in radians clockwise from twelve o'clock, for a 1-based base.
///
/// The positive modulo, rather than a saturating subtraction, is what makes
/// base 0 land just *before* the origin instead of on it — the same reading
/// `baseToAngle` takes, and the reason [`angle_past`] closes an arc that ends
/// at the last base.
///
/// **Done in `u64`.** The same function was written `((base as i64 - 1) % l +
/// l) % l`, and `len` is not bounded: `pl-fileio`'s GenBank reader parses the
/// LOCUS length with a bare `parse::<u64>()`, `Molecule::validate` only checks
/// it against the sequence when there *is* a sequence, and nothing between
/// there and [`scene`] clamps it. Above about 4.6e18 the `+ l` overflowed —
/// debug panicked on this line — and above `i64::MAX` `l` went negative and the
/// expression quietly returned 0 for every base, putting every feature on a
/// hostile record at twelve o'clock.
pub fn angle(base: u64, len: u64) -> f64 {
    if len == 0 {
        return 0.0;
    }
    // Base 0 is one step *before* the origin, i.e. the last base.
    let frac = if base == 0 { len - 1 } else { (base - 1) % len };
    (frac as f64 / len as f64) * TAU
}

/// The angle one base *past* `base` — where an arc ending at `base` closes.
///
/// Identical to `angle(base + 1, len)` for every base a caller can hold, and
/// written this way because that `+ 1` is not safe: `ranges` clamps a segment's
/// end to `len`, and `len` can be `u64::MAX` off a LOCUS line, so the addition
/// overflowed before `angle` was even entered — a debug panic, and in release a
/// wrap to `angle(0, len)`, which is a whole turn away from where the arc
/// should close.
fn angle_past(base: u64, len: u64) -> f64 {
    if len == 0 {
        return 0.0;
    }
    ((base % len) as f64 / len as f64) * TAU
}

/// The angular ranges a segment occupies, splitting an origin-spanning one.
///
/// 1-based inclusive, matching `pl-core`. `segmentRanges` returns the same
/// spans 0-based half-open; `tests/agreement.rs` converts and compares.
pub fn ranges(start: u64, end: u64, len: u64, circular: bool) -> Vec<(u64, u64)> {
    if len == 0 {
        return Vec::new();
    }
    // A segment lying wholly past the end of the molecule describes nothing
    // here. Clamping both endpoints independently collapses it onto a 1 bp
    // range at the last base — drawn, and labelled with the real feature's
    // name, at 359.6 degrees. That is fabrication, which is worse than loss,
    // so the caller reports it as malformed instead.
    if start > len && end > len {
        return Vec::new();
    }
    // The mirror image, which was missing on both renderers. A segment wholly
    // below base 1 — `0-0`, which the SnapGene reader produces from an importer
    // that wrote 0-based coordinates — names no base either, and
    // `clamp(1, len)` on both endpoints collapsed it onto a 1 bp arc at base 1
    // under the real feature's name. The same fabrication as above, at the
    // other end of the molecule.
    if start < 1 && end < 1 {
        return Vec::new();
    }
    let s = start.clamp(1, len);
    let e = end.clamp(1, len);
    if s <= e {
        return vec![(s, e)];
    }
    if !circular {
        // A line has no origin to cross; the only honest reading is the span
        // the coordinates name.
        return vec![(e.min(s), s.max(e))];
    }
    vec![(s, len), (1, e)]
}

/// The base a feature's label should point at: the middle of its whole extent.
///
/// Accumulated across the parts, because half the *total* span added to the
/// first part is not the same thing and the difference is a quarter turn. The
/// old form was `parts[0].0 + (span / 2).min(parts[0].1 - parts[0].0)`: for a
/// 502 bp feature at 999..500 on a 1000 bp plasmid the parts are
/// `[(999, 1000), (1, 500)]`, so the clamp pinned the anchor to base 1000 —
/// 359.6 degrees, twelve o'clock — and because the column is chosen by
/// `angle.sin() >= 0.0` and sin(359.6°) is -0.006, the label went to the *left*
/// column with a leader pointing at 2 bases of the 502. The middle is base 250,
/// at 89.6 degrees, in the right column. The identical feature at 100..601
/// anchored correctly, so the picture depended on where the origin happened to
/// sit, which it must never do.
///
/// `featureMidBase` in the TypeScript renderer accumulates the same way. The
/// half is floored rather than rounded, which keeps a single-part feature's
/// anchor bit-identical to what this crate has always drawn and leaves the two
/// renderers at most one base apart at a part boundary.
fn mid_base(parts: &[(u64, u64)], span: u64) -> u64 {
    let half = span / 2;
    let mut acc = 0u64;
    for &(a, b) in parts {
        let w = b - a + 1;
        // `acc <= half` at every iteration, so the subtraction cannot wrap:
        // the loop only continues while `acc + w < half`.
        if acc + w >= half {
            return a + (half - acc);
        }
        acc += w;
    }
    parts.first().map_or(1, |p| p.0)
}

/// Half-width of the twelve- and six-o'clock label rows, in degrees.
///
/// See [`ring::RingGeom::row_half`] for why 30 and not 40.
const ROW_HALF_DEGREES: f64 = 30.0;

/// Everything between the backbone and the first glyph of a side-column label.
///
/// The leader leaves the ring at `ro + 2` and the text starts at
/// `ro + LEADER_GAP`, so 34 is the sum of that gap and the small clearance the
/// canvas edge keeps. Held as a constant because `ring::reserve_for` takes it
/// as an argument and the number is the difference between the reserve and the
/// room — the arithmetic `map.rs` used to leave out entirely.
const LABEL_MARGIN: f64 = 34.0;

/// Distance from the ring's outer radius to a label's own anchor.
const LEADER_GAP: f64 = 26.0;

/// The stroke of the mark a cut site puts on the ring.
///
/// Also the threshold two sites are folded into one tick at, via
/// [`ring::bases_per_arc`]: below this the two marks are the same mark.
const SITE_TICK_STROKE: f64 = 1.25;

/// A cut site's resistance to being dropped when a run overflows.
///
/// Above **any** feature weight, which is `1 + log10(1 + span)` and so cannot
/// exceed 1 + log10(u64::MAX) = 20.3 even for a hostile file. Deliberately not a
/// nudge: a cut coordinate is what a reader plans a digest from, and a feature
/// name is a description of something the map already draws as a coloured arc
/// with its own `<title>`. On stock pET28a the 137-character
/// `Multiple Cloning Site (MCS); contains unique sites for NcoI, EcoRI, ...`
/// outweighed all nine of the enzyme labels it was describing and evicted every
/// one of them, so the figure carried a note about a polylinker and no polylinker.
///
/// It resists *displacement* by the same factor, which is also what you want: a
/// site label pinned beside its own tick and a feature label pushed a line along
/// is the right way round.
const SITE_WEIGHT: f64 = 24.0;

/// One thing wanting a label on the ring: a feature, or a cut site.
#[derive(Debug, Clone)]
struct Label {
    text: String,
    angle: f64,
    weight: f64,
    /// A restriction site rather than a feature. Sites also get a tick on the
    /// ring, because a leader alone points at a place and a tick says a cut
    /// happens there.
    site: bool,
    /// How many enzymes this one label names — more than one when a tick folded.
    ///
    /// Carried so [`Report::sites_named`] can count enzymes rather than labels.
    /// A disclosure line built from the label count understates itself by exactly
    /// this excess and its arithmetic stops closing, which reads as enzymes having
    /// gone missing.
    names: usize,
}

/// How wide a label is *assumed* to be when the ring reserves room for it.
///
/// The estimate, not Helvetica's real advances, and it is used for the radius
/// reservation only: `packages/circular-map/src/render.ts` sizes its margin
/// with the same 0.55 em/character, so keeping this here is what keeps the two
/// renderers' geometry identical.
///
/// **It is not what a label measures when it is drawn.** The PDF and EPS back
/// ends position and clip against `pdf::text_width_in`, which is up to ~26%
/// wider for capital-heavy Latin — `pCMV-WPRE` at 12 pt is 59.4 units here and
/// 73.33 pt there. Deciding whether a name fits with this number is how a
/// 9-character plasmid name ran 5.93 pt past the `/MediaBox`, so [`fit_label`]
/// asks `drawn_width` instead. This doc used to say "both call sites use this
/// so they cannot drift apart"; the emitters were a third call site that never
/// did.
fn label_width(name: &str, font_size: f64) -> f64 {
    name.chars().count() as f64 * font_size * 0.55
}

/// How wide a label really is once drawn, in scene units.
///
/// Helvetica's own advances, in the regular weight every feature label is drawn
/// in — the same measurement `pdf::to_pdf` and `eps::to_eps` use to place an
/// `Anchor::End` label and that the `/MediaBox`, the `%%BoundingBox` and the
/// `viewBox` therefore crop against.
fn drawn_width(name: &str, font_size: f64) -> f64 {
    crate::pdf::text_width_in(name, font_size, false)
}

/// A label shortened to what the canvas can actually hold, or `None` if not
/// even one character and an ellipsis fit.
///
/// `Some(name.to_string())` when the whole name fits — the caller compares
/// against the original to decide whether anything was lost.
///
/// **Measured with [`drawn_width`], not [`label_width`].** A label runs from
/// `cx ± (ro + 26)`, and `room` is the distance from there to the canvas edge,
/// so "the drawn glyphs are wider than `room`" and "the label is cropped by the
/// `/MediaBox`" are the same statement — but only if the two are measured the
/// same way. Deciding with the 0.55 em/character estimate while the emitters
/// drew with Helvetica's advances left a band where the name was declared to
/// fit and did not: `pCMV-WPRE` on a 4 kb plasmid at the defaults came out at
/// 59.4 estimate units against 67.4 of room, kept whole, and then typeset 73.33
/// pt wide from x=652.6 on a 720 pt page — most of the final E cropped off the
/// figure, with `Report::labels_truncated` empty. Measured this way the two
/// conditions coincide exactly, so nothing is shortened that would not
/// otherwise have been cropped.
///
/// The ellipsis is three ASCII full stops, not U+2026, because three dots are
/// three dots in every encoding and the real character is not.
///
/// A cut-site label is `EcoRI  7,530` — a name, two spaces, a coordinate — and
/// the coordinate goes **whole or not at all**. Cutting into it produces
/// `EcoRI  7,5...`, which reads as a cut position and is not one, and a wrong
/// coordinate on a plasmid map is the failure this whole pass is about. A name
/// with no coordinate beside it claims nothing; the caller counts it as
/// shortened either way.
fn fit_label(name: &str, room: f64, font_size: f64) -> Option<String> {
    if drawn_width(name, font_size) <= room {
        return Some(name.to_string());
    }
    // Two spaces, so an ordinary feature name with one space in it is untouched.
    if let Some((head, _)) = name.rsplit_once("  ") {
        if !head.is_empty() && drawn_width(head, font_size) <= room {
            return Some(head.to_string());
        }
    }
    const ELLIPSIS: &str = "...";
    let mut kept = String::new();
    for c in name.chars() {
        let mut trial = kept.clone();
        trial.push(c);
        if drawn_width(&(trial.clone() + ELLIPSIS), font_size) > room {
            break;
        }
        kept = trial;
    }
    if kept.is_empty() {
        None
    } else {
        Some(kept + ELLIPSIS)
    }
}

/// Build the device-independent picture.
///
/// The one place the map's geometry lives. [`circular_svg`] and
/// [`circular_pdf`] both render this, which is why they cannot disagree about
/// where anything is — there is nothing above the level of ink for them to
/// disagree about.
pub fn scene(mol: &Molecule, opts: Options) -> (Scene, Report) {
    let mut report = Report::default();
    let mut items: Vec<Item> = Vec::new();
    let len = mol.span().max(1);
    let circular = mol.topology.is_circular();
    let (cx, cy) = (opts.width / 2.0, opts.height / 2.0);
    let pane_min = opts.width.min(opts.height);
    let row_half = ROW_HALF_DEGREES.to_radians();

    // Resolve every feature to the parts a painter can draw, *before* the ring
    // knows how big it is. The radius depends on which labels land in a side
    // column, and that depends on their angles, so the angles have to exist
    // first. This loop used to sit below the radius and push labels as it went.
    struct Drawn {
        name: String,
        parts: Vec<(u64, u64)>,
        colour: String,
        degrees: f64,
        arrow_on: isize,
        strand: Strand,
    }
    let mut drawn: Vec<Drawn> = Vec::new();
    let mut anchors: Vec<Label> = Vec::new();

    for f in &mol.features {
        let mut parts: Vec<(u64, u64)> = Vec::with_capacity(f.segments.len());
        // Counted per segment, not over the whole feature. `ranges` returns
        // nothing for a segment lying wholly past the end, and a feature with
        // one such segment and one good one still has a non-empty `parts` — so
        // the all-or-nothing check below never fired for it and half of
        // `CDS join(100..200,5000..6000)` on a 1000 bp plasmid went out as a
        // whole 101 bp `orfX` with nothing said.
        let mut lost_segments = 0usize;
        for s in &f.segments {
            let r = ranges(s.start, s.end, len, circular);
            if r.is_empty() {
                lost_segments += 1;
            }
            parts.extend(r);
        }
        if parts.is_empty() {
            report.malformed.push(f.name.clone());
            continue;
        }
        if lost_segments > 0 {
            report.partly_drawn.push(f.name.clone());
        }
        let span: u64 = parts.iter().map(|(a, b)| b - a + 1).sum();
        let mid = mid_base(&parts, span);
        anchors.push(Label {
            text: f.name.clone(),
            angle: angle(mid, len),
            weight: 1.0 + (1.0 + span as f64).log10(),
            site: false,
            names: 1,
        });
        drawn.push(Drawn {
            name: f.name.clone(),
            colour: safe_color(f.color(), colour_for(&f.kind)),
            degrees: (span as f64 / len as f64) * 360.0,
            arrow_on: match f.strand {
                Strand::Forward => parts.len() as isize - 1,
                Strand::Reverse => 0,
                _ => -1,
            },
            strand: f.strand,
            parts,
        });
    }

    // Restriction sites, one label each to begin with. The radius is decided
    // over these, before anything is folded together, because merging may not
    // buy itself room: an honest merged label carries every name and the range —
    // see [`ring::Site::label`] for why it must — so it is always wider than
    // either name alone, and sizing the ring to hold `XmaI/SmaI  6,917-6,919`
    // costs a quarter of the radius to gain tidiness.
    let site_label = |name: &str, pos: u64| Label {
        text: format!("{name}  {}", commas(pos)),
        angle: angle(pos, len),
        weight: SITE_WEIGHT,
        site: true,
        names: 1,
    };
    let widest_of = |labels: &[Label]| -> f64 {
        labels
            .iter()
            .filter(|l| matches!(ring::side_of(l.angle, row_half), Side::Left | Side::Right))
            .map(|l| label_width(&l.text, opts.font_size))
            .fold(0.0_f64, f64::max)
    };
    let mut unmerged: Vec<Label> = anchors.clone();
    unmerged.extend(opts.sites.iter().map(|(n, p)| site_label(n, *p)));
    // The row strip is what the twelve- and six-o'clock runs need vertically:
    // the leader gap, one line, and the canvas padding twice. Charged to the
    // height, while the reserve is charged to the width — see [`ring::radius`]
    // for the 26% of radius the single-axis rule gave away on a wide figure.
    let row_strip = LEADER_GAP + opts.font_size + 3.0 + 2.0 * 8.0;
    let ro = ring::radius(
        opts.width,
        opts.height,
        ring::reserve_for(widest_of(&unmerged), LABEL_MARGIN, pane_min).reserve,
        row_strip,
    );

    // Now fold the sites that share a tick — but only where the honest label
    // fits the room the individual names had already earned. Where it does not,
    // the sites stay separate and the packer moves them one line apart. Untidy
    // and true: the alternative is an ellipsis through a merged label, which can
    // drop a whole enzyme name, and that is the failure `Site::label` exists to
    // prevent.
    //
    // The room is `cx - (tick_r + LEADER_GAP)` with the same `tick_r` the labels
    // are placed with, not `ro`. Reading it off `ro` left the two 2 units apart,
    // which is enough for a folded label to pass this check and then be
    // shortened by `fit_label` — and shortening a folded label is how a whole
    // enzyme name disappears from a figure that still shows its slash.
    let site_room = cx - (ro + 2.0 + LEADER_GAP);
    if !opts.sites.is_empty() {
        // The tick's own stroke, in bases. Two cuts closer than the mark that
        // draws them ARE one mark; two cuts a label-height apart are two marks
        // whose names collide, which is the packer's problem and not a fact
        // about the molecule.
        let within = ring::bases_per_arc(SITE_TICK_STROKE, ro + 2.0, len);
        for s in ring::merge_sites(&opts.sites, within) {
            let folded = s.label();
            if s.names.len() == 1 || label_width(&folded, opts.font_size) <= site_room {
                anchors.push(Label {
                    text: folded,
                    angle: angle(s.anchor(), len),
                    weight: SITE_WEIGHT,
                    site: true,
                    names: s.names.len(),
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
    // What is left for a label once the ring has taken its radius, on either
    // side, is `cx - (ro + LEADER_GAP)` — computed at each use site below,
    // because the twelve- and six-o'clock rows are limited by the canvas width
    // instead and a single `room` cannot describe both.
    //
    // Uncapped the reservation closes with 8 units to spare **in the estimate's
    // units** — and that is not the unit the figure is cropped in. `label_width`
    // assumes 0.55 em a character; Helvetica charges up to ~0.7 for capitals, so
    // for a capital-heavy name the 8 units of slack are spent and more. This
    // comment used to end at "8 units to spare whatever the name", which was the
    // reason nobody looked: `pCMV-WPRE` had 8 units of estimated slack and ran
    // 5.93 pt off the page. `fit_label` therefore decides in `drawn_width`, so
    // the cap and the floor are no longer the only ways to overflow the
    // reservation — they are just the ways that overflow it by a lot.
    let ri = ro - opts.ring_width;
    let mid_r = (ro + ri) / 2.0;

    // --- labels, placed exactly, in an L-shaped ring ---
    //
    // Two columns and two rows rather than two columns. A label within 30
    // degrees of twelve or six o'clock leaves the side column for a horizontal
    // run above or below the ring, which is what stops its leader running most
    // of the radius at a degree and a half off horizontal. See
    // [`ring::place_ring`] for the measurements, and [`ring::row_span`] for why
    // the rows stop at the columns' inner edge.
    //
    // Decided here, above the ruler, because the ring's geometry is what bounds
    // the lines written in the middle and the middle is what the ruler has to be
    // kept off.
    let line_h = opts.font_size + 3.0;
    let pad = 8.0;
    let geom = RingGeom {
        cx,
        cy,
        tick_r: ro + 2.0,
        gap: LEADER_GAP,
        row_half,
        row_gap: 10.0,
        left: pad,
        right: opts.width - pad,
        top: pad + opts.font_size,
        bottom: opts.height - pad,
    };

    // --- what is written in the middle ---
    //
    // Cut to the ring before the ruler is placed, never after. `mol.name` first,
    // because a GenBank LOCUS name is a real name and a filename is a guess;
    // `"unnamed"` last, because it is not a name at all — it was the centre
    // caption *and* the SVG `<title>` of every figure ever exported from a `.dna`
    // file, since the SnapGene container carries no molecule name and nothing
    // passed the filename in.
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
    let ruler_size = opts.font_size * 0.72;
    // The widest number the ruler will draw. `commas(len)` is an upper bound on
    // every tick's text, because a tick position is at most the length.
    let widest_number = drawn_width(&commas(len), ruler_size);
    let centre_room = ring::centre_room(ri, 0.0, 0.0, ruler_size, widest_number);
    // Elided, not dropped: this is the line with a `<title>` behind it carrying
    // the whole string, and `Scene::title` below keeps it whole whatever the
    // drawn form. A 69-character filename off NCBI typesets about 517 pt wide,
    // and deriving the ruler's clearance from *that* is what cost a 4.6 Mb genome
    // its entire scale.
    let title_drawn = fit_label(&title, centre_room, opts.font_size * 1.25);
    let bp = format!("{} bp", commas(len));
    let bp_drawn = fit_label(&bp, centre_room, opts.font_size * 0.9);
    let note_size = opts.font_size * 0.8;
    let note_drawn = opts.note.as_ref().and_then(|d| {
        [d.long(), d.short(), d.tiny()]
            .into_iter()
            .find(|f| drawn_width(f, note_size) <= centre_room)
    });
    let centre_w = [
        title_drawn
            .as_deref()
            .map_or(0.0, |t| drawn_width(t, opts.font_size * 1.25)),
        bp_drawn
            .as_deref()
            .map_or(0.0, |t| drawn_width(t, opts.font_size * 0.9)),
        note_drawn
            .as_deref()
            .map_or(0.0, |t| drawn_width(t, note_size)),
    ]
    .into_iter()
    .fold(0.0_f64, f64::max);
    // No inward feature lanes: this renderer draws every feature OUTSIDE the
    // backbone, so the only two things sharing the inside are the ruler and the
    // caption. That is still one collision more than none — at
    // `--width 340` the caption was typeset straight across the backbone and
    // through `2,000` and `6,000`, and `pl export` reported label drops and
    // shortenings and said nothing about it.
    let inside = ring::inside_of(
        ri,
        0.0,
        0.0,
        1,
        ruler_size,
        ring::keep_clear_for(centre_w, widest_number),
    );

    // --- backbone ---
    if circular {
        items.push(Item::Circle {
            cx,
            cy,
            r: mid_r,
            stroke: ink::BACKBONE_STROKE.into(),
            stroke_width: 1.25,
        });
    } else {
        // A linear molecule drawn as a closed ring would be a lie about
        // topology, so it gets a gap.
        let gap = 0.06 * TAU;
        let (x0, y0) = polar(cx, cy, mid_r, gap / 2.0);
        items.push(Item::Path {
            segs: vec![
                Seg::Move(x0, y0),
                Seg::Arc {
                    cx,
                    cy,
                    r: mid_r,
                    from: gap / 2.0,
                    to: TAU - gap / 2.0,
                },
            ],
            fill: None,
            stroke: Some(ink::BACKBONE_STROKE.into()),
            stroke_width: 1.25,
            title: None,
        });
    }

    // --- ruler ---
    if opts.ruler {
        let step = nice_step(len as f64 / 12.0);
        let mut base = step;
        while base <= len {
            let a = angle(base, len);
            let (x0, y0) = polar(cx, cy, inside.ruler_tick.1, a);
            let (x1, y1) = polar(cx, cy, inside.ruler_tick.0, a);
            items.push(Item::Path {
                segs: vec![Seg::Move(x0, y0), Seg::Line(x1, y1)],
                fill: None,
                stroke: Some(ink::SUBTITLE_FILL.into()),
                stroke_width: 1.0,
                title: None,
            });
            // The mark is drawn either way; the number only when the middle has
            // room for it. See `ring::Inside::numbers` — a caller that bounds its
            // centre lines with `centre_room` never reaches the false branch, and
            // this one does, but the branch is the honest answer for a canvas so
            // small that nothing fits.
            if inside.numbers {
                let (tx, ty) = polar(cx, cy, inside.ruler_text_r, a);
                items.push(Item::Text {
                    x: tx,
                    y: ty,
                    size: ruler_size,
                    anchor: Anchor::Middle,
                    color: ink::SUBTITLE_FILL.into(),
                    bold: false,
                    text: commas(base),
                });
            }
            // `base + step` is not safe at the top of the u64 range, and `len`
            // comes straight off a GenBank LOCUS line. For a declared length of
            // 18446744073709551615 the step is 2e18, the ninth tick is 1.8e19,
            // and the tenth overflows: debug panicked here, and the shipped
            // release build wrapped to 1553255926290448384 — still `<= len`, so
            // the loop never ended and pushed two `Item`s a turn at about
            // 1.4 GB/s until the process was killed with no output and no error.
            match base.checked_add(step) {
                Some(next) => base = next,
                None => break,
            }
        }
    }

    // --- features ---
    for d in &drawn {
        for (i, &(a, b)) in d.parts.iter().enumerate() {
            if d.degrees < opts.min_feature_degrees {
                // Below a pixel an arrowhead reads as dirt on the figure, so a
                // very short feature is a tick instead.
                let ang = angle(a, len);
                let (x0, y0) = polar(cx, cy, ri, ang);
                let (x1, y1) = polar(cx, cy, ro, ang);
                items.push(Item::Path {
                    segs: vec![Seg::Move(x0, y0), Seg::Line(x1, y1)],
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
                    segs: arc_segs(cx, cy, ri, ro, angle(a, len), angle_past(b, len), arrow),
                    fill: Some(d.colour.clone()),
                    stroke: Some(ink::FEATURE_STROKE.into()),
                    stroke_width: 0.6,
                    title: Some(d.name.clone()),
                });
            }
        }
    }

    // The rows are Middle-anchored, so their box is the *drawn* width — the one
    // the viewBox, the /MediaBox and the %%BoundingBox crop against — and not
    // the 0.55 em estimate the radius is reserved with. Packing a row in
    // estimate units and cropping it in Helvetica's is the same mistake
    // `fit_label` was moved off, one axis over.
    // Read off the geometry the label is actually placed with, not rebuilt from
    // `ro`. The two drifted by the 2 units the leader starts outside the ring,
    // and `pCMV-WPRE` went 1.27 pt past a 720 pt canvas with `labels_truncated`
    // empty — the same class of defect as deciding in one unit and drawing in
    // another. One number for every run, because a label the rows cannot hold
    // ends up in a column: see [`ring::label_room`].
    let room = ring::label_room(&geom, cx);
    let texts: Vec<Option<String>> = anchors
        .iter()
        .map(|l| fit_label(&l.text, room, opts.font_size))
        .collect();
    let boxes: Vec<RingLabel> = anchors
        .iter()
        .zip(&texts)
        .map(|(l, t)| RingLabel {
            angle: l.angle,
            width: t.as_deref().map_or(0.0, |t| drawn_width(t, opts.font_size)),
            height: line_h,
            weight: l.weight,
        })
        .collect();
    let placed = ring::place_ring(&boxes, &geom);
    for &d in &placed.dropped {
        report.labels_hidden.push(anchors[d].text.clone());
    }

    let mut overlay: Vec<Item> = Vec::new();
    for (i, l) in anchors.iter().enumerate() {
        let Some(p) = placed.placed[i] else { continue };
        let Some(text) = texts[i].clone() else {
            // Not even one character and an ellipsis fit. Drawing the leader
            // with nothing on the end of it would look like a renderer bug
            // rather than a canvas too small to hold the name, so the label
            // goes, and it is named.
            report.labels_hidden.push(l.text.clone());
            continue;
        };
        if text != l.text {
            report.labels_truncated.push(l.text.clone());
            if l.site {
                report.sites_shortened += 1;
            }
        }
        if l.site {
            report.sites_named += l.names;
        }
        // A cut site gets a mark on the ring as well as a leader: a leader
        // alone points at a place, a tick says a cut happens there.
        if l.site {
            let (sx, sy) = polar(cx, cy, ro, l.angle);
            let (ex, ey) = polar(cx, cy, ro + 6.0, l.angle);
            items.push(Item::Path {
                segs: vec![Seg::Move(sx, sy), Seg::Line(ex, ey)],
                fill: None,
                stroke: Some(ink::BACKBONE_STROKE.into()),
                stroke_width: 1.25,
                title: Some(l.text.clone()),
            });
        }
        // The last leg stops short of the glyphs so the rule never touches the
        // text it points at.
        let stop = match p.side {
            Side::Right => (p.at.0 - 4.0, p.at.1),
            Side::Left => (p.at.0 + 4.0, p.at.1),
            Side::Top => (p.at.0, p.at.1 + line_h * 0.5),
            Side::Bottom => (p.at.0, p.at.1 - line_h * 0.5),
        };
        overlay.push(Item::Path {
            segs: vec![
                Seg::Move(p.tip.0, p.tip.1),
                Seg::Line(p.bend.0, p.bend.1),
                Seg::Line(stop.0, stop.1),
            ],
            fill: None,
            stroke: Some(ink::LEADER_STROKE.into()),
            stroke_width: 0.9,
            title: None,
        });
        overlay.push(Item::Text {
            x: p.at.0,
            y: p.at.1,
            size: opts.font_size,
            anchor: p.anchor,
            color: ink::LABEL_FILL.into(),
            bold: false,
            text,
        });
        report.labels_placed += 1;
    }
    // Enzymes the filter admitted and the ring could not fit. Derived by
    // subtraction from what was *asked for*, so a name lost anywhere between the
    // fold and the paint is counted — a `sites_dropped` accumulated at each drop
    // site is a `sites_dropped` that misses the next drop site somebody adds.
    report.sites_dropped = opts.sites.len().saturating_sub(report.sites_named);

    // --- centre ---
    //
    // Each line already cut to `centre_room` above, because the ruler's radius
    // was derived from what these actually measure. `Scene::title` keeps the
    // whole name whatever was drawn, so the SVG `<title>`, the PDF `/Title` and
    // a hover all still carry it.
    if let Some(text) = title_drawn {
        report.title_truncated = text != title;
        overlay.push(Item::Text {
            x: cx,
            y: cy - 4.0,
            size: opts.font_size * 1.25,
            anchor: Anchor::Middle,
            color: ink::TITLE_FILL.into(),
            bold: true,
            text,
        });
    }
    if let Some(text) = bp_drawn {
        overlay.push(Item::Text {
            x: cx,
            y: cy + opts.font_size + 2.0,
            size: opts.font_size * 0.9,
            anchor: Anchor::Middle,
            color: ink::SUBTITLE_FILL.into(),
            bold: false,
            text,
        });
    }
    // What the figure is NOT showing, in the figure. The desktop map has said
    // this since the L-ring landed; the SVG, the PDF and the EPS said nothing,
    // and `--sites unique` — the default — drops every dual and multi cutter.
    if let Some(text) = note_drawn {
        overlay.push(Item::Text {
            x: cx,
            y: cy + opts.font_size * 2.4,
            size: note_size,
            anchor: Anchor::Middle,
            color: ink::SUBTITLE_FILL.into(),
            bold: false,
            text,
        });
    }

    items.extend(overlay);
    (
        Scene {
            width: opts.width,
            height: opts.height,
            title,
            items,
        },
        report,
    )
}

/// Render a molecule as a standalone SVG document.
pub fn circular_svg(mol: &Molecule, opts: Options) -> (String, Report) {
    let (sc, report) = scene(mol, opts);
    (svg_of(&sc), report)
}

/// Render a molecule as a one-page PDF.
///
/// The same [`Scene`] as [`circular_svg`], so the two are the same picture.
/// `Report` carries what the *drawing* could not show; the second report is
/// what the PDF's font could not encode.
pub fn circular_pdf(mol: &Molecule, opts: Options) -> (Vec<u8>, Report, pdf::Report) {
    let (sc, report) = scene(mol, opts);
    let (bytes, pdf_report) = pdf::to_pdf(&sc);
    (bytes, report, pdf_report)
}

/// A scene as SVG.
pub fn svg_of(sc: &Scene) -> String {
    let mut body = String::new();
    for item in &sc.items {
        match item {
            Item::Circle {
                cx,
                cy,
                r,
                stroke,
                stroke_width,
            } => body.push_str(&format!(
                r##"<circle cx="{}" cy="{}" r="{}" fill="none" stroke="{stroke}" stroke-width="{}"/>"##,
                n(*cx),
                n(*cy),
                n(*r),
                n(*stroke_width)
            )),
            Item::Path {
                segs,
                fill,
                stroke,
                stroke_width,
                title,
            } => {
                let d = svg_path(segs);
                let fill = fill.clone().unwrap_or_else(|| "none".into());
                let stroke = stroke.clone().unwrap_or_else(|| "none".into());
                match title {
                    Some(t) => body.push_str(&format!(
                        r##"<path d="{d}" fill="{fill}" stroke="{stroke}" stroke-width="{}"><title>{}</title></path>"##,
                        n(*stroke_width),
                        esc(t)
                    )),
                    None => body.push_str(&format!(
                        r##"<path d="{d}" fill="{fill}" stroke="{stroke}" stroke-width="{}"/>"##,
                        n(*stroke_width)
                    )),
                }
            }
            Item::Text {
                x,
                y,
                size,
                anchor,
                color,
                bold,
                text,
            } => {
                let a = match anchor {
                    Anchor::Start => "start",
                    Anchor::Middle => "middle",
                    Anchor::End => "end",
                };
                let weight = if *bold { r##" font-weight="600""## } else { "" };
                body.push_str(&format!(
                    r##"<text x="{}" y="{}" font-size="{}"{weight} fill="{color}" text-anchor="{a}" dominant-baseline="middle">{}</text>"##,
                    n(*x),
                    n(*y),
                    n(*size),
                    esc(text)
                ));
            }
        }
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}" font-family="system-ui, -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif"><title>{}</title>{body}</svg>"##,
        n(sc.width),
        n(sc.height),
        n(sc.width),
        n(sc.height),
        esc(&sc.title)
    )
}

/// A path's segments as an SVG `d` attribute.
///
/// Converts each centre-form arc into SVG's endpoint form. The sweep flag is 1
/// for increasing angle, which is clockwise on screen; the large-arc flag is
/// set past a half turn, and a sweep of a full turn or more is split, because
/// SVG's endpoint form cannot express an arc whose ends coincide.
fn svg_path(segs: &[Seg]) -> String {
    let mut d = String::new();
    for seg in segs {
        match *seg {
            Seg::Move(x, y) => d.push_str(&format!("M{},{}", n(x), n(y))),
            Seg::Line(x, y) => d.push_str(&format!("L{},{}", n(x), n(y))),
            Seg::Arc {
                cx,
                cy,
                r,
                from,
                to,
            } => {
                let sweep_flag = if to >= from { 1 } else { 0 };
                let total = (to - from).abs();
                let steps = if total >= TAU { 4 } else { 1 };
                let step = (to - from) / steps as f64;
                for i in 0..steps {
                    let t1 = from + step * (i + 1) as f64;
                    let (x, y) = polar(cx, cy, r, t1);
                    let large = if step.abs() > std::f64::consts::PI {
                        1
                    } else {
                        0
                    };
                    d.push_str(&format!(
                        "A{},{} 0 {large} {sweep_flag} {},{}",
                        n(r),
                        n(r),
                        n(x),
                        n(y)
                    ));
                }
            }
            Seg::Close => d.push('Z'),
        }
    }
    d
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Arrow {
    None,
    Start,
    End,
}

/// One feature arc, with its arrowhead, as device-independent segments.
///
/// Was an SVG `d` string. It has to be segments now so the PDF back end can
/// turn the arcs into Béziers, and so both back ends draw the same shape rather
/// than one parsing the other's output.
fn arc_segs(cx: f64, cy: f64, ri: f64, ro: f64, a0: f64, a1_in: f64, arrow: Arrow) -> Vec<Seg> {
    let mut a1 = a1_in;
    if a1 <= a0 {
        a1 += TAU;
    }
    let sweep = a1 - a0;
    let mid = (ri + ro) / 2.0;
    // Clamped to half the arc, so a short feature degrades to a triangle
    // instead of inverting into a bow tie.
    let head = if arrow == Arrow::None {
        0.0
    } else {
        (8.0 / mid).min(sweep * 0.5)
    };
    let barb = ((ro - ri) * 0.35).min(2.5);

    let mut segs = Vec::new();
    let mv = |r: f64, a: f64| {
        let (x, y) = polar(cx, cy, r, a);
        Seg::Move(x, y)
    };
    let ln = |r: f64, a: f64| {
        let (x, y) = polar(cx, cy, r, a);
        Seg::Line(x, y)
    };
    let arc = |r: f64, from: f64, to: f64| Seg::Arc {
        cx,
        cy,
        r,
        from,
        to,
    };

    match arrow {
        Arrow::End => {
            let base = a1 - head;
            segs.push(mv(ro, a0));
            segs.push(arc(ro, a0, base));
            if head > 0.0 {
                segs.push(ln(ro + barb, base));
                segs.push(ln(mid, a1));
                segs.push(ln(ri - barb, base));
            }
            segs.push(ln(ri, base));
            segs.push(arc(ri, base, a0));
        }
        Arrow::Start => {
            let base = a0 + head;
            segs.push(mv(ro, a1));
            segs.push(arc(ro, a1, base));
            if head > 0.0 {
                segs.push(ln(ro + barb, base));
                segs.push(ln(mid, a0));
                segs.push(ln(ri - barb, base));
            }
            segs.push(ln(ri, base));
            segs.push(arc(ri, base, a1));
        }
        Arrow::None => {
            segs.push(mv(ro, a0));
            segs.push(arc(ro, a0, a1));
            segs.push(ln(ri, a1));
            segs.push(arc(ri, a1, a0));
        }
    }
    segs.push(Seg::Close);
    segs
}

/// Round a tick interval up to something a human would have chosen.
pub fn nice_step(raw: f64) -> u64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 1;
    }
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    };
    ((step * mag) as u64).max(1)
}

pub fn commas(v: u64) -> String {
    let s = v.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
