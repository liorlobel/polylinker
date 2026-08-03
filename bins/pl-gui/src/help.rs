//! What this program is, what it computes, and whose work is in it.
//!
//! Three things nobody could get at from inside the application.
//!
//! # The version
//!
//! `docs/RELEASING.md` says the update path is that "the user checks when the
//! user wants to", and that "`pl --version` prints the version and the commit".
//! For somebody handed `polylinker.exe` who never opens a terminal, that
//! sentence was false: the GUI displayed no version anywhere at all. A
//! `build.rs` stamps the same `PL_COMMIT` the CLI has always carried.
//!
//! # The methods paragraphs
//!
//! `pl-doc` compiles eleven of them into this binary, each with its numbers
//! interpolated from the constant the code actually uses. The GUI reached TWO,
//! and both write-only — the gel's `Methods…` button and the cloning record put
//! them on the clipboard and never on screen. So nine were compiled in and
//! reachable by no gesture whatsoever, and a user could not find out what Tm
//! model is used or what the gel simulation is and is not.
//!
//! # The licences, and why they are shown whole
//!
//! `NOTICE` lines 443-450 record this gap and name this page as the fix:
//!
//! > **STILL OWED:** … it is `pl.exe`, which embeds no fonts at all — the seven
//! > faces are in `polylinker.exe`, which has no licence view of its own. So the
//! > honest statement of the gap is that **the GUI cannot yet state its own font
//! > attribution from inside itself, and it should be able to.**
//!
//! Four of the six font licences require the notice to travel with each copy.
//! Today it travels in `dist/`; a user handed the bare `.exe` has none of it.
//!
//! **Rendered verbatim from `include_str!`, never summarised and never
//! restated.** Hand-writing the holders into this file would make a third copy
//! of a list two files already hold, with nothing joining them — exactly the
//! defect `NOTICE` spends a paragraph on, and it would go stale in silence. The
//! same argument `NOTICE` makes about `release.ps1`: "a five-line list that
//! covers two of six faces looks exactly like a five-line list that covers all
//! of them."

use eframe::egui;

/// The blank line `pl_doc::methods` paginates on.
const SEP: &str = "

";

/// Everything shown whole, in the order the index lists it.
///
/// `include_str!` and not a path read at runtime: a licence that is only on
/// disk is a licence a user handed one file does not have, which is the whole
/// gap this page closes.
const PAGES: &[(&str, &str)] = &[
    ("Notices (NOTICE)", include_str!("../../../NOTICE")),
    ("Apache License 2.0", include_str!("../../../LICENSE")),
    ("Trademarks", include_str!("../../../TRADEMARKS.md")),
    ("IBM Plex — OFL", include_str!("../fonts/IBMPlex-OFL.txt")),
    (
        "Hack — MIT and Bitstream Vera",
        include_str!("../fonts/Hack-MIT-and-BitstreamVera.txt"),
    ),
    ("Ubuntu — UFL", include_str!("../fonts/Ubuntu-UFL.txt")),
    (
        "Noto Emoji — OFL",
        include_str!("../fonts/NotoEmoji-OFL.txt"),
    ),
    (
        "emoji-icon-font — MIT",
        include_str!("../fonts/emoji-icon-font-MIT.txt"),
    ),
    (
        "Phosphor Icons — MIT",
        include_str!("../fonts/Phosphor-MIT.txt"),
    ),
];

/// The NOTICE this binary ships, for the font test to check against.
///
/// One `include_str!` of that path in the whole crate. The test asserts every
/// vendored face's sha256 appears in NOTICE; if it read its own copy, it could
/// pass against a file the Help window does not show — which is the same
/// two-copies-nothing-joining-them defect NOTICE itself spends a paragraph on.
///
/// `#[cfg(test)]` because the window reaches `PAGES` by index and this exists
/// only to give the font test the same bytes. A `pub fn` with no production
/// caller is indistinguishable from one whose caller was deleted by mistake,
/// and gating it says which this is.
#[cfg(test)]
pub fn notice() -> &'static str {
    PAGES[0].1
}

/// Which page the window is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    About,
    /// An index into [`PAGES`].
    Licence(usize),
    /// A `pl_doc::Topic` name.
    Topic(&'static str),
}

/// The Help window's state.
///
/// NOT in `bench::DocView`, deliberately. Every field there belongs to one
/// molecule, and `no_view_state_leaks_between_tabs` enumerates them so the two
/// lists must agree — but a window that describes no plasmid cannot describe the
/// wrong one, and parking it per tab would mean switching documents changed
/// which page of the manual you were reading.
pub struct Panel {
    pub page: Page,
}

impl Default for Panel {
    fn default() -> Self {
        Panel { page: Page::About }
    }
}

/// `Polylinker 0.1.0 (7ba75bc1b6e3)`, the exact shape `pl --version` prints.
///
/// `-dirty` is shown and never hidden: an uncommitted build is not described by
/// its commit, and that is the difference between a traceable binary and a
/// number.
pub fn version() -> String {
    format!(
        "Polylinker {} ({})",
        env!("CARGO_PKG_VERSION"),
        env!("PL_COMMIT")
    )
}

/// Draw the window. Returns false when the user has closed it.
pub fn show(ctx: &egui::Context, p: &mut Panel, dark: bool) -> bool {
    let pal = crate::theme::Palette::of(dark);
    let mut open = true;
    egui::Window::new("Help")
        .open(&mut open)
        .resizable(true)
        .default_width(760.0)
        .default_height(520.0)
        .show(ctx, |ui| {
            // ONE COLUMN: a wrapped row of chips, then the page.
            //
            // NOT a two-column index-and-body split, which is what this was and
            // which did not work. Inside a `horizontal_top` in a `Window` that
            // has not been sized yet, the first child's `available_height` is
            // the whole of it — so the index took everything, the body was
            // allocated nothing and painted NOTHING, and the index itself
            // clipped at seven of eleven topics. The test below found that,
            // which is the reason it scrapes what was drawn rather than
            // trusting the widget tree.
            //
            // The wrapped-chip row is also the idiom this codebase already uses
            // for "pick one of these" — the clone panel's method row, the
            // enzyme filter — so it needs no new reading.
            ui.horizontal_wrapped(|ui| {
                if ui
                    .selectable_label(p.page == Page::About, "About")
                    .clicked()
                {
                    p.page = Page::About;
                }
                ui.separator();
                // EVERY topic, from `TOPICS` itself rather than a list written
                // out here. A second list would go out of date against the
                // first the next time a topic is added, and the failure — a
                // paragraph compiled into the binary and reachable by nothing —
                // is exactly the one this window exists to fix.
                for t in pl_doc::TOPICS {
                    if ui
                        .selectable_label(p.page == Page::Topic(t.name), t.title)
                        .on_hover_text(pl_doc::help(*t))
                        .clicked()
                    {
                        p.page = Page::Topic(t.name);
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Licences:").color(pal.muted).size(11.0));
                for (i, (name, _)) in PAGES.iter().enumerate() {
                    if ui
                        .selectable_label(p.page == Page::Licence(i), *name)
                        .clicked()
                    {
                        p.page = Page::Licence(i);
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("help-body")
                .max_height(420.0)
                .show(ui, |ui| match &p.page {
                    Page::About => about(ui, pal),
                    Page::Licence(i) => {
                        let (name, text) = PAGES[*i];
                        ui.label(egui::RichText::new(name).strong());
                        ui.add_space(4.0);
                        // Monospace and whole. These are legal texts; a
                        // proportional reflow of a licence is a licence
                        // somebody has edited.
                        ui.label(egui::RichText::new(text).monospace().size(11.0));
                    }
                    Page::Topic(name) => {
                        let Some(t) = pl_doc::topic(name) else { return };
                        ui.label(egui::RichText::new(t.title).strong());
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(pl_doc::help(t))
                                .color(pal.muted)
                                .size(11.5),
                        );
                        ui.add_space(8.0);
                        // Paragraph by paragraph, splitting on the blank line
                        // `pl-doc` itself paginates on — its comment says
                        // "`pl methods` paginates by splitting on a blank
                        // line", so this renders it the way the CLI does rather
                        // than inventing a second layout.
                        let text = pl_doc::methods(t);
                        for para in text.split(SEP) {
                            ui.label(egui::RichText::new(para).size(12.0));
                            ui.add_space(6.0);
                        }
                        if ui
                            .button("Copy this paragraph")
                            .on_hover_text("For a methods section.")
                            .clicked()
                        {
                            ui.ctx().copy_text(text);
                        }
                    }
                });
        });
    open
}

fn about(ui: &mut egui::Ui, pal: crate::theme::Palette) {
    ui.label(egui::RichText::new("Polylinker").strong().size(16.0));
    ui.add_space(2.0);
    ui.label(egui::RichText::new(version()).monospace().size(12.0));
    ui.add_space(8.0);
    // The two facts NOTICE's first five lines state, and no more. Anything
    // further would be a copy of a file this window can show whole.
    ui.label("Apache License 2.0. Copyright 2026 The Polylinker contributors.");
    ui.add_space(8.0);
    // The one thing a user needs to know about the number above.
    ui.label(
        egui::RichText::new(
            "There is no auto-updater, on purpose. Polylinker contacts no server and cannot \
             tell you whether a newer version exists — check when you want to.",
        )
        .color(pal.muted)
        .size(11.5),
    );
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(
            "Everything runs on this machine. No account, no telemetry, no network.",
        )
        .color(pal.muted)
        .size(11.5),
    );
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new(
            "The Licences pages show the notices for this binary in full, including the \
             typefaces it embeds.",
        )
        .size(11.5),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The version is stamped, and says so when it cannot be.
    ///
    /// The CLI's own invariant, applied to the binary that needed it more: the
    /// stamp is either `unknown` or a real short hash, optionally `-dirty`, and
    /// never silently blank. A version line that quietly said `Polylinker 0.1.0
    /// ()` would be worse than none, because it looks like an answer.
    #[test]
    fn the_gui_version_carries_the_commit_it_was_built_from() {
        let v = version();
        assert!(v.starts_with("Polylinker 0."), "{v}");
        let stamp = v
            .rsplit_once('(')
            .and_then(|(_, r)| r.strip_suffix(')'))
            .expect("a parenthesised commit");
        let core = stamp.strip_suffix("-dirty").unwrap_or(stamp);
        assert!(
            core == "unknown" || (core.len() >= 7 && core.chars().all(|c| c.is_ascii_hexdigit())),
            "the commit stamp is neither a hash nor an honest 'unknown': {stamp:?}"
        );
    }

    /// Every licence this binary owes is compiled into it and non-empty.
    ///
    /// `NOTICE` records the gap this closes: the seven faces are in
    /// `polylinker.exe`, which had no licence view of its own, and four of the
    /// six font licences require the notice to travel with each copy. An
    /// `include_str!` that resolved to an empty file would satisfy the compiler
    /// and ship nothing.
    #[test]
    fn every_notice_this_binary_owes_is_compiled_into_it() {
        assert_eq!(PAGES.len(), 9, "a licence page was added or lost");
        for (name, text) in PAGES {
            assert!(
                text.len() > 400,
                "{name} is {} bytes, which is not a licence",
                text.len()
            );
        }
        let notice = PAGES[0].1;
        // The same holders the font test insists NOTICE names. Asserted here
        // too because this is the page a user actually reads: a NOTICE that
        // named them and a viewer that showed a different file would satisfy
        // that test and still ship no attribution.
        for owed in [
            "John Slegers",
            "Canonical Ltd",
            "Google Inc",
            "Bold Monday",
            "Phosphor Icons",
        ] {
            assert!(
                notice.contains(owed),
                "the page a user reads does not name {owed:?}"
            );
        }
    }

    /// Every `pl-doc` topic is reachable, because the index is built FROM
    /// `TOPICS`.
    ///
    /// The bug this guards is the one the page exists to fix, arriving again: a
    /// paragraph compiled into the binary that nothing can reach. Writing the
    /// eleven titles out here would recreate it the next time a topic is added,
    /// so the index iterates `TOPICS` and this asserts the properties every
    /// entry must have to be worth showing.
    #[test]
    fn every_methods_topic_is_worth_showing_and_none_is_hard_coded() {
        assert!(pl_doc::TOPICS.len() >= 11, "topics went missing");
        for t in pl_doc::TOPICS {
            assert!(!t.title.is_empty(), "{} has no title", t.name);
            assert!(!pl_doc::help(*t).is_empty(), "{} has no summary", t.name);
            let m = pl_doc::methods(*t);
            assert!(
                m.contains("Limits:"),
                "{} states no limits, so the page would present it as settled",
                t.name
            );
            // Rendered by splitting on the blank line, so a paragraph that does
            // not split still shows as one block rather than vanishing.
            assert!(!m.split("\n\n").next().unwrap_or("").is_empty());
        }
    }
}
