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
        }
    }
}

/// The shortest and longest ORF threshold a file may name.
///
/// The same treatment `panel_width` gets and for the same reason: the file is
/// hand-editable, and a `code: 99` that reached `find_orfs` would be a panic or
/// a silently wrong protein.
const MIN_AA: std::ops::RangeInclusive<usize> = 1..=100_000;

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
        };
        assert_eq!(parse(&render(l)), l);
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
