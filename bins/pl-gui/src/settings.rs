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
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Layout {
    pub panel_w: Option<f32>,
}

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
        if k.trim() == "panel_width" {
            // `is_finite` is the point of the parse, not a formality: see SANE.
            if let Ok(w) = v.trim().parse::<f32>() {
                if w.is_finite() && SANE.contains(&w) {
                    out.panel_w = Some(w);
                }
            }
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
        };
        assert_eq!(parse(&render(l)), l);
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
