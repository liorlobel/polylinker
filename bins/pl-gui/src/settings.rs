//! The handful of numbers a window remembers between runs.
//!
//! Written on a clean exit and read once at startup — except for the switches
//! whose own doc comments say they are saved on the click, which is every one
//! where a crash silently reverting the choice would be worse than losing it.
//!
//! This line used to read "Today that is one: how wide the details panel was
//! left", and it had been false for years of fields by the time anybody read it
//! again. No count is written here on purpose: [`Layout`] below IS the list,
//! and a number in a header is a number nobody recomputes.
//!
//! # Why not eframe's `persistence`
//!
//! Turning that feature on pulls `serde`, `ron` and `home` into the dependency
//! tree of the one binary allowed to have dependencies, plus `egui/persistence`
//! and `egui-winit/serde` — four crates to store one `f32`. It would also start
//! silently serialising the rest of egui's memory, which is a far larger
//! behaviour change than anyone asked for. A `key: value` line in a text file
//! is the whole of what is needed and costs nothing.
//!
//! # Why an absent file says nothing
//!
//! This is the opposite of the recovery file, whose *presence* is the crash
//! flag. A missing, unreadable or corrupt layout file falls back to the default
//! silently: a layout is not the user's data, and a banner about one would be
//! noise.

use std::path::PathBuf;

/// Format marker, so a future change can be recognised rather than guessed at.
const HEADER: &str = "polylinker-layout 1";

/// Light, dark, or whatever the desktop is set to.
///
/// **THREE STATES AND NOT A BOOLEAN, because "follow the system" is a real
/// answer and not the absence of one.** Polylinker has always painted whichever
/// theme the desktop asked for — `App::new` builds both `Style`s and lets egui
/// choose — and a two-state toggle would have quietly ended that the first time
/// anybody touched it: there would be no way back, and a user who changes their
/// desktop to dark at sunset would find one application that did not follow.
///
/// Spelled as words in the file rather than as `0`/`1`, unlike every other
/// switch here. Those are booleans and a digit is the honest encoding of one;
/// this is a choice among three, and `theme: dark` is what a person hand-editing
/// the file would write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Follow the desktop. The default, and what every release before this one
    /// did unconditionally.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    /// The spelling in the file. The same shape [`crate::aa::TrackMode`] uses,
    /// so both round-trip through one idiom.
    pub fn key(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    pub fn from_key(s: &str) -> Option<Self> {
        match s {
            "system" => Some(Theme::System),
            "light" => Some(Theme::Light),
            "dark" => Some(Theme::Dark),
            _ => None,
        }
    }
}

/// What a window remembers.
///
/// The sequence view's track switches live here and not in the document. They
/// are VIEW preferences: one per user, never per file, never in the `.dna` or
/// the `.gb`, and never an `OpKind`. A track is a view, so choosing to look at
/// one must not make a document dirty.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub panel_w: Option<f32>,
    pub aa_track: crate::aa::TrackMode,
    pub complement: Option<bool>,
    pub orf_track: bool,
    /// NCBI `transl_table` id. 11, for the reason `bins/pl/src/main.rs` already
    /// writes down: this is a plasmid tool and its molecules are read in
    /// bacteria.
    pub code: u8,
    pub orf_min_aa: usize,
    /// Put back the documents that were open when Polylinker last closed.
    ///
    /// Shipped ON, because it is what the workspace was asked for: no project
    /// file, no Save Workspace, the bench is simply there. Shipped WITH A SWITCH
    /// in the same commit, because "the app opens six files on launch" is a
    /// preference and not a fact of nature — a user who works from a file
    /// manager wants an empty window, and one whose last session ended in a
    /// directory that is now unmounted wants it more.
    pub restore_tabs: bool,
    /// The printed width of an exported figure, in millimetres.
    ///
    /// `None` exports at the scene's own units, which is what every export did
    /// before this existed and stays the default: a figure with no stated size
    /// is one a journal's system will scale itself, which is the right answer
    /// until somebody says otherwise.
    ///
    /// A NUMBER and not a named preset, because a preset is a pair of numbers
    /// (single and double column) plus a minimum type size, and storing the name
    /// would make the file's meaning depend on a table that can change under it.
    /// The presets choose the number; the file records what was chosen.
    pub figure_mm: Option<f64>,

    /// Ask the release page, once per run, whether a newer version exists.
    ///
    /// **Shipped OFF, and this is the one setting here where the default is a
    /// promise rather than a preference.** Every other field in this struct
    /// decides what the window looks like. This one decides whether Polylinker
    /// contacts a server at all, and the answer out of the box has to be no:
    /// an update check is a beacon — it tells whoever runs that hostname that
    /// this machine exists and is running this version — and on a bench machine
    /// holding unpublished sequence that is a real cost, paid by somebody who
    /// never asked to pay it. So it is off until a person turns it on, having
    /// read what the checkbox says gets sent.
    ///
    /// Note what it does NOT govern: `pl update`, which is a command somebody
    /// types, needs no setting and does not read this one. This field exists
    /// only because the desktop app has no argv to say "yes, now".
    ///
    /// It also does not enable downloading. The GUI checks and shows a notice;
    /// the download is `pl update`. See `crate::update` for why.
    pub update_check: bool,

    /// Ask the feature database what is in a molecule as soon as it is opened.
    ///
    /// **Shipped ON, and the argument is the exact opposite of
    /// [`Layout::update_check`]'s** — which is why the two are written out
    /// separately instead of sharing a sentence. The update check is off
    /// because it decides whether this machine contacts a server at all, and no
    /// default may make that choice for somebody. Annotation decides nothing of
    /// the kind: the database is three tables compiled into the binary, the
    /// scan runs in this process, and nothing leaves the machine. There is no
    /// privacy cost to weigh, so the only cost left is time.
    ///
    /// That cost was measured rather than guessed — whole-process `pl annotate`
    /// less a `pl --version` floor of about 30 ms — at 25 ms for a 10 kb
    /// plasmid, 115 ms at 400 kb, 315 ms at 1.2 Mb and 4.2 s at 4.64 Mb. **None
    /// of it is paid by the first paint**, because it happens on a worker — the
    /// same place the 58-enzyme digest already goes, which every document
    /// starts unconditionally on open with no setting at all. Timed the same
    /// way, that digest is about 10 ms at 10 kb and 110 ms at 400 kb, so at
    /// plasmid scale this adds a second scan of about the size of one the
    /// application already runs without asking anybody. Shipping the flagship
    /// feature off to save that would be a reflex, not a decision.
    ///
    /// It is at GENOME scale that the two part company, and there annotation is
    /// the more expensive of the pair: 4.2 s against the digest's 1,712 ms at
    /// 4.6 Mb, and unlike the digest it cannot be stopped part-way (see
    /// `crate::doc::ProposalState`). That case is what the switch is for — that,
    /// and the person who annotates by hand and finds a panel offering names
    /// nothing but noise. It is not an apology for the default.
    ///
    /// What it does NOT govern: `pl annotate`, which is a command somebody
    /// types; and whether anything is ever written to the document. Proposals
    /// stay proposals until the user presses Accept, whatever this is set to.
    pub annotate_on_open: bool,

    /// Show hits that cover only part of a database feature.
    ///
    /// OFF, matching `pl annotate`, whose `--fragments` is the same escape
    /// hatch. A fragment is a hit below
    /// `pl_features::annotate::Config::fragment_coverage` — a piece of a
    /// feature, not the feature — and it is offered under the whole feature's
    /// name, so the first thing a user sees must not be `AmpR` against 200 bp
    /// of it. One click away, because a clipped promoter or a truncated origin
    /// is a real and common thing to want to see.
    pub annotate_fragments: bool,

    /// Search the rows no curator has signed off, as well as the shippable
    /// subset.
    ///
    /// OFF, matching `pl annotate --include-proposed`, and this default is the
    /// project's central rule rather than a preference:
    /// [`pl_features::Db::reviewed`] says "A caller that wants the proposed rows
    /// too has to ask for them by name, and owes the user that sentence." The
    /// checkbox is the asking; the line beside it is the sentence.
    ///
    /// It changed nothing findable until 2026-08-10, and that was exactly why
    /// it had to exist before it mattered. It matters now: the table holds 115
    /// rows against 89 signatures, so turning this on adds 26 machine-extracted
    /// records — 14 selection markers and 12 promoters, terminators and poly(A)
    /// signals — that no human has read. Because the switch was built before it
    /// did anything, the difference between the two searches is still something
    /// the user chose rather than something that appeared.
    pub annotate_unreviewed: bool,

    /// Which way round the window is painted. See [`Theme`].
    ///
    /// A VIEW preference like the track switches above: one per user, never per
    /// file, never an `OpKind`. Choosing to look at a plasmid in the dark must
    /// not make the document dirty.
    pub theme: Theme,

    /// Resolution for the raster export, in dots per inch.
    ///
    /// Not an `Option`: unlike a printed width, a raster always has SOME
    /// resolution, and there is no honest way to write a PNG without one. 300
    /// is the floor every journal preset in `pl_draw::page` states for line
    /// art, so it is the default that does not need explaining.
    pub figure_dpi: f64,
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            panel_w: None,
            aa_track: crate::aa::TrackMode::default(),
            // `None` means "follow the molecule": on when the file says double
            // stranded or does not say, off when it says single.
            complement: None,
            orf_track: false,
            code: 11,
            orf_min_aa: 30,
            restore_tabs: true,
            update_check: false,
            annotate_on_open: true,
            annotate_fragments: false,
            annotate_unreviewed: false,
            theme: Theme::System,
            figure_mm: None,
            figure_dpi: 300.0,
        }
    }
}

/// The shortest and longest ORF threshold a file may name.
///
/// The same treatment `panel_width` gets and for the same reason: the file is
/// hand-editable, and a `code: 99` that reached `find_orfs` would be a panic or
/// a silently wrong protein.
const MIN_AA: std::ops::RangeInclusive<usize> = 1..=100_000;

/// The ORF thresholds the combo offers.
///
/// A short list rather than a spinner over a hundred thousand values, because
/// the number is not continuous in practice — it is a choice between four
/// questions, and the list is what makes that legible:
///
/// - **20 aa** — small ORFs and short peptides. Noisy on a plasmid and
///   deliberately available: a 60 bp leader peptide is real and is invisible at
///   any higher setting.
/// - **30 aa** — the default, and `pl_core::orf::Params`' default, so the app
///   and the CLI agree unless the user says otherwise.
/// - **50 aa** — roughly where chance ORFs in a GC-balanced sequence stop being
///   common, so the strip starts reading as signal.
/// - **100 aa** — real genes only. What you want on a genome, where 30 aa
///   returns thousands.
///
/// `pub` because `MIN_AA` above is private and the widget must not invent its
/// own bounds; every entry here is inside it, which the round-trip test pins.
pub const ORF_MIN_AA_CHOICES: &[usize] = &[20, 30, 50, 100];

/// The widest and narrowest a stored width may be before it is disbelieved.
///
/// Not the panel's own limits — those are recomputed every frame from the live
/// window and clamp a merely *stale* number correctly. This band exists for a
/// hand-edited or truncated file: `"panel_width: nan"` parses, and a NaN
/// reaching `Rangef` does not panic. It propagates through `Rect` arithmetic
/// into the painter, where geometry simply vanishes with nothing on screen
/// explaining why — the worst failure mode available.
const SANE: std::ops::RangeInclusive<f32> = 100.0..=10_000.0;

fn path() -> Result<PathBuf, String> {
    Ok(crate::recover::state_base()?.join("layout"))
}

/// Read the layout, or the defaults. Never an error the user is told about.
pub fn load() -> Layout {
    let Ok(p) = path() else {
        return Layout::default();
    };
    let Ok(text) = std::fs::read_to_string(p) else {
        return Layout::default();
    };
    parse(&text)
}

pub fn parse(text: &str) -> Layout {
    let mut out = Layout::default();
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(HEADER) {
        return out;
    }
    for line in lines {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim();
        // Unknown keys are skipped, which is why the header stays at 1: bumping
        // it would make an older binary discard a newer file wholesale, where
        // adding keys costs nothing in either direction.
        match k.trim() {
            "panel_width" => {
                // `is_finite` is the point of the parse, not a formality: see
                // SANE.
                if let Ok(w) = v.parse::<f32>() {
                    if w.is_finite() && SANE.contains(&w) {
                        out.panel_w = Some(w);
                    }
                }
            }
            "aa_track" => {
                if let Some(m) = crate::aa::TrackMode::from_key(v) {
                    out.aa_track = m;
                }
            }
            "complement" => {
                out.complement = match v {
                    "0" => Some(false),
                    "1" => Some(true),
                    _ => None,
                }
            }
            "orf_track" => out.orf_track = v == "1",
            "code" => {
                // Through `translate::table`, so a hand-edited `code: 99` is
                // disbelieved here rather than reaching `find_orfs`.
                if let Some(c) = v.parse::<u8>().ok().and_then(pl_core::translate::table) {
                    out.code = c.id;
                }
            }
            "orf_min_aa" => {
                if let Ok(m) = v.parse::<usize>() {
                    if MIN_AA.contains(&m) {
                        out.orf_min_aa = m;
                    }
                }
            }
            // `!= "0"` rather than `== "1"`, and that is the only place in this
            // parser where the default is not what an unreadable value falls
            // back to. The default here is ON, so a garbled line must land on
            // ON: reading `restore_tabs: yes` as OFF would silently stop
            // restoring anybody's bench and look exactly like the feature being
            // broken.
            "restore_tabs" => out.restore_tabs = v != "0",
            // `== "1"`, the exact opposite of the line above, and for the same
            // reason read the other way round: an unreadable value must land on
            // the default, and this default is OFF. A `!= "0"` here would turn a
            // truncated write, a hand-edit, or a `update_check: no` into a
            // machine that contacts a server because its settings file was
            // damaged. Failing closed is the only acceptable direction for a
            // switch that governs whether anything is sent at all.
            "update_check" => out.update_check = v == "1",
            // `!= "0"`, like `restore_tabs` above and NOT like `update_check`
            // one line up, and the direction is the whole of the decision. The
            // default here is ON, so a garbled value must land on ON: a
            // truncated write or a hand-edited `annotate_on_open: yes` reading
            // as OFF would silently stop annotating anybody's plasmids and be
            // indistinguishable from the feature being broken. Failing OPEN is
            // safe here for the reason it is not safe there — nothing is sent
            // anywhere, and nothing is written to a document either way.
            "annotate_on_open" => out.annotate_on_open = v != "0",
            // ...and these two are `== "1"`, because their defaults are OFF and
            // the same rule read the other way round. `annotate_unreviewed`
            // especially: a damaged settings file must not be able to widen a
            // search to rows no human has checked, because the row it then
            // proposes is one the user never asked to be offered.
            "annotate_fragments" => out.annotate_fragments = v == "1",
            "annotate_unreviewed" => out.annotate_unreviewed = v == "1",
            // Through `Theme::from_key`, like `aa_track` above and for the same
            // reason: the file is hand-editable, and the only values that may
            // reach the window are the three that name a theme. Anything else
            // keeps the default, which is `System` — the behaviour of every
            // release before this setting existed, so a damaged or older file
            // lands exactly where it used to be.
            "theme" => {
                if let Some(t) = Theme::from_key(v) {
                    out.theme = t;
                }
            }
            // Same band as every other number here, and for the same reason:
            // the file is hand-editable and a `figure_mm: nan` that reached
            // `Fit::to_width_mm` would propagate through the scale into every
            // coordinate, and geometry would simply vanish with nothing on
            // screen explaining why. 20 mm is a small inset; 500 mm is past any
            // journal's page.
            "figure_mm" => {
                if let Ok(mm) = v.parse::<f64>() {
                    if mm.is_finite() && (20.0..=500.0).contains(&mm) {
                        out.figure_mm = Some(mm);
                    }
                }
            }
            // Banded like every other number here, and matching `pl export
            // --dpi` exactly, so the app and the command line refuse the same
            // things. A garbled value keeps the default rather than reaching
            // the encoder: a `figure_dpi: 0` would size the canvas to nothing.
            "figure_dpi" => {
                if let Ok(dpi) = v.parse::<f64>() {
                    if dpi.is_finite() && (72.0..=2400.0).contains(&dpi) {
                        out.figure_dpi = dpi;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

pub fn render(l: Layout) -> String {
    let mut s = String::from(HEADER);
    s.push('\n');
    if let Some(w) = l.panel_w {
        s.push_str(&format!("panel_width: {:.0}\n", w));
    }
    s.push_str(&format!("aa_track: {}\n", l.aa_track.key()));
    if let Some(c) = l.complement {
        s.push_str(&format!("complement: {}\n", u8::from(c)));
    }
    s.push_str(&format!("orf_track: {}\n", u8::from(l.orf_track)));
    s.push_str(&format!("code: {}\n", l.code));
    s.push_str(&format!("orf_min_aa: {}\n", l.orf_min_aa));
    s.push_str(&format!("restore_tabs: {}\n", u8::from(l.restore_tabs)));
    // Always written, including the `0`, unlike `figure_mm` which is omitted
    // when unset. A user who wants to know whether their copy is configured to
    // contact anything should be able to read the answer out of this file
    // rather than infer it from a line's absence — absence is what a truncated
    // write also looks like.
    s.push_str(&format!("update_check: {}\n", u8::from(l.update_check)));
    // Always written, all three, including the values that equal the default.
    // A user asking "why is this plasmid covered in suggestions" — or why it is
    // not — should be able to read the answer out of this file rather than
    // infer it from a line's absence, and absence is also what a truncated
    // write looks like.
    s.push_str(&format!(
        "annotate_on_open: {}\n",
        u8::from(l.annotate_on_open)
    ));
    s.push_str(&format!(
        "annotate_fragments: {}\n",
        u8::from(l.annotate_fragments)
    ));
    s.push_str(&format!(
        "annotate_unreviewed: {}\n",
        u8::from(l.annotate_unreviewed)
    ));
    // Always written, including `system`, and for `figure_dpi`'s reason rather
    // than `figure_mm`'s: there is no "unset" theme. A window is painted one
    // way or the other on every frame, `system` is a decision about how that is
    // chosen and not the absence of one, and writing it is what makes the file
    // say what the application will actually do.
    s.push_str(&format!("theme: {}\n", l.theme.key()));
    // Written only when set, so an untouched file says nothing about figure size
    // rather than asserting the default as a choice.
    if let Some(mm) = l.figure_mm {
        s.push_str(&format!("figure_mm: {mm:.1}\n"));
    }
    // Always written, unlike the width above, because a raster always has a
    // resolution: there is no "unset" for it, only the default, and writing the
    // default is what makes the file say what the app will actually do.
    s.push_str(&format!("figure_dpi: {:.0}\n", l.figure_dpi));
    s
}

/// Write the layout. Failure is silent for the same reason the read is.
pub fn save(l: Layout) {
    let Ok(p) = path() else { return };
    if let Some(dir) = p.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let _ = std::fs::write(p, render(l));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_width_round_trips() {
        let l = Layout {
            panel_w: Some(560.0),
            ..Default::default()
        };
        assert_eq!(parse(&render(l)), l);
    }

    #[test]
    fn the_track_switches_round_trip() {
        let l = Layout {
            panel_w: Some(560.0),
            aa_track: crate::aa::TrackMode::Selection,
            complement: Some(false),
            orf_track: true,
            code: 4,
            orf_min_aa: 12,
            restore_tabs: false,
            // Deliberately not the default, for the same reason `figure_dpi`
            // below is not: a field that never reached `render` would still
            // round-trip if the value under test were the one `Default` puts
            // back.
            update_check: true,
            // All three deliberately opposite to their defaults, for the reason
            // stated above: a field that never reached `render` would still
            // round-trip if the value under test were the one `Default` puts
            // back.
            annotate_on_open: false,
            annotate_fragments: true,
            annotate_unreviewed: true,
            // Deliberately not the default, for the reason stated above.
            theme: Theme::Dark,
            figure_mm: Some(89.0),
            // Deliberately not the default, so a `figure_dpi` that never
            // reached the file would fail here rather than round-trip through
            // its own absence.
            figure_dpi: 600.0,
        };
        assert_eq!(parse(&render(l)), l);
    }

    /// The update check survives a round trip in **both** directions.
    ///
    /// Round-tripping `true` is the weaker half and on its own it is close to a
    /// check that cannot fail: `update_check: false` also round-trips through a
    /// `render` that never wrote the key and a `parse` that never read it,
    /// because both ends land on the default. So both are asserted, and the ON
    /// case is additionally required to appear in the text — that is what says
    /// the value is stored rather than reconstructed.
    #[test]
    fn the_update_check_round_trips_in_both_directions() {
        for on in [false, true] {
            let l = Layout {
                update_check: on,
                ..Default::default()
            };
            let text = render(l);
            assert!(
                text.contains(if on {
                    "update_check: 1"
                } else {
                    "update_check: 0"
                }),
                "the file does not record update_check={on}:\n{text}"
            );
            assert_eq!(parse(&text).update_check, on);
            assert_eq!(parse(&text), l);
        }
    }

    /// Nothing a damaged file can hold switches the update check ON.
    ///
    /// The direction is the whole test. `restore_tabs` falls back to ON because
    /// losing somebody's bench is the worse failure; this falls back to OFF
    /// because the worse failure here is a machine that contacts a server its
    /// owner did not tell it to. A truncated write, a hand-edit and a file from
    /// a future version all have to land on silence.
    #[test]
    fn nothing_a_damaged_file_can_hold_switches_the_update_check_on() {
        for bad in [
            "update_check: yes",
            "update_check: true",
            "update_check: on",
            "update_check: 2",
            "update_check: -1",
            "update_check:",
            "update_check: 1x",
            "update_check: 1 1",
            "Update_Check: 1", // keys are matched exactly, not case-folded
            "update_check: 0",
        ] {
            let l = parse(&format!("{HEADER}\n{bad}\n"));
            assert!(
                !l.update_check,
                "{bad:?} switched the update check on; the default must fail closed"
            );
        }
        // And the guard is not vacuous — the one spelling that really does mean
        // on still means on. Without this the assertions above would pass
        // against a parser that ignored the key entirely.
        assert!(parse(&format!("{HEADER}\nupdate_check: 1\n")).update_check);
    }

    /// A file with no `update_check` line at all — every layout written before
    /// this setting existed — leaves it off.
    #[test]
    fn a_layout_file_from_before_this_setting_does_not_opt_anybody_in() {
        for older in [
            "polylinker-layout 1\npanel_width: 560\nrestore_tabs: 1\n",
            "polylinker-layout 1\n",
            "",
            "something else entirely\nupdate_check: 1\n",
        ] {
            assert!(
                !parse(older).update_check,
                "{older:?} opted an existing user in without being asked"
            );
        }
    }

    /// The theme survives a round trip in **all three** directions.
    ///
    /// `the_update_check_round_trips_in_both_directions`'s argument, one state
    /// wider. Round-tripping the two non-default values alone would be the
    /// weaker half: `System` also round-trips through a `render` that never
    /// wrote the key and a `parse` that never read it, because both ends land
    /// on the default and the test passes over a field that is not stored at
    /// all. So every state is asserted, and each is additionally required to
    /// appear in the text by name.
    #[test]
    fn the_theme_choice_round_trips_in_every_direction() {
        for t in [Theme::System, Theme::Light, Theme::Dark] {
            let l = Layout {
                theme: t,
                ..Default::default()
            };
            let text = render(l);
            assert!(
                text.contains(&format!("theme: {}", t.key())),
                "the file does not record theme={:?}:\n{text}",
                t
            );
            assert_eq!(parse(&text).theme, t);
            assert_eq!(parse(&text), l);
        }
        // And the three keys are distinct, so the assertions above cannot be
        // satisfied by an enum whose variants all spell the same word.
        let keys: std::collections::BTreeSet<_> = [Theme::System, Theme::Light, Theme::Dark]
            .iter()
            .map(|t| t.key())
            .collect();
        assert_eq!(keys.len(), 3);
    }

    /// Nothing a damaged or older file can hold moves the window off the
    /// desktop's own theme.
    ///
    /// The direction is the test, as it is for `update_check`. The default here
    /// is `System`, which is what every release before this setting did
    /// unconditionally, so an unreadable value has to land there: a truncated
    /// write that pinned somebody to light mode would look exactly like the
    /// application ignoring their desktop.
    #[test]
    fn nothing_a_damaged_file_can_hold_pins_the_window_to_one_theme() {
        for bad in [
            "theme: Dark", // keys are matched exactly, not case-folded
            "theme: DARK",
            "theme: 1",
            "theme: true",
            "theme:",
            "theme: dark light",
            "theme: darkness",
            "theme: auto",
            "theme: system",
        ] {
            let l = parse(&format!("{HEADER}\n{bad}\n"));
            assert_eq!(
                l.theme,
                Theme::System,
                "{bad:?} took the window off the desktop's theme"
            );
        }
        // Not vacuous: the two spellings that really do pin a theme still do.
        assert_eq!(
            parse(&format!("{HEADER}\ntheme: dark\n")).theme,
            Theme::Dark
        );
        assert_eq!(
            parse(&format!("{HEADER}\ntheme: light\n")).theme,
            Theme::Light
        );
    }

    /// A layout file written before the theme setting existed follows the
    /// desktop, which is what that user already had.
    #[test]
    fn a_layout_file_from_before_the_theme_setting_still_follows_the_desktop() {
        for older in [
            "polylinker-layout 1\npanel_width: 560\nrestore_tabs: 1\n",
            "polylinker-layout 1\n",
            "",
            "something else entirely\ntheme: dark\n",
        ] {
            assert_eq!(
                parse(older).theme,
                Theme::System,
                "{older:?} changed an existing user's theme without being asked"
            );
        }
    }

    /// Each annotation switch survives a round trip in **both** directions.
    ///
    /// `the_update_check_round_trips_in_both_directions`, applied to the three
    /// settings this feature added, and for its reason: round-tripping the
    /// non-default value alone is the weaker half, because the default also
    /// round-trips through a `render` that never wrote the key and a `parse`
    /// that never read it — both ends land on the default and the test passes
    /// over a field that is not stored at all. So both values are asserted, and
    /// both are additionally required to appear in the text.
    #[test]
    fn the_annotation_settings_round_trip_in_both_directions() {
        for on in [false, true] {
            let l = Layout {
                annotate_on_open: on,
                annotate_fragments: on,
                annotate_unreviewed: on,
                ..Default::default()
            };
            let text = render(l);
            let d = u8::from(on);
            for key in [
                "annotate_on_open",
                "annotate_fragments",
                "annotate_unreviewed",
            ] {
                assert!(
                    text.contains(&format!("{key}: {d}")),
                    "the file does not record {key}={on}:\n{text}"
                );
            }
            assert_eq!(parse(&text), l);
        }
    }

    /// A layout file written before annotation existed leaves it ON, and leaves
    /// the two "show me more" switches OFF.
    ///
    /// The three defaults do not point the same way, so an upgrade is the one
    /// moment all three can be got wrong at once. An existing user must not
    /// lose the flagship feature to a file that predates it, and must not be
    /// opted into unreviewed rows by one either.
    #[test]
    fn a_layout_file_from_before_annotation_gets_the_defaults() {
        for older in [
            "polylinker-layout 1\npanel_width: 560\nrestore_tabs: 1\n",
            "polylinker-layout 1\n",
            "",
            "something else entirely\nannotate_on_open: 0\n",
        ] {
            let l = parse(older);
            assert!(
                l.annotate_on_open,
                "{older:?} silently switched annotation off"
            );
            assert!(!l.annotate_fragments, "{older:?} opted into fragments");
            assert!(
                !l.annotate_unreviewed,
                "{older:?} opted into rows no curator has signed off"
            );
        }
    }

    /// Nothing a damaged file can hold widens the search.
    ///
    /// The direction, again, is the test. `annotate_on_open` fails OPEN because
    /// the worse failure is a user who quietly stops being offered anything;
    /// the other two fail CLOSED because the worse failure is a search widened
    /// by a corrupt settings file — most of all to rows no human has checked,
    /// which is the one thing `features/SIGNOFF.tsv` exists to gate.
    #[test]
    fn a_damaged_file_cannot_widen_the_annotation_search() {
        for bad in ["yes", "true", "on", "2", "-1", "", "1x", "1 1", "0"] {
            let l = parse(&format!(
                "{HEADER}\nannotate_fragments: {bad}\nannotate_unreviewed: {bad}\n"
            ));
            assert!(
                !l.annotate_fragments && !l.annotate_unreviewed,
                "{bad:?} widened the search; both must fail closed"
            );
            let l = parse(&format!("{HEADER}\nannotate_on_open: {bad}\n"));
            assert!(
                l.annotate_on_open || bad == "0",
                "{bad:?} switched annotation off; only a literal 0 may"
            );
        }
        // And neither guard is vacuous: the spellings that really do mean what
        // they say still mean it.
        let on = parse(&format!(
            "{HEADER}\nannotate_fragments: 1\nannotate_unreviewed: 1\nannotate_on_open: 0\n"
        ));
        assert!(on.annotate_fragments && on.annotate_unreviewed && !on.annotate_on_open);
        // Keys are matched exactly, not case-folded, here as everywhere else.
        assert!(!parse(&format!("{HEADER}\nAnnotate_Fragments: 1\n")).annotate_fragments);
    }

    /// A setting whose default is ON, so an unreadable value must land on ON.
    /// Falling back to OFF would stop restoring anybody's bench and be
    /// indistinguishable from the feature not working.
    ///
    /// This said "the one setting whose default is ON" until `annotate_on_open`
    /// joined it — see `a_damaged_file_cannot_widen_the_annotation_search`,
    /// which asserts the same direction for the same reason.
    #[test]
    fn a_garbled_restore_tabs_line_leaves_the_workspace_switched_on() {
        for line in ["restore_tabs: yes", "restore_tabs:", "restore_tabs: true"] {
            let l = parse(&format!("{HEADER}\n{line}\n"));
            assert!(l.restore_tabs, "{line:?} switched the workspace off");
        }
        // And the one value that really does mean off.
        assert!(!parse(&format!("{HEADER}\nrestore_tabs: 0\n")).restore_tabs);
    }

    #[test]
    fn nothing_a_file_can_hold_reaches_find_orfs_as_a_table_that_does_not_exist() {
        // The same treatment `panel_width` gets. A `code: 99` is not a table,
        // and `translate::table(99)` is `None`; reaching `find_orfs` with one
        // would be a panic or a silently wrong protein, and the second is
        // worse.
        for bad in [
            "polylinker-layout 1\ncode: 99\n",
            "polylinker-layout 1\ncode: 0\n",
            "polylinker-layout 1\ncode: 300\n",
            "polylinker-layout 1\ncode: -1\n",
            "polylinker-layout 1\ncode: banana\n",
            "polylinker-layout 1\ncode:\n",
            "polylinker-layout 1\norf_min_aa: 0\n",
            "polylinker-layout 1\norf_min_aa: -5\n",
            "polylinker-layout 1\norf_min_aa: 99999999999999999999\n",
            "polylinker-layout 1\naa_track: sideways\n",
        ] {
            let l = parse(bad);
            assert!(
                pl_core::translate::table(l.code).is_some(),
                "{bad:?} produced code {}",
                l.code
            );
            assert!(
                MIN_AA.contains(&l.orf_min_aa),
                "{bad:?} produced min_aa {}",
                l.orf_min_aa
            );
        }
    }

    #[test]
    fn nothing_a_file_can_hold_reaches_the_panel_as_a_nan() {
        // A NaN in `Rangef` does not panic. It makes geometry disappear.
        for bad in [
            "polylinker-layout 1\npanel_width: nan\n",
            "polylinker-layout 1\npanel_width: inf\n",
            "polylinker-layout 1\npanel_width: -inf\n",
            "polylinker-layout 1\npanel_width: -400\n",
            "polylinker-layout 1\npanel_width: 99\n",
            "polylinker-layout 1\npanel_width: 1e9\n",
            "polylinker-layout 1\npanel_width: banana\n",
            "polylinker-layout 1\npanel_width:\n",
            "polylinker-layout 1\n",
            "",
            "something else entirely\npanel_width: 560\n",
            "\u{0}\u{1}\u{2}",
        ] {
            let l = parse(bad);
            assert!(
                l.panel_w.is_none_or(|w| w.is_finite() && SANE.contains(&w)),
                "{bad:?} produced {:?}",
                l.panel_w
            );
        }
    }

    #[test]
    fn a_layout_file_is_not_written_into_the_recovery_directory() {
        // Clearing crash drafts must not reset the window layout, and `stale`
        // ignoring the file is not the same as the file not being there.
        let Ok(p) = path() else { return };
        assert!(
            !p.components().any(|c| c.as_os_str() == "recovery"),
            "{}",
            p.display()
        );
    }
}
