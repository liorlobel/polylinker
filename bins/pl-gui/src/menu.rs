//! The application's menu, as data — and nothing about how it is installed.
//!
//! There is no AppKit in this file and there must never be one. The menu bar is
//! a macOS artefact, but "what commands does this application have, what are
//! they called, and which chord does each one advertise" is not: it is the same
//! question the toolbar answers, and the answer has to be one answer. So the
//! table lives here, compiles on every platform, and is checked by tests that
//! run on every CI leg. `macmenu.rs` — the only file in this binary with an
//! `objc2` in it — reads this table and installs it, and the `gui-smoke` job is
//! what proves the install does not crash on a real runner.
//!
//! **THE DEFECT THIS FILE IS SHAPED AROUND** is a menu that advertises a chord
//! the application does not bind, or binds to something else: a File ▸ Save
//! reading "⌘S" beside an app where ⌘S opens the Library. Nothing about writing
//! the table twice catches that. What catches it is
//! `the_menu_and_the_keyboard_agree_about_every_chord` in `main.rs`, which
//! presses every chord on this board, asks `App::global_shortcuts` what the
//! application really does with it, and requires the two to be equal — and
//! `every_key_the_application_honours_is_on_the_board`, which sweeps the other
//! way, so the menu can neither lie about a chord nor stay silent about one.
//!
//! **THE SECOND DEFECT, which decided the shape of [`Gate`] and [`Origin`]:**
//! an `NSMenuItem` key equivalent does not merely ADVERTISE a chord — it takes
//! it. AppKit offers every key-down to the main menu's `performKeyEquivalent:`
//! before the responder chain sees it, and winit's `NSView` overrides nothing,
//! so a chord the menu claims never reaches egui at all. Measured 2026-09-05
//! with synthetic `NSEvent`s posted through `NSApplication.postEvent:atStart:`
//! — the queue a real key press lands in: with ⌘S on a menu item, egui logged
//! no key event and the menu action fired; with the item's key equivalent
//! removed, egui logged `Key { key: S, command: true }`. A disabled item still
//! eats it. So every guard `global_shortcuts` applies to a keystroke has to be
//! applied again, here, to what the menu hands back — or installing the menu
//! silently deletes those guards one chord at a time.

// THE TABLE IS INSTALLED BY `macmenu.rs` ALONE, so on Linux and Windows a few
// items here — `Standard`, `Show`, `key_equivalent` — have no reader outside
// the tests. That is the point of the file rather than an oversight: it must
// compile and be checked on every leg, and it is why the allow is here rather
// than a `cfg` on the module, which would delete the checking with it.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::sync::Mutex;

use eframe::egui;

/// Define `Command` and the list of every command from ONE source.
///
/// A hand-kept `ALL` beside a hand-written enum is two lists, and this file's
/// whole argument is that two lists is the defect. It was written that way
/// first, in a throwaway — an `ALL` constant plus an exhaustive `index` match,
/// with a test asserting `ALL.len() == 1 + max index` to tie them together —
/// and THAT TEST COULD NOT FAIL for the obvious mutation: delete the LAST entry
/// from `ALL` and the maximum index falls with the length, so 27 == 27 and the
/// missing command goes unreported. Measured, not reasoned: the mutation was
/// applied and the test printed `ok`.
///
/// So the list is generated. There is nothing left to keep in step.
macro_rules! commands {
    ($($(#[$about:meta])* $name:ident),* $(,)?) => {
        /// One thing the user can ask this application to do.
        ///
        /// The whole vocabulary, in one enum, because the alternative is two:
        /// this was a `Shortcuts` field for the fourteen commands that happen to
        /// have a chord and a closure inside `App::top_bar` for the fourteen
        /// that do not, and a menu bar would have been a third. `Shortcuts` is
        /// still the frame's *keyboard* answer and is still a struct of bools —
        /// see `Shortcuts::commands` in `main.rs`, the one place a set flag
        /// becomes a `Command`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Command {
            $($(#[$about])* $name),*
        }

        impl Command {
            /// Every command. Generated from the same tokens as the enum, so it
            /// cannot be short by one.
            pub const ALL: &'static [Self] = &[$(Self::$name),*];
        }
    };
}

commands! {
    // --- File -------------------------------------------------------------
    /// The New dialog. `App::newdoc.show()`.
    NewDoc,
    /// `App::pick_file` — the native open dialog.
    Open,
    /// The synthetic 3,180 bp construct that ships inside the binary.
    OpenExample,
    /// `App::export(false)` — GenBank, which is what Ctrl+S has always meant
    /// here and what `pl convert` defaults to.
    Save,
    /// `App::save_dna(None)`.
    SaveDna,
    /// `App::export(true)` — bases only.
    SaveFasta,
    /// `App::export_protein` — the translations, not the molecule.
    SaveProtein,
    /// `App::save_project` / `App::open_project` — the named list of paths.
    SaveProject,
    OpenProject,
    /// The four figure exports. Which picture they write — map or gel — is
    /// decided by `App::central_view`, exactly as the toolbar's own
    /// `Export figure` menu decides it, and NOT by four more commands.
    ExportFigureSvg,
    ExportFigurePdf,
    ExportFigureEps,
    ExportFigurePng,
    /// Close the active tab; reopen the last one closed.
    CloseTab,
    ReopenTab,
    /// Ask to quit, THROUGH the application's own close path.
    ///
    /// Not `terminate:`. See the Quit item in [`MENUS`] for what that costs.
    Quit,

    // --- Edit -------------------------------------------------------------
    Undo,
    Redo,
    /// Open the find bar and put the caret in it.
    Find,
    FindNext,
    FindPrev,
    /// Escape — close the find bar.
    CloseFind,

    // --- View -------------------------------------------------------------
    NextTab,
    PrevTab,

    // --- Help -------------------------------------------------------------
    /// The Help window, on its default page.
    Help,
    /// The Help window, on a named page. Three of them, because the toolbar's
    /// Help menu already offers exactly these three and a native menu offering
    /// a different three would be the second answer this file exists to stop.
    HelpAbout,
    HelpTm,
    HelpLicences,
}

impl Command {
    /// Position in [`Command::ALL`].
    pub fn index(self) -> usize {
        // `ALL` is generated from the same tokens as the enum, so this cannot
        // fail; `expect` rather than `unwrap_or(0)` because a silent zero would
        // make every unknown command mean `NewDoc`.
        Self::ALL
            .iter()
            .position(|c| *c == self)
            .expect("Command::ALL is generated from the enum")
    }

    /// The `NSMenuItem` tag this command travels under, and back.
    ///
    /// `index + 1`, because 0 is `NSMenuItem`'s default tag: an item nobody
    /// tagged would otherwise dispatch as the first command in the list.
    /// `a_tag_round_trips_and_zero_is_nobody` pins both halves.
    pub fn tag(self) -> isize {
        self.index() as isize + 1
    }

    pub fn from_tag(tag: isize) -> Option<Self> {
        usize::try_from(tag)
            .ok()
            .and_then(|t| t.checked_sub(1))
            .and_then(|i| Self::ALL.get(i).copied())
    }
}

/// A menu item that AppKit itself services, through a selector on
/// `NSApplication` or on the responder chain.
///
/// Listed as data rather than left out because the *collisions* are the point.
/// macOS's own Window menu carries a Close item on ⌘W, and this application has
/// bound ⌘W to "close the active tab" since the bench was written. Two items
/// claiming ⌘W is a defect a reader will not spot and
/// `no_two_menu_items_claim_the_same_chord` will, and it can only spot it if
/// the standard items are in the same table as ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standard {
    /// `hide:`, `hideOtherApplications:`, `unhideAllApplications:` — all on
    /// `NSApplication`, all safe: they touch no document.
    Hide,
    HideOthers,
    ShowAll,
    /// The Services submenu. AppKit fills it.
    Services,
    /// `performMiniaturize:` and `performZoom:` on `NSWindow`.
    Minimize,
    Zoom,
    /// `arrangeInFront:`.
    BringAllToFront,
}

/// A chord, in the vocabulary the application's own keyboard code reads.
///
/// `egui::Key` AND NOT A KEY ENUM OF OUR OWN, which is the single most
/// load-bearing decision in this file. A private enum would need a translation
/// into `egui::Key` before a test could press it, and a wrong line in that
/// translation is precisely the defect — a menu saying ⌘S beside an app where
/// ⌘S does something else — arriving through the one door the test cannot
/// watch. With `egui::Key` the chord the menu advertises IS the chord the test
/// presses; there is nothing in between to be wrong.
///
/// The AppKit side needs the other direction — an `NSMenuItem` key equivalent
/// string — and that lives in [`Chord::key_equivalent`], here rather than in
/// `macmenu.rs`, so `every_shown_chord_has_a_key_equivalent` can say it is
/// total over this table on every leg and not only on the one that installs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: egui::Key,
    pub cmd: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    pub const fn cmd(key: egui::Key) -> Self {
        Self {
            key,
            cmd: true,
            shift: false,
            alt: false,
        }
    }
    pub const fn cmd_shift(key: egui::Key) -> Self {
        Self {
            key,
            cmd: true,
            shift: true,
            alt: false,
        }
    }
    pub const fn plain(key: egui::Key) -> Self {
        Self {
            key,
            cmd: false,
            shift: false,
            alt: false,
        }
    }
    pub const fn shift(key: egui::Key) -> Self {
        Self {
            key,
            cmd: false,
            shift: true,
            alt: false,
        }
    }
    pub const fn cmd_alt(key: egui::Key) -> Self {
        Self {
            key,
            cmd: true,
            shift: false,
            alt: true,
        }
    }

    /// The modifier state a frame carrying this chord has.
    ///
    /// `command` AND `ctrl` together, which is what the existing shortcut tests
    /// build by hand (`ctrl` in main.rs's test module) and what egui-winit
    /// produces on the platforms where `command` means Ctrl. `global_shortcuts`
    /// reads `modifiers.command` and `modifiers.shift` and nothing else, so the
    /// third flag costs nothing and keeps this identical to the fixture the
    /// guard tests already trust.
    ///
    /// `cfg(test)`, with `raw_input` below: the installer never presses a
    /// chord, so outside the tests these have no caller on any platform.
    #[cfg(test)]
    pub fn modifiers(self) -> egui::Modifiers {
        egui::Modifiers {
            command: self.cmd,
            ctrl: self.cmd,
            shift: self.shift,
            alt: self.alt,
            mac_cmd: false,
        }
    }

    /// One frame carrying exactly this chord, and nothing else.
    #[cfg(test)]
    pub fn raw_input(self) -> egui::RawInput {
        let modifiers = self.modifiers();
        egui::RawInput {
            modifiers,
            events: vec![egui::Event::Key {
                key: self.key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        }
    }

    /// The `NSMenuItem` key equivalent string for this chord's key, or `None`
    /// for a key AppKit has no single-character spelling of.
    ///
    /// Lower-case letters, always: AppKit reads an UPPER-case letter as "Shift
    /// is implied", and the modifier mask is set explicitly by the installer
    /// instead, so that a chord's spelling and its modifiers are two facts and
    /// not one fact hiding in a case change. The function keys are the private
    /// Unicode code points `NSF1FunctionKey` onward, `0xF704` for F1.
    ///
    /// Only the keys this table uses or could plausibly use are spelled. A key
    /// missing here is a compile-time `None` and
    /// `every_shown_chord_has_a_key_equivalent` refuses it; extending the match
    /// is one line and the test says which.
    pub fn key_equivalent(self) -> Option<&'static str> {
        use egui::Key as K;
        Some(match self.key {
            K::A => "a",
            K::B => "b",
            K::C => "c",
            K::D => "d",
            K::E => "e",
            K::F => "f",
            K::G => "g",
            K::H => "h",
            K::I => "i",
            K::J => "j",
            K::K => "k",
            K::L => "l",
            K::M => "m",
            K::N => "n",
            K::O => "o",
            K::P => "p",
            K::Q => "q",
            K::R => "r",
            K::S => "s",
            K::T => "t",
            K::U => "u",
            K::V => "v",
            K::W => "w",
            K::X => "x",
            K::Y => "y",
            K::Z => "z",
            K::F1 => "\u{F704}",
            K::F2 => "\u{F705}",
            K::F3 => "\u{F706}",
            K::F4 => "\u{F707}",
            K::F5 => "\u{F708}",
            K::F6 => "\u{F709}",
            K::F7 => "\u{F70A}",
            K::F8 => "\u{F70B}",
            K::F9 => "\u{F70C}",
            K::F10 => "\u{F70D}",
            K::F11 => "\u{F70E}",
            K::F12 => "\u{F70F}",
            _ => return None,
        })
    }
}

/// Whether AppKit may claim this chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Show {
    /// Installed as the item's `keyEquivalent` and printed beside it. At most
    /// one per item — `an_item_prints_at_most_one_chord`.
    KeyEquivalent,
    /// The application binds it and the menu must NOT claim it. The reason is
    /// compiled in so the exception cannot be taken silently.
    ///
    /// Three kinds hide behind this one variant and the `why` says which: a
    /// second chord for a command that already prints one (⌘Y for Redo, the
    /// Windows habit); a chord macOS itself owns (⌘Tab); and a chord that
    /// belongs to whichever text box has the caret before it belongs to this
    /// application (⌘Z, Escape), where claiming it would take it from every
    /// text field in the window — see the module header for the measurement.
    Withheld { why: &'static str },
}

/// One chord an item answers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bind {
    pub chord: Chord,
    pub show: Show,
}

pub const fn shown(chord: Chord) -> Bind {
    Bind {
        chord,
        show: Show::KeyEquivalent,
    }
}
pub const fn withheld(chord: Chord, why: &'static str) -> Bind {
    Bind {
        chord,
        show: Show::Withheld { why },
    }
}

/// What an item does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// This application decides it. `App::act` runs it and
    /// `the_menu_and_the_keyboard_agree_about_every_chord` drives its chords
    /// through `App::global_shortcuts`.
    App(Command),
    /// AppKit runs it. This application binds nothing and claims nothing; the
    /// entry exists so its chord is in the collision check.
    System(Standard),
}

#[derive(Debug, Clone, Copy)]
pub enum Entry {
    Item {
        label: &'static str,
        action: Action,
        binds: &'static [Bind],
    },
    Separator,
}

pub const fn item(label: &'static str, action: Action, binds: &'static [Bind]) -> Entry {
    Entry::Item {
        label,
        action,
        binds,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Menu {
    pub title: &'static str,
    pub entries: &'static [Entry],
}

use Action::{App as A, System as S};
use Command as C;

/// The name the menus print. The CONSTANT, and not `NSProcessInfo.processName`
/// — which is the executable's name, `polylinker` in lower case inside the
/// shipped `.app` (measured 2026-09-05: LaunchServices reports "Polylinker"
/// for the bundle, `processName` reports "polylinker"), so winit's default
/// "Quit <processName>" reads "Quit polylinker" on a Mac that has just dragged
/// "Polylinker" to Applications.
pub const APP_NAME: &str = "Polylinker";

/// The menu bar.
///
/// `Polylinker` first because macOS puts the application menu first and titles
/// it from the bundle whatever this string says; the rest is the order every
/// document application on the platform uses, so a user's hand finds Save
/// without reading.
pub const MENUS: &[Menu] = &[
    Menu {
        title: APP_NAME,
        entries: &[
            // OUR About window and not `orderFrontStandardAboutPanel:`, which
            // is what winit installs today (winit-0.30.13,
            // src/platform_impl/macos/menu.rs). This application HAS an About
            // page — the version, the licences, the notices — reachable from
            // the toolbar's Help menu, and a second About panel showing a
            // subset of the same facts is the two-notions defect in its purest
            // form.
            item("About Polylinker", A(C::HelpAbout), &[]),
            Entry::Separator,
            item("Services", S(Standard::Services), &[]),
            Entry::Separator,
            item(
                "Hide Polylinker",
                S(Standard::Hide),
                &[shown(Chord::cmd(egui::Key::H))],
            ),
            item(
                "Hide Others",
                S(Standard::HideOthers),
                &[shown(Chord::cmd_alt(egui::Key::H))],
            ),
            item("Show All", S(Standard::ShowAll), &[]),
            Entry::Separator,
            // ⌘Q THROUGH THIS APPLICATION'S OWN CLOSE PATH, not `terminate:`.
            //
            // winit installs a menu bar of its own with Quit bound to
            // `terminate:`, and it has been live in every shipped Polylinker
            // (winit-0.30.13, src/platform_impl/macos/menu.rs:68-73). Measured
            // 2026-09-05, in three links: `terminate:` reaches eframe's
            // `on_exit` — eframe wired that on purpose — but it never produces
            // a `CloseRequested`, so `App::close_request`, which is armed by
            // `ctx.input(|i| i.viewport().close_requested())`, never runs; the
            // unsaved-changes question is not asked; and `on_exit` then
            // DELETES every recovery draft, because `abandoned_unsaved` is
            // only ever set by the dialog that was skipped. ⌘Q on a dirty
            // plasmid lost the document AND its only crash copy, silently. This
            // item is the fix: `Command::Quit` sends `ViewportCommand::Close`,
            // which is the one thing the latch reads.
            //
            // What it does NOT fix, said here so nobody looks for it: Dock ▸
            // Quit and a logout send `terminate:` to `NSApplication` directly,
            // not through this menu, and winit's delegate implements no
            // `applicationShouldTerminate:` to veto them. Those two paths still
            // skip the question.
            item(
                "Quit Polylinker",
                A(C::Quit),
                &[shown(Chord::cmd(egui::Key::Q))],
            ),
        ],
    },
    Menu {
        title: "File",
        entries: &[
            // NO ELLIPSIS, and the test below is what caught it: this item was
            // written "New…" in a draft and
            // `only_the_items_that_open_a_file_dialog_carry_an_ellipsis` refused
            // it. `App::top_bar` had already settled the question for the
            // toolbar's own New button — "New reaches a modal of our own and
            // touches no path at all, so an ellipsis here would be the same
            // small lie as 'Export map' over a gel" — and the menu bar had to
            // be told.
            item("New", A(C::NewDoc), &[shown(Chord::cmd(egui::Key::N))]),
            item("Open…", A(C::Open), &[shown(Chord::cmd(egui::Key::O))]),
            // The label says GenBank because ⌘S means GenBank, and the toolbar
            // had to answer the same ambiguity the moment `.dna` was added:
            // "Ctrl+S saves GenBank" is on that menu's hover for this reason.
            item(
                "Save as GenBank…",
                A(C::Save),
                &[shown(Chord::cmd(egui::Key::S))],
            ),
            item("Save as SnapGene .dna…", A(C::SaveDna), &[]),
            item("Save as FASTA…", A(C::SaveFasta), &[]),
            item("Save protein FASTA…", A(C::SaveProtein), &[]),
            Entry::Separator,
            item("Save project…", A(C::SaveProject), &[]),
            item("Open project…", A(C::OpenProject), &[]),
            Entry::Separator,
            item("Export figure as SVG…", A(C::ExportFigureSvg), &[]),
            item("Export figure as PDF…", A(C::ExportFigurePdf), &[]),
            item("Export figure as EPS…", A(C::ExportFigureEps), &[]),
            item("Export figure as PNG…", A(C::ExportFigurePng), &[]),
            Entry::Separator,
            // ⌘W IS THIS APPLICATION'S, AND THE WINDOW MENU MUST NOT HAVE A
            // CLOSE ITEM. macOS's standard Window menu puts Close on ⌘W; this
            // app has bound ⌘W to "close the active tab" since the bench
            // existed, and a tab is what a user with six plasmids open means by
            // "close". Installing both would silently repoint the chord at the
            // window and take five documents with it.
            item(
                "Close Tab",
                A(C::CloseTab),
                &[shown(Chord::cmd(egui::Key::W))],
            ),
            item(
                "Reopen Closed Tab",
                A(C::ReopenTab),
                &[shown(Chord::cmd_shift(egui::Key::T))],
            ),
        ],
    },
    Menu {
        title: "Edit",
        entries: &[
            // UNDO AND REDO PRINT NO CHORD, and that is the module header's
            // measurement applied to the one place it bites hardest. ⌘Z is
            // bound by this application AND by egui's `TextEdit`, which handles
            // it itself for whichever box has the caret — the Features filter,
            // the Library query, every field of the feature editor. The
            // `typing` guard in `global_shortcuts` exists so that a ⌘Z typed
            // there undoes the typo and not the molecule. A key equivalent on
            // this item would take ⌘Z before EITHER of them saw it, and a
            // refused key equivalent is not handed back: ⌘Z would be dead in
            // every text box in the window, with nothing on screen to say why.
            //
            // So the items are clickable, the chords stay exactly where they
            // are — with egui, under the guards that already govern them — and
            // the table still names them, so the completeness sweep knows
            // whose they are. An Edit menu that prints no ⌘Z is unusual on a
            // Mac. An Edit menu that breaks ⌘Z in every text field is worse,
            // and it was measured rather than feared.
            item(
                "Undo",
                A(C::Undo),
                &[withheld(
                    Chord::cmd(egui::Key::Z),
                    "a key equivalent takes ⌘Z before egui sees it, and ⌘Z belongs to \
                     whichever text box has the caret before it belongs to the molecule; \
                     claiming it here would make ⌘Z dead in every text field in the window",
                )],
            ),
            item(
                "Redo",
                A(C::Redo),
                &[
                    withheld(
                        Chord::cmd_shift(egui::Key::Z),
                        "the other half of ⌘Z; see Undo",
                    ),
                    // Bound since the shortcut block was written and advertised
                    // nowhere until the Redo button's hover. It stays live
                    // because a user arriving from Windows types it.
                    withheld(
                        Chord::cmd(egui::Key::Y),
                        "⌘Y is the Windows habit and stays bound; it belongs with ⇧⌘Z",
                    ),
                ],
            ),
            Entry::Separator,
            // "Find" and not "Find…", which the ellipsis test also caught. The
            // platform convention is "Find…" because on most applications it
            // opens a dialog; here it opens the in-window find BAR and no
            // window of any kind, so the ellipsis would say something untrue in
            // the one notation this program has agreed to use for it.
            item("Find", A(C::Find), &[shown(Chord::cmd(egui::Key::F))]),
            item(
                "Find Next",
                A(C::FindNext),
                &[shown(Chord::plain(egui::Key::F3))],
            ),
            item(
                "Find Previous",
                A(C::FindPrev),
                &[shown(Chord::shift(egui::Key::F3))],
            ),
            item(
                "Hide Find Bar",
                A(C::CloseFind),
                &[withheld(
                    Chord::plain(egui::Key::Escape),
                    "AppKit's key-equivalent loop runs before the responder chain, so an \
                     Escape key equivalent would take Escape from every text field in the \
                     window — including the find box this command exists to serve",
                )],
            ),
        ],
    },
    Menu {
        title: "View",
        entries: &[
            item(
                "Next Tab",
                A(C::NextTab),
                &[withheld(
                    Chord::cmd(egui::Key::Tab),
                    "macOS owns ⌘Tab: it is the application switcher and no application \
                     menu can claim it. The binding stays because it reaches this app when \
                     the switcher does not, and the item is clickable either way",
                )],
            ),
            item(
                "Previous Tab",
                A(C::PrevTab),
                &[withheld(
                    Chord::cmd_shift(egui::Key::Tab),
                    "the other half of ⌘Tab; see Next Tab",
                )],
            ),
        ],
    },
    Menu {
        title: "Window",
        entries: &[
            item(
                "Minimize",
                S(Standard::Minimize),
                &[shown(Chord::cmd(egui::Key::M))],
            ),
            item("Zoom", S(Standard::Zoom), &[]),
            Entry::Separator,
            item("Bring All to Front", S(Standard::BringAllToFront), &[]),
            // NO CLOSE ITEM. See File ▸ Close Tab.
        ],
    },
    Menu {
        title: "Help",
        entries: &[
            // F1 AND NOT ⌘?, and that is a refusal rather than an oversight.
            // macOS convention is ⇧⌘/ for Help; this application binds F1 and
            // binds nothing to ⇧⌘/, and an item advertising a chord the app
            // does not bind is the one defect this whole table exists to make
            // impossible. If ⌘? is wanted, `global_shortcuts` gets it first and
            // this line follows — never the other way round.
            item(
                "Polylinker Help",
                A(C::Help),
                &[shown(Chord::plain(egui::Key::F1))],
            ),
            item("What it computes", A(C::HelpTm), &[]),
            item("Licences and notices", A(C::HelpLicences), &[]),
            Entry::Separator,
            item("Open an example plasmid", A(C::OpenExample), &[]),
        ],
    },
];

/// Every item on the board, menu title and entry together.
///
/// The tests' view of the table. The installer walks `MENUS` itself, because
/// it needs the separators this flattening drops.
#[cfg(test)]
pub fn items() -> impl Iterator<Item = (&'static str, &'static str, Action, &'static [Bind])> {
    MENUS.iter().flat_map(|m| {
        m.entries.iter().filter_map(move |e| match e {
            Entry::Item {
                label,
                action,
                binds,
            } => Some((m.title, *label, *action, *binds)),
            Entry::Separator => None,
        })
    })
}

/// Where a menu command came from.
///
/// **A KEY EQUIVALENT IS A KEYSTROKE**, and that is not a pedantic distinction
/// — it is the whole reason this enum exists. See the module header: a chord
/// the menu claims never reaches egui, so `global_shortcuts` never sees it and
/// its guards never run. Which means the menu inherits the guards, all of
/// them, and one of them — `typing` — has to know whether what arrived was a
/// keystroke or a click, because it means different things for the two. The
/// AppKit side reads `NSApplication.currentEvent` to fill this in: a key-down
/// means `KeyEquivalent`, anything else means `Click`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// AppKit intercepted a keystroke that would otherwise have reached egui.
    KeyEquivalent,
    /// A pointer, or a menu walked with the arrow keys. Not a chord.
    Click,
}

/// Which commands may run right now.
///
/// **THE ONE ANSWER, AND IT IS DELIBERATELY NOT ONE ANSWER PER ITEM.** A native
/// menu item bypasses everything egui knows: AppKit has never heard of
/// `egui::Modal`, so a click on File ▸ Save under the unsaved-changes question
/// would reach `App::export` while the question about that very document is on
/// screen — the exact failure `App::asking` was written for, arriving through a
/// door it was never told about.
///
/// So the menu path runs through this, and `global_shortcuts` decides the same
/// three predicates for the keyboard path, and
/// `the_menu_gate_and_the_keyboard_guards_are_one_policy` drives both against
/// the real function to prove they agree:
///
/// - **`asking`** — a question about the document is on screen. Blocks
///   EVERYTHING, both origins, because the reason is about the document's state
///   and not about the input device: "the state answered about must be the
///   state acted on" (`sequence_keys`). It blocks Quit too, which is right:
///   while the unsaved-changes question is up, that question IS the quit.
/// - **`typing`** — a text box has the keyboard. Blocks a `KeyEquivalent` and
///   NOT a `Click`, and that split is the one place the two origins differ. The
///   guard exists because a Ctrl+Z typed into the Features filter belongs to the
///   filter — which is true whether egui saw the keystroke or AppKit ate it
///   first. A mouse click on a menu belongs to nobody else, and refusing it
///   would leave File ▸ Save inert whenever a caret happened to be in a search
///   box, with nothing on screen to say why.
///
///   Two exceptions, each for a reason already written down elsewhere. The find
///   keys, because `global_shortcuts` reads them above its own guard:
///   "stepping through hits is what you do WHILE the query box has focus". And
///   Quit, because the guard exists to keep text-editing chords with the text
///   box, and ⌘Q has never been one — a ⌘Q refused because a caret was in the
///   Library query would be a quit that silently does nothing, which is the
///   one thing worse than the `terminate:` it replaces.
/// - **`designing`** — the design panel or the feature editor is open. Blocks
///   Undo and Redo on both origins, for the reason `global_shortcuts` gives: the
///   panel snapshots the bases it is describing and an undo underneath it leaves
///   the panel describing bases that are no longer there. Which device the undo
///   came from does not change that.
///
/// The per-command PRECONDITIONS — "there is a document", "the bench is not
/// empty" — are NOT here. They live in `App::can`, the menu path funnels into
/// the same dispatcher, and AppKit greys an item on `Gate::allows` AND
/// `App::can` rather than on a fresh opinion of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gate {
    pub asking: bool,
    pub typing: bool,
    pub designing: bool,
}

impl Gate {
    /// The three guards `App::global_shortcuts` decides, read off the same
    /// `App` and the same `Context`, so there is one reading rather than two.
    pub fn read(asking: bool, typing: bool, designing: bool) -> Self {
        Self {
            asking,
            typing,
            designing,
        }
    }

    /// Whether `c` may pass the typing guard when it arrived as a keystroke.
    pub fn exempt_from_typing(c: Command) -> bool {
        matches!(
            c,
            Command::FindNext | Command::FindPrev | Command::CloseFind | Command::Quit
        )
    }

    pub fn allows(self, c: Command, from: Origin) -> bool {
        if self.asking {
            return false;
        }
        if self.typing && from == Origin::KeyEquivalent && !Self::exempt_from_typing(c) {
            return false;
        }
        !(self.designing && matches!(c, Command::Undo | Command::Redo))
    }
}

/// What the native menu has been clicked, waiting for the next frame.
///
/// A `static` and not a field on `App`, because the AppKit callback that fills
/// it has no `&mut App` and can have none: it runs on the main thread inside
/// AppKit's own event loop, arbitrarily deep inside a call stack this
/// application does not own. Measured 2026-09-05: re-entering the frame from
/// there aborts the process — winit's `event_handler.rs:135`, "tried to handle
/// event while another event is currently being handled", then a panic in a
/// function that cannot unwind, exit 134. So the callback does the one thing
/// that is safe from there — pushes a `Command` and returns — and
/// `App::commands` drains it on the next frame, where the guards are, where
/// `&mut self` is, and where the keyboard's answer is decided too.
///
/// THAT DEFERRAL IS THE POINT AND NOT AN IMPLEMENTATION DETAIL. Running
/// `App::export` from inside a menu callback would also open a native modal
/// file dialog from inside AppKit's menu-tracking run loop — the same shape of
/// mistake `App::top_bar` already writes down for `rfd` inside a menu closure.
static QUEUE: Mutex<Vec<(Command, Origin)>> = Mutex::new(Vec::new());

/// Where to knock so the frame happens.
///
/// egui runs a frame only when something asks for one, and a menu click is not
/// something it knows about. Measured 2026-09-05: an idle window ran NO frames
/// for fifteen seconds, and four menu actions fired into the queue during that
/// time sat unseen for up to thirteen seconds — not late, unseen — until an
/// unrelated mouse movement woke the loop. `push` calls `request_repaint` on
/// this, so a click is a frame.
///
/// A `Mutex<Option<_>>` AND NOT A `OnceLock`, because `main` calls `start()`
/// twice when the glow backend fails and wgpu is tried — "eframe caches its
/// winit EventLoop in a thread-local and reuses it" — and a `OnceLock` would
/// keep the first, dead `Context` and knock on it forever.
static WAKE: Mutex<Option<egui::Context>> = Mutex::new(None);

/// Which commands the menu may currently fire, as a bit per [`Command::ALL`]
/// position. Published by `App::commands` every frame; read by the installer's
/// `validateMenuItem:` so an item greys out for the same reason a click on it
/// would be refused. Everything on until the first frame has spoken.
static ENABLED: Mutex<u64> = Mutex::new(u64::MAX);

/// Register the context a menu click should wake. Called once per `App::new`.
pub fn attach(ctx: &egui::Context) {
    *WAKE.lock().unwrap_or_else(|e| e.into_inner()) = Some(ctx.clone());
}

/// Called from the AppKit menu callback. One push and one knock; nothing else.
///
/// `unwrap_or_else(PoisonError::into_inner)` and not `unwrap`: one panic in
/// some other lock holder must not disable the menu bar for the rest of the
/// session.
pub fn push(c: Command, from: Origin) {
    QUEUE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((c, from));
    if let Some(ctx) = WAKE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        ctx.request_repaint();
    }
}

/// Take what has been clicked since the last frame, each command at most once.
///
/// AT MOST ONCE, because the keyboard is: `i.key_pressed(Key::N)` is a per-frame
/// bool, so two Ctrl+N inside one frame open ONE New dialog, and two File ▸ New
/// clicks between two frames must therefore open one as well, or the same
/// gesture means different things depending on which input device performed
/// it. The first arrival's `Origin` is kept, since it is the one that would
/// have been dispatched had a frame run in between.
///
/// Empty on every platform but macOS, which is why `App::commands` needs no
/// `cfg`: the menu path exists everywhere and nothing ever arrives on it
/// except where a menu bar was installed.
pub fn drain() -> Vec<(Command, Origin)> {
    let taken = std::mem::take(&mut *QUEUE.lock().unwrap_or_else(|e| e.into_inner()));
    let mut out: Vec<(Command, Origin)> = Vec::with_capacity(taken.len());
    for (c, o) in taken {
        if !out.iter().any(|(seen, _)| *seen == c) {
            out.push((c, o));
        }
    }
    out
}

/// Publish this frame's answer to "may this item fire if clicked".
pub fn publish_enabled(mut allowed: impl FnMut(Command) -> bool) {
    let mut mask = 0u64;
    for (i, c) in Command::ALL.iter().enumerate() {
        if allowed(*c) {
            mask |= 1 << i;
        }
    }
    *ENABLED.lock().unwrap_or_else(|e| e.into_inner()) = mask;
}

/// Read by the installer's `validateMenuItem:`.
pub fn enabled(c: Command) -> bool {
    (*ENABLED.lock().unwrap_or_else(|e| e.into_inner())) & (1 << c.index()) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bitmask has a bit per command.
    ///
    /// Not a formality: `publish_enabled` shifts by the command's index, and
    /// the 65th command would shift out of a `u64` and be permanently enabled
    /// — silently, which is the failure this file is written against.
    #[test]
    fn the_enabled_mask_has_a_bit_for_every_command() {
        assert!(
            Command::ALL.len() <= 64,
            "{} commands; ENABLED is a u64",
            Command::ALL.len()
        );
    }

    /// `tag` and `from_tag` are inverses, and nobody is tag 0.
    ///
    /// PROVEN TO FAIL on 2026-09-05 by making `tag` return `index()` with no
    /// `+ 1`: `from_tag(0)` was `Some(NewDoc)`, which is what an untagged
    /// `NSMenuItem` — every separator, every standard item — would dispatch as.
    #[test]
    fn a_tag_round_trips_and_zero_is_nobody() {
        for c in Command::ALL {
            assert_eq!(Command::from_tag(c.tag()), Some(*c), "{c:?}");
            assert_ne!(
                c.tag(),
                0,
                "{c:?} would be indistinguishable from an untagged item"
            );
        }
        assert_eq!(Command::from_tag(0), None);
        assert_eq!(Command::from_tag(-1), None);
        assert_eq!(Command::from_tag(Command::ALL.len() as isize + 1), None);
    }

    /// Every chord the menu prints has a spelling AppKit accepts.
    ///
    /// The translation lives here and not in `macmenu.rs` so that this runs on
    /// the Linux and Windows legs too: a chord added to the board with a key
    /// `key_equivalent` does not know would otherwise be found only by the
    /// macOS release build, installed as an item with an empty key equivalent
    /// and no chord printed beside it.
    ///
    /// PROVEN TO FAIL on 2026-09-05 by deleting the `K::F3` arm:
    /// `Edit > "Find Next" prints Chord { key: F3, .. } and key_equivalent has
    /// no spelling for it`.
    #[test]
    fn every_shown_chord_has_a_key_equivalent() {
        for (m, label, _, binds) in items() {
            for b in binds {
                if b.show == Show::KeyEquivalent {
                    assert!(
                        b.chord.key_equivalent().is_some(),
                        "{m} > {label:?} prints {:?} and key_equivalent has no spelling for it",
                        b.chord
                    );
                }
            }
        }
    }

    /// Two clicks between frames are one command, and the first origin wins.
    ///
    /// PROVEN TO FAIL on 2026-09-05 by making `drain` return `taken` unfiltered:
    /// `left: 2, right: 1`.
    #[test]
    fn a_command_clicked_twice_between_frames_is_drained_once() {
        // Nothing else in this test binary pushes, and `drain` empties the
        // queue, so the assertions below are about these pushes alone.
        drain();
        push(Command::SaveDna, Origin::Click);
        push(Command::SaveDna, Origin::KeyEquivalent);
        push(Command::Open, Origin::KeyEquivalent);
        let got = drain();
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0], (Command::SaveDna, Origin::Click));
        assert_eq!(got[1], (Command::Open, Origin::KeyEquivalent));
        assert!(drain().is_empty(), "drain did not empty the queue");
    }

    /// The published mask is what `enabled` reads back.
    ///
    /// PROVEN TO FAIL on 2026-09-05 by making `enabled` test `1 << c.tag()`
    /// instead of `1 << c.index()`: every answer was off by one command.
    #[test]
    fn the_enabled_mask_round_trips() {
        publish_enabled(|c| matches!(c, Command::Save | Command::Quit));
        for c in Command::ALL {
            assert_eq!(
                enabled(*c),
                matches!(c, Command::Save | Command::Quit),
                "{c:?}"
            );
        }
        publish_enabled(|_| true);
    }
}
