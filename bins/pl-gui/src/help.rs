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
//! Since 2026-08-06 the user *can* have that comparison made for them, by a
//! switch in the same Help menu that ships off ([`crate::update`]). The About
//! page states what that switch does and what it does not — see [`UPDATE_NOTE`]
//! — and `the_about_page_states_the_network_behaviour_this_binary_actually_has`
//! is what keeps the statement attached to the code. The sentence that used to
//! be there said the program "contacts no server", which was true on the day it
//! was written and is the exact shape of prose this repository keeps having to
//! fix.
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
//! `NOTICE`'s "HOW THE OBLIGATION TRAVELS, and what is still owed" paragraph
//! recorded this gap and named this page as the fix. It now records the page as
//! what closed it:
//!
//! > It said the GUI had no way to state its own font attribution from inside
//! > itself, and should have one. It has one. `bins/pl-gui/src/help.rs` is that
//! > page. It shows NOTICE, both of this project's own licence texts,
//! > TRADEMARKS.md and all eight font licence texts whole, each from an
//! > `include_str!` rather than a summary, so what a user reads is the
//! > committed bytes and not a restatement of them.
//!
//! **Cited by heading and by quotation, not by line number.** This paragraph
//! said "`NOTICE` lines 443-450" until 2026-08-04; by then 443-450 was the
//! Phosphor shaping discussion and the passage had moved to line 511. A line
//! citation into another file goes stale on any edit above it, silently, while
//! still reading as authority — and the quotation it carried had never been
//! verbatim either, so nothing could have caught the drift.
//! `the_notice_passage_this_page_quotes_is_still_in_notice` now asserts the
//! quoted words are in the NOTICE this binary ships.
//!
//! Eight font licence texts are in [`PAGES`] below, under four licences that
//! require the notice to accompany each copy: SIL OFL 1.1, MIT, the Bitstream
//! Vera licence reached through Hack, and UFL 1.0. This said "four of the six"
//! until 2026-08-04, one short since Liberation arrived, and "seven" until
//! 2026-08-09, when Inter arrived with the design-system port. They travel in
//! `dist/` as well — `tools/release.ps1` copies all eight and refuses to ship
//! without them, which it did not do for `Liberation-OFL.txt` until
//! 2026-08-04. A user handed the bare `.exe` still has none of that, which is
//! what this page is for.
//!
//! THREE OF THE EIGHT ARE THE SAME LICENCE AND NONE OF THE THREE IS REDUNDANT.
//! Plex, Liberation and Inter are all SIL OFL 1.1 and all three texts are shown
//! whole, because what differs between them is the part that carries weight:
//! the copyright lines, and the Reserved Font Names. Plex reserves "Plex",
//! Liberation reserves four, and Inter reserves none — and that absence is the
//! clause the shipped Inter file actually depends on, being a subset and so a
//! Modified Version. A reader shown one OFL and told the other two are "the
//! same licence" cannot check that.
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
    // The other half of `MIT OR Apache-2.0`, and it was missing here for the
    // same reason it was missing from the repository until 2026-08-06: the
    // Apache text was the one that existed, so it was the one that got shown.
    // This page exists for the user holding only the .exe, and that user cannot
    // exercise a choice whose second option is not in the binary.
    ("MIT License", include_str!("../../../LICENSE-MIT")),
    ("Trademarks", include_str!("../../../TRADEMARKS.md")),
    ("IBM Plex — OFL", include_str!("../fonts/IBMPlex-OFL.txt")),
    // A third OFL file, and shown whole like the other two rather than
    // deduplicated against them. Same licence, a different copyright line, and
    // — unlike Plex and Liberation — no Reserved Font Name at all, which is the
    // clause the vendored copy of this face actually depends on. A reader who
    // was shown only the Plex OFL could not check that.
    ("Inter — OFL", include_str!("../fonts/Inter-OFL.txt")),
    // A different file from the Plex OFL above -- the same licence, but
    // different copyright lines and different Reserved Font Names -- so it is
    // shown separately rather than deduplicated against it. These two faces
    // are never drawn on screen; `pl-draw` fills their outlines when exporting
    // a PNG, and a licence that travels only with the crate is a licence the
    // person holding the .exe does not have.
    (
        "Liberation Sans — OFL",
        include_str!("../../../crates/pl-draw/fonts/Liberation-OFL.txt"),
    ),
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
                        // WHERE IT IS. See `where_in_the_app` for why a page
                        // that describes an operation without saying where to
                        // perform it is the weak form of the defect this
                        // window has already had twice.
                        if let Some(here) = where_in_the_app(t.name) {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(here).color(pal.ink2).size(11.5));
                        }
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

/// Where in THIS PROGRAM a documented operation is performed, or `None`.
///
/// # Why the pointer is here and not in `pl-doc`
///
/// `pl-doc` is shared with `bins/pl`, and `pl methods primers` prints the same
/// paragraph for somebody who has no window open. A crate that named a tab
/// would be wrong half the time it was read. The GUI is the only place that can
/// say where a GUI puts something, so this lives here.
///
/// # Why it is here at all
///
/// This window shipped a page titled "Primer binding sites" — describing the
/// 3'-anchored seed, the footprint/tail split and a footprint-only Tm — while
/// the binary had no way to find a binding site at all. The day before, it
/// shipped "Feature annotation" under the same condition. Both operations are
/// now reachable, and the remaining failure is quieter but the same in kind: a
/// page that explains a method and never says the program in front of you
/// performs it, or where, reads as documentation for the command line.
///
/// # `None` is most of the list, and is not laziness
///
/// A pointer is written here only where the destination was READ OFF THE CODE:
/// the tab labels are `main.rs`'s own strings and the window title is
/// `design::show`'s. Everything else returns `None` and shows nothing, because
/// a confidently-worded direction to a panel that is not there is worse than
/// silence — it sends a user hunting for a tab that does not exist and makes
/// them doubt the rest of the page. `every_help_location_names_a_real_topic`
/// holds the keys to `pl_doc::TOPICS`, so a misspelling here fails the suite
/// rather than silently going quiet.
fn where_in_the_app(topic: &str) -> Option<&'static str> {
    match topic {
        // The tab this whole module note is about. Both painters are named
        // because "in place" is the feature: the list is the least of it.
        "primers" => Some(
            "In this app: the Primers tab. Paste an oligo and every place it anneals is \
             listed, on both strands, marked on the map and boxed in the sequence view. \
             Clicking a site selects its footprint.",
        ),
        "annotate" => Some(
            "In this app: the Features tab, which lists what the built-in database found \
             as proposals. Nothing reaches the document until you accept one.",
        ),
        "tm" => Some(
            "In this app: select bases in the Sequence tab and the Tm is in the readout \
             beneath, with these conditions on its hover. The Primers tab reports one per \
             binding site, over the annealed footprint only.",
        ),
        // TWO HALVES, because the page is titled "Open reading frames and
        // TRANSLATION" and only the first half had a pointer. The second half
        // stopped being read-only on 2026-08-07: the reader of a methods
        // paragraph about genetic codes is exactly the reader who wants the
        // letters, and until that day the only way to get them was to retype
        // them off the screen.
        //
        // All three doors are named because they give three DIFFERENT readings
        // of the same bases, and the difference is the thing this page's own
        // Limits clause is about. A feature carries its own `/transl_table` and
        // `/codon_start`; a selection carries neither and is read under the
        // document's table. Naming one door would have made the other two look
        // like the same answer arrived at differently.
        //
        // The labels are quoted from `main.rs` rather than remembered, and
        // `the_help_page_quotes_button_labels_that_exist` reads that file — the
        // same guard the "Check for new releases" sentence is under, for the
        // reason `where_in_the_app`'s own header gives about "Design primers…".
        "orfs" => Some(
            "In this app: the ORF track under the Sequence tab, and the amino-acid track \
             beside it. To take the residues away: \"Copy protein\" beside the sequence \
             readout (Ctrl+Shift+P) reads the selection, \"Copy protein\" in the Features \
             toolbar reads the selected feature under its own /transl_table and \
             /codon_start, and \"Protein FASTA…\" under Save writes every reading in the \
             document. Every record names the table that produced it.",
        ),
        "digest" => Some("In this app: the Enzymes tab, and the cut marks on the map."),
        // The SHAPE is said out loud, because the question a reader arrives at
        // this page with is "why is my plasmid a straight line", and the answer
        // is that the figure follows the molecule's own topology. Until
        // 2026-08-07 the answer would have been "it is not", which is the
        // defect this pointer was written alongside: a PCR product exported as
        // a ring with a notch in it.
        //
        // "Export figure" is quoted because it IS a literal in `main.rs` and
        // `the_help_page_quotes_button_labels_that_exist` reads that file. The
        // leaves under it are built as `format!("{subject} as {}…")`, so
        // quoting "Map as SVG…" would put a string in this page that no search
        // of the source can confirm — which is how a help page starts naming
        // buttons that were renamed.
        "map" => Some(
            "In this app: the Map tab, and \"Export figure\" in the toolbar, which \
             writes SVG, PDF, EPS or PNG at a printed width. The figure is the shape \
             the molecule is — a ring for a circular one, a track for a linear one — \
             and a plasmid asked for as a track says in its caption that it was cut \
             open. The line beside the export names anything the canvas could not fit.",
        ),
        "sanger" => Some("In this app: the Reads tab."),
        // Not a menu item: the button sits beside the selection readout and is
        // disabled until there IS a selection, so naming a menu would send a
        // user somewhere there is nothing to click. Checked against
        // `main.rs`'s own button label rather than remembered.
        "design" => Some(
            "In this app: select the region to amplify in the Sequence tab, then \
             \"Design primers…\" beside the readout.",
        ),
        _ => None,
    }
}

/// What this program does about new versions, for the reader of the version
/// number above it.
///
/// **Every clause here is a claim about code in this binary, and it is a
/// constant so that a test can hold it to that.** Until 2026-08-06 this said:
///
/// > There is no auto-updater, on purpose. Polylinker contacts no server and
/// > cannot tell you whether a newer version exists — check when you want to.
///
/// Both halves of the middle sentence stopped being true on the day the Help
/// menu grew a "Check for new releases" box: switched on, this program does
/// contact a server, and telling you whether a newer version exists is the
/// only thing it asks. The first clause survived, because what was decided
/// against was an *auto*-updater — something on a timer that installs what it
/// finds — and there is still none of that anywhere.
///
/// So the claim is now conditional, and the conditions are the ones the code
/// enforces: off in a fresh installation (`settings::Layout::default`), one
/// request per launch (the latch in `crate::update`), and no download at all
/// (the desktop app has no call to `fetch_and_verify`, which
/// `only_the_update_module_can_reach_the_network` fails if it ever gains one).
///
/// **It says "this program" and not "Polylinker", and that is the difference
/// between a true sentence and a nearly-true one.** The checkbox governs this
/// binary. `pl update` is a separate program in the same folder, it contacts a
/// server when somebody types it, and no tick anywhere permits or forbids that
/// — so a sentence promising that Polylinker contacts nothing until this box is
/// ticked would be the same species of comfortable overstatement as the one it
/// replaced. The command is named in the last clause instead.
const UPDATE_NOTE: &str = "There is no auto-updater, on purpose: nothing here runs on a timer \
     and nothing installs itself. This program contacts no server unless you tick \"Check for new \
     releases\" in this Help menu. It ships unticked; ticked, it asks github.com once per launch \
     whether a newer version exists, and downloads nothing either way. Downloading is the command \
     line's half: `pl update` fetches a release and checks its signature, and runs when you run \
     it.";

/// What is true whatever that box is set to, which is the part that never had
/// to change.
///
/// The retired wording ended "No account, no telemetry, no network." The first
/// two are as true as they ever were; the third was the unconditional claim
/// that the update check made false, and replacing it with what is actually
/// guaranteed — that nothing about the user's work leaves the machine — says
/// more than the word it replaces did.
const LOCAL_NOTE: &str = "Everything that reads or writes your files runs on this machine. No \
     account, no telemetry, and no sequence, file name or identifier is sent anywhere, ever.";

/// What happens by itself when a molecule is opened, and what does not.
///
/// This page already answers "does it phone home". Annotation is the other
/// question a user is entitled to ask of software that starts doing something
/// the moment a file opens, and it has two halves that must not be run
/// together. **Nothing leaves the machine** — the database is three tables
/// compiled into this binary. **Nothing enters the document** — what the scan
/// finds is offered in the Features tab and is not in the file until Accept is
/// pressed, which is `features/SIGNOFF.tsv`'s rule carried into the interface:
/// the tool may propose and may not assert.
///
/// A function and not a `const`, because the record count is interpolated from
/// the table this binary actually ships rather than typed here. That is the
/// module header's rule about every other number on this page — a count written
/// into prose is the first thing to go stale, and this file exists partly
/// because a sentence about the network went stale exactly that way.
///
/// The clause about what the library does NOT hold is deliberately here and not
/// only in the proposals panel. Somebody reading About is finding out what the
/// program is; "it annotates plasmids" and "it annotates the 89 things it knows,
/// which do not include a single promoter" are different claims, and only the
/// second is true.
fn annotate_note() -> String {
    let (db, _) = pl_features::Db::builtin();
    let reviewed = db.reviewed();
    let gaps = match reviewed.absent_common_kinds().split_last() {
        None => String::new(),
        Some((last, rest)) => format!(
            " It is not comprehensive — it holds no {} record at all — so a feature it \
             does not offer may still be there.",
            if rest.is_empty() {
                (*last).to_string()
            } else {
                format!("{} or {last}", rest.join(", "))
            }
        ),
    };
    format!(
        "Opening or pasting a molecule searches it against {} curated feature records \
         compiled into this program. That search runs here and sends nothing anywhere. \
         What it finds is offered in the Features tab as proposals, with the identity and \
         the coverage of each match and the record it came from: nothing is added to your \
         document until you accept it, and Ctrl+Z takes back anything you \
         do.{gaps} Switch the search off with \"Annotate on open\" in that panel.",
        reviewed.records.len()
    )
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
    ui.label(egui::RichText::new(UPDATE_NOTE).color(pal.muted).size(11.5));
    ui.add_space(8.0);
    ui.label(egui::RichText::new(LOCAL_NOTE).color(pal.muted).size(11.5));
    ui.add_space(8.0);
    // Under the two network sentences, because the first thing it says is that
    // this one is not a network behaviour either.
    ui.label(
        egui::RichText::new(annotate_note())
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

    /// Every location pointer is keyed to a topic that exists.
    ///
    /// [`where_in_the_app`] matches on `&str`, so a misspelled key is not a
    /// compile error — it is a page that quietly stops saying where the
    /// operation is, which is the exact condition the function was added to
    /// remove and is invisible from the outside. Renaming a topic in `pl-doc`
    /// does the same thing from the other end.
    ///
    /// So the keys are listed once, here, and each is required to resolve
    /// AND to answer. Nothing asserts the reverse direction: most topics
    /// deliberately have no pointer, and demanding one for every topic would
    /// force somebody to invent a destination, which is the failure this is
    /// guarding against rather than a stricter version of it.
    ///
    /// PROVEN TO FAIL by changing the `"primers"` arm's key to `"primer"`:
    /// `"primers" is a real topic with no pointer written`. Note WHICH half
    /// caught it — the topic lookup passed, because the key list here still
    /// reads `"primers"` and that topic exists; it was the pointer coming back
    /// `None` that failed. The two assertions catch opposite mistakes, a
    /// renamed topic and a mistyped arm, and only one of them fires per
    /// mistake.
    #[test]
    fn every_help_location_names_a_real_topic() {
        for key in [
            "primers", "annotate", "tm", "orfs", "digest", "sanger", "design",
        ] {
            assert!(
                pl_doc::topic(key).is_some(),
                "{key:?} is not a pl_doc topic, so the page it was written for says nothing \
                 about where the operation is"
            );
            let says = where_in_the_app(key)
                .unwrap_or_else(|| panic!("{key:?} is a real topic with no pointer written"));
            assert!(
                says.starts_with("In this app:"),
                "{key:?}: the pointer does not read as one -- {says}"
            );
        }
        // The topic this module note is about, named specifically: the app
        // shipped this page while unable to perform the operation, and a
        // pointer that stopped mentioning the tab would be the quiet half of
        // that defect returning.
        let p = where_in_the_app("primers").expect("the Primers page has a pointer");
        assert!(
            p.contains("Primers tab"),
            "the primer page does not name the tab that does it: {p}"
        );
        // And a topic with no panel says nothing rather than guessing.
        assert_eq!(where_in_the_app("checksum"), None);
    }

    /// Every control this page names by its label is a control that exists.
    ///
    /// `where_in_the_app`'s own header claims the destinations were "READ OFF
    /// THE CODE" and the `design` arm says its label is "checked against
    /// `main.rs`'s own button label rather than remembered". **Nothing checked
    /// it.** The About page has had exactly this guard for "Check for new
    /// releases" since it was written, and the pointers underneath it — which
    /// are the sentences that send a user hunting — had none, which is the
    /// same species of comfortable prose this whole module exists to correct.
    ///
    /// A user told to click something that is not there is worse off than one
    /// told nothing: they go looking, and the app is the thing that lied.
    ///
    /// **THE COMMENTS ARE STRIPPED FIRST, and that is not fussiness.** The
    /// first version of this searched `main.rs` whole, and renaming the real
    /// button from `"Protein FASTA…"` to `"Export proteins…"` left it GREEN:
    /// `main.rs:3303` is a doc comment that quotes the old label, so a check
    /// meant to prove a control exists was satisfied by prose about the
    /// control. That is the same defect one level up, and it would have made
    /// this test the thing that certified the lie. `"Copy protein"` has the
    /// identical shape — three comments name it and two `ui.button` /
    /// `Button::new` calls create it.
    ///
    /// Line-based, dropping lines whose trimmed start is `//`: both `//` and
    /// `///` go, a string containing `//` mid-line does not, and this crate has
    /// no `/* */` anywhere. The label search then runs over code only.
    ///
    /// PROVEN TO FAIL twice, once for each half, with the whole module re-run
    /// each time. Renaming the button in `main.rs` and leaving this page alone:
    ///
    /// ```text
    /// ---- help::tests::the_help_page_quotes_button_labels_that_exist stdout ----
    /// the "orfs" pointer sends a user to "Protein FASTA…"; no widget in
    ///   main.rs carries that label
    /// ```
    ///
    /// ...and the other way, changing the pointer to name `"Export protein…"`:
    ///
    /// ```text
    /// the "orfs" pointer stopped quoting "Protein FASTA…": In this app: the
    ///   ORF track under the Sequence tab, and the amino-acid track beside it…
    /// ```
    #[test]
    fn the_help_page_quotes_button_labels_that_exist() {
        let main = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("main.rs"),
        )
        .expect("bins/pl-gui/src/main.rs");
        let code: String = main
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for (topic, label) in [
            ("design", "Design primers…"),
            ("orfs", "Copy protein"),
            ("orfs", "Protein FASTA…"),
        ] {
            let says = where_in_the_app(topic).expect("a pointer");
            assert!(
                says.contains(&format!("\"{label}\"")),
                "the {topic:?} pointer stopped quoting {label:?}: {says}"
            );
            assert!(
                code.contains(&format!("\"{label}\"")),
                "the {topic:?} pointer sends a user to {label:?}; no widget in main.rs \
                 carries that label"
            );
        }
        // The chord as well as the labels. It is decided in `sequence_keys` and
        // written nowhere a string search can reach, so this asserts the two
        // halves that make the chord what it is rather than a literal.
        let orfs = where_in_the_app("orfs").expect("a pointer");
        assert!(orfs.contains("Ctrl+Shift+P"), "{orfs}");
        assert!(
            main.contains("egui::Key::P if cmd && shift"),
            "the Help page advertises Ctrl+Shift+P and nothing in main.rs binds it"
        );
    }

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
    /// `NOTICE` records the gap this closes: the faces are in
    /// `polylinker.exe`, which had no licence view of its own, and four of the
    /// font licences require the notice to travel with each copy. An
    /// `include_str!` that resolved to an empty file would satisfy the compiler
    /// and ship nothing.
    ///
    /// The count went 9 → 10 on 2026-08-03 with the two Liberation faces
    /// `pl-draw` fills outlines from. They are never drawn on screen, and that
    /// is exactly why the page matters: nothing in the interface would look
    /// wrong if their licence were missing.
    ///
    /// 10 → 11 on 2026-08-06 with the MIT text. That one is not a third party's
    /// at all — it is half of Polylinker's own offer, and the half that did not
    /// exist as a file until that day.
    ///
    /// 11 → 12 on 2026-08-09 with Inter's OFL, for the heading face the design
    /// system port brought in. A third copy of the same licence, and the one
    /// whose *differences* from the other two carry the argument: no Reserved
    /// Font Name, which is what permits the subset that ships.
    #[test]
    fn every_notice_this_binary_owes_is_compiled_into_it() {
        assert_eq!(PAGES.len(), 12, "a licence page was added or lost");
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
            // The Liberation faces, whose OFL clause 4 attribution has no other
            // route to somebody holding only the .exe.
            "Red Hat, Inc.",
            "Steve Matteson",
        ] {
            assert!(
                notice.contains(owed),
                "the page a user reads does not name {owed:?}"
            );
        }
    }

    /// The About page describes the network behaviour this binary has, not the
    /// one it used to have.
    ///
    /// This page is where somebody who was handed `polylinker.exe` finds out
    /// what it does behind their back, so its two paragraphs are the load-
    /// bearing privacy statement of the whole product — and until 2026-08-06
    /// one of them was false, having been true when it was written. Prose that
    /// goes stale in that direction is worse than no prose: it is a promise the
    /// program has stopped keeping, in the place a user goes to check.
    ///
    /// So each clause is pinned to the thing that makes it true:
    ///
    /// * "ships unticked" — to `settings::Layout::default()`, so flipping the
    ///   default to on fails here and not in review;
    /// * the name of the control — to `main.rs`, so a renamed checkbox cannot
    ///   leave this page pointing at a box that no longer exists;
    /// * the two retired sentences — banned outright, so a revert to the older,
    ///   more flattering wording is red rather than plausible.
    ///
    /// PROVEN TO FAIL three ways before being called done: with `UPDATE_NOTE`
    /// restored to its pre-2026-08-06 text (fails on the banned sentence and on
    /// four missing clauses), with `update_check: true` in `Layout::default`
    /// (fails on "ships unticked" describing a default that is on), and with
    /// the checkbox in `main.rs` relabelled (fails on the About page naming a
    /// control that is not there). Every other test in this binary stays green
    /// through all three.
    #[test]
    fn the_about_page_states_the_network_behaviour_this_binary_actually_has() {
        // Off out of the box, and the sentence that says so.
        assert!(
            !crate::settings::Layout::default().update_check,
            "the update check now ships ON; the About page says it ships unticked"
        );
        assert!(
            UPDATE_NOTE.contains("ships unticked"),
            "the About page no longer says the check is off in a new installation"
        );

        // The clauses that are the consent, each one a property of the code:
        // one request per launch, no download, and the command that does
        // download so the paragraph is not merely a refusal.
        for required in [
            "once per launch",
            "downloads nothing",
            "github.com",
            "pl update",
            "no auto-updater",
        ] {
            assert!(
                UPDATE_NOTE.contains(required),
                "the About page no longer says {required:?}"
            );
        }
        for required in ["No account", "no telemetry", "no sequence"] {
            assert!(
                LOCAL_NOTE.contains(required),
                "the About page no longer says {required:?}"
            );
        }

        // The two sentences that were true when written and are not now. Named
        // exactly, because the failure to guard against is not a typo but a
        // revert: both read perfectly well and both would be lies.
        // Annotation is the other thing this binary does on its own, and the
        // About page has to say the two things it is not: not a network call,
        // and not a change to the file. The count comes from the shipped table,
        // so a library that grew and a sentence that did not cannot both be on
        // this page.
        let shipped = pl_features::Db::builtin().0.reviewed();
        let note = annotate_note();
        assert!(
            note.contains(&format!(
                "{} curated feature records",
                shipped.records.len()
            )),
            "the About page states a record count that is not this binary's: {note}"
        );
        for required in [
            "sends nothing anywhere",
            "nothing is added to your document until you accept it",
            "Ctrl+Z",
            "not comprehensive",
            "Annotate on open",
        ] {
            assert!(
                note.contains(required),
                "the About page no longer says {required:?}: {note}"
            );
        }
        // And it names the gaps the shipped table really has, read from the
        // same call the panel and the methods paragraph read.
        for kind in shipped.absent_common_kinds() {
            assert!(
                note.contains(kind),
                "the About page does not say the library holds no {kind}: {note}"
            );
        }
        assert!(
            crate::settings::Layout::default().annotate_on_open,
            "annotation now ships OFF; the About page describes it as happening on open"
        );

        for retired in ["contacts no server and cannot", "no telemetry, no network"] {
            for (which, note) in [("UPDATE_NOTE", UPDATE_NOTE), ("LOCAL_NOTE", LOCAL_NOTE)] {
                assert!(
                    !note.contains(retired),
                    "{which} has gone back to claiming {retired:?}, which the Help \
                     menu's update check made false"
                );
            }
        }

        // And the control it names has to be the control that is there. The
        // label is written once in `main.rs`; this page quotes it, and a quote
        // of something that no longer exists is how a help page starts lying
        // about the interface it describes.
        let main = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("main.rs"),
        )
        .expect("bins/pl-gui/src/main.rs");
        let label = "Check for new releases";
        assert!(
            main.contains(&format!("\"{label}\"")),
            "the About page quotes a checkbox called {label:?}; main.rs has no such label"
        );
        assert!(
            UPDATE_NOTE.contains(label),
            "the About page no longer says where the switch is"
        );
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
