//! The handful of numbers a window remembers between runs.
//!
//! Today that is one: how wide the details panel was left. It is written on a
//! clean exit and read once at startup.
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
        };
        assert_eq!(parse(&render(l)), l);
    }

    /// The one setting whose default is ON, so an unreadable value must land on
    /// ON. Falling back to OFF would stop restoring anybody's bench and be
    /// indistinguishable from the feature not working.
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
