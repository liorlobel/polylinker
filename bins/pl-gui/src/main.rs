//! Polylinker — a plasmid viewer that runs offline and asks nothing of anyone.
//!
//! Everything it decides about a molecule it asks `pl-core`, `pl-fileio` and
//! `pl-enzymes`, the same crates behind the `pl` command line and the browser
//! build. This binary is presentation.

// A console window alongside the app on Windows is noise for a GUI, but keep it
// in debug builds so panics and eprintln stay visible while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod aa;
mod annot;
mod bench;
mod clone;
mod design;
mod doc;
mod featedit;
mod gel;
mod library;
mod map;
mod reads;
mod recover;
mod scene;
mod seqedit;
mod settings;
/// The ligature guard reads the vendored faces at test time and nothing at run
/// time, so it is not compiled into the shipped binary. Gated rather than left
/// dead because `clippy -D warnings` is right to object to an unused parser, and
/// because a reader should be able to tell an assertion from a feature.
#[cfg(test)]
mod sfnt;
mod theme;

use std::path::PathBuf;

use eframe::egui::{self, Align, Layout, RichText, Sense, Ui};
use pl_core::Strand;

use doc::{describe, fmt_int, DigestState, Document};
use theme::Palette;

/// The monospace face the sequence grid, the gutters and the map labels are set
/// in. IBM Plex Mono 2.005 Regular, SIL Open Font License 1.1.
///
/// Vendored unmodified, byte-for-byte the file in IBM's own release archive, so
/// the sha256 in NOTICE is checkable by a third party against upstream. It is
/// deliberately NOT subsetted: OFL 1.1 defines a Modified Version as any
/// derivative made by "deleting ... any of the components of the Original
/// Version", a subset deletes components, and clause 3 then forbids the result
/// carrying the Reserved Font Name "Plex" as its primary name. Subsetting would
/// buy about 60 KB and cost a rename plus a hash that matches nothing upstream.
///
/// WHAT THE SWAP COSTS, RECORDED BECAUSE IT IS THE ONE THING THAT GOT WORSE and
/// no assertion in this file can see it. `OS/2.sxHeight` is 516/1000 = 0.5160 em
/// here against Hack 3.003's 1120/2048 = 0.5469 -- 5.6% less lowercase at the
/// same nominal size, 6.29 pt of x-height at the grid's 11.5 pt becoming 5.93.
/// Sampled off the running app on the user's own plasmid, a row of sixty
/// lowercase bases carries 16.8% less ink than it did.
///
/// IT IS ACCEPTED AT 11.5 PT AND THE OBVIOUS COMPENSATION IS BARRED. The advance
/// band is in em, so a size bump does not move the ratio -- but `per_row` is in
/// POINTS, and 11.5 -> 12.0 pt widens the cell from 6.900 to 7.200 and loses the
/// sixtieth base. The whole headroom is 0.60375/0.600 x 11.5 = 11.5719 pt, which
/// is 0.0719 pt, and the same bound caps any `FontTweak.scale` at 1.00625.
///
/// TAKE 0.60375 FROM THE BAND TEST'S OWN OUTPUT AND NOT FROM HERE. It is the
/// band's measured upper edge FOR THE SHIPPED FACE, and it moved when the face
/// did: at 0aa0f88, with Hack, the same bisection printed 0.60400 and the chrome
/// it is measured against was 30.00 pt rather than 30.16. This comment carried
/// the Hack figures for one review cycle, which put the stated `FontTweak.scale`
/// cap ABOVE the real edge -- a maintainer who scaled to the number written here
/// would have lost the sixtieth base and had no test say so. Run
/// `cargo test -p pl-gui the_advance_band -- --nocapture` and read the line.
/// It is bisected to +/-0.00025, so treat the edge as a bound to stay under
/// rather than a value to sit on.
///
/// If the grid ever does need to be larger the lever is `App::DEF_PANEL`, and
/// moving it re-opens the band calibration and takes width off the map pane.
///
/// CONTRAST IS UNAFFECTED AND THAT WAS CHECKED RATHER THAN ASSUMED. A WCAG ratio
/// is a function of two luminances and contains no typeface term, so the palette
/// numbers cannot move; the question a font swap really raises is whether a
/// lighter stem puts so little ink under antialiasing that the SAMPLED foreground
/// drifts toward the background. Measured off screenshots of both builds at this
/// machine's 120 dpi: the ruler and both gutters still reach `muted`'s full
/// 5.46:1 in dark mode and the bases still reach 14.83:1. Fewer inked pixels, the
/// same colour in the ones that are inked.
const PLEX_MONO: &[u8] = include_bytes!("../fonts/IBMPlexMono-Regular.ttf");

/// The proportional face: IBM Plex Sans 3.005 Regular, same licence, same
/// archive, also unsubsetted for the same reason.
///
/// Chosen over the inherited Ubuntu Light for one measured reason. Ubuntu
/// Light's capital `I` is a bare stem 0.068 em wide, all but indistinguishable
/// from `l` at 0.141 em; Plex Sans gives it crossbars at 0.280 em against `l` at
/// 0.156. This app's proportional text is enzyme names, and `HindIII`, `SfiI`,
/// `AflII` and `BspLU11III` all end in runs of capital I. Mis-reading one names
/// a different enzyme, so this is legibility as correctness, not as taste.
const PLEX_SANS: &[u8] = include_bytes!("../fonts/IBMPlexSans-Regular.ttf");

/// The icon face: Phosphor Icons 2.1 **Bold**, MIT, arriving through
/// `egui-phosphor` 0.13.0 rather than being vendored here — the same route the
/// four `default_fonts` faces take, and NOTICE records it the same way.
///
/// **THIS FACE IS DANGEROUS AND IS SHIPPED ANYWAY, WHICH IS ONLY DEFENSIBLE
/// BECAUSE OF WHERE IT IS INSTALLED.** Its cmap covers all 26 lowercase letters,
/// space and hyphen, every one of them with an `hmtx` advance of ZERO — `' '` at
/// 512/1024 is the only non-zero advance anywhere in printable ASCII — and it
/// carries a `liga` feature with 1,513 ligature rules on top of that. Put it in
/// the Monospace chain ahead of Plex Mono and a 60-base lowercase row lays out
/// 115.00 pt wide where the grid computes 414.00, with x going *backwards*
/// between glyphs 5 and 6. `seqedit` rests on `x(base) = x0 + col * advance`, so
/// that is 43 cells of drift by the end of a row and every click lands on the
/// wrong base. Measured, not supposed; see [`font_definitions`] for the
/// containment and `NOTICE` for the history.
///
/// BOLD RATHER THAN REGULAR, AND THE REASON IS MEASURED. Every LigatureSubst
/// rule in all five faces was reverse-mapped through the cmap: Phosphor Regular
/// has forty rules spelled entirely from the IUPAC nucleotide alphabet —
/// including `at`, `cat`, `tag`, `dna`, `scan`, `star` — and 200-odd spelled
/// from the amino-acid alphabet, because the rule names come from the icon set's
/// own `selection.json`. Bold, Fill, Light and Thin have ZERO, because every
/// non-regular variant suffixes the icon name with the variant, so a rule needs
/// `a,t,-,b,o,l,d` and cannot fire on a sequence row. Nothing is lost by the
/// choice: the five generated constant files are byte-identical (121,165 bytes
/// each) and the five cmaps are the same 1,543 codepoints, so
/// `bold::ARROW_U_UP_LEFT` and `regular::ARROW_U_UP_LEFT` are both U+E08A.
/// Bold also holds the most ink per pixel at toolbar sizes, which is the axis
/// that decides whether a stroked glyph clears 3:1 after antialiasing.
///
/// That is DEFENCE IN DEPTH and not the defence: the zero advances are in Bold
/// too, so Bold in the Monospace chain destroys the grid just as thoroughly.
/// The isolation is what makes this safe. The variant only removes one of the
/// two mechanisms, for free.
fn phosphor() -> &'static [u8] {
    egui_phosphor::Variant::Bold.font_bytes()
}

/// The family the icon face is installed under, and the only family it is in.
///
/// **A NAMED CONSTANT RATHER THAN A LITERAL AT EACH USE SITE**, for the reason
/// [`MOLECULE_MENU`] is one: `FontFamily::Name(Arc<str>)` is a `BTreeMap` key
/// compared by string equality, so `Name("icons")` and `Name("Icons")` are two
/// different families — and asking for one that was never registered is not a
/// fallback but a PANIC, `FontsImpl::font`'s
/// `panic!("FontFamily::{family:?} is not bound to any fonts")` (epaint 0.35
/// `fonts.rs:1031`). A typo there is a crash on first paint with no compile
/// error anywhere.
static ICON_FAMILY: std::sync::LazyLock<egui::FontFamily> =
    std::sync::LazyLock::new(|| egui::FontFamily::Name("icons".into()));

/// The `FontDefinitions` the binary installs, as a value a test can inspect.
///
/// **SPLIT OUT OF [`install_fonts`] SO THE GUARDS READ THE SHIPPED VALUE RATHER
/// THAN A COPY OF IT.** A structural test that rebuilds the definitions itself
/// asserts something about the test, not about the binary; that is the same
/// class of mistake as measuring a `Context` the fonts were never installed
/// into, which `test_ctx` exists to prevent.
///
/// The two text chains are untouched — `.insert(0, ..)` prepends, nothing is
/// removed — and Phosphor is added as a THIRD FAMILY rather than as a fourth
/// entry in either of them. See [`install_fonts`] for the resolved order and
/// `the_icon_face_is_in_its_own_family_and_in_neither_text_chain` for the
/// assertion.
fn font_definitions() -> egui::FontDefinitions {
    // `FontDefinitions::default()` is already the four `default_fonts` faces in
    // their default order; this prepends to that rather than replacing it.
    let mut defs = egui::FontDefinitions::default();
    for (name, bytes) in [
        ("IBMPlexMono", PLEX_MONO),
        ("IBMPlexSans", PLEX_SANS),
        // Registered as DATA. Being in `font_data` puts a face in the binary
        // and in nothing's fallback chain; only the `families` map below can
        // make it reachable from a `FontId`.
        ("Phosphor", phosphor()),
    ] {
        defs.font_data.insert(
            name.to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(bytes)),
        );
    }
    for (family, name) in [
        (egui::FontFamily::Monospace, "IBMPlexMono"),
        (egui::FontFamily::Proportional, "IBMPlexSans"),
    ] {
        defs.families
            .entry(family)
            .or_default()
            .insert(0, name.to_owned());
    }

    // A THIRD FAMILY, HOLDING ONE FACE, APPENDED TO NOTHING.
    //
    // **DO NOT REPLACE THIS WITH `egui_phosphor::add_to_fonts`.** It is the
    // crate's own documented helper and it is the obvious thing to reach for,
    // and it is wrong here — not because it is unsafe today, which it is not.
    // Read at 0.13.0 it inserts "phosphor" at Proportional index 1 and touches
    // Monospace not at all, and measured through the real `Fonts` against this
    // very chain it moves nothing: 414.00 pt Monospace, 329.44 pt Proportional,
    // byte-identical to a build without it. It is refused because it is safe
    // only by an ORDERING that nothing asserts. Index 1 is harmless purely
    // because Plex Sans sits at index 0 and covers a-z; rewrite the loop above
    // to `.push(name)`, or drop the Plex Sans prepend, and every proportional
    // label in the app is handed the lowercase alphabet of a zero-advance
    // ligating face. The crate has already shipped that defect once — its own
    // changelog reads "0.7.2 add_to_fonts now sets phosphor as top priority
    // font instead of last", then "0.7.3, same day, Fixed issue with phosphor
    // overriding some normal latin text glyphs in egui". Reproduced here at
    // Proportional index 0: a feature named "cat" renders as one 9.00 pt cat
    // icon, and `main.rs`'s `name_font` draws feature names proportionally at
    // 9 pt. Nineteen of the 89 curated rows in `features/features.tsv` contain
    // a Phosphor ligature name; `tag` is an icon, and epitope tags are the most
    // common annotation class in a plasmid. It also `Vec::insert(1, ..)`s,
    // which panics on the empty Proportional chain a `default_fonts`-off build
    // would have, and it hard-codes the `font_data` key with no way to name a
    // family, so it cannot express this shape at all. Eleven lines replaced by
    // four.
    //
    // ONE FACE, WITH NO TEXT FACE BEHIND IT, AND THAT IS DELIBERATE.
    // `CachedFamily::new` looks for U+25FB and then '?' to choose a replacement
    // face; Phosphor has neither, so it logs `Failed to find replacement
    // characters ... Will use empty glyph` once and every family member that
    // misses renders at zero width — "Save" laid out in this family is three
    // glyphs and 0.00 pt wide. That is the correct behaviour and the warning is
    // an honest signal, not a defect to silence: a label that strays into the
    // icon family must be an obvious hole in the UI at review time rather than
    // a slightly-wrong-looking word that ships. Do not add a text face here.
    defs.families
        .insert(ICON_FAMILY.clone(), vec!["Phosphor".to_owned()]);
    defs
}

/// Install the vendored faces at the head of both family chains.
///
/// **THIS FUNCTION EXISTS AS A FUNCTION, RATHER THAN AS FOUR LINES INSIDE
/// [`App::new`], BECAUSE OF THE FAILURE THAT WOULD OTHERWISE HAVE MADE EVERY
/// TEST IN THIS FILE A LIE.** Every font-touching test builds its own
/// `egui::Context`. Had the install lived in `App::new` — the only place with a
/// `CreationContext` — then the shipped binary would draw Plex while all thirty
/// or so of those tests went on measuring Hack and Ubuntu Light, and the one
/// assertion written to catch a face change, the advance band's pin on the
/// incumbent ratio, would have stayed green through the swap. `test_ctx` below
/// calls this, so the tests and the binary install the same fonts by
/// construction rather than by anyone remembering to.
///
/// `eframe`'s `default_fonts` feature stays on and nothing is removed from
/// either chain, so the resolved order is:
///
///   Monospace     IBM Plex Mono, Hack, Ubuntu-Light, NotoEmoji, emoji-icon-font
///   Proportional  IBM Plex Sans, Ubuntu-Light, NotoEmoji, emoji-icon-font
///   icons         Phosphor
///
/// The fallbacks are load-bearing and not politeness. Plex Mono has no U+25B6,
/// which is `HISTORY_HERE`, the History tab's cursor on the current state; it
/// comes from Hack. Neither Plex face has U+26A0, the hidden-cut-sites warning
/// marker; it comes from Noto Emoji. Dropping `default_fonts` would draw both as
/// tofu boxes, which is the concrete reason the Ubuntu Font Licence question was
/// worth answering rather than sidestepping.
///
/// **AN ICON FONT IS NOW INSTALLED, AND THE PARAGRAPH THAT USED TO STAND HERE
/// SAID THE OPPOSITE.** It read "NO ICON FONT IS INSTALLED, and that is a
/// decision rather than an omission ... 488 KB of Phosphor would have put
/// `a + t -> uniE0AC` in reach of a lowercase plasmid". The hazard was real and
/// is unchanged; what changed is that the face is now in a family of its own
/// that no text in this application is laid out in, so the grid cannot reach it
/// by any ordering. See [`phosphor`] for the measurements and
/// [`font_definitions`] for the containment. Both text chains above are exactly
/// what they were at 7ce59c1 — verified, not assumed: the 60-base row still
/// lays out 414.00 pt in Monospace and 329.44 pt in Proportional, to the last
/// hundredth.
///
/// THE CARET IN [`menu_with_caret`] IS STILL A POLYGON, and that was never
/// contingent on this. `CARET_DOWN` (U+E136) is now installed and available and
/// is still the wrong tool for that mark; the reasons are in that function's own
/// doc comment and none of them was "we have no icon font".
fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
}

/// A `Context` with the shipped fonts in it, which is the only kind any test
/// that measures a glyph may use.
///
/// **THE PASS IS NOT OPTIONAL.** `Context::set_fonts` does not install anything;
/// it parks the definitions in `Memory::new_font_definitions` (egui 0.35
/// `context.rs:2038`) and the next pass picks them up. A context that is built,
/// handed the fonts and then measured without running a pass returns the DEFAULT
/// face and looks entirely healthy doing it. That is the second way this change
/// could have shipped a green suite that proved nothing about the binary, and it
/// is why the `run_ui` below is part of the helper instead of being left to each
/// caller to remember.
///
/// `the_test_context_installs_the_faces_the_binary_ships` is the proof that this
/// function does something: it measures the advance through a bare
/// `Context::default()` and through this, and asserts they DIFFER.
#[cfg(test)]
pub(crate) fn test_ctx() -> egui::Context {
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
    ctx
}

/// The toolbar menu holding the whole-molecule operations.
///
/// Named here and not written out at the two places that need it, because
/// [`seqedit`]'s caret refusals quote this path IN PROSE — "use Molecule > Set
/// origin at selected feature" — and 0ebaa41 renamed the menu from "Edit"
/// without touching them. A user told to use a menu that does not exist is worse
/// off than one told nothing: they go looking, and the app is the thing that
/// lied. Two literals cannot be kept in step by care; one const can.
///
/// The separator in the prose is a plain `>` and not U+25B8 `▸`, which the
/// messages used. U+25B8 is present in Hack and in NOTHING else compiled into
/// the binary — not Ubuntu-Light, not either emoji font — and those messages are
/// drawn PROPORTIONALLY (`RichText::new(msg).size(11.0)`, `sequence_tab`), where
/// the fallback chain has no Hack in it. It rendered as a tofu box. `strand_word`
/// exists for the same reason one family up.
pub const MOLECULE_MENU: &str = "Molecule";
/// The item in [`MOLECULE_MENU`] that renumbers the plasmid.
pub const SET_ORIGIN_ITEM: &str = "Set origin at selected feature";
/// The item in [`MOLECULE_MENU`] that opens the feature editor on nothing.
///
/// A const for the same reason as the two above: the tests name it, and a
/// literal in the menu and another in a test drift apart the first time the
/// wording is improved.
pub const ADD_FEATURE_ITEM: &str = "Add feature…";
/// The item in [`MOLECULE_MENU`] that opens the feature editor on the selection.
pub const EDIT_FEATURE_ITEM: &str = "Edit selected feature…";
/// The item in [`MOLECULE_MENU`] that cuts and religates.
pub const CLONE_ITEM: &str = "Cut and religate…";

/// The path a user has to walk to set the origin, as prose points at it.
pub fn set_origin_path() -> String {
    format!("{MOLECULE_MENU} > {SET_ORIGIN_ITEM}")
}

/// Theme-resolved colours for whatever `ui` is currently drawing into.
fn pal(ui: &Ui) -> Palette {
    Palette::of(ui.visuals().dark_mode)
}

/// How wide the disclosure caret's own space is, and how tall the triangle in it.
const CARET_W: f32 = 7.0;
const CARET_H: f32 = 3.5;

/// A menu button that says it is a menu.
///
/// **THE ONLY ICON IN THIS APPLICATION THAT IS A POLYGON, AND HERE IS WHY IT
/// STAYS ONE.** That sentence used to read "the only icon in this application",
/// full stop, and the Undo and Redo glyphs below made it false. The caret's own
/// argument is untouched by their arrival and was never contingent on it.
///
/// The toolbar's own comment records that a caret was tried at 0ebaa41 and
/// photographed: `egui::menu_button` paints a plain button, so a triangle had to
/// be a character, U+25BE is in none of the embedded faces, and it came out an
/// empty box on all three menus. That is an argument against font-delivered
/// chrome, not against the caret — three points need no `cmap`, cannot tofu,
/// cannot ligate, need no licence entry, and scale with the widget rather than
/// with a text size.
///
/// AND THE LAST OF THOSE IS NOW THE DECIDING ONE, because Phosphor's
/// `CARET_DOWN` (U+E136) is installed and available and is still refused.
/// `CARET_W`/`CARET_H` are 7.0 x 3.5, a deliberately non-square 2:1 chevron
/// sized to the BUTTON; a glyph is laid out as text at 1.000 em square, so it
/// would be sized by a font size, land as a 1:1 box, and need a size and an
/// offset picked by eye and re-derived every time the button padding moves.
/// `the_disclosure_caret_clears_three_to_one_on_the_button_it_is_drawn_on`
/// measures the polygon's `pal(ui).ink` against the button fill; a glyph takes
/// `visuals.widgets.*.fg_stroke.color` instead, so swapping in a glyph would
/// leave that test green while measuring a colour the caret no longer uses —
/// the exact failure this project names most often. Three menus, a working
/// tested zero-byte polygon, and nothing visible to gain.
///
/// WHAT IT BUYS, which is the test every candidate icon in the audit had to pass
/// and the only one that did. A menu is otherwise distinguished from a button
/// only by the private convention "nouns are menus, verbs are buttons" — a rule
/// the user cannot know. The caret says *this opens, it does not act*, before the
/// click rather than after it.
///
/// IT ACCOMPANIES AND DOES NOT REPLACE. The label is still the word: "Save",
/// "Export map", "Molecule". `accesskit` is on, and the accessible name is that
/// word, unchanged — which an icon font could not have said, because a Private
/// Use Area codepoint is what a screen reader would have been handed.
///
/// The space is reserved by an empty [`egui::Atom`] so the triangle cannot land
/// on the last letter, and the triangle is then painted from the button's own
/// right edge inward. Cost: `CARET_W` plus one `icon_spacing` per menu, about
/// 11 pt each and 33 pt over the three — which is why the audit stopped at three
/// and did not put a CARET on `Open…`, `Undo` or `Redo`: none of them opens
/// anything, so the mark would have been a lie as well as 99 pt of an 880 pt
/// window. Undo and Redo did later get an icon, and a different one, for a
/// different reason; see [`button_with_icon`].
fn menu_with_caret<R>(
    ui: &mut Ui,
    label: &str,
    add: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    use egui::AtomExt as _;
    let out = ui.menu_button(
        (
            label,
            egui::Atom::default().atom_size(egui::vec2(CARET_W, CARET_H)),
        ),
        add,
    );
    let r = out.response.rect;
    let pad = ui.spacing().button_padding.x;
    let right = r.right() - pad;
    let mid = r.center().y;
    // UI chrome, so SC 1.4.11 applies and 3:1 is the bar. `ink` is the body-text
    // ink and clears 4.5:1 in both themes, so it clears 3:1 with room; `faint`
    // (1.75 light / 2.29 dark) and `line` (2.82 light) do not, and the trap of
    // picking a palette role that passes against the panel and fails against the
    // thing it is actually drawn on is already recorded in `ring.rs`. Measured
    // against the button fill, not the panel, by
    // `the_disclosure_caret_clears_three_to_one_on_the_button_it_is_drawn_on`.
    let ink = pal(ui).ink;
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(right - CARET_W, mid - CARET_H * 0.5),
            egui::pos2(right, mid - CARET_H * 0.5),
            egui::pos2(right - CARET_W * 0.5, mid + CARET_H * 0.5),
        ],
        ink,
        egui::Stroke::NONE,
    ));
    out
}

/// The Undo and Redo glyphs, and the point size an icon is laid out at.
///
/// U-TURN ARROWS AND NOT `ARROW_COUNTER_CLOCKWISE` / `ARROW_CLOCKWISE`, which
/// are the obvious picks and are the wrong ones. A circular arrow is the
/// near-universal *reload* idiom — it is what a browser's reload button looks
/// like and what Phosphor itself uses for refresh — so on a control that mutates
/// the user's molecule it would suggest the wrong operation. The house rule is
/// that an icon must not replace text whose meaning could be mistaken; an icon
/// that INTRODUCES a mistakable meaning is worse than none.
///
/// `ICON_SIZE` doubles as the reserved width because Phosphor's `hmtx` advance
/// is 1024/1024 upem: an icon is exactly 1.000 em, so 13 pt of font size is
/// 13 pt of toolbar and no measurement is needed to say so.
const ICON_UNDO: &str = egui_phosphor::bold::ARROW_U_UP_LEFT;
const ICON_REDO: &str = egui_phosphor::bold::ARROW_U_UP_RIGHT;
const ICON_SIZE: f32 = 13.0;

/// A button whose word is preceded by an icon that the screen reader never sees.
///
/// **THE TWO CONTROLS THIS IS USED ON ARE UNDO AND REDO, AND NOTHING ELSE.**
/// 495 KB of face for two arrows, and the specification says so plainly rather
/// than padding the list to justify the payload. "Undo" and "Redo" are
/// four-letter words differing in one interior letter, in the same weight, the
/// same colour and the same button shape, sitting adjacent — the worst
/// discriminability case in the bar, on the pair reached fastest and under the
/// most time pressure, usually straight after doing something regrettable. A
/// MIRRORED ARROW ENCODES DIRECTION, which is a pre-attentive channel the words
/// do not have: it is legible in peripheral vision, which is the condition these
/// two are actually clicked in. Every other control in the bar is either a
/// distinct noun ("Open…", "Save", "Export map", "Molecule") or already carries
/// a caret, and none is reached in a hurry. The six details tabs get nothing:
/// they live in a `horizontal_wrapped` whose own comment records the File tab
/// going unclickable below ~357 pt, six icons is ~96 pt out of a width already
/// contested with the map pane, and six unambiguous nouns have no
/// discriminability problem to solve. U+26A0, the hidden-cut warning, keeps its
/// real Unicode character rather than moving to Phosphor's `WARNING`: a PUA
/// codepoint is a downgrade for anything a screen reader or a copied string
/// might touch.
///
/// **THE ICON IS PAINTED, NOT PASSED AS A TEXT ATOM, AND THAT IS AN
/// ACCESSIBILITY DECISION RATHER THAN A LAYOUT ONE.** `Atoms::text()` (egui 0.35
/// `atomics/atoms.rs:51`) concatenates every text atom with a space and `Button`
/// hands the result straight to `WidgetInfo::labeled` (`widgets/button.rs:401`),
/// so `ui.button((icon_text, "Undo"))` gives accesskit the name
/// `"\u{E08A} Undo"` — a Private Use Area codepoint read out to a screen-reader
/// user. That is one of NOTICE's three recorded reasons for rejecting an icon
/// font and it survives the family isolation untouched. Reserving the space with
/// an empty [`egui::Atom`] and then painting into the button's own rect from the
/// `Response` — [`menu_with_caret`]'s mechanism exactly — keeps the accessible
/// name the bare word "Undo", keeps the word visible beside the glyph, and names
/// the family explicitly in the `FontId` at the call site, so a hand-painted
/// icon can never inherit Monospace or Proportional even by accident.
///
/// CONTRAST, SAMPLED OFF THE RUNNING APP RATHER THAN COMPUTED FROM TWO PALETTE
/// CONSTANTS. The glyph takes the same `fg_stroke.color` the button's own label
/// takes, so arithmetic would say it is exactly as contrasty as the word beside
/// it — but the real question for a STROKED glyph is whether enough ink survives
/// antialiasing for any pixel to reach that colour, and that is the thing
/// `main.rs:73-80` had to photograph for the Plex swap too. Measured on this
/// machine's 120 dpi screenshots, enabled Undo, against the BUTTON FILL and not
/// the panel (the trap `ring.rs` records):
///
///   dark   ink 180,180,180 on fill 60,60,60    5.32:1, 35 fully-inked pixels
///   light  ink  60, 60, 60 on fill 230,230,230 8.84:1, 35 fully-inked pixels
///
/// Identical to the label's own 5.32 and 8.84 in the same button, so the ink
/// does survive at 13 pt and Bold is carrying its weight. SC 1.4.11 asks 3:1 of
/// UI chrome.
///
/// The DISABLED state measures 2.86:1 and is EXEMPT under the same criterion's
/// "inactive user interface component" carve-out — the disabled word measures
/// the same, because both take the faded colour from the same place. Written
/// down so that nobody later "fixes" a greyed-out Undo by making it look
/// enabled, which would be a real regression dressed as an accessibility one.
fn button_with_icon(ui: &mut Ui, icon: &str, label: &str) -> egui::Response {
    use egui::AtomExt as _;
    let r = ui.button((
        egui::Atom::default().atom_size(egui::vec2(ICON_SIZE, ICON_SIZE)),
        label,
    ));
    let pad = ui.spacing().button_padding.x;
    let at = egui::pos2(r.rect.left() + pad + ICON_SIZE * 0.5, r.rect.center().y);
    // Not `pal(ui).ink`: this is inside a button, and a button's foreground is
    // whichever `WidgetVisuals` the interaction state selects. Taking it from
    // the same place `Button` takes its label colour is what keeps the glyph and
    // the word in step through hover, press and `add_enabled_ui(false)` — the
    // painter's own fade handles the last of those, so nothing here has to know
    // about it.
    let colour = ui.style().interact(&r).fg_stroke.color;
    ui.painter().text(
        at,
        egui::Align2::CENTER_CENTER,
        icon,
        // The family, named at the call site. `FontId::monospace` or
        // `::proportional` here would put a PUA codepoint into a text chain that
        // has no glyph for it and paint a tofu box; the icons family is the only
        // one that can draw this and the only one Phosphor is in.
        egui::FontId::new(ICON_SIZE, ICON_FAMILY.clone()),
        colour,
    );
    r
}

/// How wide a string is, laid out in the font it will actually be drawn in.
fn text_width(ui: &Ui, s: &str, size: f32) -> f32 {
    ui.painter()
        .layout_no_wrap(
            s.to_string(),
            egui::FontId::proportional(size),
            egui::Color32::WHITE,
        )
        .size()
        .x
}

/// A string cut to fit `room`, with a trailing ellipsis when anything went.
///
/// Used for the toolbar's filename, which is the one part of the title block
/// that may give: the whole path is on hover, so nothing becomes unrecoverable.
/// Three ASCII full stops rather than U+2026, matching `pl-draw` — three dots
/// are three dots in every encoding and the real character is not.
fn elide(ui: &Ui, s: &str, room: f32) -> String {
    elide_at(ui, s, room, egui::TextStyle::Body.resolve(ui.style()).size)
}

/// [`elide`] at a stated size, for text not drawn in the body style.
///
/// The status line is drawn at 12 and `TextStyle::Body` is not 12, so measuring
/// it with the body size is deciding in one unit and drawing in another — the
/// mistake `pl_draw::fit_label` was moved off for the same reason.
fn elide_at(ui: &Ui, s: &str, room: f32, size: f32) -> String {
    // No room means nothing, not everything.
    //
    // `room <= 0.0 || fits` returned the string UNCHANGED for non-positive room,
    // which inverts the degradation order at exactly the moment it matters:
    // `room` is `available_width - state_w - 12`, so a long status string drives
    // it negative, and the branch then paid out the *whole* filename on top of
    // the un-elided status. Both ran over the reserved right-hand cluster and the
    // theme switch was painted through the letters — the same collision the
    // cluster-first layout was supposed to end, running the other way. It is not
    // a synthetic path length: the user's own genome files sit 160 characters
    // deep in OneDrive, so saving beside a source file overflowed every time.
    //
    // The `kept.is_empty()` arm below already answers this correctly for room
    // that is merely small; there was no reason for zero to be different.
    if room <= 0.0 {
        return String::new();
    }
    if text_width(ui, s, size) <= room {
        return s.to_string();
    }
    let mut kept = String::new();
    for c in s.chars() {
        let trial = format!("{kept}{c}...");
        if text_width(ui, &trial, size) > room {
            break;
        }
        kept.push(c);
    }
    if kept.is_empty() {
        // Not even one character: better an empty label than a bare "..."
        // claiming a name is there when nothing of it can be read.
        String::new()
    } else {
        kept + "..."
    }
}

/// Set `PL_GUI_DEBUG_GEOMETRY=1` to print the layout rects each frame.
///
/// Worth keeping in the shipped binary: when a window looks wrong, this is the
/// only trustworthy number. A helper process that is not per-monitor DPI aware
/// is told *virtualised* coordinates by Windows, so measuring or screenshotting
/// the app from one shows a window that appears clipped when it is not.
fn debug_geometry() -> bool {
    matches!(std::env::var("PL_GUI_DEBUG_GEOMETRY").as_deref(), Ok("1"))
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 840.0])
            .with_min_inner_size([880.0, 560.0])
            .with_title("Polylinker"),
        ..Default::default()
    };
    eframe::run_native(
        "Polylinker",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

/// A vector format a figure can leave in.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Fmt {
    Svg,
    Pdf,
    Eps,
}

impl Fmt {
    fn ext(self) -> &'static str {
        match self {
            Fmt::Svg => "svg",
            Fmt::Pdf => "pdf",
            Fmt::Eps => "eps",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Fmt::Svg => "SVG",
            Fmt::Pdf => "PDF",
            Fmt::Eps => "EPS",
        }
    }
}

/// Which picture the central pane is showing.
///
/// A second VIEW of the open document, not a second document and not a tab.
/// The gel's lane set is an enzyme choice made in the Enzymes tab, so the
/// picker and the picture have to be on screen together; a seventh tab would
/// make ticking an enzyme and seeing the result mutually exclusive.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum CentralView {
    Map,
    Gel,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Tab {
    Features,
    Library,
    Enzymes,
    Sequence,
    Reads,
    History,
    File,
}

/// What a stale draft is, beyond what its header says.
#[derive(Default)]
struct StaleExtra {
    /// From the LOCUS line of the snapshot body — `genbank::write` emits
    /// `{n:>11} bp`, so it is one `split_whitespace` away. Rejected: a `bp:`
    /// header key, which would be a second source of truth for a number the
    /// body already carries, against this module's ethos that the body is the
    /// molecule.
    bp: Option<u64>,
    /// Seconds since the epoch, from `fs::metadata(original).modified()` — the
    /// same wall clock `autosave` writes `saved_at` from, so the two are
    /// comparable at one-second granularity.
    original_mtime: Option<u64>,
}

/// The LOCUS line's length, if the body has one.
fn locus_bp(genbank: &str) -> Option<u64> {
    let line = genbank.lines().find(|l| l.starts_with("LOCUS"))?;
    let f: Vec<&str> = line.split_whitespace().collect();
    let i = f.iter().position(|w| *w == "bp")?;
    f.get(i.checked_sub(1)?)?.parse().ok()
}

/// "saved 14 minutes ago · newer than the file on disk", and the three other
/// forms of the sentence that actually answers the question the banner asks.
///
/// Three traps, each of which has its own branch here:
///
/// 1. **`saved_at == 0` means "the header would not parse", not 1970.** `decode`
///    does `v.parse().unwrap_or(0)` and the recovery tests assert "unknown
///    rather than invented". Rendering it as an age produces "56 years ago" on
///    the file a user most needs to trust.
/// 2. **Clocks go backwards** — NTP, dual boot, a VM resuming. Saturating
///    subtraction, so a draft dated in the future reads "just now".
/// 3. **No absolute clock time.** The workspace has `civil_from_days` and no
///    timezone database, so an absolute stamp would be UTC, and a UTC time shown
///    to a user in Israel without a label is a wrong time three hours out.
///    Relative ages need no timezone and are what "is it newer" wants anyway.
///    Do not "improve" this into a date.
///
/// `ops` is here for one branch and it is the branch that mattered: a draft
/// holding no edits is not "newer" than anything, whatever the two mtimes say.
fn draft_age(
    saved_at: u64,
    now: u64,
    original_mtime: Option<u64>,
    had_original: bool,
    ops: usize,
) -> String {
    if saved_at == 0 {
        return "saved at an unknown time".into();
    }
    let secs = now.saturating_sub(saved_at);
    let age = if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{} minute(s) ago", secs / 60)
    } else if secs < 86_400 {
        format!("{} hour(s) ago", secs / 3600)
    } else {
        format!("{} day(s) ago", secs / 86_400)
    };
    let against = match (had_original, original_mtime) {
        (false, _) => " · this draft was never in a file".to_string(),
        // NO COMPARISON AT ALL when the draft holds no edits, because there is
        // nothing in it the file does not have. The banner used to advertise a
        // 0-edit draft of an untouched 8,117 bp `.dna` as "newer than the file
        // on disk" — true of the two mtimes and false of the two contents — and
        // acting on it costs the container, the nine typed primers (a draft is
        // GenBank, so they come back as `primer_bind`) and the methylation
        // flags. The one line that exists to help the user choose pointed at
        // the worse copy.
        (true, _) if ops == 0 => {
            " · it holds no edits, so the file has everything in it".to_string()
        }
        (true, None) => " · the file it came from is no longer there".to_string(),
        // "written after", not "newer". These are file timestamps and not a
        // comparison of contents, and "newer" reads as "better" for a draft
        // that is a GenBank rendering of a richer file.
        (true, Some(m)) if saved_at > m => {
            " · written after the file on disk was last saved".to_string()
        }
        (true, Some(m)) if saved_at < m => {
            " · the file on disk is newer — it was saved after this draft".to_string()
        }
        (true, Some(_)) => " · the same age as the file on disk".to_string(),
    };
    format!("saved {age}{against}")
}

/// Why the unsaved-changes question is being asked, which is the only sentence
/// in the dialog that varies with the path that raised it.
#[derive(Clone, Copy, PartialEq, Eq)]
/// What is about to cost the user their unsaved work.
///
/// ONE VARIANT, WHERE THERE WERE FOUR. `Open`, `Restore` and `Product` all
/// meant "a document is about to be replaced", and since the bench holds more
/// than one document nothing replaces anything: opening a file, restoring a
/// crash draft and opening a religation product all add a tab. Closing the
/// window is the only remaining way to lose work, so it is the only remaining
/// question.
///
/// Kept as an enum rather than collapsed into bare strings because the next
/// thing that can genuinely destroy work — Stage 2's "forget this bench" — will
/// want to be a second variant, and a `match` is where that gets noticed.
enum Losing {
    Close,
}

impl Losing {
    /// What the discard button will do, in the user's terms.
    ///
    /// `one` is whether the sentence before this one described a single thing.
    /// It exists because the two disagreed: the app took the trouble to write "1
    /// edit that is not in any file" and then followed it with "Closing
    /// Polylinker discards **them**", in the one dialog whose whole job is to
    /// make a user stop and read.
    fn consequence(self, one: bool) -> &'static str {
        match (self, one) {
            (Losing::Close, true) => "Closing Polylinker discards it.",
            (Losing::Close, false) => "Closing Polylinker discards them.",
        }
    }

    /// The discard button's label. It carries the consequence, never "OK" or
    /// "Yes": people click "OK" without reading it and do not click "Discard"
    /// without reading it, and that is the cheapest and largest single
    /// mitigation available against a guard becoming a reflex.
    fn discard_label(self) -> &'static str {
        match self {
            Losing::Close => "Close without saving",
        }
    }
}

/// A `.dna` write whose destination is chosen and whose losses are not yet
/// acknowledged.
struct PendingDna {
    path: PathBuf,
    bytes: Vec<u8>,
    /// What the writer could not carry, verbatim from
    /// `snapgene::from_molecule_reporting`.
    unwritable: Vec<String>,
    /// Whether a cloning-history tree is about to be replaced with none — the
    /// SOURCE's, or the DESTINATION's, whichever exists.
    history: bool,
    /// Note paths this model has no shape for, as the File tab names them.
    notes: Vec<String>,
    /// The chosen path is the file the document was opened from.
    overwriting_source: bool,
    /// Blocks the DESTINATION file holds today that these bytes do not, and
    /// which replacing it therefore destroys.
    ///
    /// Its own term, separate from `source_lost`, and the one whose absence was
    /// the worst defect in this change: the gate was computed from the OPEN
    /// document's container, so opening a GenBank file and saving it over an
    /// existing `.dna` took the fast path — no modal, no report — and turned a
    /// 17-block 75 kB file carrying a cloning history, five history nodes and
    /// nine typed primers into a 4-block 15 kB one. The only warning anywhere
    /// was the OS asking whether to replace a file, and the status line named
    /// the one thing that was NOT lost: the regenerable cache.
    dest_lost: Vec<pl_fileio::snapgene::DroppedBlocks>,
    /// Blocks the SOURCE container held that these bytes do not.
    ///
    /// Still a term beside `dest_lost`, for Save-As to a NEW path: nothing is
    /// destroyed there, but the copy being made is a downgrade of the file the
    /// user has open, and they should learn that before they hand it to a
    /// student rather than after.
    source_lost: Vec<pl_fileio::snapgene::DroppedBlocks>,
    /// Raised by the unsaved-changes guard's save button, so a successful write
    /// should carry on into the discard rather than stopping.
    then: Option<Losing>,
}

impl PendingDna {
    /// Everything that will be gone and is not a cache, whichever file holds
    /// it, deduplicated by kind.
    ///
    /// The two lists overlap completely when saving a `.dna` over itself and
    /// not at all when writing a GenBank molecule over someone else's `.dna`,
    /// so the union is the only honest thing to show and the merge has to be by
    /// kind rather than by concatenation.
    fn losing(&self) -> Vec<pl_fileio::snapgene::DroppedBlocks> {
        let mut out: Vec<pl_fileio::snapgene::DroppedBlocks> = Vec::new();
        for d in self.dest_lost.iter().chain(self.source_lost.iter()) {
            if d.derived {
                continue;
            }
            match out.iter_mut().find(|x| x.kind == d.kind) {
                // The larger of the two, because the destination and the source
                // can hold different numbers of the same kind and understating
                // is the direction that matters.
                Some(x) if x.bytes < d.bytes => *x = *d,
                Some(_) => {}
                None => out.push(*d),
            }
        }
        out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.kind.cmp(&b.kind)));
        out
    }

    /// The caches, which cost the user nothing and are said separately.
    fn caches(&self) -> Vec<pl_fileio::snapgene::DroppedBlocks> {
        let mut out: Vec<pl_fileio::snapgene::DroppedBlocks> = Vec::new();
        for d in self.dest_lost.iter().chain(self.source_lost.iter()) {
            if d.derived && !out.iter().any(|x| x.kind == d.kind) {
                out.push(*d);
            }
        }
        out.sort_by_key(|d| d.kind);
        out
    }

    /// Whether anything is at stake beyond a cache.
    ///
    /// The gate the modal is keyed on. Blocks 2 and 3 are caches: nobody loses
    /// work when they are omitted, and announcing them on every save is the
    /// mechanism by which this dialog stops being read.
    fn asks(&self) -> bool {
        !self.unwritable.is_empty()
            || self.history
            || !self.notes.is_empty()
            || !self.losing().is_empty()
    }
}

struct App {
    /// A file that could not be read. Renders as a full-screen takeover, which
    /// is right for "there is no document" and wrong for anything else.
    error: Option<String>,
    /// An edit that was refused, or a report about one that was not.
    ///
    /// Deliberately not `error`. A refused *edit* used to go there, so the
    /// application answered "cannot delete 12 bp at 1,204" with a full-screen
    /// "Could not read that file" and removed the map — telling the user their
    /// file is unreadable when the document is fine and nothing was changed.
    notice: Option<String>,
    /// Caret, selection and the open typing run for the Sequence tab.
    edit: seqedit::SeqEdit,
    tab: Tab,
    selected: Option<usize>,
    /// Feature under the pointer, from either the map or the list, COLLECTED
    /// this frame.
    hot: Option<usize>,
    /// What `hot` was at the end of the previous frame, which is what both
    /// panels paint from.
    ///
    /// Two fields, because one cannot work here. egui requires side panels to
    /// be added before the central panel, so the Features list is painted
    /// before the map has hit-tested anything: reading and writing a single
    /// field in panel order meant the map's hover reached the list only after
    /// `self.hot = None` had wiped it at the top of the next frame, and the
    /// list's own wash was painted before its rows had been hovered. Measured
    /// in the running app, every row background was byte-identical hovered and
    /// unhovered, in both directions — so the whole click-to-select
    /// interaction read as inert, which is exactly what review finding 6 is
    /// about.
    ///
    /// The one-frame lag is not visible because `App::ui` requests a repaint
    /// whenever the two disagree; without that request a pointer coming to rest
    /// could leave the echo undrawn until something else asked for a frame.
    hot_shown: Option<usize>,
    filter: String,
    status: String,
    /// Which enzymes the panel is showing.
    ///
    /// Defaults to `All`. Every tool in this category defaults to a *subset*,
    /// and `docs/PLAN.md` item 33 records that hiding sites behind a default
    /// filter is the one documented case of this software category costing a
    /// user a month of bench time. Defaulting to the whole truth costs a
    /// slightly longer list; defaulting to a subset costs someone an
    /// experiment.
    enzyme_set: pl_enzymes::EnzymeSet,
    /// The indexed folder, if one has been opened.
    scan: Option<library::ScanState>,
    lib_mode: library::Mode,
    lib_query: String,
    lib_absent: bool,

    /// Where this window's autosave goes, and what was left behind by a
    /// process that did not exit cleanly.
    ///
    /// The recovery file's *presence* is the crash flag. Nothing is written to
    /// say "we crashed" — a flag recorded during a crash is a flag that does
    /// not get recorded — so quitting normally deletes it and anything still
    /// there at startup is by definition an unclean exit.
    recovery: Option<std::path::PathBuf>,
    stale: Vec<(std::path::PathBuf, Result<recover::Snapshot, String>)>,
    /// What each stale draft is, beyond its header: the length off its LOCUS
    /// line and the mtime of the file it came from.
    ///
    /// Parsed ONCE, here, and not per frame: `MAX_SLOTS` is 64 and one of those
    /// drafts may be a 4.6 Mb genome. Keyed by the recovery path rather than
    /// held parallel to `stale`, which is removed from.
    stale_extra: std::collections::HashMap<std::path::PathBuf, StaleExtra>,
    /// Whether the banner is showing every draft or only the newest per file.
    show_old_drafts: bool,
    /// Which Discard button has been clicked once and is asking to be sure.
    discard_armed: Option<usize>,
    /// What the OS title bar currently says, so the command is sent on change
    /// rather than every frame.
    title_shown: String,
    /// A parsed replacement waiting on "what about the document you have
    /// open?", with the status line it should arrive carrying and which gesture
    /// asked for it.
    ///
    /// Parsed, not a path: asking before the parse means asking about a swap
    /// that a cancelled dialog or an unreadable file will never perform, and a
    /// prompt the user answers about nothing is precisely the false positive
    /// that trains people to click through.
    ///
    /// The status travels with the document because `load` builds it up — the
    /// records-in-file and unrepresentable-locations clauses — *before* the
    /// adoption, and a cancelled question must not leave the toolbar describing
    /// a file that is not open.
    /// The window has asked to close and the guard held it back.
    closing: bool,
    /// Set by the guard when the window may finally go. Read once in `ui`,
    /// which is the only place with a `Context` to send the command from.
    close_now: bool,
    /// The guard is done and the next close request must be let through.
    ///
    /// Separate from `closing`, and cleared before `ViewportCommand::Close` is
    /// sent: egui-winit pushes `ViewportEvent::Close` into the viewport info,
    /// so the *next* frame sees `close_requested` again, and a still-armed
    /// latch would cancel its own close and make the window impossible to shut.
    let_it_go: bool,
    /// The user answered "close without saving", so `on_exit` must keep the
    /// recovery draft the dialog just promised them.
    abandoned_unsaved: bool,
    /// A `.dna` write waiting on the lossiness question.
    pending_dna: Option<PendingDna>,
    /// What the recovery file on disk already holds, so an idle window does not
    /// rewrite the same bytes forever.
    autosaved: Option<Autosaved>,
    last_autosave: Option<std::time::Instant>,
    /// Which application owns `.dna` on this machine, read at most once.
    ///
    /// `None` means "not asked yet"; `Some(None)` means nothing owns it. The
    /// answer is a machine-wide registry setting that cannot change between
    /// frames, and it used to be read live inside the welcome screen's paint
    /// closure — a `cmd /C assoc .dna` child process spawned on every repaint,
    /// blocking the UI thread on `.output()` until cmd.exe exited. Moving the
    /// pointer across the empty window drove dozens of those a second, which is
    /// what made the welcome screen stutter.
    dna_owner: Option<Option<String>>,

    /// The Design primers panel, if it is open.
    ///
    /// It holds a snapshot of the selection it was opened on, not a live
    /// reference to it -- see `design.rs`.
    design: Option<design::Panel>,
    /// The cut-and-religate panel, when it is open.
    clone_panel: Option<clone::Panel>,
    /// What is open. See [`bench::Bench`].
    bench: bench::Bench,
    /// Tabs the user closed, newest last, for Ctrl+Shift+T.
    ///
    /// A closed tab keeps its document AND its undo history, which is what lets
    /// Close Tab be unguarded: nothing is destroyed, so nothing has to be asked
    /// about. Bounded, because an unbounded one is a memory leak shaped like a
    /// feature — a user who closes fifty tabs in a session is not going to
    /// reopen the first.
    closed: Vec<bench::Tab>,

    /// The Feature editor, if it is open.
    ///
    /// Holds a CLONE of the feature and the index it was opened on, never a
    /// borrow: `RemoveFeature` shifts every later index and `remap_annotations`
    /// drops a feature whose bases were all deleted, so a live index is a
    /// different feature after any other edit. `featedit::Panel::stale_reason`
    /// is what refuses to write through a moved one.
    ///
    /// Only one of this and [`App::design`] may be open at a time. Both are
    /// non-modal `egui::Window`s and both suppress the sequence keys, so two of
    /// them up together means each is guarding the keyboard for a reason the
    /// other does not know about.
    feature_edit: Option<featedit::Panel>,

    /// How many times the feature editor has been opened this session.
    ///
    /// Salts the editor body's `ScrollArea` id so each open starts at the top;
    /// see `featedit::Panel::generation`.
    feature_editor_opens: u64,

    /// What this window remembers between runs. Read once in [`App::new`],
    /// written once in `on_exit`.
    layout: settings::Layout,

    /// Bumped every time [`App::adopt`] replaces the document.
    ///
    /// The annotation index is keyed on `(this, log.cursor())` and the cursor
    /// alone is not enough: a freshly opened document sits at cursor `None`, so
    /// opening plasmid A and then plasmid B compares equal, the index is not
    /// rebuilt, and A's features are painted onto B. Every ribbon lands
    /// somewhere plausible, nothing errors, and it happens on the second file
    /// the user opens.
    doc_generation: u64,
    /// Features by position, lanes, and the enzyme cuts — see `annot.rs`.
    annot: annot::AnnotIndex,
    /// The `(version, filter, digest-is-done)` the cut list was built for.
    cuts_for: Option<(annot::Version, pl_enzymes::EnzymeSet, bool)>,
    /// Scratch for the per-row index queries, kept so a frame does not allocate
    /// forty vectors.
    annot_scratch: Vec<annot::Iv>,

    /// Last frame's row geometry for the Sequence tab, so a reflow can put the
    /// scroll back on the base the user was reading rather than on the pixel
    /// offset that base used to be at. See [`seqedit::row_of`].
    seq_per_row: u64,
    seq_row_h: f32,
    /// Where the sequence grid was actually painted last frame.
    ///
    /// The same numbers `PL_GUI_DEBUG_GEOMETRY` prints, kept rather than only
    /// printed so a test can put a pointer on a *named column*. The alternative
    /// is a test that bakes in 6.92 px per cell and a panel edge at x = 790,
    /// which stops testing the thing it names the moment the default font or
    /// the frame margin changes — quietly, and while still passing.
    seq_grid: Option<GridGeom>,
    /// Where the readout ended up.
    ///
    /// Kept for the same reason as `seq_grid`: the defect this change replaced
    /// was a sentence drawn past the panel's rect and cut by its clip rect, and
    /// the only honest check of that is where the thing actually landed. A
    /// panel is not a widget, so egui has no `Response` to ask.
    seq_readout: Option<egui::Rect>,
    /// What is under the pointer in the sequence grid, in words. Computed
    /// inside the grid and shown by the readout, which is laid out *before* the
    /// grid — so it is one frame old, and egui repaints on pointer motion.
    seq_hover: Option<String>,
    /// This document's translations: which features are read, in which frame,
    /// and how many residue lanes they need. Rebuilt with `annot`, on the same
    /// key, for the same reason.
    tr: aa::Translations,
    /// The document's default genetic code, read from the file on open.
    ///
    /// View state, not molecule state. Reading a `/transl_table` is not
    /// editing: choosing a table must not enter the append-only log and must
    /// not make the document dirty, so this lives here beside the index and
    /// never in `Molecule`.
    doc_code: pl_core::translate::Code,
    /// Whether this document's rows reserve the ORF strip.
    ///
    /// `enz_strip`'s rule exactly: what the last COMPLETED scan of this
    /// document said, held across the next one, so the row pitch never depends
    /// on a worker's phase.
    orf_strip: bool,
    /// Which picture the central pane is showing.
    ///
    /// NOT persisted, deliberately, though `settings::Layout` persists other
    /// view preferences and doing so would be consistent. The map is what a
    /// double-clicked `.dna` should show; a gel is a QUESTION YOU ASK, not a
    /// state to live in, and launching into a gel of a file whose digest has
    /// not run yet is a worse first frame than the map. The gel's CONDITIONS
    /// are worth remembering; which picture is up is not.
    central_view: CentralView,
    /// Chromatograms held against whatever document is open.
    ///
    /// ON `App`, NOT ON `Document`, and that is the load-bearing choice. Reads
    /// must survive the arrival AND the replacement of a document, because "I
    /// opened the wrong plasmid, let me open the right one" is the commonest
    /// correction in this workflow and must not cost the user their files.
    /// What does not survive is the REPORT: a `Report` names a reference, so
    /// `adopt` discards every one and re-arms.
    reads: Vec<reads::Read>,
    /// Which read the Reads tab is showing.
    read_shown: usize,
    /// Which window of that read's bases the chromatogram is drawn over.
    read_window: usize,
    /// The `(doc_generation, seq_version)` every held report was computed
    /// against.
    ///
    /// A report is a property of the FILE AND the molecule, and the molecule is
    /// being edited underneath it. Repainting a perfect trace beside a
    /// discrepancy list computed three edits ago is a confident wrong answer —
    /// precisely the defect `Document::apply`'s unconditional re-digest exists
    /// to prevent. `seq_version` alone is not enough: it starts at 0 in every
    /// document, so opening plasmid A, editing it, then opening B compares
    /// equal and the reports carry over.
    reads_for: (u64, u64),
    /// The virtual gel's conditions and lane set. Reset by `adopt`.
    gel: gel::View,
    /// The last built gel and everything it was built from. See [`App::gel_ready`].
    gel_cache: Option<(GelKey, gel::Built)>,
    /// Whether this document's sequence rows reserve the enzyme strip.
    ///
    /// What the last COMPLETED digest of this document found, and deliberately
    /// not what the live cut list holds: every edit restarts the digest, and
    /// deriving the row height from a running worker reflowed the whole view
    /// twice per keystroke on anything big enough for the scan to take a
    /// moment. Cleared in `adopt`, written only in `refresh_annotations`.
    enz_strip: bool,
}

/// Where one row of the sequence grid sits on screen.
#[derive(Clone, Copy, Debug)]
struct GridGeom {
    /// x of column 0's left edge.
    x0: f32,
    advance: f32,
    /// y of the top of the first visible row.
    top: f32,
    row_h: f32,
    first_row: u64,
    per_row: u64,
    /// Where every strip inside a row sits, so a test can put a pointer on a
    /// NAMED band rather than on `row_h * 0.5` and hope.
    ///
    /// The same argument the rest of this struct is here for: baking
    /// `row_h * 0.5` into a test stops testing the thing it names the moment a
    /// strip is added above the letters — quietly, and while still passing.
    strips: seqedit::RowStrips,
}

/// Which document the recovery file holds, and exactly where in its history.
///
/// An op *count* is not a document identity, and using one silently kept the
/// wrong molecule. [`pl_core::oplog::OpLog::path`] shrinks on undo and regrows
/// when the next edit forks from the same parent, so "circularise, undo,
/// reverse-complement" lands back on a path length of 1 with a different
/// molecule: the old `ops == autosaved_at_ops` gate then returned on every
/// frame and the recovery file kept the abandoned branch, with the Recover
/// banner showing a matching op count so the staleness was invisible. The same
/// collision crossed documents, because opening a second file left the counter
/// alone: edit A once, open B and edit it once inside the thirty-second window,
/// and the single recovery file still held A under A's title.
///
/// The cursor is content-addressed, so two different edits from the same parent
/// cannot share it, and the path and title separate two documents that happen
/// to sit at the same point in their own histories.
#[derive(PartialEq, Eq)]
struct Autosaved {
    original: Option<std::path::PathBuf>,
    title: String,
    cursor: Option<pl_core::oplog::OpId>,
}

impl Autosaved {
    /// The same document, wherever its history has since got to.
    ///
    /// The title is part of it because a document that was never saved has no
    /// path, and two of those are not the same document.
    fn same_document(&self, other: &Autosaved) -> bool {
        self.original == other.original && self.title == other.title
    }
}

impl App {
    /// How long an unsaved edit can survive a crash.
    ///
    /// Thirty seconds. The cost of getting this wrong is asymmetric: too often
    /// wastes a few milliseconds of disk, too rarely costs an afternoon. It is
    /// also skipped entirely when nothing has changed, so an idle window never
    /// touches the disk at all.
    const AUTOSAVE_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

    /// Documents left by a session that did not close cleanly, or abandoned on
    /// purpose.
    ///
    /// Listed, with age, length and edit count, and never restored
    /// automatically. Silently reopening a draft over whatever the user meant to
    /// open is the failure mode; choosing between two drafts is something they
    /// can do and this program cannot.
    ///
    /// The one decision this exists to support is **"is this draft newer than my
    /// file?"**, and `saved_at` alone does not answer it — both sides do. The
    /// banner used to sort by a number it refused to show, so two drafts of one
    /// plasmid rendered as two textually identical rows.
    fn recovery_banner(&mut self, ui: &mut Ui) {
        if self.stale.is_empty() {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut restore: Option<usize> = None;
        let mut discard: Option<usize> = None;
        let mut show_all = self.show_old_drafts;
        // Three groups, and only the ones that exist. Left as one heading, a
        // deliberately abandoned draft comes back under "did not close cleanly",
        // which tells a user their app crashed when it did not.
        //
        // GROUP 0 IS THE ONE THAT WAS MISSING. `stale` returns every `*.recover`
        // that is not this process's and never asks whether the process that
        // owns it is still running — there is no lock file and no PID probe
        // anywhere in this module. So a second window listed the FIRST window's
        // live drafts as a crashed session, and its Discard permanently deleted
        // the running session's only crash copy while that user was still
        // typing into it. See `recover::maybe_live` for why freshness rather
        // than a PID probe.
        let group_of = |s: &Result<recover::Snapshot, String>| match s {
            Ok(s) if recover::maybe_live(s.saved_at, now) => 0,
            Ok(s) if s.abandoned => 2,
            // A damaged header lands here: nothing readable said it was
            // abandoned, and nothing readable said it was fresh either.
            _ => 1,
        };
        let groups = [
            "Another Polylinker window may still be using these",
            "A previous session did not close cleanly",
            "You closed Polylinker with unsaved changes",
        ];
        // A group that ages out needs a frame on which to age out. Same lesson
        // as the autosave wake-up eleven hundred lines below: eframe waits for
        // an event rather than spinning, so on an idle app no frame comes and
        // the disabled Discard stays disabled until the user happens to move
        // the mouse — "not yet" quietly becoming "never".
        if let Some(secs) = self
            .stale
            .iter()
            .filter(|(_, s)| group_of(s) == 0)
            .filter_map(|(_, s)| s.as_ref().ok())
            .map(|s| recover::LIVE_WINDOW_SECS.saturating_sub(now.saturating_sub(s.saved_at)) + 1)
            .min()
        {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(secs));
        }
        // Rows after the first for one (original, title) go behind a single
        // line. Bounding the DISPLAY, never reaping the files: this module's
        // whole posture is that guessing which draft matters is how the wrong
        // one is destroyed.
        let mut seen: Vec<(Option<std::path::PathBuf>, String)> = Vec::new();
        let mut hidden = 0usize;
        let extra = &self.stale_extra;
        egui::Frame::NONE
            .fill(pal(ui).selection())
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                for (g, heading) in groups.into_iter().enumerate() {
                    if !self.stale.iter().any(|(_, s)| group_of(s) == g) {
                        continue;
                    }
                    ui.label(RichText::new(heading).color(pal(ui).ink).strong());
                    // Said in the panel, not only on hover: a disabled button
                    // with its reason behind a hover is a disabled button with
                    // no reason for anyone who does not hover it.
                    if g == 0 {
                        ui.label(
                            RichText::new(
                                "Written moments ago. You can open a copy; discarding is \
                                 unavailable until that window closes.",
                            )
                            .color(pal(ui).muted)
                            .size(11.0),
                        );
                    }
                    for (i, (path, snap)) in self.stale.iter().enumerate() {
                        if group_of(snap) != g {
                            continue;
                        }
                        if let Ok(s) = snap {
                            let key = (s.original.clone(), s.title.clone());
                            if seen.contains(&key) && !show_all {
                                hidden += 1;
                                continue;
                            }
                            seen.push(key);
                        }
                        // BUTTONS FIRST, right-aligned, and the text takes what
                        // is left. The other way round — a `vertical` of three
                        // labels followed by two buttons — sizes the text block
                        // to its widest line, which is the untruncated
                        // `from <path>`: measured with a 118-character original,
                        // "Open" was bisected by the panel edge and "Discard" was
                        // entirely outside it, leaving a draft with no reachable
                        // action at all. The user's own files sit 160 characters
                        // deep in OneDrive, so this is the ordinary case.
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Rightmost first in this layout, so Discard
                                    // is added before Open and the pair still
                                    // reads "Open  Discard" left to right.
                                    //
                                    // Two clicks, not one. This permanently
                                    // deletes another session's draft, and one
                                    // unconfirmed click is not enough for that —
                                    // but it is not this document's work either,
                                    // so it gets an inline confirmation rather
                                    // than a modal.
                                    //
                                    // ...and in the live group, not at all.
                                    // The work at risk belongs to the other
                                    // window, whose user is not the person
                                    // reading this banner and cannot be asked.
                                    // Disabled rather than hidden, so the row
                                    // still says what it is, and Open still
                                    // works: reading another session's draft is
                                    // safe, deleting it is not.
                                    if g == 0 {
                                        ui.add_enabled(false, egui::Button::new("Discard"))
                                            .on_disabled_hover_text(
                                                "Another Polylinker window wrote this in the \
                                                 last minute or two and is probably still \
                                                 using it. Close that window and this becomes \
                                                 available.",
                                            );
                                    } else if self.discard_armed == Some(i) {
                                        if ui
                                            .button(
                                                RichText::new("Discard — this deletes the draft")
                                                    .color(pal(ui).warn),
                                            )
                                            .clicked()
                                        {
                                            discard = Some(i);
                                        }
                                    } else if ui.button("Discard").clicked() {
                                        self.discard_armed = Some(i);
                                    }
                                    if ui.button("Open").clicked() {
                                        restore = Some(i);
                                    }
                                    // What the buttons left. Read AFTER them, so
                                    // it is the remainder and not the whole row.
                                    let room = (ui.available_width() - 8.0).max(0.0);
                                    match snap {
                                        Ok(s) => {
                                            let e = extra.get(path);
                                            let title = if s.title.is_empty() {
                                                "untitled"
                                            } else {
                                                &s.title
                                            };
                                            let bp = e
                                                .and_then(|e| e.bp)
                                                .map(|n| format!(", {} bp", fmt_int(n)))
                                                .unwrap_or_default();
                                            // Not "1 edit(s)", in the same session
                                            // in which the modal writes "1 edit".
                                            let edits = match s.ops {
                                                1 => "1 edit".to_string(),
                                                n => format!("{n} edits"),
                                            };
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new(format!("{title} — {edits}{bp}"))
                                                        .color(pal(ui).ink2),
                                                );
                                                ui.label(
                                                    RichText::new(draft_age(
                                                        s.saved_at,
                                                        now,
                                                        e.and_then(|e| e.original_mtime),
                                                        s.original.is_some(),
                                                        s.ops,
                                                    ))
                                                    .color(pal(ui).muted)
                                                    .size(11.0),
                                                );
                                                let from = s
                                                    .original
                                                    .as_ref()
                                                    .map(|p| format!("from {}", p.display()))
                                                    .unwrap_or_else(|| {
                                                        "never saved to a file".into()
                                                    });
                                                // Elided, with the whole path on
                                                // hover. The path is orientation;
                                                // the buttons are the only way to
                                                // act on the draft.
                                                ui.label(
                                                    RichText::new(elide_at(ui, &from, room, 11.0))
                                                        .color(pal(ui).muted)
                                                        .size(11.0),
                                                )
                                                .on_hover_text(&from);
                                            });
                                        }
                                        // Damaged, and still offered: the body of
                                        // the file is plain GenBank, so the
                                        // sequence is very likely recoverable even
                                        // when the header is not.
                                        Err(e) => {
                                            let msg = format!(
                                                "{} — damaged ({e}), the sequence may still be \
                                                 readable",
                                                path.display()
                                            );
                                            ui.label(
                                                RichText::new(elide(ui, &msg, room))
                                                    .color(pal(ui).warn),
                                            )
                                            .on_hover_text(&msg);
                                        }
                                    }
                                },
                            );
                        });
                    }
                }
                if hidden > 0
                    && ui
                        .link(
                            RichText::new(format!(
                                "and {hidden} older draft(s) of the same file(s)"
                            ))
                            .color(pal(ui).muted)
                            .size(11.0),
                        )
                        .clicked()
                {
                    show_all = true;
                }
                ui.label(
                    RichText::new("These are copies. Your original files were not modified.")
                        .color(pal(ui).muted)
                        .size(11.0),
                );
                // The two sentences the restore path owes the user, and both
                // cost one muted line. The first is the representation change
                // that produced the review's refuted R1: a user who is not told
                // reads nine `primer_bind` features as corruption.
                ui.label(
                    RichText::new(
                        "A draft is a GenBank snapshot. A .dna file's primers come back as \
                         primer_bind features.",
                    )
                    .color(pal(ui).muted)
                    .size(11.0),
                );
                // ...and the second, because `Document::from_bytes` gives an
                // empty op log, so Undo is greyed out and the "6 edits" the row
                // just advertised cannot be inspected or reversed.
                ui.label(
                    RichText::new("The edits are restored; the undo history is not.")
                        .color(pal(ui).muted)
                        .size(11.0),
                );
            });
        self.show_old_drafts = show_all;

        if let Some(i) = restore {
            let (path, snap) = self.stale[i].clone();
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            // Whatever state the header is in, the body is the molecule.
            let body = match &snap {
                Ok(s) => s.genbank.clone(),
                Err(_) => recover::salvage(&text).unwrap_or("").to_string(),
            };
            let title = snap.as_ref().map(|s| s.title.clone()).unwrap_or_default();
            match Document::from_bytes(body.as_bytes(), title, None) {
                // Through `take_over`, not `adopt`. Restoring a crash draft over
                // an edited document is the most bitter possible instance of the
                // silent-replace defect. The row is removed only when the
                // adoption actually happens — removing it here and then having
                // the user cancel would make the draft unreachable.
                //
                // The path is deliberately dropped: a recovered document is
                // unsaved, so Save has to ask where, and cannot overwrite the
                // original with a draft the user has not looked at.
                Ok(d) => {
                    let status = format!("recovered from {}", path.display());
                    self.take_over(d, status, Some(i));
                }
                Err(e) => self.error = Some(format!("{}: {e}", path.display())),
            }
        } else if let Some(i) = discard {
            let (path, _) = self.stale.remove(i);
            recover::clear(&path);
            self.discard_armed = None;
            self.status = format!("discarded {}", path.display());
        }
    }

    /// Write the current document to the recovery file, if it is time.
    ///
    /// Never writes to the file the user opened. An editor that quietly
    /// rewrites the original every few minutes has turned "close without
    /// saving" into a lie.
    ///
    /// `forced` is the unsaved-changes guard making its promise true, and it
    /// skips exactly three things: the throttle, the identity memo and the
    /// base-cursor guard below. It is a PARAMETER and not a field cleared by
    /// the caller because clearing `self.autosaved` to force a write also
    /// destroyed the `same_document` escape hatch the base-cursor guard depends
    /// on — so the forced write silently did nothing for a document sitting at
    /// its own base, the `exit: unsaved` flag never reached the file, and the
    /// next launch greeted a deliberate quit with "A previous session did not
    /// close cleanly". Two concerns expressed through one field, and the second
    /// one to arrive won.
    /// How long until an autosave is owed, or `None` when nothing is at risk.
    ///
    /// A function rather than three lines in `ui`, because `ui` needs an
    /// `eframe::Frame` and cannot be driven from a test — and an untestable
    /// scheduling decision is how the missing wake-up got here in the first
    /// place. `Some(ZERO)` means "now": the caller still asks
    /// `request_repaint_after`, which treats zero as the next frame.
    fn autosave_due_in(&self) -> Option<std::time::Duration> {
        if !self.bench.any_unsaved() {
            return None;
        }
        Some(match self.last_autosave {
            Some(t) => Self::AUTOSAVE_EVERY.saturating_sub(t.elapsed()),
            None => std::time::Duration::ZERO,
        })
    }

    fn autosave(&mut self, forced: bool) {
        // THE THROTTLE COMES FIRST, and that ordering is the whole of a defect
        // this shipped with.
        //
        // `settle` below is not a read: it turns the open typing run into an
        // operation. `App::ui` calls this function on every frame, and the
        // throttle used to sit thirty lines further down, so a run opened in
        // frame N was committed at the top of frame N+1 — before the next
        // keystroke was even read. Every keystroke became its own `InsertAt`,
        // so coalescing — the load-bearing decision of this whole surface —
        // never happened once. Forty characters typed into the running
        // application now produce two operations (the one settle the throttle
        // allows, and the run); the same forty through
        // `a_typing_run_survives_the_autosave_that_runs_on_every_frame` with
        // this ordering undone produce thirty-nine.
        //
        // With the order swapped, a run is forced closed at most once per
        // `AUTOSAVE_EVERY` and otherwise closes on its own idle timer, which is
        // the design. The invariant the settle exists for is untouched: it
        // still runs before `here` is computed and before anything is written,
        // so a recovery file can never be missing the user's last keystrokes.
        let now = std::time::Instant::now();
        if let Some(last) = self.last_autosave {
            if !forced && now.duration_since(last) < Self::AUTOSAVE_EVERY {
                return;
            }
        }
        if self.document().is_none() || self.recovery.is_none() {
            return;
        }
        // Design B's rule 6, and the one whose absence loses data: an autosave
        // that wrote `log.current()` while a typing run was open would write a
        // recovery file missing the user's last forty keystrokes.
        self.settle();
        let Some(doc) = self.document() else { return };
        let here = Autosaved {
            original: doc.path.clone(),
            title: doc.title.clone(),
            cursor: doc.log.cursor(),
        };
        // Already on disk, byte for byte. See [`Autosaved`] for why this is a
        // cursor and not the op count it used to be.
        //
        // `forced` overrides it because the memo has no notion of the header:
        // the guard's final write changes `exit: unsaved`, not the molecule, and
        // a document last written thirty seconds ago at this same cursor
        // short-circuited it. Caught in the running application, not by a test —
        // the draft survived "Close without saving" and the relaunch still said
        // the session had crashed.
        if !forced && self.autosaved.as_ref() == Some(&here) {
            return;
        }
        // An unedited document THAT CAME FROM A FILE has nothing to protect:
        // the user's own file already holds it. Writing one anyway would also
        // let merely *opening* a second file discard the first one's unsaved
        // draft, which is the opposite of this function's job.
        //
        // Undoing back to the base of the document already in the recovery file
        // is a different case: that really is the state on screen, so it is
        // written, and the file stops offering a branch the user has stepped
        // off.
        //
        // `here.original.is_some()` is the half that was missing, and its
        // absence lost data. A document restored from the recovery banner (the
        // restore path drops the path deliberately) and a payload dropped in as
        // bytes from a browser both sit at the base of an empty log with
        // nothing on disk behind them — "the user's own file already holds it"
        // is simply false for them, and this branch refused to write either.
        // The unsaved-changes guard's forced final autosave walks straight into
        // it, so the dialog's promise of a kept copy would have been a lie.
        //
        // `forced` is the third exemption, and it is the one that was missing.
        // The guard has just told the user "a crash-recovery copy is kept"; a
        // document undone back to its own base still has to honour that,
        // because what the forced write records is not the molecule but the
        // fact that the exit was deliberate.
        let same_document = self
            .autosaved
            .as_ref()
            .is_some_and(|a| a.same_document(&here));
        if !forced && here.cursor.is_none() && here.original.is_some() && !same_document {
            return;
        }
        let ops = doc.log.path().len();
        let Some(path) = self.recovery.clone() else {
            return;
        };
        let snap = recover::Snapshot {
            original: doc.path.clone(),
            title: doc.title.clone(),
            saved_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            ops,
            abandoned: self.abandoned_unsaved,
            genbank: pl_fileio::genbank::write(doc.molecule(), &doc.title, today()),
        };
        // The clock is set either way. A failure that left it unset made the
        // next frame due again, so a full disk retried a multi-megabyte write
        // on every frame — and, now that the throttle also bounds the settle,
        // would have taken run coalescing down with it. Thirty seconds is the
        // right retry interval for the same reason it is the right write
        // interval.
        self.last_autosave = Some(now);
        match recover::write(&path, &snap) {
            Ok(()) => self.autosaved = Some(here),
            // A failed autosave must not interrupt the work it exists to
            // protect, but it must not be silent either -- a user who thinks
            // they are covered and is not is worse off than one who knows.
            Err(e) => self.status = format!("autosave failed: {e}"),
        }
    }

    /// An app with nothing open and nothing scanned.
    ///
    /// Split out of [`App::new`] so the state machine can be exercised without
    /// an egui context: everything below this line is plain data, and the parts
    /// that decide whether a recovery file is written are worth testing without
    /// a window on the screen.
    fn blank() -> Self {
        App {
            bench: bench::Bench::default(),
            closed: Vec::new(),
            error: None,
            notice: None,
            edit: seqedit::SeqEdit::new(),
            tab: Tab::Features,
            selected: None,
            hot: None,
            hot_shown: None,
            filter: String::new(),
            status: String::new(),
            enzyme_set: pl_enzymes::EnzymeSet::All,
            scan: None,
            lib_mode: library::Mode::Name,
            lib_query: String::new(),
            lib_absent: false,
            recovery: None,
            stale: Vec::new(),
            stale_extra: std::collections::HashMap::new(),
            show_old_drafts: false,
            discard_armed: None,
            title_shown: String::new(),
            closing: false,
            close_now: false,
            let_it_go: false,
            abandoned_unsaved: false,
            pending_dna: None,
            autosaved: None,
            last_autosave: None,
            dna_owner: None,
            design: None,
            clone_panel: None,
            feature_edit: None,
            feature_editor_opens: 0,
            layout: settings::Layout::default(),
            doc_generation: 0,
            annot: annot::AnnotIndex::default(),
            cuts_for: None,
            annot_scratch: Vec::new(),
            seq_per_row: 0,
            seq_row_h: 0.0,
            seq_grid: None,
            seq_readout: None,
            seq_hover: None,
            central_view: CentralView::Map,
            reads: Vec::new(),
            read_shown: 0,
            read_window: 0,
            reads_for: (0, 0),
            gel: gel::View::default(),
            gel_cache: None,
            enz_strip: false,
            tr: aa::Translations::default(),
            doc_code: pl_core::translate::TABLE11,
            orf_strip: false,
        }
    }

    /// Who owns `.dna` on this machine, asked at most once per window.
    ///
    /// `read` is a parameter so the memo itself can be tested: the defect this
    /// replaces was not a wrong answer but the *number of times* the answer was
    /// fetched, and a test that cannot count the reads cannot see it.
    fn dna_owner_with(&mut self, read: impl FnOnce() -> Option<String>) -> Option<&str> {
        self.dna_owner.get_or_insert_with(read).as_deref()
    }

    fn dna_owner(&mut self) -> Option<&str> {
        self.dna_owner_with(|| recover::association("dna"))
    }

    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Before any style is read and before anything measures a glyph. The
        // faces decide the row height the map's label columns are packed at and
        // the advance the sequence grid's column mapping rests on, so a pass
        // that ran with the default chain and then swapped would lay out twice.
        install_fonts(&cc.egui_ctx);
        // Styles are per-theme in egui 0.35, so adjust both rather than
        // stamping one over the user's light/dark preference.
        cc.egui_ctx.all_styles_mut(|style| {
            theme::apply(&mut style.visuals);
            style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        });

        let mut app = App::blank();
        // Read once, and an absent or unreadable file falls back to the default
        // without a word. The recovery file's *presence* is meaningful; a
        // layout file's absence means nothing at all, and saying so is noise.
        app.layout = settings::load();
        // Anything left in the recovery directory by another process is an
        // unclean exit. Listed, never auto-restored: which of two drafts is the
        // wanted one is something the user knows and this program does not.
        match recover::recovery_dir() {
            Ok(dir) => {
                app.stale = recover::stale(&dir);
                // `claim`, not `recovery_path(&dir, 0)`. A PID identifies a run
                // only while the run is alive, so a draft left by a crashed
                // session that held our number is skipped by `stale` *and*
                // targeted by `recovery_path` — hidden from the banner, then
                // deleted by the next clean quit. `claim` lists it and takes the
                // next free slot instead.
                let (path, mine) = recover::claim(&dir);
                app.stale.extend(mine);
                // Newest first, the order `stale` returns and the banner shows.
                app.stale.sort_by_key(|(p, s)| {
                    (
                        std::cmp::Reverse(s.as_ref().map(|s| s.saved_at).unwrap_or(0)),
                        p.clone(),
                    )
                });
                app.recovery = path;
                // Both halves of "is this draft newer than my file?", computed
                // once. Per frame this would stat up to 64 files and re-parse a
                // 4.6 Mb LOCUS line on every repaint.
                for (p, snap) in &app.stale {
                    let Ok(s) = snap else { continue };
                    app.stale_extra.insert(
                        p.clone(),
                        StaleExtra {
                            bp: locus_bp(&s.genbank),
                            original_mtime: s.original.as_ref().and_then(|o| {
                                std::fs::metadata(o)
                                    .ok()?
                                    .modified()
                                    .ok()?
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .ok()
                                    .map(|d| d.as_secs())
                            }),
                        },
                    );
                }
                if !app.stale.is_empty() {
                    app.status = format!(
                        "{} document(s) from a session that did not close cleanly — see Recover",
                        app.stale.len()
                    );
                }
                if app.recovery.is_none() {
                    app.status =
                        "autosave is off: every recovery slot for this process is taken by a \
                         session that did not close cleanly — see Recover"
                            .to_string();
                }
            }
            // No recovery directory means no autosave, and saying so is the
            // point: a user who believes they are covered and is not is worse
            // off than one who knows they are not.
            Err(e) => app.status = format!("autosave is off: {e}"),
        }
        app.open_argv(std::env::args_os().skip(1));
        app
    }

    /// Open whatever the command line named.
    ///
    /// This makes the app usable as a file association and from a terminal, and
    /// it takes EVERY argument rather than only the first: `polylinker
    /// plasmid.gb A01.ab1 A02.ab1` is the whole sequencing workflow in one
    /// line, and `load_as` already sorts molecules from chromatograms by
    /// CONTENT, so the order on the line does not have to be learned. Naming
    /// two molecules is the one ambiguous case and it resolves the way the rest
    /// of the app does: the last adopted wins, exactly as opening a second file
    /// from the dialog does.
    ///
    /// Flags are SKIPPED rather than opened. Every argument being a path meant
    /// `polylinker --help` tried to open a file called `--help` and, on a fresh
    /// launch, presented it as an unreadable file — a confusing answer to a
    /// typo and a worse one to a flag this binary gains later. `--` ends the
    /// flags, as everywhere else.
    ///
    /// And the failures are COLLECTED. `load_failed` assigns `self.notice`, so
    /// opening three files of which two are unreadable reported only the last
    /// one — and before this loop existed that was unreachable, because only
    /// `argv[1]` was ever opened.
    ///
    /// Takes an iterator rather than reading `std::env` itself so that both
    /// rules can be asserted: a rule about argument handling that can only be
    /// exercised by launching a process does not get exercised.
    fn open_argv<I: IntoIterator<Item = std::ffi::OsString>>(&mut self, args: I) {
        let mut flags_over = false;
        let mut failed: Vec<String> = Vec::new();
        let mut ignored: Vec<String> = Vec::new();
        for arg in args {
            let s = arg.to_string_lossy().to_string();
            if !flags_over && s == "--" {
                flags_over = true;
                continue;
            }
            if !flags_over && s.starts_with('-') && s.len() > 1 {
                ignored.push(s);
                continue;
            }
            self.notice = None;
            self.load(PathBuf::from(arg));
            if let Some(why) = self.notice.take().or_else(|| self.error.clone()) {
                failed.push(why);
            }
        }
        // AFTER the loop, and APPENDED. Said inside the loop it was overwritten
        // the moment the next argument opened successfully and set the status
        // to the molecule's own description — so `polylinker --help file.gb`
        // silently swallowed the flag. Appending keeps both facts: what is open,
        // and what was not acted on.
        if !ignored.is_empty() {
            let note = format!(
                "ignored {}: this application takes file names, not flags",
                ignored.join(", ")
            );
            self.status = if self.status.is_empty() {
                note
            } else {
                format!("{}  —  {note}", self.status)
            };
        }
        match failed.len() {
            0 => {}
            1 => self.notice = Some(failed.remove(0)),
            n => {
                self.notice = Some(format!(
                    "{n} file(s) could not be opened: {}",
                    failed.join(" | ")
                ))
            }
        }
    }

    /// Take on a document, from wherever it came.
    ///
    /// The one place `self.document` is assigned, because two of the four
    /// places that used to assign it forgot the second half. `load` reset the
    /// editor with the comment "a caret from the previous document names bases
    /// this one does not have"; the Recover banner and a dropped byte payload
    /// did not, so a selection made on a 5 kb plasmid survived onto a 200 bp
    /// recovered document as a highlight of its tail — and Backspace deletes
    /// what is highlighted. The content-addressed `seen` map carried over too,
    /// and its keys really do collide: two different documents circularised
    /// from their base have the same `OpId`.
    ///
    /// It also starts this document's autosave clock. The recovery file is a
    /// periodic snapshot and the period starts when the document opens; that
    /// is what keeps `autosave` from forcing the very first typing run closed
    /// on the frame after it opens.
    fn adopt(&mut self, d: Document) {
        self.bench.set(d);
        // The annotation index's other half of its identity. Two documents can
        // sit at the same cursor — every one of them starts at `None` — so
        // without this the second file opened is drawn with the first file's
        // features, plausibly and silently.
        self.doc_generation = self.doc_generation.wrapping_add(1);
        // A different molecule is a different question about whether the strip
        // is needed, so the answer held across the previous file's re-digests
        // does not carry over.
        self.enz_strip = false;
        self.orf_strip = false;
        self.tr = aa::Translations::default();
        // Read from the file, not carried over: the modal `/transl_table`
        // across this document's CDS features, or the global default.
        self.doc_code = self
            .document()
            .and_then(|d| aa::modal_table(d.molecule()))
            .or_else(|| pl_core::translate::table(self.layout.code))
            .unwrap_or(pl_core::translate::TABLE11);
        self.error = None;
        self.notice = None;
        // A lane set names enzymes that cut THIS molecule, and its seeded
        // default was chosen from this molecule's digest. Carried over it would
        // draw the previous file's diagnostic digest as if it were this one's.
        //
        // Reset HERE, in `adopt`, and nowhere on the failure path: cc36cf7 made
        // a failed load leave the open document intact, and clearing the lane
        // set when an open was merely *attempted* would be a small regression
        // of the same contract. Conditions the user chose survive, because they
        // are about the gel and not about the molecule.
        self.gel = gel::View {
            conditions: self.gel.conditions,
            ladder: self.gel.ladder,
            inverted: self.gel.inverted,
            ..Default::default()
        };
        // And the map is what a newly opened file shows. See `central_view`.
        self.central_view = CentralView::Map;
        // The reads themselves SURVIVE — "I opened the wrong plasmid" must not
        // cost the user their files — but every report is discarded and re-run,
        // because a report names a reference.
        self.rearm_reads();
        self.edit = seqedit::SeqEdit::new();
        self.selected = None;
        self.hot = None;
        self.hot_shown = None;
        // The design panel belongs to the molecule it was opened on. It used to
        // survive a document swap holding the previous file's title, length,
        // topology, target and report while being redrawn against the new
        // molecule's bases — and "Add to document" then wrote file A's primer
        // coordinates, under file A's name, into file B. Nothing in the panel
        // says which file it came from once the title bar has changed, so it is
        // closed rather than relabelled.
        self.close_design("the design panel was closed: it was designed against the previous file");
        // THE SAME RULE, AND 28e9d91 DID NOT FOLLOW IT. The cut-and-religate
        // panel holds a plan built from one molecule's bases: its fragments,
        // its ends, the parent intervals each fragment was traced back to, and
        // the finished constructs. `adopt` did not touch it, so it survived a
        // document swap showing plasmid A's digest while B was on screen — and
        // "Open" then built A's construct, from a file no longer open, labelled
        // with A's name, as though it had come from the plasmid in front of you.
        //
        // A wrong construct is the worst thing this program can produce, so the
        // panel is closed rather than relabelled: a religation plan costs
        // milliseconds to recompute and nothing in it says which molecule it
        // came from once the title bar has changed.
        if self.clone_panel.take().is_some() {
            self.notice = Some(
                "the cut-and-religate panel was closed: its digest was of the previous file".into(),
            );
        }
        // And for the same reason: the editor holds an INDEX into the previous
        // file's feature list plus a clone of the feature at it. Left open
        // across a document swap, one press of Save writes file A's feature over
        // whatever happens to sit at that index in file B.
        self.close_feature_editor(
            "the feature editor was closed: it was opened on the previous file",
        );
        self.last_autosave = Some(std::time::Instant::now());
    }

    /// Drop the design panel, saying so if there was a report worth keeping.
    ///
    /// Silence would be its own defect: the panel holds constraints the user
    /// typed and a report that took seconds to compute, and it vanishes for a
    /// reason they cannot see.
    fn close_design(&mut self, why: &str) {
        if let Some(p) = self.design.take() {
            if p.result.is_some() {
                self.notice = Some(why.to_string());
            }
        }
    }

    /// Drop the feature editor, saying so only if there was unsaved work in it.
    ///
    /// The same contract as [`App::close_design`], and silence is right for the
    /// untouched case: a panel opened and closed without a keystroke has nothing
    /// to mourn, and a notice for it would train the user to ignore notices.
    fn close_feature_editor(&mut self, why: &str) {
        if let Some(p) = self.feature_edit.take() {
            if p.dirty() {
                self.notice = Some(why.to_string());
            }
        }
    }

    /// The sequence selection as a segment: 1-based inclusive, wrap bit read
    /// rather than inferred.
    ///
    /// Two conversions, each of which has already been got wrong once in this
    /// file. **Carets sit BETWEEN bases and a segment is 1-based inclusive**, so
    /// selecting the first ten bases is `anchor 0, head 10` and the segment is
    /// `1..10`; `select_feature_under` does the inverse with a documented
    /// `saturating_sub`. Off by one here and every feature drawn from a
    /// selection is one base out at one end, permanently, with nothing anywhere
    /// to contradict it — the feature is legal, it is just not where the user
    /// drew it.
    ///
    /// And `through_origin` is READ, never inferred from the ordering: a pair of
    /// carets on a circle names two arcs, and the app already ships a "take the
    /// other arc" button rather than guess. This is the same expression
    /// `design::Panel::open` uses, for the same reason.
    fn selection_segment(&self) -> Option<pl_core::Segment> {
        let d = self.document()?;
        let mol = d.molecule();
        let n = mol.len();
        let circular = mol.topology.is_circular();
        let sel = self.edit.sel?.canonical(n, circular);
        if sel.is_empty(n) {
            return None;
        }
        let (a, b) = if sel.through_origin {
            (sel.hi() + 1, sel.lo())
        } else {
            (sel.lo() + 1, sel.hi())
        };
        Some(pl_core::Segment::new(a, b))
    }

    /// Open the feature editor: `None` adds, `Some(i)` edits feature `i`.
    ///
    /// The commit comes first for the same reason `open_design`'s does: between
    /// keystrokes the log is one run behind the screen, and a feature whose
    /// coordinates were read from the committed molecule while three more typed
    /// bases are visible is a feature in the wrong place.
    fn open_feature_editor(&mut self, index: Option<usize>) {
        self.settle();
        let Some(d) = self.bench.get() else {
            return;
        };
        let mol = d.molecule();
        let base = match index {
            Some(i) => match mol.features.get(i) {
                Some(f) => f.clone(),
                None => {
                    self.notice = Some(format!("There is no feature {i}.\nNothing was changed."));
                    return;
                }
            },
            None => {
                let mut f = pl_core::Feature::new("", "misc_feature");
                // Seeded from the selection when there is one, and from base 1
                // otherwise. Never from nothing: a feature with no segments is
                // `Invalid::FeatureWithoutSegments`, which the gate always
                // refuses, so an empty table would open a form that cannot be
                // saved until the user works out why.
                f.segments.push(
                    self.selection_segment()
                        .unwrap_or(pl_core::Segment::new(1, 1)),
                );
                f
            }
        };
        let (span, circular, at) = (
            mol.annotation_span(),
            mol.topology.is_circular(),
            d.log.cursor(),
        );
        match featedit::Panel::open(index, base, span, circular, at) {
            Ok(mut p) => {
                // Only one non-modal window may guard the keyboard.
                self.close_design(
                    "the design panel was closed: the feature editor took the keyboard",
                );
                // A new id for the body's ScrollArea, so this open starts at the
                // top. See `featedit::Panel::generation`.
                self.feature_editor_opens = self.feature_editor_opens.wrapping_add(1);
                p.generation = self.feature_editor_opens;
                self.feature_edit = Some(p);
                // THE FEATURE BEING EDITED IS THE HIGHLIGHTED ONE, from every
                // entry point, because the alternative was reachable from two of
                // them: egui delivers `clicked()` on the first press of a
                // double-click, and both the Features row and the map arc TOGGLE
                // on a click, so double-clicking an already-selected feature
                // deselected it. The editor then opened on a feature the list was
                // no longer highlighting, the map arc lost its highlight, and
                // Edit…/Duplicate/Remove went disabled behind the window —
                // leaving the window title "Feature 4" as the only clue which
                // feature was open. Set HERE and not at each call site, so a
                // future entry point cannot forget it.
                if index.is_some() {
                    self.selected = index;
                }
            }
            Err(e) => self.notice = Some(e),
        }
    }

    /// Remove feature `i`, and put its NAME in the status line.
    ///
    /// `OpKind::RemoveFeature::describe()` reads "remove feature 3" — an index
    /// and no name, so a deletion cannot be read back a week later. Fixing that
    /// means putting the name into the op, which changes `OpKind::content`,
    /// which changes every `OpId` ever derived: a provenance break for one word
    /// of prose. So the History tab keeps the hash-stable sentence and the line
    /// the user actually reads gets the name, exactly as the primer path already
    /// overwrites the generic status with its own.
    ///
    /// `hot` is cleared alongside `selected`, which the old call site did not do.
    /// `mol.features.remove(index)` shifts every later index, so a pointer
    /// resting on the removed row leaves `hot` naming a different feature — drawn
    /// highlighted on the map, under someone else's name.
    fn remove_feature(&mut self, i: usize) {
        let name = self
            .document()
            .and_then(|d| d.molecule().features.get(i))
            .map(|f| f.name.clone())
            .unwrap_or_default();
        if self.edit(pl_core::OpKind::RemoveFeature { index: i }) {
            self.status = if name.is_empty() {
                format!("removed feature {i} — Ctrl+Z to undo")
            } else {
                format!("removed \"{name}\" — Ctrl+Z to undo")
            };
            self.selected = None;
            self.hot = None;
            self.hot_shown = None;
        }
    }

    /// Open `d` in a new tab.
    ///
    /// THE UNSAVED-CHANGES QUESTION IS GONE FROM HERE, and its absence is the
    /// point. This used to be the funnel for a document that REPLACED another,
    /// and every path through it had to ask permission first because the answer
    /// decided whether somebody's edits survived. Since the bench holds more
    /// than one document, opening replaces nothing: the new file arrives beside
    /// the old one, both are still there, and there is no longer a question to
    /// ask. cc36cf7 guarded seven such paths; the container removed the hazard
    /// they were guarding.
    ///
    /// Prompting anyway would be worse than useless. "Opening another file
    /// closes it" would simply be false, and a guard that fires when nothing is
    /// at stake is exactly how a user learns to click through the one that
    /// matters — which is now the window-close guard, and asks about every tab.
    ///
    /// The run is still settled first: it is uncommitted work on `App`, and the
    /// new tab is about to take `App`'s view fields.
    fn take_over(&mut self, d: Document, status: String, stale_row: Option<usize>) {
        self.settle();
        if let Some(i) = stale_row {
            if i < self.stale.len() {
                let _ = self.stale.remove(i);
            }
        }
        self.adopt(d);
        self.status = status;
    }

    fn load(&mut self, path: PathBuf) {
        self.load_as(path)
    }

    /// One Open, one dispatcher, one answer.
    ///
    /// Decided on CONTENT, never on the extension — `pl-fileio`'s own rule, and
    /// it is not theoretical: 20 of 394 files named `.ab1` on a real lab drive
    /// are SCF or ZTR.
    ///
    /// # This cannot regress cc36cf7
    ///
    /// The trace branch returns BEFORE `Document::open` is reached and contains
    /// no assignment to `self.document`, no `self.status.clear()`, no
    /// `close_design` and no `close_feature_editor`. Its own failure arm uses
    /// the identical rule cc36cf7 established below — a notice when a document
    /// is open, the takeover only when there is none — so nothing about a
    /// chromatogram can destroy an open document, because a chromatogram never
    /// enters the document path at all.
    ///
    /// It also must NOT reach `take_over`. Opening a read does not close a
    /// document, so the unsaved-changes question must never fire for one:
    /// asking "you have unsaved changes — discard and open?" when a user drops
    /// a chromatogram onto their plasmid is precisely the false positive
    /// cc36cf7's redefinition of `unsaved()` exists to eliminate, and it is how
    /// a guard becomes a reflex click.
    fn load_as(&mut self, path: PathBuf) {
        // A read error is deliberately ignored here and left to
        // `Document::open` below, so an unreadable file gives the one sentence
        // it has always given rather than two different ones depending on which
        // reader got to it first.
        if let Ok(data) = std::fs::read(&path) {
            match pl_fileio::detect(&data) {
                Some(pl_fileio::Format::Abif) => return self.take_read(path, &data),
                // Named rather than refused as "unreadable": a reader that says
                // "parse error" sends the user looking for a corrupt file; one
                // that says "this is ZTR" sends them to the right tool.
                Some(f @ (pl_fileio::Format::Scf | pl_fileio::Format::Ztr)) => {
                    return self.load_failed(format!(
                        "{}: this is {}, not a molecule and not ABIF. Convert it to \
                         .ab1 to look at the trace.",
                        path.display(),
                        f.name()
                    ));
                }
                _ => {}
            }
        }
        match Document::open(&path) {
            Ok(d) => {
                // Built into a LOCAL and handed to `take_over`, not assigned to
                // `self.status` on the way past: the whole string has to travel
                // with a parked document, or a cancelled unsaved-changes
                // question leaves the toolbar describing a file that is not
                // open.
                let mut status = describe(d.molecule(), d.format);
                // Say so when the file held more than we are showing. A viewer
                // that stays silent is indistinguishable from a file with
                // fewer records in it — which is how 1,879 features went
                // missing from a 124-record file without anyone noticing.
                if d.records_in_file > 1 {
                    status = format!(
                        "{}  —  showing record 1 of {} in this file",
                        status, d.records_in_file
                    );
                }
                if !d.unrepresentable_locations.is_empty() {
                    status = format!(
                        "{}  —  {} location(s) skipped as unrepresentable",
                        status,
                        d.unrepresentable_locations.len()
                    );
                }
                // ...and when a file's own annotations do not describe it.
                //
                // `Molecule::validate` already detects coordinates past the end
                // of the sequence, inverted spans and zero starts, and nothing
                // under `bins/` was calling it — so a `.dna` claiming a feature
                // at `1..18446744073709551615` reached the renderer unchecked
                // and took the app down. The drawing code is now hardened too,
                // but a bad file should be *reported*, not merely survived.
                let problems = d.molecule().validate();
                if !problems.is_empty() {
                    status = format!(
                        "{}  —  {} coordinate problem{} in this file: {}",
                        status,
                        problems.len(),
                        if problems.len() == 1 { "" } else { "s" },
                        problems
                            .iter()
                            .take(3)
                            .map(|p| p.to_string())
                            .collect::<Vec<_>>()
                            .join("; ")
                    );
                }
                // A caret from the previous document names bases this one does
                // not have.
                self.take_over(d, status, None);
            }
            // A FAILED load must leave the open document alone.
            //
            // This arm used to assign `self.document = None`, so choosing a
            // `.ab1`, a folder-that-is-a-file or anything unparseable destroyed
            // an edited document and replaced the screen with the takeover —
            // the user lost their work AND got nothing, because there was no
            // new document to have traded it for. `error` is documented above
            // as "right for 'there is no document' and wrong for anything
            // else", and this was the branch that violated its own rule.
            //
            // Through `load_failed`, which is where that rule now lives — the
            // function's own doc claimed it kept the trace path and the
            // molecule path from drifting, and the molecule path was not
            // calling it. The path is NAMED here for the same reason it is
            // there: the command line can now report several failures at once,
            // and "unrecognised format" without a file name in a list of two
            // says nothing about which.
            Err(e) => self.load_failed(format!("{}: {e}", path.display())),
        }
    }

    /// A load that produced nothing, reported without destroying what is open.
    ///
    /// cc36cf7's rule, in one place so the trace path and the molecule path
    /// cannot drift: `error` is documented as "right for 'there is no document'
    /// and wrong for anything else", and the arm that violated that is what
    /// used to lose a user's work.
    fn load_failed(&mut self, e: String) {
        if self.document().is_some() {
            self.notice = Some(e);
            return;
        }
        self.error = Some(e);
        self.status.clear();
        self.close_design(
            "the design panel was closed: the document it described is no longer open",
        );
        self.close_feature_editor(
            "the feature editor was closed: the document it described is no longer open",
        );
    }

    /// Take on a chromatogram. NEVER touches `self.document`.
    fn take_read(&mut self, path: PathBuf, data: &[u8]) {
        let trace = match pl_abif::parse(data) {
            Ok(t) => t,
            Err(e) => return self.load_failed(format!("{}: {e}", path.display())),
        };
        let name = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "read".into());
        let mut r = reads::Read::new(name.clone(), Some(path), trace);
        if let Some(d) = self.document() {
            r.compare(&d.molecule().seq, d.molecule().topology.is_circular());
        }
        self.status = format!("{name} · {} bases", r.trace.sequence.len());
        self.reads.push(r);
        self.read_shown = self.reads.len() - 1;
        self.tab = Tab::Reads;
    }

    /// Keep the held reads' answers current, and collect the ones that have
    /// arrived. Returns `(something changed, something is still running)`.
    ///
    /// AN EDIT MOVES BASES, so every report about them is now about a sequence
    /// that no longer exists. The chromatogram is a property of the file and
    /// never changes; the report is a property of the file AND the molecule,
    /// which the user is editing underneath it. A panel that repaints a perfect
    /// trace beside a discrepancy list computed three edits ago is showing a
    /// confident wrong answer — the defect `Document::apply`'s unconditional
    /// re-digest exists to prevent.
    ///
    /// Keyed on `(doc_generation, seq_version)` for the reason `annot` is:
    /// `seq_version` alone starts at 0 in every document, so opening plasmid A,
    /// editing it and then opening B compares equal and the reports carry over.
    fn refresh_reads(&mut self) -> (bool, bool) {
        let key = (
            self.doc_generation,
            self.document().map_or(0, |d| d.seq_version),
        );
        let mut changed = false;
        if key != self.reads_for {
            self.reads_for = key;
            if !self.reads.is_empty() {
                self.rearm_reads();
                changed = true;
            }
        }
        let mut running = false;
        for r in &mut self.reads {
            changed |= r.poll();
            running |= matches!(r.state, reads::CompareState::Running { .. });
        }
        (changed, running)
    }

    /// Compare every held read against whatever is open now.
    ///
    /// A `Report` names a reference, so nothing is carried across a document
    /// swap: a panel that repaints a perfect trace beside a discrepancy list
    /// computed against another plasmid is a confident wrong answer, which is
    /// the defect `Document::apply`'s unconditional re-digest exists to
    /// prevent.
    fn rearm_reads(&mut self) {
        let target = self.document().map(|d| {
            (
                d.molecule().seq.clone(),
                d.molecule().topology.is_circular(),
            )
        });
        for r in &mut self.reads {
            match &target {
                Some((seq, circular)) => r.compare(seq, *circular),
                None => {
                    r.cancel();
                    r.state = reads::CompareState::NoReference;
                }
            }
        }
    }

    fn pick_file(&mut self) {
        let picked = rfd::FileDialog::new()
            // `.ab1` was in none of the four filters, so a user literally could
            // not select a chromatogram through this dialog. This is the door
            // that did not exist.
            .add_filter(
                "Everything Polylinker opens",
                &[
                    "dna", "gb", "gbk", "genbank", "fa", "fasta", "seq", "ape", "ab1",
                ],
            )
            .add_filter("SnapGene", &["dna"])
            .add_filter("GenBank", &["gb", "gbk", "genbank"])
            .add_filter("FASTA", &["fa", "fasta", "fna"])
            .add_filter("Sanger trace", &["ab1"])
            .pick_file();
        if let Some(p) = picked {
            self.load(p);
        }
    }

    fn export(&mut self, as_fasta: bool) {
        self.settle();
        // Assigned by the FASTA arm below; GenBank is always faithful enough to
        // count as a save.
        let mut faithful = true;
        let Some(d) = self.bench.get() else { return };
        let stem = pl_fileio::genbank::locus_name(&d.title);
        let (ext, filter) = if as_fasta {
            ("fa", "FASTA")
        } else {
            ("gb", "GenBank")
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.{ext}"))
            .add_filter(filter, &[ext])
            .save_file()
        else {
            return;
        };
        let text = if as_fasta {
            pl_fileio::fasta::write(d.molecule(), &d.title, 70)
        } else {
            pl_fileio::genbank::write(d.molecule(), &d.title, today())
        };
        let note = if as_fasta {
            // FASTA is bases and a header, and that is all. The GenBank path
            // carefully warns that unoriented features become forward while
            // this one set `lossy = Vec::new()` and said nothing at all — so
            // the format that discards *every* feature, every note and the
            // topology was the quieter of the two.
            let m = d.molecule();
            let mut lost: Vec<String> = Vec::new();
            if !m.features.is_empty() {
                lost.push(format!("{} feature(s)", m.features.len()));
            }
            if m.topology.is_circular() {
                lost.push("the topology (it will reopen as linear)".into());
            }
            // The same list decides whether this write CLEARS the dirty state,
            // below. Reused rather than recomputed: `export` already knows, per
            // format, exactly what that format cannot carry, and a second
            // notion of fidelity is how two answers to one question appear.
            faithful = lost.is_empty();
            if lost.is_empty() {
                String::new()
            } else {
                format!(
                    "FASTA keeps only the bases; this drops {}",
                    lost.join(" and ")
                )
            }
        } else {
            let lossy = d.molecule().features_without_expressible_orientation();
            if lossy.is_empty() {
                String::new()
            } else {
                // GenBank has no way to say "unoriented", so those features are
                // written as forward. For about half of them that is a
                // directional claim the source never made.
                format!(
                    "{} feature(s) written as forward; GenBank cannot express their strand",
                    lossy.len()
                )
            }
        };
        match std::fs::write(&path, text) {
            Ok(()) => {
                // GenBank's own note ("N features written as forward") is a
                // strand-EXPRESSIBILITY nuance, not lost work, so GenBank
                // always clears the dirty state. FASTA clears it only when its
                // loss list is empty — no features and linear — because a FASTA
                // that drops nine features has not saved the user's work, and
                // marking it clean would make the dot and the guard lie in the
                // one case they exist for.
                if faithful {
                    if let Some(d) = self.bench.get_mut() {
                        d.mark_saved();
                        // AND THE DOCUMENT NOW LIVES THERE. `path` was assigned
                        // once, at construction, and never again — a grep for
                        // `.path = ` across bins/pl-gui returned nothing — so a
                        // document written by Save As stayed `path: None`
                        // forever. It is clean and it is nowhere.
                        //
                        // What that costs today: every subsequent Ctrl+S opens
                        // the picker again, because there is no path to write
                        // to; and `autosave`'s base-cursor guard tests
                        // `here.original.is_some()`, so a saved-but-pathless
                        // document keeps writing recovery drafts of a file that
                        // is already on disk.
                        //
                        // Only on a FAITHFUL write, alongside `mark_saved` and
                        // for the same reason: a FASTA that dropped nine
                        // features has not saved the user's work, and pointing
                        // the document at it would make the app believe
                        // otherwise twice over.
                        d.path = Some(path.clone());
                    }
                }
                self.wrote(&path, &note);
                if !faithful {
                    self.status = format!("{} — the document is still unsaved", self.status);
                }
            }
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }

    /// Write the molecule as a SnapGene `.dna`.
    ///
    /// `snapgene::from_molecule_reporting` has existed, tested, since the writer
    /// landed, and its only caller outside tests was `pl convert --to dna`: a
    /// user who opens `.dna` all day and wants to hand a `.dna` back to a
    /// student had no route out of this program that was not lossy.
    ///
    /// Always `from_molecule_reporting`, never `snapgene::write(container, …)`.
    /// `write` re-emits the blocks the file was READ from, which is byte-exact
    /// and wrong for anything the user has edited: it would write the original
    /// sequence back out under the impression of having saved.
    ///
    /// Order is picker, then the losses, then the bytes — see
    /// [`App::dna_lossiness_modal`] for why the question cannot be asked before
    /// a destination exists.
    fn save_dna(&mut self, then: Option<Losing>) {
        self.settle();
        let Some(d) = self.bench.get() else { return };
        // `file_stem`, not `locus_name`: `locus_name` replaces every character
        // outside [A-Za-z0-9_.-] and truncates to the sixteen columns a GenBank
        // LOCUS name gets, because overrunning columns 13-28 shifts every field
        // after it. `.dna` has no such field, and `pKoV with His decR.dna` must
        // not save as `pKoV_with_His_de.dna`.
        let stem = d
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| pl_fileio::genbank::locus_name(&d.title));
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.dna"))
            .add_filter("SnapGene", &["dna"])
            .save_file()
        else {
            return;
        };
        let Some(pending) = self.plan_dna(path, then) else {
            return;
        };
        // `from_molecule_reporting`'s own docstring says the report "is empty
        // for every molecule that came from a real file", and a GenBank-sourced
        // molecule written to a path that holds nothing loses nothing — so that
        // case still saves in one click, which is the common one and must stay
        // cheap. See `PendingDna::asks` for the rest of the gate.
        if pending.asks() {
            self.pending_dna = Some(pending);
        } else {
            self.write_dna(pending);
        }
    }

    /// Everything `save_dna` decides once a destination exists.
    ///
    /// Split from the picker so a test can drive it: `rfd::FileDialog` opens a
    /// native window, so the whole of this — including the blocker it exists to
    /// fix, a GenBank molecule about to replace somebody's `.dna` — was
    /// unreachable from the suite.
    fn plan_dna(&self, path: PathBuf, then: Option<Losing>) -> Option<PendingDna> {
        let d = self.document()?;
        let (bytes, unwritable) = pl_fileio::snapgene::from_molecule_reporting(d.molecule());
        let overwriting_source = d.path.as_deref().is_some_and(|o| same_file(o, &path));
        // The modal's claims are the File tab's claims, out of the same fields
        // on the same container. Two surfaces disagreeing about one file is the
        // defect the review's finding 3 is about.
        let (mut history, notes) = match &d.container {
            Some(c) => (c.history_present, c.unrepresentable_notes.clone()),
            None => (false, Vec::new()),
        };
        // What these bytes actually contain, read back from themselves. "What
        // the writer emits" is a property of the writer, and a hardcoded list
        // here goes stale the first time it gains a block.
        let ours = pl_fileio::snapgene::read_blocks(&bytes).unwrap_or_default();
        let source_lost = d
            .container
            .as_ref()
            .map(|c| pl_fileio::snapgene::dropped_blocks(&c.blocks, &ours))
            .unwrap_or_default();
        // THE DESTINATION, which nothing here used to look at.
        //
        // The question this modal exists to answer is "what does writing here
        // cost", and until the destination is read the program cannot answer it
        // for the case that costs the most: a GenBank molecule written over
        // somebody's `.dna`. The source's container says nothing about that
        // file. Read on the save path only — never on a paint — and a
        // destination that does not exist, cannot be read or is not a `.dna`
        // costs nothing beyond a failed parse.
        let dest_lost = match std::fs::read(&path) {
            Ok(raw) => match pl_fileio::snapgene::parse(&raw) {
                Ok(dest) => {
                    // The destination's history counts as much as the source's:
                    // this write replaces it with none either way.
                    history |= dest.history_present;
                    pl_fileio::snapgene::dropped_blocks(&dest.blocks, &ours)
                }
                Err(_) => Vec::new(),
            },
            Err(_) => Vec::new(),
        };
        Some(PendingDna {
            path,
            bytes,
            unwritable,
            history,
            notes,
            overwriting_source,
            dest_lost,
            source_lost,
            then,
        })
    }

    /// The bytes, and the bookkeeping that must only happen on `Ok`.
    fn write_dna(&mut self, p: PendingDna) {
        match std::fs::write(&p.path, &p.bytes) {
            Ok(()) => {
                if let Some(d) = self.bench.get_mut() {
                    d.mark_saved();
                    // Same rule as `export`: a write the document is marked
                    // clean by is a write the document now lives at. The `.dna`
                    // path's losses were disclosed and accepted in the modal
                    // that raised this, so it counts as faithful in the sense
                    // `mark_saved` already uses one line above.
                    d.path = Some(p.path.clone());
                }
                // The cache omission still gets said, once, through the channel
                // that already puts the consequence leftmost.
                self.wrote(
                    &p.path,
                    "the cut-site cache is not written; SnapGene rebuilds it",
                );
                // Raised by the unsaved-changes guard's Save button. Only a save
                // that actually cleared `unsaved()` may proceed to the discard —
                // a guard that closed the window after a failed write would have
                // done the exact damage it was added to prevent.
                if let Some(why) = p.then {
                    if self.document().is_none_or(|d| !d.unsaved()) {
                        // Preserved: the bytes are on disk. No recovery draft is
                        // kept and the next launch says nothing, because nothing
                        // happened here that a user needs warning about.
                        self.resolve_guard(why, true);
                    }
                }
            }
            Err(e) => self.error = Some(format!("{}: {e}", p.path.display())),
        }
    }

    /// Report a file written, with any consequence of writing it FIRST.
    ///
    /// `format!("wrote {path}{note}")` put the bookkeeping in front of the
    /// warning, and the status line is finite: saving FASTA to a 150-character
    /// path left the bar reading `wrote C:\Users\…\scratchpad\vout\seq` with the
    /// whole of `FASTA keeps only the bases; this drops 9 feature(s) and the
    /// topology` off the window. The clause that has to survive clipping must be
    /// leftmost, and the path is the part the user already knows — they chose it
    /// in the dialog a second ago, and it is still on hover.
    ///
    /// The user's own genome files sit 160 characters deep in OneDrive, so
    /// "saving beside a source file" was the ordinary case, not an edge one.
    fn wrote(&mut self, path: &std::path::Path, note: &str) {
        self.status = if note.is_empty() {
            format!("wrote {}", path.display())
        } else {
            format!("{note}  —  wrote {}", path.display())
        };
    }

    /// What the figure exporters should draw, for the open document.
    ///
    /// The one place the app decides what a figure is, so "Map SVG…" and
    /// "Map PDF…" cannot differ. Two things go in that never used to:
    ///
    /// - the **title**, because `pl_draw::scene` falls back to the literal
    ///   `"unnamed"` and no caller passed a name in. The map on screen said
    ///   "pKoV with His decR.dna" and the exported figure said `unnamed`, in
    ///   the centre of the ring and in the SVG `<title>`, for every `.dna`
    ///   file ever exported.
    /// - the **cut sites**, because `pl-draw` has no reference to an enzyme
    ///   anywhere. The user reads 22 unique cutters off the map to plan a
    ///   digest, clicks "Figure SVG…", and gets a map with no restriction
    ///   sites on it at all.
    ///
    /// Unique cutters only, which is the same rule the on-screen map applies,
    /// so the figure is the picture the user was looking at when they exported
    /// it.
    fn figure_options(d: &doc::Document, set: pl_enzymes::EnzymeSet) -> pl_draw::Options {
        let mut opts = pl_draw::Options {
            title: Some(pl_fileio::caption_of(&d.title).to_string()),
            // The SAME intersection the on-screen map takes. Without it the
            // picture you export is not the picture you filtered, which is
            // exactly the complaint: narrow to "Unique 6+" to find a
            // linearisation site and the figure comes out with every unique
            // cutter on it.
            sites: d
                .digest
                .results()
                .iter()
                .filter(|x| x.is_unique_cutter() && set.admits(x))
                .map(|x| (x.enzyme.name.to_string(), x.positions[0]))
                .collect(),
            ..Default::default()
        };
        // And the line saying what the figure is NOT showing, which the screen
        // has had since the L-ring landed and the figure did not — not in the
        // SVG, not in the PDF, not in the EPS. Unique cutters only is a filter,
        // and `docs/PLAN.md` item 33 is about filters that hide without saying
        // so; of the two artefacts the figure is the one that leaves the machine
        // and reaches a reader with no Enzymes tab to check against.
        //
        // Two passes, because the line has to name how many labels the ring
        // could not fit and only placing them answers that. Exact, not
        // approximate: the note reaches `centre_room` -> `keep_clear` -> the
        // ruler's radius and nothing there feeds back into the reserve, the
        // geometry or the packing, so the first pass's counts describe the
        // second pass's picture. `the_note_does_not_change_what_it_counts` holds
        // that rather than trusting it.
        let mut told = Self::figure_disclosure(d.digest.results(), set);
        let (_, first) = pl_draw::scene(d.molecule(), opts.clone());
        told.labelled = first.sites_named;
        told.hidden = first.sites_dropped;
        told.shortened = first.sites_shortened;
        debug_assert!(told.closes(), "{told:?} does not account for every cutter");
        opts.note = (told.cutters > 0).then_some(told);
        opts
    }

    /// The exported figure's five buckets, before the ring has been laid out.
    ///
    /// Its own function so a test can drive it over an enzyme table this project
    /// does not ship. The bug it replaces was invisible for exactly that reason:
    /// `single` was hardcoded 0 under a comment saying the site filter "never
    /// turns a single cutter away", which stopped being true when the filter was
    /// intersected with the user's enzyme set, and stayed harmless only because
    /// every enzyme in the built-in table has a 6-base or longer site. One
    /// four-cutter in that table and `debug_assert!` fires in a test build — and
    /// is COMPILED OUT of the release binary, so the exported SVG, PDF and EPS
    /// would carry a note whose numbers do not add up, to a reader with no
    /// Enzymes tab to check it against.
    fn figure_disclosure(
        results: &[pl_enzymes::Digest],
        set: pl_enzymes::EnzymeSet,
    ) -> pl_draw::ring::Disclosure {
        let cutting = |f: &dyn Fn(usize) -> bool| results.iter().filter(|x| f(x.count())).count();
        pl_draw::ring::Disclosure {
            cutters: cutting(&|n| n > 0),
            // Unfiltered, and deliberately: the figure draws unique cutters
            // only, so EVERY dual and EVERY multi cutter is undrawn whatever the
            // enzyme set says, and these two numbers are facts about the
            // molecule rather than about the filter. `map.rs` says the same
            // thing the same way, so the screen and the figure cannot diverge.
            dual: cutting(&|n| n == 2),
            multi: cutting(&|n| n > 2),
            // A UNIQUE cutter the filter turned away, which is the only class
            // the intersection can subtract from this ring.
            single: results
                .iter()
                .filter(|x| x.count() == 1 && !set.admits(x))
                .count(),
            ..Default::default()
        }
    }

    /// Write the map as PDF.
    ///
    /// The same `Scene` as the SVG, so the two are the same picture. Helvetica
    /// is one of the fourteen fonts every viewer provides, so nothing is
    /// embedded -- at the cost of WinAnsi, which has no Greek. Names that lose
    /// characters are listed rather than silently written with `?`.
    fn export_pdf(&mut self) {
        // The map is drawn from `log.current()`, so an open run would be
        // missing from it.
        self.settle();
        let Some(d) = self.bench.get() else { return };
        let stem = pl_fileio::genbank::locus_name(&d.title);
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.pdf"))
            .add_filter("PDF", &["pdf"])
            .save_file()
        else {
            return;
        };
        let set = self.enzyme_set;
        let (bytes, drawn, font) =
            pl_draw::circular_pdf(d.molecule(), Self::figure_options(d, set));

        let mut note = Self::figure_note(&drawn);
        if !font.unencodable.is_empty() {
            note.push(format!(
                "{} name(s) hold characters Helvetica cannot show and were written with '?': {}. Export SVG to keep them",
                font.unencodable.len(),
                font.unencodable.join(", ")
            ));
        }
        match std::fs::write(&path, bytes) {
            Ok(()) => self.wrote(&path, &note.join("  —  ")),
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }

    /// What a figure lost, as clauses to put in front of the destination path.
    ///
    /// A map missing three labels looks exactly like a plasmid with three fewer
    /// features, so the count goes somewhere rather than nowhere.
    ///
    /// `labels_truncated` is in here because it was in neither exporter's status
    /// while `pl export` printed it on stderr: a shortened name was named on the
    /// command line and silent in the app, and `pCMV-WP...` is a different
    /// plasmid's name from `pCMV-WPRE`.
    fn figure_note(drawn: &pl_draw::Report) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if !drawn.labels_hidden.is_empty() {
            out.push(format!(
                "{} label(s) did not fit: {}",
                drawn.labels_hidden.len(),
                drawn.labels_hidden.join(", ")
            ));
        }
        if !drawn.labels_truncated.is_empty() {
            out.push(format!(
                "{} label(s) shortened with '...': {}",
                drawn.labels_truncated.len(),
                drawn.labels_truncated.join(", ")
            ));
        }
        if !drawn.malformed.is_empty() {
            out.push(format!(
                "{} feature(s) lie outside the molecule and are not drawn: {}",
                drawn.malformed.len(),
                drawn.malformed.join(", ")
            ));
        }
        if !drawn.partly_drawn.is_empty() {
            out.push(format!(
                "{} feature(s) drawn from only some of their segments: {}",
                drawn.partly_drawn.len(),
                drawn.partly_drawn.join(", ")
            ));
        }
        if drawn.title_truncated {
            out.push(
                "the caption was too wide for the ring and was shortened; \
                 the SVG's <title> carries the whole name"
                    .into(),
            );
        }
        out
    }

    /// Write the map as SVG.
    ///
    /// Deliberately the same `pl_draw::Options` as "Map PDF…" and, but for the
    /// enzyme list, as `pl export`, so the app and the command line produce
    /// byte-identical files for the same molecule and the same sites. A figure
    /// that changes depending on which of the two you reached for is a figure
    /// you cannot cite. `pl export` reaches the same sites with
    /// `--sites unique`, which is its default.
    fn export_svg(&mut self) {
        self.settle();
        let Some(d) = self.bench.get() else { return };
        let stem = pl_fileio::genbank::locus_name(&d.title);
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.svg"))
            .add_filter("SVG", &["svg"])
            .save_file()
        else {
            return;
        };
        let set = self.enzyme_set;
        let (svg, drawn) = pl_draw::circular_svg(d.molecule(), Self::figure_options(d, set));

        match std::fs::write(&path, svg) {
            Ok(()) => self.wrote(&path, &Self::figure_note(&drawn).join("  —  ")),
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }

    /// Write the gel as SVG, PDF or EPS.
    ///
    /// THE SAME `Scene` THE PANE IS PAINTING, not one rebuilt for export. That
    /// is the whole benefit of the Scene→egui path: rebuilding it here would
    /// reintroduce exactly the drift `figure_options` was written to remove
    /// from the map, where the screen and the file shared only their range
    /// arithmetic and a label fix on one left the other untouched.
    fn export_gel(&mut self, fmt: Fmt) {
        // The lanes come from the digest of `log.current()`, so an open typing
        // run would be missing from them.
        self.settle();
        let inverted = self.gel.inverted;
        // The reason comes from `gel_ready`, which is where the condition is,
        // rather than being guessed here as "the digest has not finished" —
        // which was false for three of the four ways there can be no picture.
        if let Err(why) = self.gel_ready() {
            self.notice = Some(format!("there is no gel to export: {why}"));
            return;
        }
        // Taken and put back, for the same reason `central` does: the `Scene`
        // is the expensive thing the cache exists to keep.
        let Some((key, built)) = self.gel_cache.take() else {
            return;
        };
        // A PAGE BOX A VIEWER WILL REFUSE IS NOT AN EXPORT. `pdf::to_pdf`
        // writes `/MediaBox` straight from the scene with no bound, and a gel
        // whose labels ran away came to 280,947 pt — 3,900 inches, against
        // Acrobat's documented 14,400-unit limit. `pl_gel::MAX_LISTED` is the
        // fix for the cause; this is the guard, because a file that opens as
        // "damaged" tells the user nothing about why.
        const PAGE_LIMIT: f64 = 14_400.0;
        if matches!(fmt, Fmt::Pdf | Fmt::Eps)
            && (built.scene.width > PAGE_LIMIT || built.scene.height > PAGE_LIMIT)
        {
            self.notice = Some(format!(
                "this gel is {:.0} x {:.0} pt, past the {PAGE_LIMIT:.0} pt page limit \
                 {} readers enforce. Export it as SVG, which has no page, or narrow the gel.",
                built.scene.width,
                built.scene.height,
                fmt.name()
            ));
            self.gel_cache = Some((key, built));
            return;
        }
        let Some(d) = self.bench.get() else {
            self.gel_cache = Some((key, built));
            return;
        };
        // `-gel` is not decoration. The CLI carries a whole `claim_output`
        // mechanism because two inputs sharing a file stem must not silently
        // overwrite each other, and a map and a gel of one plasmid share a stem.
        let stem = format!("{}-gel", pl_fileio::genbank::locus_name(&d.title));
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.{}", fmt.ext()))
            .add_filter(fmt.name(), &[fmt.ext()])
            .save_file()
        else {
            self.gel_cache = Some((key, built));
            return;
        };
        let bytes = match fmt {
            Fmt::Svg => pl_draw::svg_of(&built.scene).into_bytes(),
            Fmt::Pdf => pl_draw::pdf::to_pdf(&built.scene).0,
            Fmt::Eps => pl_draw::eps::to_eps(&built.scene, 1.0).0.into_bytes(),
        };

        let mut note = Vec::new();
        if built.hidden.0 > 0 {
            note.push(format!(
                "{} fragment(s) hide in {} band(s)",
                built.hidden.0, built.hidden.1
            ));
        }
        if built.unplaced > 0 {
            note.push(format!(
                "{} fragment(s) named rather than drawn",
                built.unplaced
            ));
        }
        if !built.suspended.is_empty() {
            note.push(format!(
                "{} lane(s) suspended by the enzyme filter: {}",
                built.suspended.len(),
                built.suspended.join(", ")
            ));
        }
        // AND THE AUDIT'S LIMIT IS STATED, not quietly enjoyed. `audit` matches
        // `Item::Path { stroke: Some(_) }` only, so fill-only paths — which is
        // every band, every well and the background — are skipped entirely. It
        // covers the TEXT and not the INK, and reporting a pass it did not earn
        // is exactly UX review finding 8.
        let bg = pl_gel::render::Options {
            inverted,
            ..Default::default()
        };
        let bad = pl_draw::contrast::audit(&built.scene, bg.background(), 1.0);
        if !bad.is_empty() {
            note.push(format!(
                "{} label(s) below WCAG AA on this field: {}",
                bad.len(),
                bad.iter()
                    .take(3)
                    .map(|f| format!("{} at {:.1}:1", f.what, f.ratio))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        self.gel_cache = Some((key, built));
        match std::fs::write(&path, bytes) {
            Ok(()) => self.wrote(&path, &note.join("  —  ")),
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }

    /// Write the map as EPS.
    ///
    /// The same `Scene` as the SVG and the PDF. `pl_draw::eps::to_eps` has
    /// shipped and been tested since the publication-export work and the GUI
    /// simply never called it, so the app and the command line disagreed about
    /// which formats exist.
    fn export_map_eps(&mut self) {
        self.settle();
        let Some(d) = self.bench.get() else { return };
        let stem = pl_fileio::genbank::locus_name(&d.title);
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.eps"))
            .add_filter("EPS", &["eps"])
            .save_file()
        else {
            return;
        };
        let set = self.enzyme_set;
        let (sc, drawn) = pl_draw::scene(d.molecule(), Self::figure_options(d, set));
        let (eps, _) = pl_draw::eps::to_eps(&sc, 1.0);
        match std::fs::write(&path, eps) {
            Ok(()) => self.wrote(&path, &Self::figure_note(&drawn).join("  —  ")),
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }

    /// A question is on screen about the document, and nothing may change the
    /// document until it is answered.
    ///
    /// One predicate for all four, because they all fail the same way and it
    /// failed silently for three of them: `egui::Modal` dims the screen and
    /// blocks widget interaction, but `global_shortcuts` and `sequence_keys`
    /// both read raw events off `ctx.input` before a single widget is built, so
    /// no amount of modality reaches them. Only the paste consent was listed,
    /// and the two guards added since — the unsaved-changes question and the
    /// `.dna` lossiness question — left Ctrl+Z, Ctrl+S, Ctrl+O, Ctrl+V and every
    /// printable key live underneath a dialog whose own text is a count of the
    /// thing they change.
    ///
    /// `closing` — the latched window close — is now the only document-level
    /// question this has to cover. `pending_open`, a document parked behind the
    /// unsaved-changes prompt, was here too until the bench made opening
    /// non-destructive and there stopped being anything to park.
    fn asking(&self) -> bool {
        self.edit.pending_paste.is_some() || self.pending_dna.is_some() || self.closing
    }

    /// The window close, held back until the user has been asked.
    ///
    /// Established from the pinned versions, not assumed. `App::on_exit`'s own
    /// doc says to check `close_requested()` and answer with
    /// `ViewportCommand::CancelClose`. `ViewportInfo::close_requested` is
    /// `events.contains(&ViewportEvent::Close)` and events are per-frame — the
    /// flag is true for exactly ONE frame, so it is observed once and latched;
    /// do not expect to see it again while the modal is up. eframe decides in
    /// `epi_integration`: on the root viewport, if `close_requested` was set on
    /// this frame's input and this frame's OUTPUT does not contain
    /// `CancelClose`, the app exits. So the command must be sent in the same
    /// frame the flag is read, which is why this runs from `ui` and not from
    /// `on_exit`.
    ///
    /// Its own method so a test can drive it with a plain `egui::Context`:
    /// `App::ui` takes an `eframe::Frame`, which a test has no way to build.
    fn close_request(&mut self, ctx: &egui::Context) {
        if self.close_now {
            // `let_it_go` is already set, so the `Close` event this raises does
            // not re-arm the guard on the next frame. That trap is what makes a
            // window impossible to shut: egui-winit pushes `ViewportEvent::Close`
            // into the viewport info, so the next frame sees `close_requested`
            // again, and a still-armed latch cancels its own close forever.
            self.close_now = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if ctx.input(|i| i.viewport().close_requested()) && !self.let_it_go {
            self.settle();
            // EVERY TAB, not the one on screen. With a single document those
            // were the same sentence; with a bench they are not, and the
            // difference is somebody's work. Edit a plasmid, open a second in a
            // new tab, close the window — and a guard that asked only about the
            // active document would let the first go without a word. That is
            // the class of loss cc36cf7 spent a whole commit closing when there
            // was only one way to reach it.
            //
            // `settle` above still settles only the ACTIVE tab's run, and that
            // is right rather than an oversight: a run is uncommitted work
            // living on `App`, so only the active tab can have one, and
            // `switch_tab` settles as it leaves.
            if self.bench.any_unsaved() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.closing = true;
            }
        }
    }

    /// The state on screen is not in any file, and something is about to
    /// replace or close it.
    ///
    /// `egui::Modal`, matching the one modal this app already has (the paste
    /// consent) and for the reason recorded there: a plain `Window` is not
    /// modal in egui, `Button` registers focus interest without taking focus,
    /// and the document behind stays fully live — so the caret could be moved
    /// between the question and the answer.
    fn unsaved_modal(&mut self, ctx: &egui::Context) {
        // Closing is the only gesture left that can lose work. Opening a file,
        // restoring a draft and opening a construct all add a tab now, so the
        // three cases this used to choose between no longer exist.
        let why = if self.closing {
            Losing::Close
        } else {
            return;
        };
        // One question per gesture. The `.dna` lossiness modal is raised BY this
        // one's save button; stacking the two would ask twice about a single
        // click, so this one stands down while that one is up.
        //
        // The PASTE consent is the same argument arriving from the other side:
        // a close requested while it is open latches `closing` invisibly, and
        // this modal then painted underneath it. Two questions about two
        // different things, one hidden behind the other, and the hidden one is
        // the one that decides whether a document survives.
        if self.pending_dna.is_some() || self.edit.pending_paste.is_some() {
            return;
        }
        // The predicate, re-read every frame, not the latch that raised it.
        //
        // Ctrl+Z is live underneath this dialog by design (the shortcut guard
        // stands the document down only for the paste question), and undoing
        // back to the opening state genuinely clears `unsaved()`. The guard
        // stayed up anyway, changed its own sentence to "has not been saved to
        // a file", and the obvious answer — "Close without saving" — then left a
        // 0-edit recovery draft of an untouched file and a false "did not close
        // cleanly" on the next launch. A guard is a function of the state; when
        // the state stops being at risk the gesture the user asked for should
        // simply happen.
        // THE WHOLE BENCH, NOT THE ACTIVE TAB.
        //
        // ea436aa taught `close_request` to arm on `bench.any_unsaved()` and
        // left this predicate reading the document on screen, which disarmed it
        // again one frame later. Edit plasmid A, open plasmid B in a new tab,
        // close the window: B is clean, so this resolved the guard with
        // `preserved = true`, the app exited, and A's edits were gone — no
        // dialog, and no recovery draft either, because `preserved` is exactly
        // the answer that says none is needed.
        //
        // The commit that introduced the hazard is the one that claimed to have
        // closed it. A guard now has two halves and they have to agree; this is
        // the second half.
        if !self.bench.any_unsaved() {
            self.resolve_guard(why, true);
            return;
        }
        // Ask about a tab that is ACTUALLY at risk, and show it. Naming the
        // active document would describe a clean file while discarding a dirty
        // one somewhere behind it, and a dialog the user can agree with while it
        // omits the thing it is about is worse than no dialog at all.
        if self.document().is_none_or(|d| !d.unsaved()) {
            if let Some(i) = self.bench.first_unsaved() {
                self.switch_tab(i);
            }
        }
        let Some(d) = self.bench.get() else { return };
        let title = pl_fileio::caption_of(&d.title).to_string();
        // "0 edits that are not in any file" is absurd and would be the first
        // thing a user sees after a crash recovery, so the never-written case
        // gets its own sentence rather than a count of nothing.
        // OTHER TABS COUNT. The sentence used to describe the document on
        // screen, which was the whole workspace; it is not any more, and a
        // dialog that named one plasmid while silently discarding three would
        // be worse than no dialog — the user would read it, agree with it, and
        // lose work it never mentioned.
        let others = self
            .bench
            .unsaved_count()
            .saturating_sub(usize::from(d.unsaved()));
        let and_others = match others {
            0 => String::new(),
            1 => " Another tab also has unsaved work.".to_string(),
            n => format!(" {n} other tabs also have unsaved work."),
        };
        let stake = match d.unsaved_ops() {
            Some(0) => format!("{title} has not been saved to a file.{and_others}"),
            Some(1) => format!("{title} has 1 edit that is not in any file.{and_others}"),
            Some(n) => format!("{title} has {n} edits that are not in any file.{and_others}"),
            // Reachable by saving and then seeking onto another branch from the
            // History tab: the distance genuinely does not exist.
            None => format!("{title} has changes that are not in any file."),
        };
        // Whether the sentence above named one thing, so the sentence below can
        // agree with it.
        let one = matches!(d.unsaved_ops(), Some(0) | Some(1));
        let save_label = if d.format == pl_fileio::Format::SnapGene {
            "Save as .dna…"
        } else {
            // A FASTA-sourced document maps to GenBank on purpose: FASTA cannot
            // hold what is on screen, and offering it here would defeat the
            // guard by letting the user answer it with a lossy write.
            "Save as GenBank…"
        };
        let armed = self.recovery.is_some();
        let mut cancel = false;
        let mut discard = false;
        let mut save = false;
        egui::Modal::new(egui::Id::new("pl-unsaved-changes")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.heading("Unsaved changes");
            ui.add_space(6.0);
            ui.label(RichText::new(stake));
            ui.label(RichText::new(why.consequence(one)));
            ui.add_space(6.0);
            if armed {
                ui.label(
                    RichText::new(
                        "A crash-recovery copy is kept. Polylinker will offer it the next time \
                         it starts.",
                    )
                    .color(pal(ui).muted)
                    .size(11.0),
                );
            } else {
                // Every slot was taken, the status already says autosave is off,
                // and this dialog must not now promise a copy that will not
                // exist.
                ui.label(
                    RichText::new(
                        "Autosave is off for this window, so these edits are not written \
                         anywhere.",
                    )
                    .color(pal(ui).warn)
                    .size(11.0),
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                // Cancel first, because that is what the paste dialog does and
                // because Escape maps to it. NO button is the default and
                // nothing is bound to Enter: an Enter reflex carried out of the
                // sequence editor must not be able to discard a document.
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                if ui.button(why.discard_label()).clicked() {
                    discard = true;
                }
                if ui.button(save_label).clicked() {
                    save = true;
                }
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if cancel {
            self.closing = false;
            self.status = "nothing was closed".into();
        } else if discard {
            self.resolve_guard(why, false);
        } else if save {
            if save_label == "Save as .dna…" {
                self.save_dna(Some(why));
            } else {
                self.export(false);
                // The hole every guard of this shape has: a cancelled picker, a
                // full disk or a permission error all leave the document dirty,
                // and a guard that proceeded anyway would have done the exact
                // damage it exists to prevent. Only a write that actually
                // cleared `unsaved()` goes on.
                //
                // AND ONLY WHEN THE WHOLE BENCH IS CLEAN. Asking the active
                // document was right when it was the only one; with tabs, saving
                // the file the dialog happens to be showing would resolve the
                // guard and take every other dirty tab down with it. The modal
                // stays up instead and `unsaved_modal` moves to the next tab
                // that is still at risk, so the guard walks the bench rather
                // than sampling it — one question per document that needs one.
                if !self.bench.any_unsaved() {
                    self.resolve_guard(why, true);
                }
            }
        }
    }

    /// The question has been answered, and the gesture may proceed.
    ///
    /// `preserved` is whether the work reached a file. It decides one thing and
    /// it matters more than its size suggests: `abandoned_unsaved` used to be
    /// set on BOTH answers, so a user who clicked "Save as GenBank…", watched
    /// the file appear on disk and let the app exit was greeted on every
    /// subsequent launch by "You closed Polylinker with unsaved changes" —
    /// naming work that was saved. Crying wolf on the one answer the guard
    /// exists to encourage is worse than not asking at all.
    fn resolve_guard(&mut self, why: Losing, preserved: bool) {
        match why {
            Losing::Close => {
                // The promise the dialog just made, made true — for the discard
                // answer only. A save needs no recovery draft: the work is in
                // the user's own file, `on_exit` clears the recovery slot as it
                // does after any other clean exit, and leaving a draft behind
                // would make the next launch contradict what just happened.
                if self.recovery.is_some() && !preserved {
                    self.abandoned_unsaved = true;
                    self.autosave(true);
                }
                self.closing = false;
                // Cleared BEFORE the command is sent. egui-winit pushes a
                // `Close` viewport event, so the next frame sees
                // `close_requested` again, and a still-armed latch would cancel
                // its own close and make the window impossible to shut.
                self.let_it_go = true;
                self.close_now = true;
            }
        }
    }

    /// What a synthesised `.dna` will not carry, said before it is written.
    ///
    /// Picker first, then this, then the bytes. The other ordering — modal, then
    /// picker — was rejected because the most consequential sentence here is
    /// about a SPECIFIC destination ("this is the file you opened, and its
    /// cloning history will be replaced by none") and that sentence cannot be
    /// written before a path exists. The OS's own overwrite prompt fires inside
    /// the picker; this modal is then the last gate before bytes hit disk, and
    /// `Cancel` writes nothing whatever the OS already asked.
    fn dna_lossiness_modal(&mut self, ctx: &egui::Context) {
        if self.pending_dna.is_none() {
            return;
        }
        let mut go = false;
        let mut cancel = false;
        // Cloned out so the closure does not borrow `self`.
        let p = self.pending_dna.as_ref().map(|p| {
            (
                p.path.clone(),
                p.unwritable.clone(),
                p.history,
                p.notes.clone(),
                p.overwriting_source,
                // Whether the destination is a `.dna` this write replaces, which
                // is a different sentence from "this copy is a downgrade".
                !p.dest_lost.is_empty(),
                pl_fileio::snapgene::dropped_summary(&p.losing()),
                pl_fileio::snapgene::dropped_summary(&p.caches()),
            )
        });
        let Some((path, unwritable, history, notes, overwriting, replacing, losing, caches)) = p
        else {
            return;
        };
        egui::Modal::new(egui::Id::new("pl-dna-lossiness")).show(ctx, |ui| {
            ui.set_max_width(560.0);
            ui.heading("Write SnapGene .dna");
            ui.add_space(6.0);
            // The WHOLE path, elided to the modal's own width, with the full
            // string on hover — not `file_name()`. This dialog's argument for
            // running after the picker is that its sentences are about a
            // SPECIFIC destination, and `pKoV.dna` does not distinguish
            // `~/Downloads/pKoV.dna` from `~/Lab/archive/pKoV.dna`. The user's
            // own files sit 160 characters deep in OneDrive, where the basename
            // is the least informative part of the path.
            let full = path.display().to_string();
            let shown = elide(ui, &full, ui.available_width() - 12.0);
            if overwriting {
                ui.label(RichText::new(format!("{shown} — this is the file you opened.")).strong())
                    .on_hover_text(&full);
            } else {
                ui.label(RichText::new(shown)).on_hover_text(&full);
            }
            ui.add_space(6.0);
            if replacing {
                // The sentence that was missing entirely, and its absence is
                // what let a GenBank molecule replace a 17-block `.dna` in
                // silence. It comes FIRST because it is the only one about
                // destroying something rather than about not copying it.
                ui.label(
                    RichText::new("A SnapGene file is already here, and this replaces it.")
                        .strong()
                        .color(pal(ui).warn),
                );
                ui.add_space(6.0);
            }
            if history {
                // Paraphrasing `from_molecule`'s own docstring, which is where
                // the refusal is argued. "Would be a fabrication" is the reason
                // and it survives into the UI on purpose.
                ui.label(RichText::new(
                    "The cloning history tree in this file is not carried. Polylinker keeps its \
                     own history and will not invent a SnapGene provenance node claiming a \
                     provenance this file does not have — that would be a fabrication. Writing \
                     here replaces that history with none.",
                ));
                ui.add_space(6.0);
            }
            if !unwritable.is_empty() {
                // The CLI's noun phrase, verbatim, same cap at three and same
                // "; " join: a user who has seen `pl convert --to dna` print
                // this line should recognise this one.
                ui.label(RichText::new(format!(
                    "{} item(s) the .dna writer could not carry: {}",
                    unwritable.len(),
                    unwritable
                        .iter()
                        .take(3)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("; ")
                )));
                ui.add_space(6.0);
            }
            if !notes.is_empty() {
                // The FILE TAB's wording for this fact, not the CLI's. It is the
                // same fact in the GUI's voice, and two different sentences for
                // one fact in one application is worse than either.
                ui.label(RichText::new(format!(
                    "{} part(s) of this file's notes block cannot be held by this model and are \
                     not shown: {}",
                    notes.len(),
                    notes.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
                )));
                ui.add_space(6.0);
            }
            // DERIVED from the blocks that are actually there, not a hardcoded
            // sentence. The old one named blocks 2, 3 and 7 and read as
            // exhaustive; measured on the user's own pKoV the synthesised file
            // also drops five block 11 history nodes (21 kB), block 8 (295 B)
            // and blocks 13/14/28 — 22 kB beyond the tree, none of it a cache
            // and none of it mentioned anywhere in the program.
            if let Some(s) = &losing {
                ui.label(RichText::new(format!(
                    "Not carried, and not rebuildable: {s}."
                )));
                ui.add_space(6.0);
            }
            ui.label(
                RichText::new(match &caches {
                    Some(s) => format!(
                        "The sequence, features, primers and notes are written. Not written \
                         because SnapGene rebuilds them: {s}."
                    ),
                    // No source and no destination container, so there is no
                    // cache to lose — the claim would be about a file that does
                    // not exist.
                    None => "The sequence, features, primers and notes are written.".to_string(),
                })
                .color(pal(ui).muted)
                .size(11.0),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                // The verb names the consequence when there is one. "Write
                // .dna" beside "a SnapGene file is already here" is the button
                // label doing none of the work the sentence above it just did.
                if ui
                    .button(if replacing {
                        "Replace the file"
                    } else {
                        "Write .dna"
                    })
                    .clicked()
                {
                    go = true;
                }
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if cancel {
            self.pending_dna = None;
            self.status = "nothing was written".into();
        } else if go {
            if let Some(p) = self.pending_dna.take() {
                self.write_dna(p);
            }
        }
    }
}

/// Are these two paths the same file?
///
/// Copied deliberately from `bins/pl/src/main.rs`'s `same_file` rather than
/// shared: `bins/pl-gui` cannot depend on `bins/pl`, and it cannot move into
/// `crates/` because `recover.rs` records that `pl-scan` is meant to be the
/// only crate doing I/O — and this canonicalises, which is I/O. The duplication
/// is therefore deliberate and findable rather than discovered.
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    if a == b {
        return true;
    }
    let real = |p: &std::path::Path| -> Option<PathBuf> {
        match std::fs::canonicalize(p) {
            Ok(c) => Some(c),
            // The destination may not exist yet, in which case the parent
            // directory is what can be resolved.
            Err(_) => {
                let dir = std::fs::canonicalize(p.parent().filter(|d| !d.as_os_str().is_empty())?)
                    .ok()?;
                Some(dir.join(p.file_name()?))
            }
        }
    };
    match (real(a), real(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Local date as (day, month index, year), without a date crate.
/// Howard Hinnant's civil-from-days, in UTC.
fn today() -> (u32, usize, i32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (d, (m - 1) as usize, y as i32)
}

impl eframe::App for App {
    /// Called on a normal quit. Removing the file is what records that the exit
    /// was clean, which is why nothing else needs to be written for recovery to
    /// work.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Before the recovery file goes, not after. Quitting is a durable
        // action like any other, and this one deletes the only copy: an open
        // run discarded here is gone from the log *and* from the file that
        // exists to survive exactly this.
        self.settle();
        // The absence of this file is what says the exit was clean. A user who
        // answered "close without saving" DID exit cleanly — and their work is
        // in here, and the dialog said so in as many words. Deleting it would
        // make that a lie, which is the one thing this whole guard exists to
        // stop.
        if let Some(p) = &self.recovery {
            if !self.abandoned_unsaved {
                recover::clear(p);
            }
        }
        // Once, here, and not on drag-release or per frame — that would be a
        // synchronous file write inside a paint loop. If the app crashes the
        // layout is lost, and that is the right trade: a window layout is not
        // the user's data.
        settings::save(self.layout);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // The OS title bar, which carried the literal "Polylinker" in every
        // capture in the review — no filename, no dirty marker, and several
        // instances all titled the same. Set from the same predicate the dot and
        // the guard read, so the three cannot disagree.
        {
            let want = match self.document() {
                Some(d) => format!(
                    "{}{} — Polylinker",
                    pl_fileio::caption_of(&d.title),
                    if d.unsaved() { " •" } else { "" }
                ),
                None => "Polylinker".to_string(),
            };
            if self.title_shown != want {
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(want.clone()));
                self.title_shown = want;
            }
        }

        self.close_request(&ctx);

        // Close the open typing run before anything else in this frame can
        // observe the document.
        //
        // While a run is open the log is deliberately one run behind the
        // screen — that is what buys 500 keystrokes for one operation instead
        // of five hundred, and 18 MB instead of a measured 197 MB. The cost is
        // that `log.current()` is briefly not what the user is looking at, and
        // *that* is the failure this call exists to bound: an autosave or an
        // export running mid-run would write a file missing the last forty
        // keystrokes.
        //
        // The condition is structural rather than a list of call sites: every
        // durable action in this application is reached by a pointer press or a
        // keyboard shortcut, and both settle the run here, before the frame's
        // widgets are built. What is left open is a burst of ordinary typing,
        // which closes on its own after `Run::IDLE_SECONDS`.
        let now = ctx.input(|i| i.time);
        let disturbed = ctx.input(|i| {
            i.pointer.any_pressed()
                || i.modifiers.command
                || i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::WindowFocused(false)
                            | egui::Event::Copy
                            | egui::Event::Cut
                            | egui::Event::Paste(_)
                    )
                })
        });
        let idle = self.edit.run().is_some_and(|r| r.is_idle(now));
        if disturbed || idle {
            self.settle();
        }
        // A run nothing else disturbs must still close on its own, and a
        // timeout with nothing to wake it never fires on an idle app.
        if let Some(r) = self.edit.run() {
            let left = seqedit::Run::IDLE_SECONDS - (now - r.last_input);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(left.max(0.0)));
        }

        self.autosave(false);

        // A THROTTLE WITH NOTHING TO WAKE IT NEVER FIRES.
        //
        // `autosave` declines when fewer than `AUTOSAVE_EVERY` seconds have
        // passed, and then nothing schedules the frame on which it would be due.
        // eframe waits for an event, so on an idle app there is no next frame:
        //
        //     14:00:00  an autosave lands
        //     14:00:17  a 1,240 bp deletion — one frame runs, autosave declines
        //     14:00:18  the user goes to lunch
        //     14:20:00  the machine loses power
        //
        // The deletion was never written anywhere. The throttle is right — a
        // write per keystroke is what it exists to prevent — but "the next frame
        // will come along" was an assumption, and the ONE case it fails is the
        // idle app, which is exactly the case a crash-recovery file is for.
        //
        // Same shape as the run-idle wake-up twelve lines above, which had to
        // learn this first: "a timeout with nothing to wake it never fires on an
        // idle app". Asked only while something is at risk, so a clean bench
        // still sleeps.
        if let Some(due) = self.autosave_due_in() {
            ctx.request_repaint_after(due);
        }

        if debug_geometry() {
            eprintln!(
                "geometry: root={:?} clip={:?} ppp={}",
                ui.max_rect(),
                ui.clip_rect(),
                ctx.pixels_per_point()
            );
        }

        // Files dropped anywhere on the window.
        //
        // A dropped *folder* indexes it. Before, a folder went to `load` and
        // came back as a read error, which is a poor answer to an unambiguous
        // gesture.
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(f) = dropped.first() {
            if let Some(path) = &f.path {
                if path.is_dir() {
                    self.scan = Some(library::start(path.clone()));
                    self.tab = Tab::Library;
                    self.error = None;
                } else {
                    self.load(path.clone());
                }
            } else if let Some(bytes) = &f.bytes {
                match Document::from_bytes(bytes, f.name.clone(), None) {
                    Ok(d) => {
                        let what = describe(d.molecule(), d.format);
                        self.take_over(d, what, None);
                    }
                    // Same rule as `load`'s Err arm: an unreadable payload is
                    // not a reason to destroy the document that IS open.
                    Err(e) => {
                        if self.document().is_some() {
                            self.notice = Some(e);
                        } else {
                            self.error = Some(e);
                        }
                    }
                }
            }
        }

        let keys = self.global_shortcuts(&ctx);
        if keys.open {
            self.pick_file();
        }
        if keys.undo {
            self.do_undo();
        }
        if keys.redo {
            self.do_redo();
        }
        if keys.save && self.document().is_some() {
            self.export(false);
        }

        // TAB NAVIGATION, behind the same guards as the other accelerators —
        // `asking()` and `text_edit_focused()` — so Ctrl+W typed into the
        // Features filter closes a word and not somebody's plasmid.
        //
        // Ctrl+W is unguarded on purpose: a closed tab keeps its document and
        // its undo history and Ctrl+Shift+T brings it back, so closing destroys
        // nothing. The question belongs at the one place work really goes away,
        // which is closing the window, and asking it twice is how a guard turns
        // into a reflex.
        if !self.asking() && !ctx.text_edit_focused() {
            let (cmd, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
            if cmd && ctx.input(|i| i.key_pressed(egui::Key::W)) && !self.bench.is_empty() {
                self.close_tab(self.bench.active());
            }
            if cmd && ctx.input(|i| i.key_pressed(egui::Key::Tab)) && self.bench.len() > 1 {
                let n = self.bench.len();
                let at = self.bench.active();
                let next = if shift {
                    (at + n - 1) % n
                } else {
                    (at + 1) % n
                };
                self.switch_tab(next);
            }
            if cmd && shift && ctx.input(|i| i.key_pressed(egui::Key::T)) {
                if let Some(t) = self.closed.pop() {
                    self.settle();
                    let v = self.take_view();
                    self.bench.store(v);
                    self.bench.reopen(t);
                    if let Some(v) = self.bench.take_active_view() {
                        self.put_view(v);
                    }
                    self.doc_generation = self.doc_generation.wrapping_add(1);
                }
            }
        }

        // The digest worker cannot wake the UI, so poll it and keep repainting
        // while it runs.
        let mut running = false;
        if let Some(d) = self.bench.get_mut() {
            if d.digest.poll() {
                ctx.request_repaint();
            }
            // Same contract, same worker shape. Polled here as well as in the
            // Sequence tab so a scan started on one tab lands even if the user
            // has walked away to another.
            if d.orfs.poll() {
                ctx.request_repaint();
            }
            running = d.digest.is_running() || d.orfs.is_running();
        }
        // The folder scan is the same shape and the same contract.
        if let Some(s) = &mut self.scan {
            if s.poll() {
                ctx.request_repaint();
            }
            running |= s.is_running();
        }
        // And so is every read's comparison. Polled here rather than in the
        // Reads tab so an answer lands whichever tab the user has walked away
        // to — otherwise a comparison started on one tab appears to hang until
        // you go back and look at it.
        let (changed, comparing) = self.refresh_reads();
        if changed {
            ctx.request_repaint();
        }
        running |= comparing;
        if running {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
        }

        // Last frame's answer becomes what both panels PAINT from; this
        // frame's is collected into `self.hot` and becomes next frame's. See
        // `App::hot_shown` for why one field could not do this.
        self.hot_shown = std::mem::take(&mut self.hot);
        self.top_bar(ui);
        self.tab_strip(ui);
        self.side_panel(ui);
        self.central(ui);
        self.paste_dialog(&ctx);
        self.clone_panel(&ctx);
        self.design_panel(&ctx);
        self.feature_editor(&ctx);
        // After the panels, so the question paints over the document it is
        // about. Lossiness first: it is raised BY the unsaved-changes modal's
        // save button, and `unsaved_modal` stands down while it is up so the
        // two are never stacked.
        self.dna_lossiness_modal(&ctx);
        self.unsaved_modal(&ctx);
        // The hover echo is one frame behind by construction, so ask for that
        // frame. Without it a pointer that comes to rest on a band can leave
        // the Features row unwashed until something unrelated requests a
        // repaint — which is the same invisible-hover symptom, arrived at from
        // the other direction. Only when the two disagree, so an idle window
        // still touches nothing.
        if self.hot != self.hot_shown {
            ctx.request_repaint();
        }
    }
}

/// Which of the four application-wide shortcuts fired this frame.
///
/// Deciding is separated from acting because the actions are a native file
/// dialog and a document mutation, and the *guards* are the part with a history
/// of being wrong.
///
/// Ctrl+Shift+S — "open the Save menu", for symmetry with the Ctrl+Shift+Z
/// already here — is deliberately **not** wired. egui has no supported way to
/// open a `menu_button`'s popup from a keystroke; doing it would mean writing
/// into the menu's private memory by id, and a shortcut that silently does
/// nothing when that id changes is worse than a shortcut that was never
/// advertised. Ctrl+S covers the frequent case and the format choice is one
/// click away.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Shortcuts {
    open: bool,
    undo: bool,
    redo: bool,
    /// Ctrl+S: save the molecule, defaulting to GenBank.
    ///
    /// The single most reflexive keystroke in any document application did
    /// nothing — there was no `egui::Key::S` anywhere in this binary, and four
    /// of the eight toolbar buttons had no shortcut and advertised none.
    ///
    /// Safe to bind despite writing a file: `export` goes through
    /// `rfd::FileDialog::save_file()`, so a stray Ctrl+S opens a dialog and can
    /// never silently overwrite anything. GenBank rather than FASTA because
    /// that is what the visible button has always done and what `pl convert`
    /// defaults to; the GUI and the CLI must not disagree about what "save"
    /// means.
    ///
    /// Two of the three guards, and deliberately not the third. It takes the
    /// pending-paste guard and the focused-widget guard — a Ctrl+S typed into the
    /// Features filter must not open a dialog any more than a Ctrl+Z there may
    /// reach the document. It does **not** take the design-panel guard, for the
    /// same reason `open` does not: that guard exists because an undo underneath
    /// the panel changes the bases the panel is describing, and saving changes
    /// nothing. Writing the file you are looking at while a primer report is open
    /// is a reasonable thing to want.
    ///
    /// Stated because the doc block above opens with "the *guards* are the part
    /// with a history of being wrong", and it read "inherits all three" while the
    /// field was computed from `cmd` rather than `edits`. Both existing guard
    /// tests now assert `save` as well, so the answer is pinned either way.
    save: bool,
}

impl App {
    /// Ctrl+O, Ctrl+Z and Ctrl+Y, and the three states in which they must not
    /// fire.
    ///
    /// These are read straight off the context rather than from a widget, and
    /// before a single widget has been built, so nothing downstream can take
    /// them back — no amount of modality anywhere would stop them.
    ///
    /// - **A paste is waiting on an answer.** Undoing while the question is on
    ///   screen changes the document the question is about.
    /// - **A widget has keyboard focus.** `TextEdit` handles Ctrl+Z itself and
    ///   reads its events with the non-consuming `filtered_events`, so both
    ///   handlers used to fire: Ctrl+Z after a typo in the Features filter or
    ///   the Library query undid the typo *and* the molecule, reverting a
    ///   circularisation, clearing the selection and respawning the 58-enzyme
    ///   digest — all of which the user attributes to the text box. Ctrl+O
    ///   popped a file dialog out of a search box for the same reason.
    ///   `sequence_keys` has guarded exactly this since it was written; this
    ///   block did not.
    /// - **The design panel is open**, for undo and redo only. Same rule as
    ///   `sequence_keys`' third guard: the panel snapshots the target it is
    ///   answering about, so an undo underneath it leaves the panel describing
    ///   bases that are no longer there. The panel refuses to write a stale
    ///   report either way, but a report that silently stops being addable
    ///   because of a stray keystroke is a poor answer to a question the user
    ///   is still looking at. Undo stays reachable from the toolbar, which
    ///   closes nothing and surprises nobody.
    fn global_shortcuts(&self, ctx: &egui::Context) -> Shortcuts {
        let asking = self.asking();
        // "A TEXT BOX HAS THE KEYS", not "something has focus".
        //
        // This was `ctx.memory(|m| m.focused()).is_some()`, and elsewhere in
        // this file the reasoning is written out as "`Button` never takes
        // keyboard focus". That is false in egui 0.35: `Button` is built with
        // `.sense(Sense::click())` and `Sense::click()` is `CLICK | FOCUSABLE`,
        // so every button in the toolbar is in the tab order. One Tab, and
        // Ctrl+Z, Ctrl+Y, Ctrl+O and Ctrl+S were dead for the rest of the
        // session with nothing on screen to say why. The map was a shorter path
        // to the same place — see `Sense::CLICK` there.
        //
        // `text_edit_focused` is not a synonym for the old call: it asks
        // whether the focused id has `TextEditState` behind it, so a focused
        // *button* answers no and a focused text box answers yes. That is the
        // distinction the guard always meant to draw, and note that
        // `egui_wants_keyboard_input()` is NOT it — that function is literally
        // `m.focused().is_some()`, the predicate being replaced.
        //
        // The stand-down for real text boxes stays exactly as it was:
        // `a_shortcut_typed_into_a_focused_text_box_does_not_reach_the_document`
        // is the reason it exists, and Ctrl+Z in the Features filter must still
        // undo the typo rather than the molecule.
        let typing = ctx.text_edit_focused();
        // The feature editor is guarded for exactly the design panel's reason,
        // and a sharper one: it holds an INDEX. An undo underneath it can shift
        // every index (`RemoveFeature`) or drop a feature outright (the
        // annotation remap), after which Save writes over a different feature.
        // The panel's own `stale_reason` refuses that either way; a form that
        // silently stops being savable because of a stray keystroke is still a
        // poor answer to a question the user is looking at.
        let designing = self.design.is_some() || self.feature_edit.is_some();
        if asking || typing {
            return Shortcuts::default();
        }
        // Ctrl+Z / Ctrl+Y, plus Ctrl+Shift+Z for the mac-shaped habit.
        ctx.input(|i| {
            let cmd = i.modifiers.command;
            let edits = cmd && !designing;
            Shortcuts {
                open: cmd && i.key_pressed(egui::Key::O),
                undo: edits && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                redo: edits
                    && (i.key_pressed(egui::Key::Y)
                        || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                // Decided here, with the other three, so it inherits all three
                // guards. A Ctrl+S handled at a widget would reintroduce
                // exactly what the `typing` guard exists to stop, and the
                // symptom — a save dialog opening mid-word while renaming a
                // feature — would be blamed on the text box.
                save: cmd && !i.modifiers.shift && i.key_pressed(egui::Key::S),
            }
        })
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        egui::Panel::top(egui::Id::new("toolbar")).show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                // Three runs, separated: what goes in and out as a molecule,
                // what goes out as a picture, and the open document.
                //
                // The row was eight buttons spanning three unrelated jobs with
                // nothing between them, and it did not fit: measured on the
                // user's own file the buttons alone run to 467 pt and the bar's
                // natural width is about 940, against a `min_inner_size` of
                // 880. At the smallest size the app will let you make the
                // window, the status line was clipped mid-word to "SnapGene
                // .dna · 8,117 bp · cir" — the first thing to go being the
                // topology, which is the most consequential fact about a
                // plasmid. So a separator between the four middle buttons was
                // not available: each one is 9 pt in the wrong direction.
                //
                // Collapsing the format choice one level down instead makes the
                // distinction *lexical* — "Save" takes the molecule out,
                // "Export map" takes a picture out — which survives a
                // monochrome screenshot, a screen reader and a narrow window,
                // none of which a separator does. It also takes the run from
                // 467 pt to about 323.
                //
                // AND 34.4 PT OF THAT SAVING WAS SPENT AGAIN on 2026-07-30, on
                // the Undo and Redo icons: two glyphs at 1.000 em of 13 pt plus
                // an `icon_spacing` each. Measured off the running app rather
                // than predicted — the title block moved 43 physical px right at
                // 1.25 scale — so the run is about 357 pt and the bar's natural
                // width about 975. That is affordable and it is not free, and
                // `the_toolbar_stays_inside_the_window_however_long_the_status_is`
                // is what says so at the 880 pt minimum rather than this comment.
                //
                // Undo and Redo stay visible buttons and are deliberately not
                // folded into a menu: `global_shortcuts` switches Ctrl+Z and
                // Ctrl+Y off while the design panel is open, and that decision
                // ends "Undo stays reachable from the toolbar, which closes
                // nothing and surprises nobody".
                //
                // Ellipsis discipline: "…" means "this opens a file dialog".
                // The menu buttons do not carry one; the leaf items that reach
                // `rfd::FileDialog` do.
                //
                // The three menus now carry a disclosure caret, and the nouns
                // still do the work. Both channels, not one: the word says which
                // menu, the caret says it is a menu at all.
                //
                // It was tried as a GLYPH at 0ebaa41 and photographed failing.
                // U+25BE is in none of the embedded faces and came out an empty
                // box on all three — the same trap `strand_word` documents for
                // U+2190 in the proportional face. The conclusion recorded here
                // was "no caret", and it was the wrong one: a triangle is three
                // points, and `menu_with_caret` paints them. Nothing about that
                // failure was about the caret; it was about asking a font for it.
                //
                // The nouns are kept for the reason they were chosen. "Edit" — a
                // verb, and the platform's name for a menu holding Undo and Redo
                // — still could not come back, because it named the wrong thing
                // and the caret does not fix a wrong word.
                if ui.button("Open…").on_hover_text("Ctrl+O").clicked() {
                    self.pick_file();
                }
                let has = self.document().is_some();
                ui.add_enabled_ui(has, |ui| {
                    menu_with_caret(ui, "Save", |ui| {
                        // First, above GenBank: the list runs in descending
                        // fidelity and for a user who opens `.dna` all day this
                        // is the top of it. The hover matches FASTA's register
                        // below — what it keeps, then what it does not — and the
                        // ellipsis is the documented signal that this reaches a
                        // file dialog.
                        if ui
                            .button("SnapGene .dna…")
                            .on_hover_text(
                                "features, primers and notes; not the cut-site cache or the \
                                 cloning history",
                            )
                            .clicked()
                        {
                            self.save_dna(None);
                            ui.close();
                        }
                        if ui.button("GenBank…").clicked() {
                            self.export(false);
                            ui.close();
                        }
                        if ui
                            .button("FASTA…")
                            .on_hover_text("bases only: no features, no topology")
                            .clicked()
                        {
                            self.export(true);
                            ui.close();
                        }
                    })
                    .response
                    // Was "Ctrl+S — save the molecule", which was unambiguous
                    // with two items and is not with three. Ctrl+S deliberately
                    // still means GenBank: silently repointing a shortcut at a
                    // different format is its own defect. The ambiguity is
                    // created by adding `.dna`, so it is answered here.
                    .on_hover_text("Ctrl+S saves GenBank");
                });

                ui.separator();
                ui.add_enabled_ui(has, |ui| {
                    // "Map" is kept, and it is only now honest. Beside a map on
                    // screen "Map SVG…" reads as "save what I am looking at",
                    // and until this change it was not: the exported figure had
                    // no restriction sites on it at all and said "unnamed" in
                    // the middle. It now carries the same sites and the same
                    // caption, laid out by the same `pl_draw::ring`, so the word
                    // describes the file the user gets. It still is not
                    // pixel-for-pixel the screen — `pl-draw` puts every feature
                    // on one ring and carries strand in the arrowhead, where the
                    // map stacks lanes inside and outside the backbone.
                    //
                    // "Export figure", not "Export map", because there are two
                    // pictures now and the menu title cannot name both. THE
                    // SUBJECT IS IN THE LEAF — "Map as SVG…" / "Gel as SVG…" —
                    // for the reason this toolbar already argues about its
                    // other labels: a lexical distinction survives a monochrome
                    // screenshot, a screen reader and a narrow window, and
                    // "Export map" with a gel on screen would simply be false.
                    // A fourth top-level button was measured and rejected: the
                    // row was 467 pt against an 880 pt `min_inner_size` before
                    // the formats were collapsed into menus, and reopening that
                    // for ~60 pt is not a trade worth making.
                    let showing_gel = self.central_view == CentralView::Gel;
                    let subject = if showing_gel { "Gel" } else { "Map" };
                    menu_with_caret(ui, "Export figure", |ui| {
                        for (fmt, why) in [
                            (Fmt::Svg, "Vector, for a figure"),
                            (Fmt::Pdf, "The same picture, for a manuscript"),
                            (Fmt::Eps, "For a journal that asks for EPS"),
                        ] {
                            if ui
                                .button(format!("{subject} as {}…", fmt.name()))
                                .on_hover_text(why)
                                .clicked()
                            {
                                match (showing_gel, fmt) {
                                    (true, f) => self.export_gel(f),
                                    (false, Fmt::Svg) => self.export_svg(),
                                    (false, Fmt::Pdf) => self.export_pdf(),
                                    (false, Fmt::Eps) => self.export_map_eps(),
                                }
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text(if showing_gel {
                        "the gel on screen as a picture"
                    } else {
                        "the plasmid map as a picture"
                    });
                });

                ui.separator();
                self.edit_group(ui);

                ui.separator();
                // The right edge is allocated FIRST, and the title block gets
                // what is left.
                //
                // Laid out the other way round the theme switch was painted on
                // top of the status text: at 912 pt the final "r" of "circular"
                // sat under the sun glyph, and at the app's own 880 pt
                // `min_inner_size` the switch had left the window entirely
                // while the status read "SnapGene .dna · 8,117 bp · cir". They
                // were not competing for the space, they were both taking it.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::global_theme_preference_switch(ui);
                    if let Some(d) = self.document() {
                        if d.digest.is_running() {
                            ui.add(egui::Spinner::new().size(13.0));
                            ui.label(RichText::new("digesting").color(pal(ui).muted).size(12.0));
                        }
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        if let Some(d) = self.document() {
                            // A dot rather than the usual asterisk-in-the-title.
                            //
                            // It used to mean "edits exist and are undoable,
                            // not that a file is dirty — nothing here writes
                            // over the original", and that comment stopped
                            // being true the moment Save appeared in this very
                            // menu. `edited()` is `!all_ops().is_empty()`, which
                            // is true forever after the first keystroke: after
                            // an undo back to the base, and after a save. The
                            // dot now means what a dot means, and the
                            // unsaved-changes guard reads the same predicate, so
                            // the dot and the dialog cannot disagree.
                            let marker = if d.unsaved() { " •" } else { "" };
                            // What gives under pressure, and in which order:
                            // the FILENAME, because the whole path is one hover
                            // away; never the state string, which is short,
                            // fixed for a given file, and the thing the user is
                            // reading; never the edited marker, because losing
                            // the signal that there are unsaved edits is not a
                            // cosmetic loss. This used to be exactly backwards.
                            // And the state gives too, once the filename has gone
                            // to nothing.
                            //
                            // "Never the state string" was right as a *priority*
                            // and wrong as a guarantee: it was the overflow
                            // source. `FASTA keeps only the bases; this drops 9
                            // feature(s) and the topology (it will reopen as
                            // linear)  —  wrote <150-character path>` is 230
                            // characters, and an un-elided label simply runs off —
                            // measured at 1,712 pt in an 880 pt window, through
                            // the theme switch and out of the window, cut
                            // mid-token by egui's own galley truncation with no
                            // ellipsis and no hover, so the reader cannot even
                            // tell something is missing. Second in line and
                            // hoverable is the honest form of "last to give".
                            //
                            // Both budgets are taken from ONE reading of
                            // `available_width`, before either label is drawn.
                            // Asking again afterwards reports about zero — the
                            // inner left-to-right layout inside a right-to-left
                            // parent has no width left to advertise — so the
                            // second call elided the status to nothing and the bar
                            // went blank. Same shape as the whole
                            // decide-in-one-unit-draw-in-another family: measure
                            // once, spend twice.
                            let state = self.status.clone();
                            // From the RECT the parent left for this block, not
                            // from `available_width()`. Inside a right-to-left
                            // layout the nested left-to-right `Ui` reports about
                            // zero available width whatever is actually free, so
                            // `room` was non-positive on every frame — which is
                            // how `elide`'s `room <= 0` branch came to be the one
                            // that mattered, and why it looked harmless in a wide
                            // window where the label happened to fit anyway.
                            let total = (ui.max_rect().width() - 8.0).max(0.0);
                            let state_w = text_width(ui, &state, 12.0).min(total);
                            let room = total - state_w - 12.0;
                            let name = elide(ui, &format!("{}{marker}", d.title), room);
                            let title = ui.label(RichText::new(name).strong());
                            if let Some(p) = &d.path {
                                title.on_hover_text(p.display().to_string());
                            }
                            let shown = elide_at(ui, &state, state_w, 12.0);
                            let lbl = ui.label(
                                RichText::new(shown.clone()).color(pal(ui).muted).size(12.0),
                            );
                            if shown != state {
                                lbl.on_hover_text(&state);
                            }
                        } else {
                            ui.label(
                                RichText::new(
                                    "Open a .dna, GenBank or FASTA file, or drop one here",
                                )
                                .color(pal(ui).muted),
                            );
                        }
                    });
                });
            });
            ui.add_space(4.0);
        });
    }

    /// The history controls, and the operations on the molecule as a whole.
    ///
    /// Every one of these goes through the operation log, so every one is
    /// undoable and every one shows up in the History tab. There is no other
    /// path that mutates a molecule.
    ///
    /// The doc used to read "Undo, redo, and the edits that need no selection",
    /// which two of the four menu items contradict: "Set origin at selected
    /// feature" and "Remove selected feature" both need one.
    ///
    /// The menu's own name and the origin item's are [`MOLECULE_MENU`] and
    /// [`SET_ORIGIN_ITEM`], not literals — see those.
    fn edit_group(&mut self, ui: &mut Ui) {
        let (can_undo, can_redo) = match self.document() {
            Some(d) => (d.log.can_undo(), d.log.can_redo()),
            None => (false, false),
        };

        // The one pair in this bar where an icon says something the word does
        // not: mirrored arrows carry DIRECTION, which is pre-attentive, where
        // "Undo" and "Redo" differ by one interior letter at the same weight in
        // adjacent identical buttons. The words stay, and stay first in the
        // accessible name. See `button_with_icon` for why the glyph is painted
        // rather than passed as text, and for the two controls that got one and
        // the several that deliberately did not.
        ui.add_enabled_ui(can_undo, |ui| {
            if button_with_icon(ui, ICON_UNDO, "Undo")
                .on_hover_text("Ctrl+Z")
                .clicked()
            {
                self.do_undo();
            }
        });
        ui.add_enabled_ui(can_redo, |ui| {
            // Ctrl+Shift+Z has been wired since the shortcut block was written
            // and was advertised nowhere.
            if button_with_icon(ui, ICON_REDO, "Redo")
                .on_hover_text("Ctrl+Y, or Ctrl+Shift+Z")
                .clicked()
            {
                self.do_redo();
            }
        });

        // Which is where the history controls end.
        //
        // The POSITION complaint below is only answered by this line. Renaming
        // "Edit" to "Molecule" fixed the noun and left the control the third
        // identical rounded button in a run of three, with no separator and — the
        // caret having rendered as an empty box — nothing but the word to say it
        // is a dropdown. Two of the three faults the comment levels at "Edit"
        // shipped unchanged. The row has the room: it fits at the app's own
        // 880 pt minimum with slack.
        ui.separator();

        let has = self.document().is_some();
        ui.add_enabled_ui(has, |ui| {
            // "Edit" was wrong three times over, which is why it read as
            // ambiguous next to Undo and Redo.
            //
            // Its POSITION lied: no separator between it and two history
            // controls, and the same shape as both, so it read as a third
            // immediate verb or a read-only toggle. It is neither — it is a
            // dropdown. Answered by the `ui.separator()` above.
            //
            // Its NOUN lied: nothing behind it edits bases. Insert, delete,
            // replace and paste — what a biologist means by editing a plasmid —
            // are in the Sequence tab. A user hunting for "where do I edit the
            // sequence" clicked here and was sent the wrong way, which is worse
            // than an unlabelled control.
            //
            // And it COLLIDED with the platform: on Windows "Edit" is the menu
            // holding Undo, Redo, Cut, Copy and Paste. This one sat next to
            // Undo and Redo and contained none of them.
            //
            // Every item acts on the whole molecule or on the selected feature,
            // and "Molecule" is the word `pl_core`, `pl info` and the rest of
            // this file already use for that object. Not "Sequence", "Features"
            // or "File": those are tab names.
            menu_with_caret(ui, MOLECULE_MENU, |ui| {
                let circular = self
                    .document()
                    .is_some_and(|d| d.molecule().topology.is_circular());
                let label = if circular {
                    "Make linear"
                } else {
                    "Make circular"
                };
                if ui.button(label).clicked() {
                    let t = if circular {
                        pl_core::Topology::Linear
                    } else {
                        pl_core::Topology::Circular
                    };
                    self.edit(pl_core::OpKind::SetTopology(t));
                    ui.close();
                }
                if ui
                    .button("Reverse complement")
                    .on_hover_text("flips the sequence and every annotation with it")
                    .clicked()
                {
                    self.edit(pl_core::OpKind::ReverseComplement);
                    ui.close();
                }

                // Rotating is only meaningful on a circle, and only useful
                // when the user has said where the new origin should be.
                let sel = self.selected;
                let can_rotate = circular && sel.is_some();
                ui.add_enabled_ui(can_rotate, |ui| {
                    if ui
                        .button(SET_ORIGIN_ITEM)
                        .on_hover_text("renumber the plasmid to start at this feature")
                        .clicked()
                    {
                        if let (Some(d), Some(i)) = (self.document(), sel) {
                            let m = d.molecule();
                            if let Some(f) = m.features.get(i) {
                                // `Feature::start` is the MINIMUM of the segment
                                // starts, and an origin-crossing feature in the
                                // join form `genbank::write` emits —
                                // `join(2677..2686,1..7)` — always has a part
                                // beginning at base 1. So this was always
                                // `Rotate { origin: 1 }`, which hits
                                // `if shift == 0 { return true; }`: the button
                                // reported success, pushed an undo entry and
                                // dirtied the document while renumbering
                                // nothing. Worse, the one feature a user most
                                // wants to rotate to the front is exactly the
                                // one that straddles the origin.
                                let origin = f
                                    .extent(m.span(), m.topology.is_circular())
                                    .map(|(s, _)| s)
                                    .unwrap_or_else(|| f.start());
                                self.edit(pl_core::OpKind::Rotate { origin });
                            }
                        }
                        ui.close();
                    }
                });

                ui.separator();
                // The keyboard and screen-reader path to the feature editor.
                // The button in the Features tab, the double-click on the map
                // and this item are three routes to one panel; a menu is the
                // only one of the three a keyboard user can reach.
                if ui
                    .button(ADD_FEATURE_ITEM)
                    .on_hover_text("uses the sequence selection when there is one")
                    .clicked()
                {
                    self.open_feature_editor(None);
                    ui.close();
                }
                ui.add_enabled_ui(sel.is_some(), |ui| {
                    if ui.button(EDIT_FEATURE_ITEM).clicked() {
                        self.open_feature_editor(sel);
                        ui.close();
                    }
                });
                ui.separator();
                // Restriction cloning. Seeded from whatever is ticked for the
                // gel, because somebody who has just looked at a digest is
                // asking about that digest.
                if ui
                    .button(CLONE_ITEM)
                    .on_hover_text(
                        "cut this molecule and see which fragments religate; a product opens \
                         as a new unsaved document",
                    )
                    .clicked()
                {
                    self.clone_panel = Some(clone::Panel::new(&self.gel.picked));
                    ui.close();
                }
                ui.add_enabled_ui(sel.is_some(), |ui| {
                    if ui.button("Remove selected feature").clicked() {
                        if let Some(i) = sel {
                            self.remove_feature(i);
                        }
                        ui.close();
                    }
                });
            });
        });
    }

    /// Commit whatever the user has typed but not yet paid for.
    ///
    /// The one place a pending run becomes an operation. Every path that reads
    /// the document for a durable purpose goes through here first.
    fn settle(&mut self) {
        let Some(d) = self.bench.get_mut() else {
            return;
        };
        if self.edit.run().is_none() {
            return;
        }
        // Only a genuine commit failure is promoted to the strip above the map.
        // The Sequence tab's own transient line — "'Z' is not a nucleotide" —
        // belongs under the sequence and must survive the next click.
        //
        // That sentence was in this comment before `commit` returned anything,
        // and the code could not honour it: `commit` writes into `edit.notice`
        // on SUCCESS too, via `feature_loss`, and the old `match
        // self.edit.notice` promoted that to `self.notice` as if it were a
        // failure. So typing over a feature already reported above the map,
        // contrary to the stated rule. With the returned `OpKind` the code
        // finally does what the comment says: a feature destroyed by typing
        // reports under the sequence only, and the status simultaneously names
        // the run, so both channels carry something and neither is silent.
        let held = self.edit.notice.take();
        let applied = self.edit.commit(d);
        match (applied, self.edit.notice.clone()) {
            // Landed. The status has to be assigned HERE and not only in
            // `edit()`, or typing, Backspace and Delete — the three edits that
            // actually change bases — leave the bar naming whatever discrete
            // action came before. After Ctrl+A then Delete the toolbar read
            // "add feature UX probe feature — Ctrl+Z to undo" beside a molecule
            // that no longer had any bases in it.
            (Some(kind), _) => {
                self.status = format!("{} — Ctrl+Z to undo", kind.describe());
            }
            // Refused. THIS is the "genuine commit failure" the comment above
            // has always claimed to be selecting for, and could not.
            (None, Some(failed)) => self.notice = Some(failed),
            (None, None) => self.edit.notice = held,
        }
        // Deliberately NOT clearing `self.notice` on success, which is where
        // this differs from `edit()`. `settle()` runs on paths `edit()` never
        // does — before an undo, before a save, on focus loss — and wiping a
        // notice the user has not read yet would be a regression this code does
        // not currently have.
    }

    /// Run an edit and report a refusal instead of dropping it.
    ///
    /// Returns whether it went in. Most callers issue one operation and the
    /// `notice` is the whole answer, but a gesture that is two operations has
    /// to know which of them landed: "Ctrl+Z twice to undo both" after only one
    /// took undoes the user's previous, unrelated edit as well.
    fn edit(&mut self, kind: pl_core::OpKind) -> bool {
        self.settle();
        let Some(d) = self.bench.get_mut() else {
            return false;
        };
        let what = kind.describe();
        let n_before = d.molecule().len();
        match d.apply(kind.clone()) {
            Ok(()) => {
                self.status = format!("{what} — Ctrl+Z to undo");
                self.notice = None;
                // The bases moved under the caret, so move the caret with them.
                // A caret that survives an edit still pointing at the base it
                // named before is how an editor rots. Selections are collapsed
                // on Rotate and on Circular->Linear because the arc they name
                // may no longer exist.
                self.edit.caret = seqedit::transport(self.edit.caret, &kind, n_before);
                // ...but an annotation-only edit moves no bases, so the arc the
                // selection names is exactly where it was.
                //
                // This was unconditional, justified for the ops that move bases
                // — "Selections are collapsed on Rotate and on Circular->Linear
                // because the arc they name may no longer exist." `SetFeature`
                // and `RemoveFeature` move nothing; `transport` already knows
                // that and returns the caret untouched for both. Under the old
                // rule the first thing a SnapGene user does with the feature
                // editor misfired: select 900 bp, add the CDS, and the highlight
                // vanished on a molecule where nothing had moved, so the
                // promoter and the RBS that come next had to be re-dragged by
                // hand at 60 bases a row. That is how a feature ends up starting
                // one base late, and there is no gate for off-by-one —
                // `validate()` has nothing to say about a CDS at 1975.
                if !matches!(
                    &kind,
                    pl_core::OpKind::SetFeature { .. } | pl_core::OpKind::RemoveFeature { .. }
                ) {
                    self.edit.sel = None;
                }
                self.edit.remember(d);
                true
            }
            // The log refuses an edit that would leave the annotations
            // describing something the sequence does not contain. Saying which
            // edit and why is the whole point of refusing rather than
            // corrupting — and it goes to `notice`, with the document still on
            // screen, not to the "could not read that file" takeover.
            Err(e) => {
                self.notice = Some(format!("Cannot {what}: {e}.\nNothing was changed."));
                false
            }
        }
    }

    fn do_undo(&mut self) {
        self.settle();
        // An edit across the origin is a rotate and then a range op, and both
        // have to go. Stepping back over the range op alone leaves a whole,
        // plausible plasmid whose every coordinate has moved — worse than an
        // incomplete undo, because there is nothing on screen to say so.
        let pair = self
            .document()
            .and_then(|d| self.edit.undo_over_pair(d.log.cursor()));
        if let Some(d) = self.bench.get_mut() {
            let done = match pair {
                Some(before) => d
                    .seek(before)
                    .map(|()| "undone — the origin was put back too"),
                None => d.undo().map(|()| "undone"),
            };
            match done {
                Ok(what) => {
                    self.status = what.into();
                    self.notice = None;
                }
                Err(e) => self.notice = Some(e.to_string()),
            }
            self.edit.restore(d);
            self.selected = None;
        }
    }

    fn do_redo(&mut self) {
        self.settle();
        if let Some(d) = self.bench.get_mut() {
            match d.redo() {
                Ok(()) => {
                    self.status = "redone".into();
                    self.notice = None;
                }
                Err(e) => self.notice = Some(e.to_string()),
            }
            // Both halves, the same way round.
            if let Some(tail) = self.edit.redo_over_pair(d.log.cursor()) {
                if d.seek(Some(tail)).is_ok() {
                    self.status = "redone — the plasmid is renumbered again".into();
                }
            }
            self.edit.restore(d);
            self.selected = None;
        }
    }

    /// The details panel never narrower than this.
    ///
    /// 300 is the 30-bases-per-row threshold, and the lowest width at which the
    /// Features list's right-aligned coordinate column ("7,748..7,850 ←") stops
    /// colliding with the names.
    const MIN_PANEL: f32 = 300.0;
    /// And never wider than the window less this.
    ///
    /// egui's only cap is `min(max, available_rect.width)` — it stops the panel
    /// overflowing the *window*, not overrunning the CentralPanel. With no
    /// maximum of our own the map pane goes to zero: `map::show` takes a
    /// zero-width rect, `pl_draw::ring::radius` bottoms out at its 40 pt floor,
    /// and everything is painted into a zero-width clip rect. The map silently
    /// vanishes with nothing on screen explaining why. At 360 the map is a token
    /// circle — poor to read and obviously *there*, which is the point of a stop.
    ///
    /// The floor no longer has a second job. It used to also be where "the 132 pt
    /// label reserve exceeds the box and the leader lines are drawn outside it"
    /// started, and there is no 132: `map.rs` computes the reserve from the widest
    /// label that will land in a side column, so it shrinks with the pane instead
    /// of overrunning it, and the labels it cannot hold whole are shortened and
    /// counted in the line under the caption. 360 is now only "small enough to be
    /// a thumbnail, large enough not to look broken".
    const MIN_MAP: f32 = 360.0;
    /// Where the splitter sits until the user moves it.
    ///
    /// The smallest width that reaches the GenBank 60-base row with metric
    /// headroom, and not one point more.
    ///
    /// Not a taste question, and the reason has changed. The width this does not
    /// take is width the map pane keeps.
    ///
    /// It used to be about a *fixed* 132 pt reserve, which bound only while the
    /// pane was narrower than it was tall — `min(w, h) / 2 - 132` — and below that
    /// line gave each side exactly 132 and rendered "EcoRI 7,530" as
    /// "coRI 7,530", a truncation that reads as a different enzyme rather than as
    /// damage. 560 crossed it on the user's own 1296 x 879 window and clipped
    /// seven labels on both sides. There is no 132 any more:
    /// `pl_draw::ring::reserve_for` derives the reserve from the widest label that
    /// will actually land in a side column, `ring::radius` charges it to the
    /// *width* and the row strip to the *height*, and what still will not fit is
    /// shortened by `ring::label_room` and counted in the line under the caption.
    /// So a narrow pane now costs radius and states what it cost, instead of
    /// cutting the front off a name.
    ///
    /// What is left of the argument, and it is still an argument: a wider panel is
    /// a smaller ring, a smaller ring is a shorter `label_room`, and a shorter
    /// `label_room` is more names shortened. The default is therefore the 60-base
    /// threshold plus a small margin and not one point more, and the threshold
    /// itself was moved down by measuring the coordinate gutter from the molecule
    /// instead of reserving nine digits for every plasmid (see
    /// [`seqedit::gutter_w`]). `the_default_split_has_headroom_and_leaves_the_map_square`
    /// pins both halves: 60 bases at the default and at the default less 10, and a
    /// map pane at least as wide as it is tall.
    const DEF_PANEL: f32 = 500.0;

    fn side_panel(&mut self, ui: &mut Ui) {
        // Recomputed every frame from the live window, which is what makes one
        // clamp cover three situations identically: a width restored from disk
        // that is wider than this monitor, a width being dragged now, and a
        // window resized smaller after the fact.
        //
        // `.max(MIN_PANEL)` is the tie-break when the client is too narrow to
        // hold both floors, and it is deliberate rather than accidental: below
        // `MIN_PANEL + MIN_MAP` = 660 pt the panel wins and the map gets less
        // than `MIN_MAP`. The panel is the functional surface — the tabs, the
        // sequence, the file — and it degrades gracefully, `fit_per_row`
        // bottoming out at ten bases a row. A map under 344 pt only has its
        // leader lines clipped by the pane's own clip rect, which is cosmetic.
        // `min_inner_size` is 880 x 560, so a window manager honouring it never
        // reaches this; `SetWindowPos` ignores it, and the expression has to
        // stay valid — `Rangef::new(lo, hi)` requires `lo <= hi` — when it does.
        let max_panel = (ui.available_width() - Self::MIN_MAP).max(Self::MIN_PANEL);
        if debug_geometry() {
            eprintln!(
                "geometry: before panel avail={:?} stored={:?} max_panel={max_panel}",
                ui.available_rect_before_wrap(),
                self.layout.panel_w
            );
        }
        // ORDER MATTERS, and getting it wrong ships silently. `default_size(d)`
        // *widens* the range to include `d`; `size_range(r)` *clamps* the
        // stored default into `r`. So `.default_size(w).size_range(lo..=hi)`
        // clamps a restored 1,900 into the range, and the other order lets it
        // blow the maximum open.
        let r = egui::Panel::right(egui::Id::new("details"))
            .resizable(true)
            .default_size(self.layout.panel_w.unwrap_or(Self::DEF_PANEL))
            .size_range(Self::MIN_PANEL..=max_panel)
            .show(ui, |ui| {
                ui.add_space(6.0);
                // Wrapped, not `horizontal`, which clips rather than wraps: at
                // panel widths below about 357 the "File" tab is painted
                // outside the clip rect and becomes unclickable. Without this
                // the minimum could not go below 360 and the splitter could not
                // be used to give the *map* more room, which is half the point.
                ui.horizontal_wrapped(|ui| {
                    for (tab, label) in [
                        (Tab::Features, "Features"),
                        (Tab::Library, "Library"),
                        (Tab::Enzymes, "Enzymes"),
                        (Tab::Sequence, "Sequence"),
                        (Tab::Reads, "Reads"),
                        (Tab::History, "History"),
                        (Tab::File, "File"),
                    ] {
                        if ui.selectable_label(self.tab == tab, label).clicked() {
                            self.tab = tab;
                        }
                    }
                });
                ui.separator();

                // The guard is INSIDE the dispatch, and that is the whole
                // fix. It used to sit above it and returned "Nothing open."
                // before the match ran, so it swallowed `Tab::Library` with the
                // other five — while the tab strip is drawn above it, so the
                // user could select the tab and be told the one thing it does
                // not need.
                match self.tab {
                    // The one tab whose whole state is its own — `self.scan`,
                    // `self.lib_mode`, `self.lib_query`, `self.lib_absent` —
                    // and which needs no molecule to answer its question. It is
                    // also the only cross-file search in the app, and there is
                    // no in-document search at all, so it is the only sequence
                    // search that exists. "Where did I put that plasmid?" is
                    // the first thing asked, and it was asked in exactly the
                    // state where the feature that answers it refused.
                    Tab::Library => self.library_tab(ui),
                    // The second tab that answers with no molecule open, and
                    // for a different reason: `pl trace file.ab1` already
                    // answers "let me look at my trace" with no reference at
                    // all, and the GUI refusing what the CLI does would be a
                    // fifth instance of the gap this whole direction exists to
                    // close. What it must NOT do is pretend: with no reference
                    // there is no pair, and every number in a comparison is a
                    // number about a pair.
                    Tab::Reads => self.reads_tab(ui),
                    // After the named arms and before the rest. The compiler
                    // will say so if another tab is added, which is the
                    // property worth having: the five below all open with
                    // `expect("checked by caller")`.
                    _ if self.document().is_none() => {
                        ui.add_space(20.0);
                        ui.label(RichText::new("Nothing open.").color(pal(ui).muted));
                    }
                    Tab::Features => self.features_tab(ui),
                    Tab::Enzymes => self.enzymes_tab(ui),
                    Tab::Sequence => self.sequence_tab(ui),
                    Tab::History => self.history_tab(ui),
                    Tab::File => self.file_tab(ui),
                }
            });
        // Captured every frame because `on_exit` receives no `Context` and must
        // not reach into `egui::containers::PanelState`.
        self.layout.panel_w = Some(r.response.rect.width());
    }

    fn library_tab(&mut self, ui: &mut Ui) {
        use library::{Mode, Parsed, ScanState};

        ui.horizontal(|ui| {
            if ui
                .button("Open folder…")
                .on_hover_text("Index a folder of sequence files. Dropping a folder works too.")
                .clicked()
            {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.scan = Some(library::start(dir));
                }
            }
            if let Some(s) = &self.scan {
                match s {
                    ScanState::Running { root, .. } => {
                        ui.spinner();
                        ui.label(
                            RichText::new(format!("scanning {}…", root.display()))
                                .color(pal(ui).muted)
                                .size(12.0),
                        );
                    }
                    ScanState::Done { root, .. } => {
                        ui.label(
                            RichText::new(root.display().to_string())
                                .color(pal(ui).muted)
                                .size(12.0),
                        );
                    }
                    ScanState::Failed(_) => {}
                }
            }
        });

        let Some(state) = &self.scan else {
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "No folder yet.\n\nOpen or drop one and every sequence file in it becomes \
                     searchable — by name, by feature, or by a stretch of sequence on either \
                     strand, across the origin of a circular plasmid.",
                )
                .color(pal(ui).muted)
                .size(12.5),
            );
            return;
        };

        if let ScanState::Failed(e) = state {
            ui.add_space(8.0);
            ui.label(RichText::new(e).color(pal(ui).warn).size(12.5));
            return;
        }
        let Some(lib) = state.library() else {
            return;
        };
        let report = match state {
            ScanState::Done { report, .. } => Some(report),
            _ => None,
        };

        // A permanent statement of what is in scope. A search box over an
        // unknown number of files is a search box you cannot trust.
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let problems = lib.rows.iter().filter(|r| !r.state.searchable()).count();
            ui.label(
                RichText::new(format!(
                    "{} record{} · {} bases",
                    lib.rows.len(),
                    if lib.rows.len() == 1 { "" } else { "s" },
                    fmt_int(lib.packed_bases)
                ))
                .size(12.0),
            );
            if problems > 0 {
                ui.label(
                    RichText::new(format!("· {problems} not searchable"))
                        .color(pal(ui).warn)
                        .size(12.0),
                )
                .on_hover_text(
                    "Records with no sequence: annotation tracks, chromatograms, files past \
                     the size cap, files that could not be read. They are still findable by \
                     name, and they are never counted as lacking a site.",
                );
            }
            if let Some(r) = report {
                if r.incomplete.is_some() {
                    ui.label(
                        RichText::new("· scan did not finish")
                            .color(pal(ui).warn)
                            .size(12.0),
                    )
                    .on_hover_text(
                        "Part of the folder became unreachable. Nothing was removed from the \
                         index — a folder that vanished is not a folder whose files were \
                         deleted.",
                    );
                }
            }
        });

        if !lib.complete {
            ui.label(
                RichText::new("this index is partial")
                    .color(pal(ui).warn)
                    .size(11.5),
            );
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for m in [Mode::Name, Mode::Text, Mode::Motif, Mode::Enzyme] {
                if ui.selectable_label(self.lib_mode == m, m.label()).clicked() {
                    self.lib_mode = m;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.lib_query)
                    .hint_text(self.lib_mode.hint())
                    .desired_width(f32::INFINITY),
            );
        });
        if matches!(self.lib_mode, Mode::Motif | Mode::Enzyme) {
            ui.checkbox(&mut self.lib_absent, "show records WITHOUT it")
                .on_hover_text(
                    "Inverts only the sequence criterion. A record whose bases we never had is \
                     not evidence of absence, so it is never listed here.",
                );
        }

        // Validated live, so the user sees what will be searched as they type.
        let parsed = library::parse_query(self.lib_mode, &self.lib_query, self.lib_absent);
        match &parsed {
            Parsed::Idle => {
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Type to search.")
                        .color(pal(ui).muted)
                        .size(12.0),
                );
                return;
            }
            Parsed::Rejected(why) => {
                ui.add_space(6.0);
                ui.label(RichText::new(why).color(pal(ui).warn).size(12.0));
                return;
            }
            Parsed::Ready(_, note) => {
                if let Some(note) = note {
                    ui.add_space(4.0);
                    ui.label(RichText::new(note).color(pal(ui).muted).size(11.5));
                }
            }
        }
        let Parsed::Ready(q, _) = &parsed else {
            return;
        };

        let results = library::run(lib, q);
        ui.add_space(6.0);
        ui.separator();

        let mut open: Option<PathBuf> = None;
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 92.0)
            .show(ui, |ui| {
                for m in results.matches.iter().take(500) {
                    let label = if m.row.record == 0 {
                        m.row.path.clone()
                    } else {
                        format!("{} #{}", m.row.path, m.row.record + 1)
                    };
                    let resp = ui
                        .scope(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&label).size(12.5));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if !m.hits.is_empty() {
                                        ui.label(
                                            RichText::new(format!("{} hit", m.hits.len()))
                                                .color(pal(ui).muted)
                                                .size(11.5),
                                        );
                                    }
                                    ui.label(
                                        RichText::new(format!(
                                            "{} bp {}",
                                            fmt_int(if m.row.length > 0 {
                                                m.row.length
                                            } else {
                                                m.row.declared_len
                                            }),
                                            m.row.topology.as_str()
                                        ))
                                        .color(pal(ui).muted)
                                        .size(11.5),
                                    );
                                });
                            });
                        })
                        .response;
                    if resp.interact(Sense::click()).clicked() {
                        if let ScanState::Done { root, .. } = state {
                            open = Some(pl_scan::abs(root, &m.row.path));
                        }
                    }
                    // Wrapped hits are worth seeing: they only exist because
                    // the molecule is circular, and half the time the file
                    // never said so.
                    for h in m.hits.iter().take(3) {
                        let extra = match (h.wrapped, h.assumed_circular) {
                            (true, true) => "  crosses the origin (topology not declared)",
                            (true, false) => "  crosses the origin",
                            _ => "",
                        };
                        ui.label(
                            RichText::new(format!(
                                "        {}..{} {}{extra}",
                                h.start,
                                h.end,
                                h.strand.as_str()
                            ))
                            .color(pal(ui).muted)
                            .monospace()
                            .size(11.0),
                        );
                    }
                }
            });

        ui.separator();
        ui.label(
            RichText::new(format!(
                "{} record{} matched{}",
                results.matches.len(),
                if results.matches.len() == 1 { "" } else { "s" },
                if results.total_hits > 0 {
                    format!(", {} hits", fmt_int(results.total_hits))
                } else {
                    String::new()
                }
            ))
            .strong()
            .size(12.0),
        );
        // The footer is not decoration: it is what makes "3 matched" an audited
        // claim rather than an unfalsifiable one.
        if q.motif.is_some() {
            ui.label(
                RichText::new(results.coverage.describe())
                    .color(pal(ui).muted)
                    .size(11.0),
            );
        }

        if let Some(path) = open {
            self.load(path);
            self.tab = Tab::Features;
        }
    }

    fn features_tab(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        // `horizontal_wrapped`, NOT `horizontal`, for the same reason the tab
        // strip is: below about 357 pt of panel a `horizontal` row is painted
        // outside the clip rect and stops being clickable, which would cap the
        // splitter's usable travel and take away half its point.
        let mut open_new = false;
        let mut open_edit = None;
        let mut duplicate = None;
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            let has_sel = self.selected.is_some();
            if ui
                .button("New…")
                .on_hover_text("uses the sequence selection when there is one")
                .clicked()
            {
                open_new = true;
            }
            ui.add_enabled_ui(has_sel, |ui| {
                if ui.button("Edit…").clicked() {
                    open_edit = self.selected;
                }
                // One `SetFeature { index: None }` with a renamed clone: no new
                // machinery, and it is how a biologist makes a variant of a
                // four-qualifier CDS without retyping any of it.
                if ui
                    .button("Duplicate")
                    .on_hover_text("a copy, with every qualifier and colour")
                    .clicked()
                {
                    duplicate = self.selected;
                }
                if ui.button("Remove").clicked() {
                    remove = self.selected;
                }
            });
        });
        let needle = self.filter.to_lowercase();
        // Matched here so the count beside the box and the rows below cannot
        // disagree, and matched on QUALIFIER VALUES as well as name and kind,
        // which is how features are actually named in GenBank: a filter that
        // cannot find `/product="chloramphenicol acetyltransferase"` is a filter
        // a user stops trusting, and an untrusted filter is one that gets left
        // with stale text in it.
        let matches = |f: &pl_core::Feature| -> bool {
            needle.is_empty()
                || f.name.to_lowercase().contains(&needle)
                || f.kind.to_lowercase().contains(&needle)
                || f.qualifiers.iter().any(|q| {
                    q.1.as_deref()
                        .is_some_and(|v| v.to_lowercase().contains(&needle))
                })
        };
        let (n_match, n_all) = {
            let m = self.document().expect("checked by caller").molecule();
            (
                m.features.iter().filter(|f| matches(f)).count(),
                m.features.len(),
            )
        };
        ui.horizontal(|ui| {
            ui.label("filter");
            ui.text_edit_singleline(&mut self.filter);
            if !needle.is_empty() {
                // The count doubles as the clear affordance. A control with no
                // state indicator must not have destructive reach, and this one
                // had neither a count nor a way back.
                if ui
                    .button(format!("{n_match} of {n_all} match  x"))
                    .on_hover_text("clear the filter")
                    .clicked()
                {
                    self.filter.clear();
                }
            }
        });
        // The map does NOT follow this filter, and that is deliberate. A list is
        // a picture of a QUERY and narrowing it to matches is what a filter box
        // means everywhere; a map is a picture of a MOLECULE, and dropping parts
        // of it is a different claim — one a stale "pro" left in this box would
        // make silently, on the surface that gets exported. What the filter does
        // reach is the label BUDGET: a matching feature is labelled first when
        // the budget binds. Hovering a filtered row still lights its band on the
        // map, which is what actually answers "where is the thing I searched
        // for", and it does not require the map to lie.
        let selected = self.selected;

        let mut hot = None;
        let mut clicked = None;
        let mut open_row = None;
        let mut dup_row = None;
        let mut rm_row = None;
        let doc = self.document().expect("checked by caller");
        // Read once: `extent` needs the molecule the feature belongs to, and
        // borrowing it inside the row closure would fight the iterator.
        let span = doc.molecule().span();
        let circular = doc.molecule().topology.is_circular();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, f) in doc.molecule().features.iter().enumerate() {
                if !matches(f) {
                    continue;
                }
                let resp = ui
                    .scope(|ui| {
                        ui.horizontal(|ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(egui::vec2(9.0, 13.0), Sense::hover());
                            ui.painter().rect_filled(
                                rect,
                                egui::CornerRadius::same(2),
                                theme::feature_color(f),
                            );
                            // THE NAME IS ALLOCATED LAST AND TRUNCATES.
                            //
                            // It used to be a bare `ui.label` here, before the
                            // right-hand group, and a bare label asks for the
                            // width of its whole string. One 150-character
                            // `/label` therefore set the width of the entire
                            // side panel, and docs/UX-REVIEW-2026-07-31.md
                            // finding 7 recorded what that did: the tab strip
                            // read `ce  History  File` with Features, Library
                            // and Enzymes off the LEFT edge, the
                            // New…/Edit…/Duplicate/Remove row was gone, the
                            // coordinates were off the right edge of the
                            // window, and the splitter — measured at x=763 —
                            // did not move for drags to either 200 or 1200. A
                            // name is data from a file; it must not be able to
                            // lay out the application.
                            //
                            // Order is the fix. The right-to-left group is
                            // allocated first so the coordinates and strand
                            // keep their width, and the name gets what is left
                            // and truncates into it. Reversing these two lines
                            // restores the defect.
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(strand_glyph(f.strand))
                                        .color(pal(ui).muted)
                                        .monospace()
                                        .size(11.0),
                                );
                                // `start()`/`end()` are a min and a max over the
                                // segments, so an origin-crossing feature in the
                                // `join(2677..2686,1..7)` form every save through
                                // `genbank::write` produces read as `1..2,686` —
                                // a 17 bp promoter labelled as the whole plasmid,
                                // in the panel whose entire job is saying where
                                // things are.
                                let (fs, fe) =
                                    f.extent(span, circular).unwrap_or((f.start(), f.end()));
                                ui.label(
                                    RichText::new(format!("{}..{}", fmt_int(fs), fmt_int(fe)))
                                        .color(pal(ui).muted)
                                        .monospace()
                                        .size(11.0),
                                );
                                // What is left, read the way names are read.
                                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                    let r = ui.add(
                                        egui::Label::new(
                                            RichText::new(&f.name).strong().size(12.5),
                                        )
                                        .truncate(),
                                    );
                                    // A tooltip ONLY when the row cannot show
                                    // the whole name: one that repeats what is
                                    // already legible is noise on every row of
                                    // a list a user scrolls. Measured rather
                                    // than guessed from a character count,
                                    // because whether it fits depends on the
                                    // glyphs and the splitter, not the length.
                                    // Colour does not affect text metrics.
                                    let wanted = ui
                                        .painter()
                                        .layout_no_wrap(
                                            f.name.clone(),
                                            egui::FontId::proportional(12.5),
                                            egui::Color32::WHITE,
                                        )
                                        .rect
                                        .width();
                                    if wanted > r.rect.width() + 0.5 {
                                        r.on_hover_text(&f.name);
                                    }
                                });
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.add_space(17.0);
                            // Same rule, same reason: `kind` is free text in
                            // both formats — the feature editor has a
                            // free-text Type box — so it can be as long as a
                            // name and would set the panel width the same way.
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&f.kind).color(pal(ui).muted).size(11.0),
                                )
                                .truncate(),
                            );
                            if f.segments.len() > 1 {
                                ui.label(
                                    RichText::new(format!("{} segments", f.segments.len()))
                                        .color(pal(ui).accent)
                                        .size(11.0),
                                );
                            }
                        });
                    })
                    .response
                    .interact(Sense::click());

                // `hot` first, so a row that is both reads as SELECTED. The
                // two are distinguishable by consequence as well as by wash:
                // `selected` drives the enabled state of Edit…/Duplicate/Remove
                // above, and `hot` drives nothing.
                //
                // Without this the map's hover was invisible everywhere — the
                // map widened the band, and nothing in the list moved — so the
                // whole click-to-select interaction read as inert.
                if self.hot_shown == Some(i) {
                    ui.painter().rect_filled(
                        resp.rect.expand2(egui::vec2(4.0, 2.0)),
                        egui::CornerRadius::same(3),
                        pal(ui).hover_wash(),
                    );
                }
                if selected == Some(i) {
                    ui.painter().rect_filled(
                        resp.rect.expand2(egui::vec2(4.0, 2.0)),
                        egui::CornerRadius::same(3),
                        pal(ui).selection(),
                    );
                }
                if resp.hovered() {
                    hot = Some(i);
                }
                if resp.clicked() {
                    clicked = Some(i);
                }
                // SnapGene's own muscle memory, and one branch.
                if resp.double_clicked() {
                    open_row = Some(i);
                }
                resp.context_menu(|ui| {
                    if ui.button("Edit…").clicked() {
                        open_row = Some(i);
                        ui.close();
                    }
                    if ui.button("Duplicate").clicked() {
                        dup_row = Some(i);
                        ui.close();
                    }
                    if ui.button("Remove").clicked() {
                        rm_row = Some(i);
                        ui.close();
                    }
                });
                ui.add_space(3.0);
            }
        });

        if hot.is_some() {
            self.hot = hot;
        }
        if let Some(i) = clicked {
            self.selected = if self.selected == Some(i) {
                None
            } else {
                Some(i)
            };
        }

        // Acted on after the scroll area, because every one of these needs
        // `&mut self` and the row closure is holding a borrow of the document.
        if open_new {
            self.open_feature_editor(None);
        }
        if let Some(i) = open_edit.or(open_row) {
            // After the click above has been handled, which toggles `selected`:
            // `open_feature_editor` puts the highlight back on the feature it
            // opens, for every entry point at once.
            self.open_feature_editor(Some(i));
        }
        if let Some(i) = duplicate.or(dup_row) {
            self.duplicate_feature(i);
        }
        if let Some(i) = remove.or(rm_row) {
            self.remove_feature(i);
        }
    }

    /// Append a copy of feature `i`, named "<name> copy".
    ///
    /// A CLONE of the feature and not a rebuild, for the reason the whole
    /// feature editor exists: the qualifiers, their valueless-ness, the
    /// per-segment colours, the `translated` flags and the segment order all
    /// come along, and none of them has a control here to be forgotten at.
    fn duplicate_feature(&mut self, i: usize) {
        let Some(mut f) = self
            .document()
            .and_then(|d| d.molecule().features.get(i))
            .cloned()
        else {
            return;
        };
        f.name = format!("{} copy", f.name);
        let last = self
            .document()
            .map(|d| d.molecule().features.len())
            .unwrap_or(0);
        if self.edit(pl_core::OpKind::SetFeature {
            index: None,
            feature: Box::new(f),
        }) {
            self.selected = Some(last);
        }
    }

    fn enzymes_tab(&mut self, ui: &mut Ui) {
        // `self.bench.get()`, not `self.document()`. The accessor borrows all of
        // `self` for as long as `d` lives, and this function goes on to write
        // `self.enzyme_set` from a closure; through the field the borrow is
        // disjoint, which is what `self.document.as_ref()` gave for free. That
        // is the one real cost of putting the document behind a method, and it
        // is why every mutable use goes through the field as well.
        let d = self.bench.get().expect("checked by caller");
        ui.add_space(4.0);

        match &d.digest {
            DigestState::Unavailable(why) => {
                ui.label(RichText::new(why).color(pal(ui).muted));
                return;
            }
            DigestState::Running { .. } => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(RichText::new("scanning…").color(pal(ui).muted));
                });
                return;
            }
            DigestState::Done(_) => {}
        }

        let results = d.digest.results();
        let set = self.enzyme_set;
        let vis = d.visibility(set);

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Show").color(pal(ui).muted).size(12.0));
            for s in pl_enzymes::EnzymeSet::ALL {
                let resp = ui.selectable_label(set == s, s.label());
                // A chip that cannot change anything says so, rather than
                // leaving the user to click it and infer from a picture that
                // did not move. Two of the five are permanently in this state
                // against the built-in table — every enzyme in it has a 6-base
                // or longer site — and this change renamed both of them without
                // noticing they are inert. Asked of the DIGEST, so a table that
                // gains a four-cutter makes them live with no code change here.
                let resp = if s != pl_enzymes::EnzymeSet::All && !s.discriminates(results) {
                    resp.on_hover_text(
                        "Every enzyme that cuts this molecule is in this set, so this changes \
                         nothing here.",
                    )
                } else {
                    resp
                };
                if resp.clicked() {
                    self.enzyme_set = s;
                }
            }
        });
        ui.add_space(4.0);

        // THE BADGE. `docs/PLAN.md` item 33: persistent and unmissable whenever
        // the active filter hides anything, with the way out in the same
        // breath.
        //
        // Counted in SITES, not enzymes. "3 enzymes hidden" understates three
        // enzymes cutting fourteen times between them, and it is the cut you
        // did not know about that ruins the experiment.
        if vis.hides_anything() {
            egui::Frame::new()
                .fill(pal(ui).warn.gamma_multiply(0.18))
                .inner_margin(egui::Margin::symmetric(8, 6))
                .corner_radius(6.0)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("⚠").color(pal(ui).warn).strong());
                        ui.label(
                            RichText::new(format!(
                                "{} more cut site{} hidden by “{}”, across {} enzyme{}.",
                                vis.hidden_sites,
                                if vis.hidden_sites == 1 { "" } else { "s" },
                                set.label(),
                                vis.hidden_enzymes,
                                if vis.hidden_enzymes == 1 { "" } else { "s" },
                            ))
                            .strong(),
                        );
                        if ui.button("Show all").clicked() {
                            self.enzyme_set = pl_enzymes::EnzymeSet::All;
                        }
                    });
                });
            ui.add_space(6.0);
        }

        // The methylation verdict for each shown enzyme, computed at that
        // enzyme's first site: every rule here is a property of the (enzyme,
        // methylase) pair plus local context, so a per-site answer is what the
        // model gives — this shows the first, which is exact for the unique
        // cutters that matter most and indicative for the rest.
        //
        // Read off the worker's answer rather than recomputed. This used to
        // call `cut_sites` over the whole molecule once per shown row, to
        // recover one integer the worker had already had and discarded: 58
        // full-molecule scans per frame, 1.58 s on the 4.6 Mb NC_000913.3 —
        // which is `doc.rs`'s entire digest, back on the UI thread, on every
        // repaint. `DigestState::verdict` is now a field read.
        let verdict = |i: usize| d.digest.verdict(i);

        // Indices are carried through the filter because they are the key into
        // the verdict table.
        let shown: Vec<(usize, &pl_enzymes::Digest)> = results
            .iter()
            .enumerate()
            .filter(|(_, x)| set.admits(x))
            .collect();
        // The gel's lane set, read as a snapshot and written back after the
        // list has been laid out. Collected rather than mutated in place
        // because the closure below is already borrowing the document the
        // digest came from, and because a tick must not change the list it is
        // being drawn into halfway down.
        // THE END EACH CUT LEAVES, and which of the others it can be joined to.
        //
        // Narrower than `pl ends` on purpose. The catalogue answer — every
        // enzyme anywhere that leaves `GATC` — is a reference lookup; the
        // question in front of a plasmid is "of the enzymes that cut THIS, which
        // are interchangeable", because those are the alternatives the polylinker
        // actually offers. So the partner list is intersected with the cutters.
        //
        // Computed once for the tab rather than per row: it is O(cutters²) and
        // the row is redrawn every frame.
        let cutters: Vec<&'static pl_enzymes::Enzyme> = results
            .iter()
            .filter(|x| !x.is_non_cutter())
            .map(|x| x.enzyme)
            .collect();

        let picked = self.gel.picked.clone();
        let mut toggles: Vec<(String, bool)> = Vec::new();
        let uniq: Vec<_> = shown.iter().filter(|(_, x)| x.is_unique_cutter()).collect();
        let multi: Vec<_> = shown
            .iter()
            .filter(|(_, x)| !x.is_unique_cutter())
            .collect();

        egui::ScrollArea::vertical().show(ui, |ui| {
            if !uniq.is_empty() {
                ui.label(RichText::new(format!("{} unique cutters", uniq.len())).strong());
                ui.add_space(2.0);
                for (i, e) in &uniq {
                    let mut on = picked.contains(e.enzyme.name);
                    let was = on;
                    enzyme_row(
                        ui,
                        e.enzyme.name,
                        e.enzyme.site,
                        &e.positions,
                        true,
                        verdict(*i),
                        poor_single_site_note(e.enzyme.name, e.count()),
                        &end_note(e.enzyme, &cutters),
                        &mut on,
                    );
                    if on != was {
                        toggles.push((e.enzyme.name.to_string(), on));
                    }
                }
                ui.add_space(10.0);
            }
            if !multi.is_empty() {
                ui.label(
                    RichText::new(format!("{} cut more than once", multi.len()))
                        .color(pal(ui).muted),
                );
                ui.add_space(2.0);
                for (i, e) in &multi {
                    let mut on = picked.contains(e.enzyme.name);
                    let was = on;
                    enzyme_row(
                        ui,
                        e.enzyme.name,
                        e.enzyme.site,
                        &e.positions,
                        false,
                        verdict(*i),
                        poor_single_site_note(e.enzyme.name, e.count()),
                        &end_note(e.enzyme, &cutters),
                        &mut on,
                    );
                    if on != was {
                        toggles.push((e.enzyme.name.to_string(), on));
                    }
                }
                ui.add_space(10.0);
            }
            if shown.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "No enzyme in “{}” cuts this molecule.",
                        set.label()
                    ))
                    .color(pal(ui).muted),
                );
                ui.add_space(8.0);
            }
            // Non-cutters are absent, not hidden. Keeping them out of the badge
            // is what lets the badge mean one thing.
            ui.label(
                RichText::new(format!(
                    "{} of {} enzymes do not cut this molecule at all",
                    vis.non_cutters,
                    results.len()
                ))
                .color(pal(ui).muted)
                .size(11.5),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "A textbook Type IIP set, computed live with circular wraparound handled. \
                     Real work wants REBASE.",
                )
                .color(pal(ui).muted)
                .size(11.0)
                .italics(),
            );
        });
        for (name, on) in toggles {
            if on {
                self.gel.picked.insert(name);
            } else {
                self.gel.picked.remove(&name);
            }
            // The user has said what they want in the gel, so the seeded
            // default must not come back and overwrite it — and the strip must
            // stop calling the lane set a suggestion, because from here on it
            // is not one.
            self.gel.seeded = true;
            self.gel.seed_note = None;
        }
    }

    /// The chromatograms, and what they say about the open construct.
    ///
    /// Opens with no molecule and SAYS WHAT IT CANNOT ANSWER. Everything that
    /// is a property of the FILE alone is shown — the trace, the calls, the
    /// Mott-trimmed region, the header facts `pl trace` prints — and in place
    /// of the comparison one sentence and no ornament. No identity number, no
    /// coverage figure, no tick, no colour that reads as a verdict: every
    /// number in a comparison is a number about a PAIR, and there is no pair.
    fn reads_tab(&mut self, ui: &mut Ui) {
        if self.reads.is_empty() {
            ui.add_space(20.0);
            ui.label(
                RichText::new(
                    "No reads. Open a .ab1 — the Open button, or drop one on the window — \
                     and it will be compared to whatever construct is open.",
                )
                .color(pal(ui).muted),
            );
            return;
        }
        self.read_shown = self.read_shown.min(self.reads.len() - 1);
        let mut jump: Option<u64> = None;
        let mut close: Option<usize> = None;

        ui.horizontal_wrapped(|ui| {
            for (i, r) in self.reads.iter().enumerate() {
                let resp = ui.selectable_label(self.read_shown == i, &r.name);
                // Where it came from, on the hover: two reads of one construct
                // are routinely named `A01.ab1` and `A02.ab1` in different
                // folders, and the name alone does not say which is which.
                let resp = match &r.path {
                    Some(p) => resp.on_hover_text(p.display().to_string()),
                    None => resp.on_hover_text("dropped on the window; no path"),
                };
                if resp.clicked() {
                    self.read_shown = i;
                    // AND BACK TO THE START OF IT. The page index belongs to
                    // the read being shown, not to the panel: paging into a
                    // 900-base read and then clicking a 60-base one put
                    // `first = min(window * per_view, n-1) + 1` at the very
                    // last base, so the panel opened on "bases 60..60 of 60"
                    // with one peak and a ▶ button that looked stuck.
                    self.read_window = 0;
                }
            }
            if ui.button("Close read").clicked() {
                close = Some(self.read_shown);
            }
        });
        ui.separator();

        let ref_len = self.document().map(|d| d.molecule().len());
        let trace_seq = self.reads[self.read_shown].trace.sequence.clone();
        let window = self.read_window;
        let r = &self.reads[self.read_shown];

        // THE VERDICT LINE, which always carries coverage. Never a bare
        // identity percentage, and the panel renders NO single-glyph verdict
        // for the construct as a whole: 100% identity over 200 aligned columns
        // on a 5,386 bp plasmid says nothing about the other 5,186 bases.
        ui.label(RichText::new(r.verdict(ref_len.unwrap_or(0))).size(12.0));
        ui.add_space(4.0);
        for line in r.header() {
            ui.label(RichText::new(line).size(11.0).color(pal(ui).muted));
        }
        if matches!(r.state, reads::CompareState::Done(_)) {
            ui.label(
                RichText::new(r.which_sequence())
                    .size(11.0)
                    .color(pal(ui).muted),
            );
        }
        // In READ coordinates, which is why every discrepancy row now carries
        // its read position too: the rows are in reference coordinates and
        // nothing else on screen connects the two numbering systems.
        let reliable = r.reliable();
        if let Some((a, b)) = reliable {
            ui.label(
                RichText::new(format!(
                    "the basecaller stands behind bases {a}..{b} of this read; nothing \
                     outside it is discarded, because on a read that came back strange the \
                     ragged ends are often the part worth looking at"
                ))
                .size(11.0)
                .color(pal(ui).muted),
            );
        }
        ui.add_space(6.0);

        // THE CHROMATOGRAM, from `pl_draw::trace::View::to_scene` and painted
        // by the same Scene→egui path the gel uses. Not a second renderer: that
        // one holds four correctness properties a re-implementation gets wrong
        // in exactly the documented ways — colour by `FWO_` and never by array
        // position, x in SAMPLES so compressions stay compressed, decimation by
        // bucket MAXIMUM so a stride cannot drop a base, and Y scaled per drawn
        // window with the maximum reported.
        let width = ui.available_width().max(120.0);
        // `to_scene` has no legibility rule of its own — it emits one text item
        // per called base at its own x — and 900 bases across 1,200 pt is a
        // smear, which reads as "the sequence is there" while a collided letter
        // reads as a missing base. So the CALLER never asks for a window wider
        // than legible letters allow. That gap belongs here and not in the
        // crate.
        let n_bases = trace_seq.len();
        let per_view = ((width / 14.0) as usize).clamp(8, 60);
        let first = (window * per_view).min(n_bases.saturating_sub(1)) + 1;
        let last = (first + per_view - 1).min(n_bases.max(1));
        let (scene, rep) = pl_draw::trace::View {
            channels: [
                &r.trace.channels[0],
                &r.trace.channels[1],
                &r.trace.channels[2],
                &r.trace.channels[3],
            ],
            base_order: r.trace.base_order,
            peaks: &r.trace.peaks,
            sequence: &r.trace.sequence,
            quality: &r.trace.quality,
            title: &r.name,
        }
        .to_scene(&pl_draw::trace::Options {
            bases: Some((first, last)),
            width: width as f64,
            height: 200.0,
            // Okabe–Ito, CHOSEN FOR THE FIELD IT IS PAINTED ON. `to_scene`
            // emits no background rectangle, so these colours land straight on
            // the app's panel — and `Palette::Accessible` puts G at #000000,
            // which is 1.20:1 on the dark panel: the G trace and every G letter
            // under it were invisible, so the picture had three channels and
            // the "letters are the second channel" promise failed in exactly
            // the same place the first one did.
            //
            // `Palette::Classic` is not offered at all: it puts A in green and
            // T in red, the one pair a red–green colour-blind reader cannot
            // separate, and A/T confusion is not a small error.
            palette: trace_palette(ui.visuals().dark_mode),
            quality_bars: true,
            max_points: width as usize,
        });
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(width, scene.height as f32), egui::Sense::hover());
        scene::paint(ui.painter(), &scene, rect.min, 1.0);

        let mut step: i64 = 0;
        ui.horizontal_wrapped(|ui| {
            if ui.button("◀").clicked() {
                step = -1;
            }
            if ui.button("▶").clicked() && last < n_bases {
                step = 1;
            }
            ui.label(
                RichText::new(format!("bases {first}..{last} of {n_bases}"))
                    .size(11.0)
                    .color(pal(ui).muted),
            );
        });
        // PERMANENT FURNITURE, NOT A HOVER, and this is the most damaging thing
        // the panel could get wrong. `to_scene` scales the plot to the maximum
        // sample INSIDE THE DRAWN WINDOW, for a good reason it documents — a
        // single tall peak elsewhere would flatten everything here into a line.
        // Zoom into the sixty bases after a read has died and pure baseline
        // noise is stretched to fill the plot: four ragged traces at full
        // height, evenly spaced, with confident letters underneath, which is
        // the same picture a good region gives. A number that changes as you
        // pan is a number people learn to read.
        let overall = r
            .trace
            .channels
            .iter()
            .flat_map(|c| c.iter().copied())
            .max()
            .unwrap_or(0);
        ui.label(
            RichText::new(format!(
                "peak height in this window: {}; up to {overall} elsewhere in this read",
                rep.scale_max
            ))
            .size(11.0)
            .color(pal(ui).muted),
        );
        if overall > 0 && (rep.scale_max as f64) < 0.2 * overall as f64 {
            // IN WORDS, not a tint: a tint on a chromatogram competes with the
            // four channel colours, and colour is never the only channel here.
            ui.label(
                RichText::new(
                    "these peaks are a small fraction of this read's own maximum — the plot \
                     is stretched to fill the frame, so this may be baseline noise",
                )
                .size(11.0)
                .color(pal(ui).warn),
            );
        }
        for note in &rep.notes {
            ui.label(RichText::new(note).size(11.0).color(pal(ui).muted));
        }
        ui.separator();

        // THE DIFFERENCES. A claim about the CONSTRUCT, so the rows name
        // reference coordinates; evidenced by the TRACE, so double-clicking one
        // puts the caret on that base in the Sequence tab — where the aa track
        // above it says whether it changes a residue.
        if let reads::CompareState::Done(report) = &r.state {
            if report.discrepancies.is_empty() {
                ui.label(
                    RichText::new("no differences at all over the aligned columns")
                        .size(11.5)
                        .color(pal(ui).muted),
                );
            }
            egui::ScrollArea::vertical()
                .id_salt("read-diffs")
                .show(ui, |ui| {
                    for d in &report.discrepancies {
                        let (pos, change, q, kind, conf) = reads::row(d);
                        let note = reads::read_base_note(report, d, &trace_seq, reliable);
                        let resp = ui
                            .selectable_label(
                                false,
                                RichText::new(format!("{pos:>12}  {change}{note}  {q:>4}  {kind}"))
                                    .monospace()
                                    .size(11.0)
                                    .color(match d.confidence {
                                        pl_sanger::Confidence::Low => pal(ui).muted,
                                        _ => pal(ui).ink,
                                    }),
                            )
                            // The WORD is always available; the colour above is
                            // a second channel and never the only one.
                            .on_hover_text(conf);
                        if resp.double_clicked() {
                            jump = Some(d.ref_pos);
                        }
                    }
                });
        }

        if step < 0 {
            self.read_window = self.read_window.saturating_sub(1);
        } else if step > 0 {
            self.read_window += 1;
        }
        if let Some(i) = close {
            self.reads[i].cancel();
            self.reads.remove(i);
            self.read_shown = self.read_shown.min(self.reads.len().saturating_sub(1));
            self.read_window = 0;
        }
        if let Some(at) = jump {
            self.jump_to_base(at);
        }
    }

    /// Put the caret on a reference base and show it.
    ///
    /// Through `SeqEdit::set_selection`, which is the ONE sanctioned path from
    /// outside `seqedit`: it commits any open typing run first, and assigning
    /// `sel` behind a run's back is a documented past defect.
    ///
    /// This is a VIEW change. It selects a base; it does not alter one.
    fn jump_to_base(&mut self, at: u64) {
        let Some(d) = self.bench.get_mut() else {
            return;
        };
        let n = d.molecule().len();
        if at == 0 || at > n {
            return;
        }
        self.edit.set_selection(
            d,
            seqedit::Selection {
                anchor: at - 1,
                head: at,
                through_origin: false,
            },
            at,
        );
        self.tab = Tab::Sequence;
    }

    /// Every edit, in order, with the point you are standing at.
    ///
    /// This is not a nicety bolted onto undo — `docs/PLAN.md` ADR-2 makes them
    /// the same mechanism, so the list below *is* the undo stack. Two
    /// properties are worth seeing, because no other editor in this category
    /// offers them:
    ///
    /// - Ids are derived from content, so the same edits from the same start
    ///   produce the same ids on someone else's machine. History is
    ///   comparable, not merely local.
    /// - A new edit after an undo **forks** rather than truncating. The
    ///   abandoned branch is still there and still reachable, which is the
    ///   afternoon's work every other editor silently throws away.
    fn history_tab(&mut self, ui: &mut Ui) {
        let Some(d) = self.bench.get() else { return };
        let ops = d.log.all_ops();

        ui.add_space(6.0);
        if ops.is_empty() {
            ui.label(
                RichText::new("No edits yet. Anything you change appears here and can be undone.")
                    .color(pal(ui).muted),
            );
            return;
        }

        let on_path: std::collections::BTreeSet<_> = d.log.path().iter().map(|o| o.id).collect();
        let cursor = d.log.cursor();
        // Branch points counted from the ops' parents rather than from
        // `forks()`, which returns `Vec<OpId>` and so structurally cannot
        // report a branch at the base — where the commonest one is: undo the
        // first edit, then do something else.
        let mut children: std::collections::BTreeMap<Option<_>, usize> = Default::default();
        for op in ops {
            *children.entry(op.parent).or_default() += 1;
        }
        let branch_points = children.values().filter(|n| **n > 1).count();

        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} edit(s)", ops.len())).strong());
            if branch_points > 0 {
                ui.label(
                    RichText::new(format!("· {branch_points} branch point(s)"))
                        .color(pal(ui).muted)
                        .size(12.0),
                );
            }
        });
        ui.add_space(4.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            for op in ops {
                let here = Some(op.id) == cursor;
                let live = on_path.contains(&op.id);
                ui.horizontal(|ui| {
                    // The cursor marker, then whether this edit is on the path
                    // to the current state or on an abandoned branch.
                    ui.label(
                        RichText::new(if here { HISTORY_HERE } else { " " })
                            .monospace()
                            .color(pal(ui).accent),
                    );
                    let colour = if live { pal(ui).ink } else { pal(ui).muted };
                    ui.label(
                        RichText::new(op.id.short())
                            .monospace()
                            .size(11.0)
                            .color(pal(ui).muted),
                    );
                    let text = RichText::new(op.kind.describe()).color(colour);
                    ui.label(if here { text.strong() } else { text });
                    if !live {
                        ui.label(
                            RichText::new("(other branch)")
                                .color(pal(ui).muted)
                                .size(11.0),
                        );
                    }
                });
            }
        });
    }

    /// The editing surface.
    ///
    /// Virtualised: only the visible rows are built, so this costs the same on
    /// a 4.6 Mb genome as on a 5 kb plasmid (measured flat at 0.001–0.002 ms
    /// per frame from 10 kb to 50 Mb). The caret and the selection are
    /// arithmetic against `show_rows`' row range, never widgets — one
    /// `Response` per base would be 3,600 widget ids per frame on a 60-row
    /// viewport, and a selection painted per base would be 4.6 million
    /// rectangles for Ctrl+A. A contiguous base range meets a row in a
    /// contiguous span, so a selection contributes at most one rectangle per
    /// visible row: about forty, whatever the molecule.
    /// Rebuild the annotation index if the document moved, and the cut list if
    /// the digest finished or the filter changed.
    ///
    /// Eagerly, at the top of the tab and outside every paint closure, because
    /// the row height depends on `lanes` which comes out of the build — and
    /// because a lazy rebuild inside the closure would need interior
    /// mutability this file does not otherwise use.
    fn refresh_annotations(&mut self) {
        let want = match self.document() {
            Some(d) => (self.doc_generation, d.log.cursor()),
            None => return,
        };
        if self.annot.version != want {
            let (ix, tr) = {
                let d = self.document().expect("checked above");
                (
                    annot::AnnotIndex::build(d.molecule(), want),
                    // Once per document version, beside the index and on the
                    // same key. A `TranslationPath` is a handful of ranges;
                    // measured against `AnnotIndex::build`'s 0.60 ms over
                    // 9,001 features on the 4.6 Mb genome, this is noise.
                    // Materialising the residues instead would be 1.3 million
                    // entries for MG1655, thrown away by one keystroke.
                    aa::Translations::build(d.molecule(), self.doc_code),
                )
            };
            self.annot = ix;
            self.tr = tr;
            self.cuts_for = None;
        }

        let done = matches!(
            self.document().expect("checked above").digest,
            DigestState::Done(_)
        );
        let key = (want, self.enzyme_set, done);
        if self.cuts_for == Some(key) {
            return;
        }
        let cuts = {
            let d = self.document().expect("checked above");
            let mut cuts = Vec::new();
            if done {
                for (i, dg) in d.digest.results().iter().enumerate() {
                    // Filtered here rather than at draw time, so the merged
                    // vector a row walks is already the truth the panel is
                    // showing — and so changing the filter is a rebuild from
                    // data in hand rather than a rescan.
                    if !self.enzyme_set.admits(dg) {
                        continue;
                    }
                    let site_len = dg.enzyme.site.len() as u32;
                    for s in d.digest.sites(i) {
                        cuts.push(annot::Cut {
                            // 1-based base 3' of the nick -> the 0-based gap
                            // the nick is in, which is the caret's own space.
                            at: s.position.saturating_sub(1),
                            site_lo: s.site_start.saturating_sub(1),
                            site_len,
                            enzyme: i as u32,
                        });
                    }
                }
            }
            cuts
        };
        self.annot.set_cuts(cuts);
        // Only a FINISHED digest may change the reservation. While one runs the
        // strip keeps whatever the last completed scan of this document said,
        // so the row pitch does not depend on a worker's phase. `adopt` clears
        // it, because the next document is a different question.
        if done {
            self.enz_strip = self.annot.cut_count() > 0;
        }
        self.cuts_for = Some(key);
    }

    /// Start, stop or leave the ORF scan alone, and reserve its strip from the
    /// last COMPLETED answer.
    ///
    /// Called at the top of the Sequence tab, outside every paint closure, for
    /// the same reason `refresh_annotations` is: the row height depends on the
    /// reservation, and a lazy update inside the closure would need interior
    /// mutability this file does not otherwise use.
    fn refresh_orfs(&mut self) {
        let want = self.layout.orf_track;
        let (code, min_aa) = (self.doc_code.id, self.layout.orf_min_aa);
        let Some(d) = self.bench.get_mut() else {
            return;
        };
        if !want {
            if !d.orfs.is_off() {
                d.stop_orfs();
                // The strip goes with the answer. Keeping it reserved would
                // spend a row of height on a channel that is switched off.
                self.orf_strip = false;
            }
            return;
        }
        d.start_orfs(code, min_aa);
        d.poll_orfs();
        // Only a FINISHED scan may change the reservation — `enz_strip`'s rule,
        // and skipping it is what produced the 43.41 -> 31.41 -> 43.41 pitch
        // recorded above: a 28% reflow twice per keystroke, each one
        // re-anchoring the whole view.
        if let Some(o) = d.orfs.done() {
            self.orf_strip = !o.orfs.is_empty() || !o.stopless.is_empty();
        }
    }

    /// The editing surface.
    ///
    /// Virtualised: only the visible rows are built, so this costs the same on
    /// a 4.6 Mb genome as on a 5 kb plasmid (measured flat at 0.001–0.002 ms
    /// per frame from 10 kb to 50 Mb). The caret and the selection are
    /// arithmetic against `show_rows`' row range, never widgets — one
    /// `Response` per base would be 3,600 widget ids per frame on a 60-row
    /// viewport, and a selection painted per base would be 4.6 million
    /// rectangles for Ctrl+A. A contiguous base range meets a row in a
    /// contiguous span, so a selection contributes at most one rectangle per
    /// visible row: about forty, whatever the molecule.
    ///
    /// The annotations are the same shape of promise. Every strip is a rect or
    /// a line in a band the letters do not occupy: the row is still ONE
    /// `painter.text` call, at one colour, at one `FontId`. That is what keeps
    /// case legible — an eye scanning for the boundary where a lowercase tail
    /// was added is reading x-height, and x-height only reads when the ink is
    /// uniform — and it is why the feature channel is a ribbon and not a
    /// background tint. A tint would force `theme::on_color` per base, flipping
    /// the letters between black and white at exactly the kind of boundary the
    /// case signal sits on.
    fn sequence_tab(&mut self, ui: &mut Ui) {
        use seqedit::Selection;

        let now = ui.input(|i| i.time);
        let gate = seqedit::Editability::of(self.document().expect("checked by caller").molecule());

        ui.add_space(4.0);
        if !gate.is_editable() {
            // No caret, no selection, no cursor. The engine would refuse a
            // keystroke here too, but its message is about the consequence
            // rather than the cause: a keystroke on an annotation track reads
            // "feature 0 'orphan' segment 0 start: 101 is past the 1 bp
            // molecule", which is a symptom of a question nobody should have
            // been allowed to ask.
            let why = gate.refusal().unwrap_or_default();
            ui.label(RichText::new(why).color(pal(ui).muted).size(12.0));
            return;
        }

        // Before any geometry: the row height depends on how many lanes this
        // document needs, and that comes out of the build.
        self.refresh_annotations();
        // And on whether the last completed ORF scan of this document found
        // anything. Same rule, same place, outside every paint closure.
        self.refresh_orfs();

        // ONE pitch, derived from the font the row is actually painted in, and
        // used by `show_rows`, by the y -> row hit-test and by the painter.
        //
        // The old row was a `ui.horizontal` of two labels, which advanced
        // 21.0 px against the 18.125 px `show_rows` was told to assume — the
        // `interact_size` floor. Read-only that drift is cosmetic; the moment a
        // click has to name a base it is 8 rows of overshoot at the bottom of a
        // 60-row viewport, i.e. the user clicks one base and the caret lands
        // 480 bases away. The annotation strips make that trap wider, not
        // narrower, so they are added into this one number and nowhere else.
        let font = egui::FontId::monospace(11.5);
        let gutter_font = egui::FontId::monospace(11.0);
        let ruler_font = egui::FontId::monospace(9.5);
        let name_font = egui::FontId::proportional(9.0);
        let (text_h, advance, gutter_advance) = ui.ctx().fonts_mut(|f| {
            (
                f.row_height(&font).max(1.0),
                f.glyph_width(&font, 'A').max(1.0),
                f.glyph_width(&gutter_font, '0').max(1.0),
            )
        });

        let n = self.edit.effective_len(self.document().unwrap().molecule());

        // The row width is measured, not assumed: sixty cells plus the gutter
        // is wider than the panel was until this run, and a base that is off
        // the edge cannot be clicked. Every horizontal number now comes out of
        // this one value — see `seqedit::RowLayout`.
        //
        // The gutter is measured from THIS molecule. A constant sized for
        // "4,641,652" is nine cells against pKoV's five, so it costs an 8,117 bp
        // plasmid 26.5 pt that come straight out of the base cells — and those
        // 26.5 pt decide whether a 60-base row needs a 513.0 pt panel or a
        // 486.5 pt one, which is width the map pane keeps.
        //
        // Both figures are MEASURED, by bisecting the real painter in
        // `the_advance_band_that_keeps_every_per_row_expectation`, which also
        // prints them. They read "21 pt", "509" and "488" until 2026-07-30, all
        // three from an algebraic model that put the gutter's 8 pt of air on the
        // wrong side and came out 4 pt optimistic everywhere. 488 in particular
        // reads as the knife edge it is not: the default has 14.8 pt of slack
        // over the threshold, not 12. (13.5 under Hack; the IBM Plex Mono swap
        // took the threshold from 486.5 pt to 485.2 and widened the slack.)
        let scrollbar = ui.spacing().scroll.bar_width + 4.0;
        let layout = seqedit::row_layout(
            ui.available_width(),
            advance,
            scrollbar,
            seqedit::gutter_w(n, gutter_advance),
        );
        let per_row = layout.per_row;
        self.edit.set_per_row(per_row);
        let rows = (n.div_ceil(per_row).max(1)) as usize;

        // The strips, per DOCUMENT and never per row. `show_rows` has a single
        // pitch; a height that varied by row would put the y -> row hit-test
        // back where it was.
        //
        // The enzyme strip is reserved whenever the document has an admitted
        // cut anywhere, not whenever this row has one and not whenever the
        // marks are currently drawn — suppressing the marks during a typing run
        // must not make the whole view jump vertically while somebody types.
        //
        // And not whenever the digest has FINISHED, either, which is what
        // `cut_count() > 0` really asked. Every `Document::apply` restarts the
        // digest, so one keystroke emptied the cut list, took the strip away,
        // and put it back when the worker landed. On the 4.6 Mb genome that
        // digest takes 1,634 ms, so the row pitch went 43.41 -> 31.41 -> 43.41
        // — a 28% reflow twice per keystroke, each one re-anchoring the whole
        // view. `enz_strip` is a property of the DOCUMENT: what the last
        // completed digest of this document said, held across the next one.
        const ENZ_H: f32 = 12.0;
        const TICK_H: f32 = 3.0;
        const LANE_PITCH: f32 = 5.0;
        const RIBBON_H: f32 = 4.0;
        /// One bar per reading frame, three per strand.
        const ORF_LANE_H: f32 = 3.0;
        let has_cuts = self.enz_strip;
        let enz_h = if has_cuts { ENZ_H } else { 0.0 };
        let lanes = self.annot.lanes;

        // Every strip in this row is a property of the DOCUMENT and of the
        // settings — never of whether THIS row carries a CDS, an ORF or a cut.
        // `show_rows` maps a scroll offset to a row index by dividing, so a row
        // that grew because it happened to hold something would put the
        // scrollbar out of step with the content and land a click on the wrong
        // row. The consequence worth stating: a document with nothing
        // translated reserves ZERO — a FASTA, an annotation track and a plasmid
        // carrying only promoters and origins all pay nothing at all.
        //
        // Measured on pKoV at the default 1280x840 window and 500 pt panel:
        // 39.94 pt with no track (15 rows, 900 bases visible) against 84.76 pt
        // with one forward lane, the complement row and one reverse lane
        // (7 rows, 420 bases). That 53% is what the toggle is for, and it is
        // why nothing here is on by default except the file's own translations.
        //
        // THE BOTTOM STRAND IS PART OF THIS CHANGE, not deferred. Two of pKoV's
        // three translated CDSs are on it, and a reverse translation painted
        // under a top-strand-only view reads C-terminus to N-terminus left to
        // right with nothing on screen saying so. So the reverse residue lanes
        // are GATED on the complement row: turn the strand off and they go with
        // it, and the header says what is now hidden. That makes the misleading
        // case impossible by construction rather than by care.
        let ds = self
            .document()
            .expect("checked by caller")
            .molecule()
            .double_stranded;
        // `None` is a real third state and every reader except SnapGene
        // produces it. Drawn double, because a plasmid GenBank is
        // double-stranded in fact — and the header says which was assumed.
        let complement = self.layout.complement.unwrap_or(ds != Some(false));
        let aa_on = self.layout.aa_track.is_on();
        // Reserved as soon as the selection mode is on, for the whole document
        // — not when a selection appears. Otherwise making a selection changes
        // the row pitch in the middle of the drag that is making it.
        //
        // AND IT IS A LANE OF ITS OWN, always. The first version of this
        // arithmetic got that wrong in the one way that draws letters belonging
        // to no protein: it reserved `min(fwd_lanes + sel_lane, MAX_AA_LANES)`
        // and put the selection at `min(fwd_lanes, MAX_AA_LANES - 1)`, so a
        // document already using both lanes on a strand — two overlapping
        // same-strand CDSs, which is a vector plus a tagged variant, or any
        // stretch of MG1655 — reserved 2 and drew the selection in lane 1, where
        // a file translation already was. Both were painted at the same y and
        // interleaved a column apart: `M K RA GV CA M* KN RA ...`, where the
        // `M*` a reader sees is one protein's methionine beside another's stop.
        // The `+N` badge never fired, because 1 < 2.
        //
        // `Translations::{fwd_lanes,rev_lanes}` is ALREADY capped at
        // `MAX_AA_LANES`, so it is both the number of file lanes drawn and the
        // first free lane index on that strand. Using it as the selection's lane
        // makes the collision impossible by construction, rather than by a
        // second cap that has to agree with the first.
        let sel_lane = u8::from(self.layout.aa_track == aa::TrackMode::Selection);
        let strips = seqedit::RowStrips {
            enz_h,
            tick_h: TICK_H,
            text_h,
            lane_pitch: LANE_PITCH,
            lanes,
            aa_fwd: if aa_on {
                self.tr.fwd_lanes + sel_lane
            } else {
                0
            },
            aa_rev: if aa_on && complement {
                self.tr.rev_lanes + sel_lane
            } else {
                0
            },
            complement,
            orf_h: if self.orf_strip && self.layout.orf_track {
                6.0 * ORF_LANE_H
            } else {
                0.0
            },
        };
        let row_h = strips.row_h();

        // A pending run is not in the log, so the digest describes the
        // COMMITTED sequence. A typed base can create or destroy a site, so a
        // mark translated into effective coordinates is not merely displaced —
        // it can be a site that no longer exists, drawn confidently.
        let typing = self.edit.run().is_some();
        let show_cuts = !typing && self.annot.cut_count() > 0;

        self.sequence_header(ui, n, rows, per_row, has_cuts, typing);
        self.sequence_keys(ui, now);

        // The scroll follows a BASE across a reflow, not a pixel.
        //
        // `show_rows` maps a pixel offset to a row index and multiplies by
        // `per_row`. The offset does not change when `per_row` does, so on the
        // user's 8,117 bp file a view of base 4,000 at 40 per row (offset
        // 1,330) becomes base 6,000 at 60 per row — 2,000 bases forward, while
        // the user is dragging a splitter. Worse, it is not reversible: the
        // content shrinks from 2,700 pt to 1,809, so an offset near the bottom
        // is clamped on the way out and not restored on the way back, and that
        // asymmetry is what reads as broken rather than merely jumpy.
        let reflowed = (self.seq_per_row != 0 && self.seq_per_row != per_row)
            || (self.seq_row_h != 0.0 && (self.seq_row_h - row_h).abs() > 0.01);
        //
        // `keep` is how far down the viewport the anchored row was sitting. Put
        // back at offset zero instead, a reflow yanked the page: the caret was
        // on the second visible row, and after one keystroke on the genome its
        // row had been dragged to the first slot. What the reader wants
        // preserved is where the thing they are looking at IS, not merely that
        // it is still on screen.
        let (anchor, keep) = if reflowed {
            let old = self.seq_per_row.max(1);
            let caret = self.edit.caret.min(n);
            let caret_row = seqedit::row_of(caret, old);
            let first = self.seq_grid.map_or(0, |g| g.first_row);
            // On screen means the user is editing and wants to keep watching
            // the caret; off screen means they are reading and want to keep
            // their place.
            if caret_row >= first && caret_row < first + self.edit.visible_rows.max(1) {
                (caret, caret_row - first)
            } else {
                (first * old, 0)
            }
        } else {
            (0, 0)
        };
        self.seq_per_row = per_row;
        self.seq_row_h = row_h;

        let mut click: Option<(u64, bool)> = None;
        // A codon picked off a residue lane: `(lo, hi, the residue)`.
        let mut codon_click: Option<(u64, u64, aa::Residue)> = None;
        let mut drag_to: Option<u64> = None;
        let mut released = false;
        let mut double: Option<u64> = None;
        let mut visible = 1u64;
        let mut grid: Option<GridGeom> = None;
        let mut hover_out: Option<String> = None;
        let mut scratch = std::mem::take(&mut self.annot_scratch);

        // Reserved by construction, not by a constant, and laid out BEFORE the
        // thing that would otherwise have eaten its space.
        //
        // `readout_h = 30.0` was a guess at the height of a sentence that is 72
        // characters long — "insert at 1 · before base 1, at the origin · every
        // feature's coordinates shift" — and wraps to two lines in a 364 pt
        // content width, with two buttons and a warning line after it. Against
        // a 30 pt reservation the overflow was drawn past the panel rect and
        // cut by `set_clip_rect(visible_outer_rect)`, which is why the origin
        // warning lost its second half and the Design primers button was
        // sheared through the middle. A bottom panel sizes itself to its
        // contents and takes that space out of the enclosing available rect
        // before the grid asks, so `readout_h`, `grid_h`, the `.max(60.0)`
        // floor that made a short window *worse* rather than better, and
        // `.max_height(grid_h)` are all gone, and the class of bug with them.
        self.sequence_readout(ui);

        {
            {
                let d = self.bench.get().expect("checked by caller");
                let mol = d.molecule();
                let edit = &self.edit;
                let ix = &self.annot;
                let p = pal(ui);
                let sel = edit
                    .sel
                    .map(|s| s.canonical(mol.len(), mol.topology.is_circular()));
                let caret = edit.caret.min(n);
                let run = edit.run().map(|r| r.span());
                let mut line = String::with_capacity(per_row as usize);
                let tr = &self.tr;
                let orfs = d.orfs.done();
                // Every base the track reads comes through the same accessor
                // that produces the LETTERS, so a codon and the letters
                // directly above it cannot disagree while somebody is typing.
                let read = |i: u64| edit.byte_at(mol, i);

                // The ad-hoc translation of the selection: a VIEW of the bases
                // the user pointed at, with no `OpKind` behind it and nothing
                // written to the molecule. It is the front door for "is my His
                // tag in frame?" on a feature the file never marked translated,
                // which is exactly what pKoV's `decR his` is.
                //
                // The strand is the one the drag went in. A selection dragged
                // right to left asks for the reverse reading, which is the only
                // gesture available that carries the fact — and it is only
                // OFFERED when the complement row is on, so a reverse
                // translation never appears without the strand it reads.
                //
                // XOR THE WRAP BIT, and this line read `s.head < s.anchor` alone
                // until a reviewer dragged through the origin. For an ordinary
                // selection the caret ordering IS the direction of travel; for a
                // wrapping one it is inverted, because the arc is
                // `[hi, n) ∪ [0, lo)` and travelling FORWARD across the origin
                // therefore ends at a caret BELOW the one it started from —
                // which is exactly the state the drag handler builds (`wrapped`
                // requires `to < anchor`). So every left-to-right wrap-drag was
                // read as reverse: on a 120 bp circle the arc 116..13 spells
                // MKRGC* on the strand the pointer ran along, and the track drew
                // LATAFH in the reverse lane. With the complement row off the
                // same gesture drew nothing at all and said nothing.
                let sel_path = (self.layout.aa_track == aa::TrackMode::Selection)
                    .then_some(edit.sel)
                    .flatten()
                    .filter(|s| !s.is_empty(mol.len()))
                    .and_then(|s| {
                        let reverse = (s.head < s.anchor) != s.through_origin;
                        if reverse && !complement {
                            return None;
                        }
                        let c = s.canonical(mol.len(), mol.topology.is_circular());
                        // One arc, or two when it crosses the origin —
                        // `Selection` already says which, and the path takes
                        // both pieces in reading order.
                        let mut parts = if c.through_origin {
                            vec![(c.hi(), n), (0, c.lo())]
                        } else {
                            vec![(c.lo(), c.hi())]
                        };
                        if reverse {
                            parts.reverse();
                        }
                        Some(aa::Path {
                            feat: aa::SELECTION,
                            name: "selection".into(),
                            reverse,
                            code: self.doc_code,
                            parts,
                            skip: 0,
                            // The first free lane on this strand, never one a
                            // file translation owns. See the reservation above.
                            lane: if reverse { tr.rev_lanes } else { tr.fwd_lanes },
                            from_flag: false,
                            bad_codon_start: None,
                        })
                    });

                if debug_geometry() {
                    eprintln!(
                        "seqtab: max={:?} avail_h={:.1} clip={:?} row_h={row_h:.2}",
                        ui.max_rect(),
                        ui.available_height(),
                        ui.clip_rect()
                    );
                }

                // -- the sticky column header, outside the ScrollArea --------
                //
                // Labelled in COLUMNS, not in absolute coordinates: a sticky
                // header cannot carry per-row numbers. The arithmetic that
                // would otherwise demand — row_start + col - 1, with an
                // off-by-one waiting in it — is never asked of the reader,
                // because the right gutter gives the row's last coordinate
                // directly and the hover line names the base under the pointer.
                let (hrect, _) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 13.0), Sense::hover());
                {
                    let hp = ui.painter_at(hrect);
                    let hx = hrect.left();
                    hp.text(
                        egui::pos2(hx + layout.left_gutter - 6.0, hrect.bottom() - 3.0),
                        egui::Align2::RIGHT_BOTTOM,
                        "column",
                        ruler_font.clone(),
                        p.muted,
                    );
                    for k in 1..=(per_row / 10) {
                        let x = hx + layout.col_x(k * 10);
                        hp.vline(
                            x,
                            (hrect.bottom() - 4.0)..=hrect.bottom(),
                            egui::Stroke::new(1.0, p.muted),
                        );
                        // Right-aligned, so the label's right edge lands on the
                        // boundary *after* the base it counts.
                        hp.text(
                            egui::pos2(x - 1.0, hrect.bottom() - 4.0),
                            egui::Align2::RIGHT_BOTTOM,
                            (k * 10).to_string(),
                            ruler_font.clone(),
                            p.muted,
                        );
                    }
                }

                // `show_rows` computes its pitch as `row_height +
                // item_spacing.y` from the *enclosing* ui, so zeroing the
                // spacing here makes pitch == row_h and leaves one number for
                // the renderer, the hit-test and the caret.
                ui.spacing_mut().item_spacing.y = 0.0;

                let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
                if reflowed {
                    // ONLY on the frame the row width changed. Passing it on
                    // any other frame pins the offset and the user cannot
                    // scroll at all.
                    area = area.vertical_scroll_offset(
                        seqedit::row_of(anchor, per_row).saturating_sub(keep) as f32 * row_h,
                    );
                }
                area.show_rows(ui, row_h, rows, |ui, range| {
                    visible = (range.end - range.start).max(1) as u64;
                    let first = range.start as u64;
                    let band = ui.max_rect();
                    let height = (range.end - range.start) as f32 * row_h;
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(band.width(), height), Sense::hover());
                    // A stable id, not the auto id: `show_rows` shifts auto ids
                    // by the first visible row, so an auto id would change
                    // identity on every scroll and drop the drag mid-gesture.
                    let resp = ui.interact(
                        rect,
                        egui::Id::new("pl-sequence-grid"),
                        Sense::click_and_drag(),
                    );
                    let x0 = rect.left() + layout.bases_x;
                    grid = Some(GridGeom {
                        x0,
                        advance,
                        top: rect.top(),
                        row_h,
                        first_row: first,
                        per_row,
                        strips,
                    });
                    let painter = ui.painter_at(rect);
                    if debug_geometry() {
                        // The grid's own numbers, because a screenshot cannot
                        // settle whether a base sits inside the panel: this
                        // helper process is not per-monitor DPI aware, so
                        // Windows tells anything measuring the window
                        // *virtualised* coordinates.
                        // Read off `grid` rather than off the locals, so the
                        // numbers printed are exactly the ones a test will
                        // later put a pointer on.
                        let g = grid.expect("set immediately above");
                        eprintln!(
                            "seqgrid: rect={:?} clip={:?} x0={:.1} top={:.1} advance={:.2} \
                             row_h={:.2} per_row={} first_row={} lanes={lanes} \
                             right_gutter={:.1} right_edge={:.1}",
                            rect,
                            ui.clip_rect(),
                            g.x0,
                            g.top,
                            g.advance,
                            g.row_h,
                            g.per_row,
                            g.first_row,
                            layout.right_gutter,
                            g.x0 + layout.band_w()
                        );
                        // The strips too, because a screenshot cannot settle
                        // which band a y is in either — and the y offsets are
                        // exactly what a click on a residue now depends on.
                        eprintln!(
                            "seqstrips: enz={:.2} tick={:.2} text={:.2} aa_fwd={} aa_rev={}                              complement={} orf={:.2} lanes={} row_h={:.2}                              y_tick={:.2} y_aa_fwd0={:.2} y_text={:.2} y_comp={:.2}                              y_aa_rev0={:.2} y_orf={:.2} y_lane={:.2}",
                            g.strips.enz_h,
                            g.strips.tick_h,
                            g.strips.text_h,
                            g.strips.aa_fwd,
                            g.strips.aa_rev,
                            g.strips.complement,
                            g.strips.orf_h,
                            g.strips.lanes,
                            g.strips.row_h(),
                            g.strips.y_tick(),
                            g.strips.y_aa_fwd(0),
                            g.strips.y_text(),
                            g.strips.y_comp(),
                            g.strips.y_aa_rev(0),
                            g.strips.y_orf(),
                            g.strips.y_lane(),
                        );
                    }

                    // THE ONLY PRODUCER OF AN X IN THIS FUNCTION.
                    //
                    // Not a convenience: the sentence in `RowLayout`'s own doc
                    // comment is only true if it is. The strips added by this
                    // change — tens ticks, enzyme chevron and bracket, ribbon
                    // ends, margin swatches — each had their own inline
                    // `x0 + col * advance`, so a mutation that grouped the
                    // LETTERS in tens by spacing moved the letters and left
                    // every annotation on the nominal grid, detaching each
                    // ribbon from the base it annotates while the whole suite
                    // stayed green.
                    let cx = |col: u64| rect.left() + layout.col_x(col);

                    // The one place a pointer becomes a caret. `x_col` is the
                    // only consumer of an x in this file, so the four things
                    // that used to compute this inline — this, the drag, the
                    // caret vline and the selection rectangle — cannot drift
                    // apart. The pointer's ROW, shared with the hover below.
                    let row_at = |pos: egui::Pos2| -> u64 {
                        (((pos.y - rect.top()) / row_h).floor() as i64)
                            .clamp(0, (range.end - range.start) as i64 - 1)
                            as u64
                            + first
                    };
                    let hit = |pos: egui::Pos2| -> u64 {
                        let col = layout.x_col(pos.x - x0);
                        (row_at(pos) * per_row + col).min(n)
                    };
                    // Which BAND of the row the pointer is in. One consumer of
                    // the y offsets, as `x_col` is the one consumer of an x.
                    let strip_at = |pos: egui::Pos2| -> seqedit::Strip {
                        let dy = (pos.y - rect.top()) - (row_at(pos) - first) as f32 * row_h;
                        strips.strip_at(dy.clamp(0.0, row_h - 0.01))
                    };
                    // Is this path's lane one this document actually draws?
                    //
                    // ONE rule, shared by the painter and the hit-test, because
                    // the two disagreeing is how a click reports a residue the
                    // user cannot see. A file translation is drawn only below
                    // the strand's file-lane count; the ad-hoc selection sits AT
                    // that count and is always drawn, which is what reserves it
                    // a lane of its own.
                    let drawn = |p: &aa::Path| -> bool {
                        let file_lanes = if p.reverse { tr.rev_lanes } else { tr.fwd_lanes };
                        let strip = if p.reverse { strips.aa_rev } else { strips.aa_fwd };
                        if p.feat == aa::SELECTION {
                            p.lane < strip
                        } else {
                            p.lane < file_lanes.min(strip)
                        }
                    };
                    // The codon under a pointer on a residue lane.
                    //
                    // `x_base`, the FLOOR question, and not `x_col`'s round:
                    // the user is pointing AT a residue, not at a gap between
                    // two. Sharing one mapping between the two was wrong over
                    // the right half of every cell and this view already paid
                    // for it once.
                    //
                    // Returns the base the pointer is ON as well as the residue:
                    // a codon that straddles a join or the origin is not three
                    // adjacent cells, and the caller has to know WHICH of its
                    // three the user pointed at before it can select anything.
                    let codon_at =
                        |pos: egui::Pos2, lane: u8, reverse: bool| -> Option<(aa::Residue, u64)> {
                            let col = layout.x_base(pos.x - x0)?;
                            let at = row_at(pos) * per_row + col;
                            if at >= n {
                                return None;
                            }
                            tr.paths()
                                .iter()
                                .chain(sel_path.iter())
                                // `drawn` and not merely the lane number: a file
                                // translation past the cap keeps its real lane,
                                // which can equal the selection's, and finding
                                // it first would report a residue that is not on
                                // screen and select its codon.
                                .filter(|p| p.reverse == reverse && p.lane == lane && drawn(p))
                                .find_map(|p| {
                                    let ep = p.effective(run);
                                    let k = ep.residue_covering(at)?;
                                    let r = (k < ep.aa_len()).then(|| ep.residue(k, &read))??;
                                    Some((r, at))
                                })
                        };

                    // Allocated once per frame and cleared per row, so a
                    // forty-row viewport does not allocate forty vectors.
                    let mut aa_paths: Vec<&aa::Path> = Vec::new();
                    let mut residues: Vec<aa::Residue> = Vec::new();
                    let mut aa_buf: Vec<u8> = Vec::new();
                    let mut orf_scratch: Vec<annot::Iv> = Vec::new();

                    for r in range.clone() {
                        let r = r as u64;
                        let start = r * per_row;
                        let end = (start + per_row).min(n);
                        let y = rect.top() + (r - first) as f32 * row_h;
                        // ONE producer of a y offset, exactly as `cx` is the
                        // one producer of an x. These four used to be computed
                        // inline here and nowhere else, so the hit-test knew
                        // only the row and mapped any y in the band onto the
                        // letters — which with a residue lane above them and a
                        // complement row below is a click that silently moves
                        // the caret sixty bases.
                        let y_tick = y + strips.y_tick();
                        let y_text = y + strips.y_text();
                        let y_comp = y + strips.y_comp();
                        let y_lane = y + strips.y_lane();

                        // -- selection: one rectangle per row per contiguous
                        // range, clipped against this row. Nothing iterates the
                        // selection itself.
                        if let Some(s) = sel {
                            let spans: [(u64, u64); 2] = if s.through_origin {
                                [(0, s.lo()), (s.hi(), n)]
                            } else {
                                [(s.lo(), s.hi()), (0, 0)]
                            };
                            for (a, b) in spans {
                                let (a, b) = (a.max(start), b.min(end));
                                if b > a {
                                    let xa = cx(a - start);
                                    let xb = cx(b - start);
                                    painter.rect_filled(
                                        egui::Rect::from_min_max(
                                            egui::pos2(xa, y_text),
                                            egui::pos2(xb, y_text + text_h),
                                        ),
                                        0.0,
                                        p.selection(),
                                    );
                                    // Edges, so a selection running off a row
                                    // edge is distinguishable from one ending
                                    // there. The wash alone cannot say which.
                                    let edge = egui::Stroke::new(1.0, p.accent);
                                    if a > start || a == 0 {
                                        painter.vline(xa, y_text..=(y_text + text_h), edge);
                                    }
                                    if b < end || b == n {
                                        painter.vline(xb, y_text..=(y_text + text_h), edge);
                                    }
                                }
                            }
                        }

                        // -- the row's coordinates -------------------------
                        painter.text(
                            egui::pos2(rect.left() + layout.left_gutter - 6.0, y_text),
                            egui::Align2::RIGHT_TOP,
                            fmt_int(start + 1),
                            gutter_font.clone(),
                            p.muted,
                        );
                        if layout.right_gutter > 0.0 && end > start {
                            painter.text(
                                egui::pos2(cx(per_row) + 6.0, y_text),
                                egui::Align2::LEFT_TOP,
                                fmt_int(end),
                                gutter_font.clone(),
                                p.muted,
                            );
                        }

                        // -- grouping in tens, by PAINTING and not by spacing.
                        //
                        // A real gap every ten characters would move the x of a
                        // base off `col * advance` and the error would be zero
                        // at column 0 and five whole cells at column 55 — right
                        // in a screenshot, wrong at base 47. It would also put
                        // a hole in the caret model: a separator column is both
                        // a rule and a legal caret position, and the pixels
                        // inside it belong to neither neighbouring base.
                        //
                        // 3 px and confined to the top edge, so a tick at a
                        // multiple of ten is never mistaken for the 1.5 px
                        // accent caret, which can sit on exactly the same x.
                        for k in 1..=(per_row / 10) {
                            let x = cx(k * 10);
                            painter.vline(
                                x,
                                y_tick..=(y_tick + TICK_H),
                                egui::Stroke::new(1.0, p.muted),
                            );
                        }

                        // -- enzymes, above the letters --------------------
                        let mut hidden = 0usize;
                        if show_cuts {
                            let mut name_free_from = f32::MIN;
                            // SITES touching the row, not cuts inside it. A
                            // recognition sequence is six or more bases and a
                            // row boundary falls wherever it falls: NcoI's
                            // CCATGG at pKoV 6,119..6,124 cuts at 6,120, so the
                            // row before drew a two-cell stub at its right edge
                            // and the row whose first four bases ARE the rest
                            // of that site drew nothing at all.
                            for c in ix.sites_touching(start, end, n) {
                                // The recognition site, as a bracket. Drawn
                                // from `site_lo` and not from the cut: EcoRI is
                                // G^AATTC, so the two are one base apart, and
                                // `pl-enzymes` says site_start is not
                                // recoverable from a cut position at all.
                                //
                                // Two spans, because a match on a circle can
                                // run off the end and continue at base 1.
                                let site_end = c.site_lo + c.site_len as u64;
                                let wraps = site_end > n;
                                // (lo, hi, lo is the site's real start, hi is
                                // its real end). The origin is neither.
                                let spans = [
                                    (c.site_lo, site_end.min(n), true, !wraps),
                                    if wraps {
                                        (0, site_end - n, false, true)
                                    } else {
                                        (0, 0, false, false)
                                    },
                                ];
                                for (lo, hi, real_lo, real_hi) in spans {
                                    let s_lo = lo.max(start);
                                    let s_hi = hi.min(end);
                                    if s_hi <= s_lo {
                                        continue;
                                    }
                                    let bx0 = cx(s_lo - start) + 0.5;
                                    let bx1 = cx(s_hi - start) - 0.5;
                                    let by = y_tick - 2.0;
                                    let st = egui::Stroke::new(1.0, p.ink2);
                                    painter.hline(bx0..=bx1, by, st);
                                    // The bracket's feet only where the site
                                    // really ends, so a site continuing onto
                                    // the next row — or across the origin — is
                                    // not drawn as one that stops there.
                                    if real_lo && lo >= start {
                                        painter.vline(bx0, (by - 2.0)..=by, st);
                                    }
                                    if real_hi && hi <= end {
                                        painter.vline(bx1, (by - 2.0)..=by, st);
                                    }
                                }
                            }
                            // And the cuts, on the row that owns the BOND.
                            //
                            // A second loop and not a filter on the first: a
                            // Type IIS enzyme cuts outside its own site — BsaI
                            // is GGTCTC(1/5) — so a site at the end of one row
                            // can be nicked on the next, and neither set
                            // contains the other.
                            for c in ix.cuts_in(start, end) {
                                let x = cx(c.at - start);
                                let cs = egui::Stroke::new(1.2, p.ink2);
                                painter.line_segment(
                                    [
                                        egui::pos2(x - 3.0, y_tick - 6.0),
                                        egui::pos2(x, y_tick - 1.0),
                                    ],
                                    cs,
                                );
                                painter.line_segment(
                                    [
                                        egui::pos2(x + 3.0, y_tick - 6.0),
                                        egui::pos2(x, y_tick - 1.0),
                                    ],
                                    cs,
                                );
                                let name = d
                                    .digest
                                    .results()
                                    .get(c.enzyme as usize)
                                    .map(|dg| dg.enzyme.name)
                                    .unwrap_or("?");
                                let g = painter.layout_no_wrap(
                                    name.to_string(),
                                    name_font.clone(),
                                    p.ink2,
                                );
                                let nx = x + 3.0;
                                if nx >= name_free_from && nx + g.size().x <= rect.right() {
                                    name_free_from = nx + g.size().x + 4.0;
                                    painter.galley(egui::pos2(nx, y), g, p.ink2);
                                } else {
                                    // Not dropped. Counted, and named in the
                                    // hover line: `docs/PLAN.md` item 33 is
                                    // about a hidden site, and this is the same
                                    // failure in the drawing layer.
                                    hidden += 1;
                                }
                            }
                        }

                        // -- features, below the letters, one lane each ----
                        //
                        // Lanes, never composited tints: two alpha-blended
                        // feature colours over the same base is mud whose
                        // contrast cannot be known at build time. Inside a lane
                        // a feature owns its pixels at alpha 1.0, so depth is
                        // countable by eye.
                        scratch.clear();
                        ix.query_run(start, end, run, &mut scratch);
                        // Lanes are coloured once over the whole document so a
                        // feature cannot hop while the user scrolls, which is
                        // right for everything that has a lane and wrong for
                        // everything past the cap: a file six deep somewhere
                        // hid lanes 3, 4 and 5 over their whole length, on rows
                        // where lanes 1 and 2 were empty. Only the overflow is
                        // re-placed, into this row's holes.
                        hidden += annot::compact_row(&mut scratch, lanes);
                        for iv in scratch.iter() {
                            if iv.lane >= lanes {
                                continue;
                            }
                            let Some(f) = mol.features.get(iv.feat as usize) else {
                                continue;
                            };
                            let a = iv.lo.max(start);
                            let b = iv.hi.min(end);
                            if b <= a {
                                continue;
                            }
                            let xa = cx(a - start);
                            let xb = cx(b - start);
                            let yl = y_lane + iv.lane as f32 * LANE_PITCH;
                            let col = theme::feature_color(f);
                            let rr = egui::Rect::from_min_max(
                                egui::pos2(xa, yl),
                                egui::pos2(xb, yl + RIBBON_H),
                            );
                            painter.rect_filled(rr, 0.0, col);
                            // The outline is not decoration. Sweeping the RGB
                            // cube, 43.9% of the colours a file can supply are
                            // below 3:1 against the light panel and 25.9%
                            // against the dark one, worst case 1.00:1 — so
                            // without it nearly half of all real files draw an
                            // invisible ribbon in light mode while looking
                            // perfect in the dark-mode screenshot the developer
                            // took. `muted` is the only palette role that
                            // clears 3:1 in both themes.
                            painter.rect_stroke(
                                rr,
                                0.0,
                                egui::Stroke::new(1.0, p.muted),
                                egui::StrokeKind::Inside,
                            );

                            // Boundaries, by shape: a ribbon that runs off the
                            // right edge of a row and one that ends there look
                            // identical otherwise.
                            //
                            // Two marks, because they say different things. A
                            // full-height rule through the lane strip is the
                            // FEATURE's own 5' or 3' end. A short rule confined
                            // to the ribbon is an exon boundary — the edge of
                            // one segment of a join, with more of the same
                            // feature further along. pKoV's SacB used to get
                            // the tall one in the middle of itself.
                            let lane_bot = y_lane + lanes as f32 * LANE_PITCH;
                            let bs = egui::Stroke::new(1.0, p.muted);
                            let tick = |x: f32, whole: bool| {
                                let top = if whole { y_lane - 2.0 } else { yl - 1.0 };
                                let bot = if whole { lane_bot } else { yl + RIBBON_H + 1.0 };
                                painter.vline(x, top..=bot, bs);
                            };
                            if iv.starts && iv.lo >= start && iv.lo < end {
                                tick(xa, iv.feat_lo);
                            }
                            if iv.ends && iv.hi > start && iv.hi <= end {
                                tick(xb, iv.feat_hi);
                            }
                            // Direction, by shape and not by colour: a solid
                            // cap on the 3' terminus, the same grammar the map
                            // already uses.
                            //
                            // INSIDE the ribbon. Drawn from `xb + 4.0` it was
                            // 4 px — 0.58 of a base cell — of this feature's
                            // ink painted over the next one, so an abutting
                            // neighbour read as two thirds of a base shorter
                            // than the file says it is. Nothing may paint
                            // outside its own coordinates, which is the rule
                            // the rest of this view is careful about.
                            let cap = |tip_x: f32, back_x: f32| {
                                painter.add(egui::Shape::convex_polygon(
                                    vec![
                                        egui::pos2(tip_x, yl + RIBBON_H * 0.5),
                                        egui::pos2(back_x, yl - 1.0),
                                        egui::pos2(back_x, yl + RIBBON_H + 1.0),
                                    ],
                                    col,
                                    egui::Stroke::new(1.0, p.muted),
                                ));
                            };
                            // Never wider than the ribbon it sits in, so a one-
                            // or two-base feature is still an arrow and still
                            // covers exactly its bases.
                            let cap_w = 4.0f32.min(xb - xa);
                            match f.strand {
                                // The FEATURE's terminus, not the segment's: a
                                // two-exon gene has one 3' end, and a reverse
                                // one has it at the low end of its lowest
                                // piece.
                                Strand::Forward if iv.feat_hi && iv.hi <= end => {
                                    cap(xb, xb - cap_w)
                                }
                                Strand::Reverse if iv.feat_lo && iv.lo >= start => {
                                    cap(xa, xa + cap_w)
                                }
                                _ => {}
                            }
                        }

                        // -- the bases: still one call, one colour, one font.
                        edit.row_text(mol, start, end, &mut line);
                        painter.text(
                            egui::pos2(cx(0), y_text),
                            egui::Align2::LEFT_TOP,
                            &line,
                            font.clone(),
                            p.ink,
                        );

                        // -- the bottom strand, in the top strand's own
                        // coordinates ----------------------------------------
                        //
                        // NO reverse and NO reverse_complement: column c of
                        // this row is the Watson-Crick partner of column c
                        // above it, which is the physical duplex and keeps one
                        // coordinate space for both strands. `iupac::complement`
                        // preserves case, so the lowercase/uppercase signal an
                        // eye scans for survives on both strands.
                        //
                        // A read-only mirror. No caret of its own, no selection
                        // of its own, no enzyme marks and no ribbons: a click on
                        // it places the caret on the same base. `Selection`,
                        // `through_origin`, `hit` and every op derivation are
                        // untouched.
                        if strips.complement {
                            edit.row_complement(mol, start, end, &mut line);
                            painter.text(
                                egui::pos2(cx(0), y_comp),
                                egui::Align2::LEFT_TOP,
                                &line,
                                font.clone(),
                                p.ink,
                            );
                        }

                        // -- residues ----------------------------------------
                        if aa_on {
                            // The lane's own baseline, on every row of a
                            // document that reserves the lane at all. An empty
                            // reserved strip is never blank: it says "this lane
                            // exists here and has nothing in it", which is true
                            // and is what a reader needs — an empty aa lane
                            // means no annotated protein reads through these
                            // bases. Measured on pKoV, 82 of 136 rows are in
                            // exactly that state. `muted` and not `faint`
                            // because it is the only palette role clearing 3:1
                            // against both panels.
                            let bl = egui::Stroke::new(1.0, p.muted);
                            for k in 0..strips.aa_fwd {
                                let yb = y + strips.y_aa_fwd(k) + text_h - 1.0;
                                painter.hline(cx(0)..=cx(per_row), yb, bl);
                            }
                            for k in 0..strips.aa_rev {
                                let yb = y + strips.y_aa_rev(k) + text_h - 1.0;
                                painter.hline(cx(0)..=cx(per_row), yb, bl);
                            }

                            aa_paths.clear();
                            for iv in scratch.iter() {
                                // No second interval query: the ribbons already
                                // asked which features touch this row, and
                                // every translated feature is one of them.
                                if let Some(path) = tr.for_feature(iv.feat) {
                                    if !aa_paths.iter().any(|q: &&aa::Path| q.feat == path.feat) {
                                        aa_paths.push(path);
                                    }
                                }
                            }
                            if let Some(sp) = &sel_path {
                                aa_paths.push(sp);
                            }

                            for path in aa_paths.iter() {
                                if !drawn(path) {
                                    // Past the cap. Counted into the row's
                                    // orange `+N`, which already means "N
                                    // things on this row I could not show you"
                                    // — so the badge and the lanes cannot
                                    // contradict each other on one row. The
                                    // SAME predicate the hit-test uses, so a
                                    // click can never report a residue that was
                                    // counted here instead of drawn.
                                    hidden += 1;
                                    continue;
                                }
                                let ep = path.effective(run);
                                let y0 = y + if ep.reverse {
                                    strips.y_aa_rev(ep.lane)
                                } else {
                                    strips.y_aa_fwd(ep.lane)
                                };
                                residues.clear();
                                ep.residues_in_row(start, end, &read, &mut residues);
                                if residues.is_empty() {
                                    continue;
                                }

                                // ONE `painter.text` for the ordinary residues,
                                // at `cx(0)`, in the SAME `FontId` as the bases.
                                // Because `layout.advance` is that font's glyph
                                // width, a residue placed at column c occupies
                                // exactly `[cx(c), cx(c+1))` and its codon's
                                // three cells are `[cx(c-1), cx(c+2))`,
                                // symmetric about it BY CONSTRUCTION. There is
                                // no second x anywhere in this track and no
                                // centring arithmetic: measuring the glyph's
                                // galley and centring on it would be a second
                                // producer of an x, which is exactly the drift
                                // `RowLayout`'s doc comment records.
                                aa_buf.clear();
                                aa_buf.resize(per_row as usize, b' ');
                                for res in residues.iter() {
                                    let col = (res.mid() - start) as usize;
                                    if res.mark == aa::Mark::Plain {
                                        aa_buf[col] = res.aa;
                                    }
                                }
                                painter.text(
                                    egui::pos2(cx(0), y0),
                                    egui::Align2::LEFT_TOP,
                                    // ASCII by construction: `Code::codon`
                                    // returns an amino-acid letter, `*` or `X`.
                                    String::from_utf8_lossy(&aa_buf),
                                    font.clone(),
                                    p.ink2,
                                );

                                for res in residues.iter() {
                                    let col = res.mid() - start;
                                    let (x0c, x1c) = (cx(col), cx(col + 1));
                                    // Codon boundaries, inside the aa strip and
                                    // not at `y_tick`, so they cannot be
                                    // confused with the tens ruler. At the
                                    // default 60 bases per row every codon
                                    // column is identical on every row and the
                                    // track reads as columns; at 50 the phase
                                    // walks by two a row and the residues form
                                    // a diagonal. The drawing stays right
                                    // either way — the header says so when it
                                    // is not.
                                    let (lo, hi) = res.span();
                                    if res.contiguous && lo >= start {
                                        painter.vline(
                                            cx(lo - start),
                                            y0..=(y0 + 2.0),
                                            egui::Stroke::new(1.0, p.muted),
                                        );
                                    }
                                    if !res.contiguous {
                                        // The three cells under this glyph are
                                        // NOT its three bases: it spans a join
                                        // or the origin. Unmarked, a reader
                                        // takes the cells for the codon and
                                        // reads a triplet that is not there.
                                        let ty = y0 + 1.0;
                                        let ts = egui::Stroke::new(1.0, p.accent);
                                        if lo + 1 < res.mid() || lo > res.mid() {
                                            painter.hline(cx(0)..=x0c, ty, ts);
                                        }
                                        if hi > res.mid() + 2 || hi <= res.mid() {
                                            painter.hline(x1c..=cx(per_row), ty, ts);
                                        }
                                    }
                                    if res.mark == aa::Mark::Plain {
                                        continue;
                                    }
                                    // Colour is never the only channel, so each
                                    // of these carries a shape as well.
                                    let colour = match res.mark {
                                        aa::Mark::StopInside => p.warn,
                                        _ => p.ink2,
                                    };
                                    let ub = y0 + text_h - 2.0;
                                    match res.mark {
                                        aa::Mark::StopInside => {
                                            // Loud on purpose. An internal stop
                                            // means the annotation is wrong or
                                            // an insert is out of frame, and a
                                            // reader who has to notice one red
                                            // asterisk among 470 residues will
                                            // not. Also counted in the header.
                                            painter.rect_filled(
                                                egui::Rect::from_min_max(
                                                    egui::pos2(x0c, ub),
                                                    egui::pos2(x1c, ub + 2.0),
                                                ),
                                                0.0,
                                                p.warn,
                                            );
                                        }
                                        aa::Mark::StopEnd | aa::Mark::AmbiguousStop => {
                                            let m = (x0c + x1c) * 0.5;
                                            painter.rect_filled(
                                                egui::Rect::from_min_max(
                                                    egui::pos2(m - 1.5, ub),
                                                    egui::pos2(m + 1.5, ub + 2.0),
                                                ),
                                                0.0,
                                                p.muted,
                                            );
                                        }
                                        aa::Mark::Initiator | aa::Mark::Ambiguous => {
                                            // A dotted rule, because the LETTER
                                            // is not what the codon spells:
                                            // `M` for a GTG initiator, `X` for
                                            // a codon that could be two things.
                                            let st = egui::Stroke::new(1.0, p.muted);
                                            let w = (x1c - x0c) / 5.0;
                                            for k in 0..3 {
                                                let a = x0c + w * (k as f32 * 2.0);
                                                painter.hline(a..=(a + w), ub + 1.0, st);
                                            }
                                        }
                                        aa::Mark::Plain => {}
                                    }
                                    painter.text(
                                        egui::pos2(x0c, y0),
                                        egui::Align2::LEFT_TOP,
                                        (res.aa as char).to_string(),
                                        font.clone(),
                                        colour,
                                    );
                                }
                            }
                        }

                        // -- open reading frames, one bar per frame ----------
                        //
                        // A DIFFERENT GRAMMAR from the residues, deliberately:
                        // outline bars, never letters, in their own strip. A
                        // plasmid's CDSs are what somebody asserted; its ORFs
                        // are what the sequence permits, and drawn in the same
                        // channel the ORF's start reads as a correction of the
                        // annotation. Measured on pKoV: 3 annotated CDSs
                        // against 103 ORFs, and the scan puts CmR's start 63
                        // bases upstream of the annotation. Somebody orders a
                        // primer.
                        if strips.orf_h > 0.0 && !typing {
                            if let Some(o) = orfs {
                                orf_scratch.clear();
                                o.index.query(start, end, &mut orf_scratch);
                                for iv in orf_scratch.iter() {
                                    let a = iv.lo.max(start);
                                    let b = iv.hi.min(end);
                                    if b <= a {
                                        continue;
                                    }
                                    let yl = y + strips.y_orf() + iv.lane as f32 * ORF_LANE_H;
                                    let rr = egui::Rect::from_min_max(
                                        egui::pos2(cx(a - start), yl),
                                        egui::pos2(cx(b - start), yl + ORF_LANE_H - 1.0),
                                    );
                                    painter.rect_stroke(
                                        rr,
                                        0.0,
                                        egui::Stroke::new(1.0, p.ink2),
                                        egui::StrokeKind::Inside,
                                    );
                                    // The ribbon grammar, reused exactly: a
                                    // solid cap on the 3' terminus.
                                    let rev = o
                                        .orfs
                                        .get(iv.feat as usize)
                                        .is_some_and(|orf| orf.strand.is_reverse());
                                    let cap_w = 3.0f32.min(rr.width());
                                    let tip = if rev {
                                        (iv.feat_lo && iv.lo >= start)
                                            .then(|| (rr.left(), rr.left() + cap_w))
                                    } else {
                                        (iv.feat_hi && iv.hi <= end)
                                            .then(|| (rr.right(), rr.right() - cap_w))
                                    };
                                    if let Some((tx, bx)) = tip {
                                        painter.add(egui::Shape::convex_polygon(
                                            vec![
                                                egui::pos2(tx, rr.center().y),
                                                egui::pos2(bx, rr.top()),
                                                egui::pos2(bx, rr.bottom()),
                                            ],
                                            p.ink2,
                                            egui::Stroke::NONE,
                                        ));
                                    }
                                }
                            }
                        }

                        // -- what this row holds, in words, in the surplus
                        // width the splitter buys. The name is the primary
                        // channel and the swatch the secondary one; colour is
                        // never on its own.
                        let names_x = cx(per_row) + layout.right_gutter.max(10.0) + 6.0;
                        // Only when there is a margin column at all. At a
                        // narrow split there is none, and every feature is
                        // still drawn as a ribbon and still named on hover — so
                        // badging every row would be crying wolf about a
                        // channel this layout never offered.
                        if rect.right() - names_x > 50.0 {
                            let mut nx = names_x;
                            let mut seen: Vec<u32> = Vec::new();
                            for iv in scratch.iter() {
                                if seen.contains(&iv.feat) {
                                    continue;
                                }
                                seen.push(iv.feat);
                                let Some(f) = mol.features.get(iv.feat as usize) else {
                                    continue;
                                };
                                let g = painter.layout_no_wrap(
                                    f.name.clone(),
                                    name_font.clone(),
                                    p.ink2,
                                );
                                // Counted, not silently dropped. pKoV rows
                                // 5,401-5,881 carry decR and decR his, which
                                // share a start AND a file colour; only the
                                // second fitted, and nothing on the row said
                                // the first was there. The badge means one
                                // thing — "N things on this row I could not
                                // show you" — whichever channel ran out.
                                if nx + 8.0 + g.size().x > rect.right() - 2.0 {
                                    hidden += 1;
                                    continue;
                                }
                                let sw = egui::Rect::from_min_size(
                                    egui::pos2(nx, y_text + 3.0),
                                    egui::vec2(6.0, 6.0),
                                );
                                painter.rect_filled(sw, 0.0, theme::feature_color(f));
                                painter.rect_stroke(
                                    sw,
                                    0.0,
                                    egui::Stroke::new(1.0, p.muted),
                                    egui::StrokeKind::Inside,
                                );
                                painter.galley(egui::pos2(nx + 8.0, y_text), g.clone(), p.ink2);
                                nx += 8.0 + g.size().x + 8.0;
                            }
                        }

                        if hidden > 0 {
                            // KNOWN DEFECT, PRE-EXISTING, NOT FIXED HERE: this
                            // overprints the row's own coordinate, because
                            // `gutter_w` sizes the left gutter to exactly the
                            // widest coordinate plus 8 pt of air and the badge
                            // is drawn inside that. pKoV row 5,581 reads
                            // "51,581". It reproduces with every track off, so
                            // it is not this change's — but this change does
                            // FEED the badge (a translation past the lane cap
                            // is counted into it), so the collision is commoner
                            // than it was. Left alone deliberately: every
                            // candidate home is occupied — the right gutter
                            // holds the row's end coordinate, the names column
                            // is packed to `rect.right() - 2.0`, and the ribbon
                            // band is 5 pt per lane against a 9.5 pt glyph — so
                            // the honest fix is a gutter that reserves room for
                            // it, which changes `per_row` and belongs in its own
                            // change with its own measurement.
                            painter.text(
                                egui::pos2(rect.left() + 2.0, y_text),
                                egui::Align2::LEFT_TOP,
                                format!("+{hidden}"),
                                ruler_font.clone(),
                                p.warn,
                            );
                        }

                        // The caret sits on the row that contains the gap. At
                        // the very end of the molecule that is the last row's
                        // right edge, not the first column of a row that does
                        // not exist. Painted LAST, above every new strip.
                        let on_this_row = (start..end).contains(&caret)
                            || (caret == end && (end == n || end == start));
                        if on_this_row && sel.is_none_or(|s| s.is_empty(mol.len())) {
                            let x = cx(caret - start);
                            painter.vline(
                                x,
                                y_text..=(y_text + text_h),
                                egui::Stroke::new(1.5, p.accent),
                            );
                        }
                    }

                    if let Some(pos) = resp.interact_pointer_pos() {
                        // A click on a residue lane selects that residue's
                        // codon; a click on either STRAND places the caret. The
                        // two strands share one coordinate space, so the bottom
                        // row is the top row's coordinates and nothing about
                        // the caret changes.
                        let aa_hit = match strip_at(pos) {
                            seqedit::Strip::Aa { lane, reverse } => codon_at(pos, lane, reverse),
                            _ => None,
                        };
                        if resp.drag_started() || resp.clicked() {
                            match &aa_hit {
                                // A codon that straddles a join or the origin
                                // is not one arc and `Selection` holds one arc,
                                // so the ARC THE POINTER IS ON is selected and
                                // the readout says where the rest is. Inventing
                                // a selection shape the model cannot hold would
                                // be worse.
                                //
                                // `run_containing` and not `span()`, and the
                                // difference is not cosmetic: `span()` is the
                                // OUTER BOUND, so on a 22 bp circle a codon at
                                // coordinates 20, 21, 0 spans (0, 22) and
                                // clicking it selected all 22 bases; a
                                // `join(101..150, 501..551)` seam codon selected
                                // 353, under a sentence that read "353 of 3
                                // bases selected". One Backspace away from
                                // losing everything between the arcs, from a
                                // click meant to select three bases.
                                // `codon_at` found this residue BY the base under
                                // the pointer, so the run is always there; the
                                // fallback places the caret, which is what a
                                // click on a lane with nothing in it does.
                                Some((res, at)) => match res.run_containing(*at) {
                                    Some((lo, hi)) => codon_click = Some((lo, hi, *res)),
                                    None => {
                                        click =
                                            Some((hit(pos), ui.input(|i| i.modifiers.shift)));
                                    }
                                },
                                None => {
                                    click = Some((hit(pos), ui.input(|i| i.modifiers.shift)));
                                }
                            }
                        } else if resp.dragged() {
                            // Dragging along a residue lane extends by whole
                            // codons, which is how a domain gets selected.
                            drag_to = Some(match &aa_hit {
                                Some((res, at)) => match res.run_containing(*at) {
                                    Some((lo, hi)) => {
                                        let anchor = edit.sel.map_or(edit.caret, |s| s.anchor);
                                        if hi <= anchor {
                                            lo
                                        } else {
                                            hi
                                        }
                                    }
                                    None => hit(pos),
                                },
                                None => hit(pos),
                            });
                            // The only scroll machinery this feature owes: a
                            // drag that leaves the viewport has to keep going,
                            // or an origin-crossing selection cannot be made by
                            // the gesture that unambiguously carries the fact.
                            if pos.y < rect.top() {
                                ui.scroll_with_delta(egui::vec2(0.0, row_h * 2.0));
                            } else if pos.y > rect.top() + height {
                                ui.scroll_with_delta(egui::vec2(0.0, -row_h * 2.0));
                            }
                        }
                        if resp.double_clicked() {
                            double = Some(hit(pos));
                        }
                    }
                    released = resp.drag_stopped();

                    // The non-colour channel for every channel above, and what
                    // makes the column ruler an orientation aid rather than an
                    // arithmetic exercise: the base is named absolutely, and so
                    // is everything on it — including the features with no lane
                    // and the enzyme names that could not be placed.
                    // A BASE, not a gap. `hit` rounds to the nearer boundary,
                    // which is what a caret wants and the opposite of what this
                    // line wants: over the right half of every cell it named
                    // the base after the one under the pointer, and then listed
                    // that base's features. On pKoV the right half of base 585
                    // — visibly under the yellow `pSC101 ori` ribbon — reported
                    // base 586 with no feature at all, and at the last column
                    // of a row it named a base sixty cells away.
                    //
                    // `x_base` also answers `None`, which the clamp could not:
                    // past the last cell, and past the last base of the
                    // molecule, there is nothing under the pointer to name.
                    if let Some(at) = resp.hover_pos().and_then(|pos| {
                        let col = layout.x_base(pos.x - x0)?;
                        let at = row_at(pos) * per_row + col;
                        (at < n).then_some(at)
                    }) {
                        let mut s = format!("base {}", fmt_int(at + 1));
                        scratch.clear();
                        ix.query_run(at, at + 1, run, &mut scratch);
                        for iv in scratch.iter() {
                            if let Some(f) = mol.features.get(iv.feat as usize) {
                                s.push_str(&format!(
                                    " · {} ({}, {})",
                                    f.name,
                                    f.kind,
                                    strand_word(f.strand)
                                ));
                                // The length, and whether it is a multiple of
                                // three. That is how an in-frame fusion or a
                                // His-tag insertion is checked, and neither
                                // number was available anywhere in this
                                // application.
                                if let Some(bp) = feature_bp(f, mol) {
                                    s.push_str(&if bp % 3 == 0 {
                                        format!(" {} bp (3n, {} aa)", fmt_int(bp), fmt_int(bp / 3))
                                    } else {
                                        format!(" {} bp — NOT a multiple of 3", fmt_int(bp))
                                    });
                                }
                            }
                        }
                        // The residue over this base, in the SAME line as
                        // everything else on it. This application has one
                        // status channel and exactly one `on_hover_text`
                        // window, in `map.rs`; a per-residue tooltip would be a
                        // second surface to keep in step.
                        if aa_on {
                            for path in tr.paths().iter().chain(sel_path.iter()) {
                                let ep = path.effective(run);
                                let Some(k) = ep.residue_covering(at) else {
                                    continue;
                                };
                                if k >= ep.aa_len() {
                                    continue;
                                }
                                let Some(res) = ep.residue(k, &read) else {
                                    continue;
                                };
                                s.push_str(&format!(
                                    " · {} {} of {} · codon {} · table {}",
                                    res.aa as char,
                                    fmt_int(k as u64 + 1),
                                    fmt_int(ep.aa_len() as u64),
                                    String::from_utf8_lossy(&res.codon),
                                    ep.code.id
                                ));
                                // The mark's own sentence. The glyph cannot
                                // separate an ambiguous mixture from a byte
                                // that is not a nucleotide code at all, and
                                // only this line can.
                                s.push_str(match res.mark {
                                    aa::Mark::Plain => "",
                                    aa::Mark::StopEnd => " · the terminal stop",
                                    aa::Mark::StopInside => {
                                        " · AN INTERNAL STOP: the annotation is wrong, or \
                                         something upstream is out of frame"
                                    }
                                    aa::Mark::AmbiguousStop => {
                                        " · both a stop and a residue in this table; which \
                                         one depends on context this program does not have"
                                    }
                                    aa::Mark::Initiator => {
                                        " · read as Met because this table initiates here — \
                                         the codon is not ATG"
                                    }
                                    aa::Mark::Ambiguous => {
                                        " · the codon resolves to more than one residue, or \
                                         holds a byte that is not a nucleotide code"
                                    }
                                });
                                if !res.contiguous {
                                    s.push_str(&format!(
                                        " · spans the join or the origin: {} | {} | {}",
                                        fmt_int(res.coords[0] + 1),
                                        fmt_int(res.coords[1] + 1),
                                        fmt_int(res.coords[2] + 1)
                                    ));
                                }
                                if ep.ragged() > 0 && k + 1 == ep.aa_len() {
                                    // A reading that simply stops one or two
                                    // cells early looks like a rendering bug and
                                    // reads like a shorter protein.
                                    s.push_str(&format!(
                                        " · last codon incomplete, {} base(s)",
                                        ep.ragged()
                                    ));
                                }
                            }
                        }
                        if show_cuts {
                            // The same query the bracket is drawn from, so the
                            // words and the drawing cannot disagree about which
                            // bases are inside a site. The `± 8` this replaces
                            // was a guess that a cut is never more than eight
                            // bases from its own site, which BsaI — GGTCTC(1/5)
                            // — is, and MmeI is not.
                            for c in ix.sites_touching(at, at + 1, n) {
                                {
                                    let name = d
                                        .digest
                                        .results()
                                        .get(c.enzyme as usize)
                                        .map(|dg| dg.enzyme.name)
                                        .unwrap_or("?");
                                    s.push_str(&format!(
                                        " · {name} site {}..{}",
                                        fmt_int(c.site_lo + 1),
                                        fmt_int(c.site_lo + c.site_len as u64)
                                    ));
                                }
                            }
                        }
                        hover_out = Some(s);
                    }
                });
            }
        }

        self.annot_scratch = scratch;
        self.edit.visible_rows = visible;
        self.seq_grid = grid;
        // Unconditionally. The grid closure runs on every frame this tab is
        // open, so `None` genuinely means "the pointer is not on a base" —
        // keeping the last answer left the readout naming base 3,930 while the
        // pointer sat in the middle of the map pane.
        self.seq_hover = hover_out;

        // -- apply the pointer, now that the borrows are done --------------
        //
        // Through `set_selection`, which is the ONE way anything outside
        // `seqedit` may set `sel` and `caret`, and which commits the open run
        // first. Assigning a selection behind a run's back put the typed bases
        // ten positions from the highlight, and its own docstring says so.
        if let Some((lo, hi, res)) = codon_click {
            let d = self.bench.get_mut().expect("checked by caller");
            self.edit.set_selection(
                d,
                Selection {
                    anchor: lo,
                    head: hi,
                    through_origin: false,
                },
                hi,
            );
            // `hi - lo` is now the length of the ARC that was selected — 1 or 2
            // of the codon's three — and the other coordinates are named so the
            // reader can find the rest. It used to be the outer bound's width,
            // which on a `join(101..150, 501..551)` printed the sentence
            // "353 of 3 bases selected" over a 353-base selection.
            let where_ = if res.contiguous {
                String::new()
            } else {
                let rest: Vec<String> = res
                    .coords
                    .iter()
                    .filter(|c| !(lo..hi).contains(c))
                    .map(|c| fmt_int(c + 1))
                    .collect();
                format!(
                    " — spans the join or the origin; {} of 3 bases selected, the rest at {}",
                    hi - lo,
                    rest.join(" and ")
                )
            };
            self.edit.say(format!(
                "residue {} · {} · codon {}{}",
                fmt_int(res.k as u64 + 1),
                res.aa as char,
                String::from_utf8_lossy(&res.codon),
                where_
            ));
            self.edit.dragging = true;
        }
        if let Some((to, shift)) = click {
            let d = self.bench.get_mut().expect("checked by caller");
            self.edit.place(d, to, shift);
            if !shift {
                self.edit.dragging = true;
            }
        }
        if let Some(to) = drag_to {
            let d = self.bench.get_mut().expect("checked by caller");
            let circular = d.molecule().topology.is_circular();
            let anchor = self.edit.sel.map_or(self.edit.caret, |s| s.anchor);
            let n_committed = d.molecule().len();
            // The head running off the end of the text and reappearing on the
            // first row is the user physically travelling across the origin.
            // Nothing is guessed, which is why this and Shift+Arrow are the only
            // two gestures allowed to set the bit — and once set it is sticky
            // for the rest of the drag, which is what `clamped` (rather than
            // canonical form) below preserves.
            //
            // Landing on the first row is required as well as `to < anchor`,
            // because without it an overshoot past the last base followed by a
            // correction back above the anchor — an ordinary, clumsy drag — is
            // textually identical to a wrap and was read as one.
            let wrapped = self.edit.dragging
                && self.edit.caret == n_committed
                && to < anchor
                && to < self.edit.per_row();
            let crossing = self.edit.sel.is_some_and(|s| s.through_origin) || (circular && wrapped);
            self.edit.sel = Some(
                Selection {
                    anchor,
                    head: to,
                    through_origin: crossing && circular,
                }
                .clamped(n_committed, circular),
            );
            self.edit.caret = to;
        }
        if released {
            self.edit.dragging = false;
        }
        if let Some(at) = double {
            self.select_feature_under(at);
        }
    }

    /// The line a biologist reads out loud when they order a primer, plus the
    /// two controls that act on it.
    ///
    /// The controls come FIRST and on a row of their own. A wrapped `Label`'s
    /// last line ends at an arbitrary x, and a `Button` after it in the same
    /// wrapped row lands wherever that happens to be — which is why the Design
    /// primers button read as buried rather than merely low. Putting the
    /// variable-length thing last means only it reflows and the buttons never
    /// move.
    ///
    /// The sentence itself is never truncated or shortened. At the 300 pt
    /// minimum it wraps to three lines and the region grows to hold them, which
    /// is now correct behaviour: it is the one thing explaining the genuinely
    /// confusing property of a circular molecule, and it was the half that got
    /// cut.
    fn sequence_readout(&mut self, ui: &mut Ui) {
        let (mol_line, other_arc, has_sel) = {
            let d = self.bench.get().expect("checked by caller");
            let mol = d.molecule();
            let n_c = mol.len();
            let circular = mol.topology.is_circular();
            // Offered only when there really are two arcs to choose between.
            let two_arcs = circular
                && self
                    .edit
                    .sel
                    .map(|s| s.canonical(n_c, true))
                    .is_some_and(|s| !s.is_empty(n_c) && s.base_count(n_c) < n_c);
            let alt = two_arcs.then(|| {
                let s = self.edit.sel.unwrap().canonical(n_c, true);
                n_c - s.base_count(n_c)
            });
            let has_sel = self
                .edit
                .sel
                .is_some_and(|s| !s.canonical(n_c, circular).is_empty(n_c));
            (self.edit.readout(mol), alt, has_sel)
        };
        let hover = self.seq_hover.clone();
        let notice = self.edit.notice.clone();
        let can_design = self.design.is_none() && self.feature_edit.is_none();
        let mut flip = false;
        let mut design = false;
        let mut annotate = false;
        let mut copy_rc = false;

        let readout =
            egui::Panel::bottom(egui::Id::new("seq-readout"))
                .resizable(false)
                .show(ui, |ui| {
                    // The height is given, not taken. A `right_to_left(Align::Center)`
                    // region expands to whatever vertical space it is offered, and
                    // inside a content-sized bottom panel that is a feedback loop:
                    // the row filled the panel, the panel grew to hold the row plus
                    // the sentence, the row filled *that* — measured at 51 pt of
                    // growth per frame, so the grid was down to two visible rows in
                    // under a second and still shrinking.
                    //
                    // WRAPPED, and that is a regression fix and not a
                    // preference. This row held one button; it now holds three,
                    // and measured at a 300 pt split the content is 416 px
                    // against ~361 px of panel — so "take the other arc (8,090
                    // bp)" rendered as "her arc (8,090 bp)" and the readout line
                    // "4..30 · 27 bp" rendered as "27 bp", the coordinates cut
                    // off the panel whose entire job is showing them. Same fix,
                    // same clip-rect reason, as `sequence_header` above and the
                    // Features toolbar. The height stays GIVEN — wrapping grows
                    // the region from its CONTENT, which is stable, rather than
                    // from the space on offer, which is the loop.
                    let row_h = ui.spacing().interact_size.y.max(22.0);
                    let w = ui.available_width();
                    ui.allocate_ui_with_layout(
                        egui::vec2(w, row_h),
                        Layout::right_to_left(Align::Center).with_main_wrap(true),
                        |ui| {
                            // Design lives beside the readout because that is where the
                            // selection is already described: the panel's target line
                            // repeats these numbers rather than deriving its own.
                            let b = ui.add_enabled(
                                has_sel && can_design,
                                egui::Button::new(RichText::new("Design primers…").size(11.0)),
                            );
                            if !has_sel {
                                b.on_hover_text("Select the region to amplify first.");
                            } else if b.clicked() {
                                design = true;
                            }
                            // The SnapGene gesture, next to the readout that
                            // already names the arc it will annotate: select the
                            // bases, then say what they are. Same enabling
                            // predicate as Design, and the same voice on the
                            // disabled hover.
                            let a = ui.add_enabled(
                                has_sel && can_design,
                                egui::Button::new(RichText::new("New feature…").size(11.0)),
                            );
                            if !has_sel {
                                a.on_hover_text("Select the bases first.");
                            } else if a.clicked() {
                                annotate = true;
                            }
                            // A reverse complement is a thing to put on the
                            // clipboard, not a thing to print on a status
                            // line. Not gated on `can_design`: it changes
                            // nothing about the document, so it is safe with
                            // a panel open, unlike the two buttons above.
                            let rc = ui.add_enabled(
                                has_sel,
                                egui::Button::new(RichText::new("Copy rev-comp").size(11.0)),
                            );
                            if !has_sel {
                                rc.on_hover_text("Select the bases first.");
                            } else {
                                if rc
                                    .on_hover_text(
                                        "Ctrl+Shift+R — the reverse complement of the selection, \
                                         case preserved. Not Ctrl+Shift+C: that chord cannot be \
                                         told apart from Ctrl+C with Shift still held, which is \
                                         where Shift+arrow selection leaves your hand.",
                                    )
                                    .clicked()
                                {
                                    copy_rc = true;
                                }
                            }
                            // The explicit way to flip a bit that Shift+click cannot infer,
                            // because a click has no direction of travel. Both lengths are
                            // on screen so the choice is visible, and there is no
                            // shortest-arc heuristic: a tool that silently picks the 60 bp
                            // arc because it is shorter will one day silently pick the
                            // 4,921 bp one.
                            //
                            // A button rather than a shortcut because the obvious shortcut,
                            // Ctrl+O, is already Open File — bound globally, before this tab
                            // ever sees the event, so the "toggle" would have opened a file
                            // dialog and flipped the arc at the same time.
                            if let Some(alt) = other_arc {
                                if ui
                            .button(
                                RichText::new(format!("take the other arc ({} bp)", fmt_int(alt)))
                                    .size(11.0),
                            )
                            .on_hover_text(
                                "Two carets on a circle name two arcs. This takes the other one.",
                            )
                            .clicked()
                        {
                            flip = true;
                        }
                            }
                        },
                    );
                    let line = ui.label(
                        RichText::new(mol_line.clone())
                            .monospace()
                            .size(11.5)
                            .color(pal(ui).ink2),
                    );
                    // The conditions behind the number, and — when the readout
                    // declined to give one — why. Composed in `seqedit` so the
                    // sentence and the rule that produced it live together.
                    if let Some(h) = seqedit::tm_hover(&mol_line) {
                        line.on_hover_text(h);
                    }
                    // AND THE CONDITIONS ARE DISPLAYED, not only hovered. A Tm
                    // computed under conditions other than the ones shown is
                    // worse than no Tm, and `pl tm` prints the model above every
                    // number it gives; here the same string was attached to a
                    // plain `Label`, which egui does not make focusable — so it
                    // was unreachable from the keyboard, invisible to a screen
                    // reader, and advertised by nothing. `design.rs` already
                    // writes this same `describe()` into a GenBank note rather
                    // than a tooltip, for the same reason.
                    if seqedit::tm_shown(&mol_line) {
                        ui.label(
                            RichText::new(seqedit::tm_method().describe())
                                .size(10.0)
                                .color(pal(ui).muted),
                        );
                    }
                    // Always drawn, even when there is nothing under the pointer, so
                    // the region's height does not flicker as the pointer moves.
                    ui.label(
                        RichText::new(hover.unwrap_or_else(|| {
                            "point at a base to name it and what is on it".into()
                        }))
                        .size(11.0)
                        .color(pal(ui).muted),
                    );
                    // In here too. It carries refusals like "3 features removed —
                    // Ctrl+Z to undo", and a warning about data the user just lost is
                    // not something that may be clipped.
                    if let Some(msg) = notice {
                        ui.label(RichText::new(msg).color(pal(ui).warn).size(11.0));
                    }
                });
        self.seq_readout = Some(readout.response.rect);
        if debug_geometry() {
            eprintln!("seqreadout: rect={:?}", readout.response.rect);
        }

        if flip {
            // Flipped and stored raw. Canonical form is what the op derivation
            // and the painter ask for, and it is lossy about the direction of
            // travel this button exists to state.
            if let Some(s) = &mut self.edit.sel {
                s.through_origin = !s.through_origin;
            }
        }
        if design {
            self.open_design();
        }
        if annotate {
            self.open_feature_editor(None);
        }
        if copy_rc {
            if let Some(s) = self.do_copy_rc() {
                ui.ctx().copy_text(s);
            }
        }
    }

    /// The `Show` row for the sequence view's tracks, plus everything the
    /// tracks have to disclose.
    ///
    /// One control, one answer, in the place the header already says what the
    /// row is -- copying the enzyme filter's idiom verbatim, including the
    /// hover text on a chip that cannot change anything.
    ///
    /// These are VIEW preferences. Nothing here calls `Document::apply`, enters
    /// the log or makes the document dirty. The one place this feature ever
    /// writes to a molecule is the feature editor's `aa` checkbox, which goes
    /// through `OpKind::SetFeature` like every other feature edit.
    fn sequence_tracks_row(&mut self, ui: &mut Ui, per_row: u64) {
        let (
            tr_empty,
            rev,
            over,
            unoriented,
            dropped,
            own_tables,
            stops,
            capped,
            scanned,
            readings,
            bad_cs,
        ) = {
            let t = &self.tr;
            (
                t.is_empty(),
                t.rev_lanes,
                t.over_cap,
                t.unoriented.clone(),
                t.dropped.clone(),
                t.own_tables.clone(),
                t.internal_stops.clone(),
                t.stops_capped,
                t.readings_scanned,
                t.paths().len(),
                t.bad_codon_starts.clone(),
            )
        };
        let d = self.bench.get().expect("checked by caller");
        let ds = d.molecule().double_stranded;
        let complement = self.layout.complement.unwrap_or(ds != Some(false));
        let orf_state = match &d.orfs {
            doc::OrfState::Off => None,
            doc::OrfState::Running { .. } => Some(Err("scanning...".to_string())),
            doc::OrfState::Unavailable(why) => Some(Err(why.clone())),
            doc::OrfState::Done(o) => Some(Ok((
                o.orfs.len(),
                o.lapping,
                o.stopless.len(),
                o.code,
                o.min_aa,
            ))),
        };

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Show").color(pal(ui).muted).size(12.0));
            for m in [
                aa::TrackMode::Off,
                aa::TrackMode::File,
                aa::TrackMode::Selection,
            ] {
                let resp = ui.selectable_label(self.layout.aa_track == m, m.label());
                // A chip that cannot change anything says so, rather than
                // leaving the user to click it and infer from a picture that
                // did not move.
                let resp = if m == aa::TrackMode::File && tr_empty {
                    resp.on_hover_text(
                        "Nothing in this document is marked translated and nothing in it is a \
                         CDS, so this changes nothing here. Set a strand and tick `aa` in the \
                         feature editor, or use `+ selection`.",
                    )
                } else {
                    resp
                };
                if resp.clicked() {
                    self.layout.aa_track = m;
                }
            }
            if ui
                .selectable_label(complement, "complement")
                .on_hover_text(
                    "The bottom strand, in the top strand's coordinates. Reverse-strand \
                     translations are drawn only when it is on: a reverse reading under a \
                     top-strand-only view runs C-terminus to N-terminus left to right.",
                )
                .clicked()
            {
                self.layout.complement = Some(!complement);
            }
            if ui
                .selectable_label(self.layout.orf_track, "ORFs")
                .on_hover_text(
                    "Every stretch the SEQUENCE permits, in its own strip and its own grammar. \
                     Not the same claim as an annotated CDS.",
                )
                .clicked()
            {
                self.layout.orf_track = !self.layout.orf_track;
            }
            // An explicit height, because this application has already shipped
            // the other version once: `featedit.rs`'s Type dropdown takes
            // egui's default popup height, shows 8 of 17, and egui's scrollbar
            // fades at rest so it reads as a complete list. Twenty-seven tables
            // would do the same thing, harder.
            let mut code = self.doc_code;
            egui::ComboBox::from_id_salt("pl-seq-code")
                .height(320.0)
                .selected_text(format!("table {}", code.id))
                .show_ui(ui, |ui| {
                    for c in pl_core::translate::all_tables() {
                        ui.selectable_value(&mut code, c, format!("{} - {}", c.id, c.name()));
                    }
                });
            if code != self.doc_code {
                self.doc_code = code;
                self.layout.code = code.id;
                // The translations are keyed on the document version, which has
                // not moved, so the rebuild has to be asked for.
                self.annot.version = (u64::MAX, None);
            }
        });

        // -- what the tracks have to disclose ----------------------------
        let mut say: Vec<String> = Vec::new();
        if self.layout.aa_track.is_on() {
            say.push(format!(
                "translated with table {} ({})",
                self.doc_code.id,
                self.doc_code.name()
            ));
            if !own_tables.is_empty() {
                // The sentence above names the DOCUMENT default, and a feature
                // carrying `/transl_table` is not translated with it. Unsaid,
                // the header could name a table no residue used: all three of
                // pKoV's CDSs carry `/transl_table=1`, so switching the combo to
                // table 4 for a mycoplasma insert changed the sentence and not
                // one letter — leaving a terminal TGA drawn `*`, which reads as
                // an internal stop in a code that was never applied.
                //
                // Phrased as a COUNT of the readings, so "3 of 3" says plainly
                // that the number above reached nothing, and it stays true when
                // the default and the overrides happen to be the same table.
                let which: Vec<String> = own_tables
                    .iter()
                    .map(|&(id, _)| match pl_core::translate::table(id) {
                        Some(c) => format!("table {id} ({})", c.name()),
                        // Unreachable: `feature_code` only records a number
                        // `translate::table` already resolved.
                        None => format!("table {id}"),
                    })
                    .collect();
                let k: usize = own_tables.iter().map(|&(_, k)| k).sum();
                say.push(format!(
                    "{k} of {readings} reading(s) carry their own /transl_table and use it \
                     instead: {}",
                    which.join(", ")
                ));
            }
            if per_row % 3 != 0 {
                // The drawing stays right -- the middle-base rule is computed
                // per row from absolute coordinates -- but the codon columns
                // walk by `per_row % 3` a row and the residues form a diagonal.
                // Disclosed rather than fixed by snapping `per_row` to 30,
                // which would cost 40% of the bases on screen at this width.
                say.push(format!(
                    "codon columns line up only at 60 or 30 bases per row; this row is {per_row}"
                ));
            }
            if !complement && rev > 0 {
                say.push(format!(
                    "{rev} reverse-strand translation lane(s) hidden -- turn on the complement \
                     strand"
                ));
            }
            if over > 0 {
                say.push(format!(
                    "{over} translation(s) past the {} lanes per strand, counted in the row's +N",
                    aa::MAX_AA_LANES
                ));
            }
            if !unoriented.is_empty() {
                // Reached by something that ASKED for a reading and has no
                // direction to read it in: a CDS, or a segment ticked `aa`, with
                // no strand. NOT by pKoV's `decR his`, whatever the comment that
                // used to stand here said — that is an unflagged `misc_feature`,
                // so it never asked, and this sentence has never appeared on the
                // file it was written for.
                say.push(format!(
                    "no track for {}: no strand is recorded, so there is no reading direction -- \
                     set one in the feature editor, or select the bases and use `+ selection`",
                    unoriented.join(", ")
                ));
            }
            if !dropped.is_empty() {
                // A reading with no bases in it. Said, because a CDS missing
                // from the track looks exactly like a molecule that never had
                // one.
                say.push(format!(
                    "no track for {}: the segment runs backwards round an origin this molecule \
                     does not have, because it is linear",
                    dropped.join(", ")
                ));
            }
            for b in &bad_cs {
                say.push(b.clone());
            }
        }
        if complement {
            let assumed = if ds.is_none() {
                " -- strands are not recorded in this file; drawn double"
            } else {
                ""
            };
            say.push(format!(
                "double-stranded: the lower row is the complement, read 3'->5' left to right; \
                 no overhangs are drawn and Ctrl+C still copies the top strand{assumed}"
            ));
        }
        if let Some(st) = orf_state {
            // Suppressed during a typing run, exactly as the cut marks are and
            // for the same reason: the scan describes the COMMITTED sequence,
            // and a typed base can create or destroy a start or a stop, so an
            // ORF remapped into effective coordinates is not merely displaced —
            // it can be an ORF that no longer exists, drawn confidently. An
            // empty strip and a suppressed one are otherwise indistinguishable.
            if self.edit.run().is_some() {
                say.push("ORFs hidden while typing".into());
            }
            say.push(match st {
                Err(why) => format!("ORFs: {why}"),
                Ok((count, lapping, stopless, code, min_aa)) => {
                    let mut s = format!(
                        "{} ORF(s) - table {code} - >={min_aa} aa - starts required",
                        fmt_int(count as u64)
                    );
                    if lapping > 0 {
                        // A `[lo, hi)` interval cannot hold more than one lap,
                        // so drawing one would show a fraction of the ORF and
                        // look entirely normal.
                        s.push_str(&format!(
                            " - {lapping} ORF(s) run more than once round this circle and cannot \
                             be drawn as a span"
                        ));
                    }
                    if stopless > 0 {
                        s.push_str(&format!(
                            " - {stopless} frame(s) meet no stop codon anywhere on this circle, \
                             so they have no reportable ORF"
                        ));
                    }
                    s
                }
            });
        }
        if !say.is_empty() {
            ui.label(
                RichText::new(say.join(" \u{b7} "))
                    .color(pal(ui).muted)
                    .size(11.0),
            );
        }
        if !stops.is_empty() {
            let mut loud = format!("{} internal stop codon(s): ", fmt_int(stops.len() as u64));
            loud.push_str(
                &stops
                    .iter()
                    .take(4)
                    .map(|(name, at)| format!("{name} at {}", fmt_int(*at)))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            if stops.len() > 4 {
                loud.push_str(&format!(" and {} more", stops.len() - 4));
            }
            if capped {
                // What was NOT examined, not only what was. On a genome the
                // budget is spent inside the first few hundred of thousands of
                // readings, so "counted over the first 100,000 residues" alone
                // reads like a footnote on a whole-document count — and a
                // frameshift past that point raises nothing at all.
                loud.push_str(&format!(
                    " (counted over the first {} residues; {} of {} reading(s) were not examined)",
                    fmt_int(aa::STOP_SCAN_CAP as u64),
                    fmt_int(readings.saturating_sub(scanned) as u64),
                    fmt_int(readings as u64)
                ));
            }
            ui.label(RichText::new(loud).color(pal(ui).warn).size(11.0));
        }
    }

    fn sequence_header(
        &mut self,
        ui: &mut Ui,
        n: u64,
        rows: usize,
        per_row: u64,
        has_cuts: bool,
        typing: bool,
    ) {
        let d = self.bench.get().expect("checked by caller");
        let scanning = d.digest.is_running();
        let unavailable = match &d.digest {
            DigestState::Unavailable(why) => Some(why.clone()),
            _ => None,
        };
        let hidden = d.visibility(self.enzyme_set).hidden_sites;
        let circular = d.molecule().topology.is_circular();
        // Wrapped, not `horizontal`: a `Label` inside a plain horizontal layout
        // extends past the panel and is clipped, so the origin note lost its
        // second half in a 380 px panel — a sentence explaining the one
        // genuinely confusing thing about a circular sequence, cut off.
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} bp · {} rows · case preserved · editable",
                    fmt_int(n),
                    fmt_int(rows as u64)
                ))
                .color(pal(ui).muted)
                .size(11.0),
            );
            if typing {
                // Saying so is not decoration: between keystrokes the enzyme
                // list and the map describe the document *before* this run.
                ui.label(RichText::new("· typing").color(pal(ui).accent).size(11.0));
            }
            if circular {
                ui.label(
                    RichText::new("· circular: row 1 and the last row meet at the origin")
                        .color(pal(ui).muted)
                        .size(11.0),
                );
            }
            // All three digest states are said out loud, because an empty strip
            // and a not-yet-computed strip are otherwise indistinguishable —
            // and on a 4.6 Mb genome the second one lasts nearly two seconds.
            let enz = if let Some(why) = unavailable {
                format!("· enzyme sites: {why}")
            } else if scanning {
                "· enzyme sites: scanning…".to_string()
            } else if typing && has_cuts {
                "· enzyme sites hidden while typing".to_string()
            } else if hidden > 0 {
                format!(
                    "· {} site(s) hidden by the {} filter",
                    fmt_int(hidden as u64),
                    self.enzyme_set.label()
                )
            } else {
                String::new()
            };
            if !enz.is_empty() {
                ui.label(RichText::new(enz).color(pal(ui).muted).size(11.0));
            }
        });
        self.sequence_tracks_row(ui, per_row);
        // The lane cap and the segments whose coordinates named nothing, said
        // once here as well as counted per row. Three lanes drawn where five
        // features overlap looks exactly like a file with three features, and
        // `docs/PLAN.md` item 33 is the record of what a hidden thing costs.
        let (depth, lanes, dropped) = (self.annot.depth, self.annot.lanes, self.annot.dropped);
        if depth > lanes || dropped > 0 {
            let mut say = String::new();
            if depth > lanes {
                say.push_str(&format!(
                    "features overlap {depth} deep; {lanes} lanes are drawn and the rest are \
                     counted in the row's +N and named on hover"
                ));
            }
            if dropped > 0 {
                if !say.is_empty() {
                    say.push_str(" · ");
                }
                say.push_str(&format!(
                    "{} segment(s) name no bases and are not drawn",
                    fmt_int(dropped as u64)
                ));
            }
            ui.label(RichText::new(say).color(pal(ui).warn).size(11.0));
        }
        ui.add_space(2.0);
    }

    fn do_copy(&mut self) -> Option<String> {
        let d = self.document()?;
        match self.edit.copy(d.molecule()) {
            Some((s, skipped)) => {
                self.edit.say(format!(
                    "copied {} bases{}",
                    fmt_int(s.len() as u64),
                    not_copied(skipped)
                ));
                Some(s)
            }
            None => {
                self.edit.say("Nothing is selected.");
                None
            }
        }
    }

    /// Ctrl+Shift+R, and the button beside the readout.
    ///
    /// Says "reverse complement" in the notice rather than "copied", because
    /// the clipboard now holds bases that are nowhere in the file in that
    /// order, and a user who meant plain Copy has to be able to tell.
    ///
    /// It is NOT on Ctrl+Shift+C, and that is the whole point: see the
    /// `Event::Copy` arm for why that chord silently fired on plain Ctrl+C.
    fn do_copy_rc(&mut self) -> Option<String> {
        let d = self.document()?;
        match self.edit.copy_revcomp(d.molecule()) {
            Some((s, skipped)) => {
                self.edit.say(format!(
                    "copied the reverse complement of {} bases{}",
                    fmt_int(s.len() as u64),
                    not_copied(skipped)
                ));
                Some(s)
            }
            None => {
                self.edit.say("Nothing is selected.");
                None
            }
        }
    }

    /// Ctrl+X.
    ///
    /// The delete speaks first, and what it has to say outranks the count. This
    /// assigned the notice unconditionally, so the one edit in this surface
    /// that silently renumbers the whole molecule — a cut across the origin —
    /// was the one that said nothing about it: `apply_gesture` set "the plasmid
    /// was renumbered ... Ctrl+Z twice" and the next line replaced it with
    /// "cut 4 bases". A cut that destroyed a feature lost that sentence the
    /// same way, and the identical delete by Backspace reported both.
    fn do_cut(&mut self, now: f64) -> Option<String> {
        let d = self.bench.get_mut()?;
        let Some((s, skipped)) = self.edit.copy(d.molecule()) else {
            self.edit.say("Nothing is selected.");
            return None;
        };
        self.edit.notice = None;
        let removed = self.edit.backspace(d, now);
        let said = self.edit.notice.take();
        if !removed {
            // Refused, or there was nothing to take. The bases are on the
            // clipboard and the molecule is untouched; "cut" would be false.
            self.edit.notice = said;
            return Some(s);
        }
        let head = format!(
            "cut {} bases{}",
            fmt_int(s.len() as u64),
            not_copied(skipped)
        );
        self.edit.say(match said {
            Some(more) => format!("{head} · {more}"),
            None => head,
        });
        Some(s)
    }

    /// Double-click selects the smallest feature covering the base.
    ///
    /// DNA has no words, so word-select needs a DNA-meaningful analogue and
    /// this is it. If nothing covers that base it places the caret and selects
    /// nothing, rather than inventing a span. A linear scan is fine even at
    /// MG1655's ~9,000 features: it happens on a click, not per frame.
    fn select_feature_under(&mut self, caret: u64) {
        let Some(d) = self.bench.get() else { return };
        let base = caret.max(1);
        let mut best: Option<(u64, usize, u64, u64)> = None;
        for (i, f) in d.molecule().features.iter().enumerate() {
            for s in &f.segments {
                let covers = if s.end < s.start {
                    base >= s.start || base <= s.end
                } else {
                    base >= s.start && base <= s.end
                };
                if covers {
                    // A wrapped segment's real length needs the molecule;
                    // `Segment::len` deliberately answers 0 rather than guess.
                    let len = if s.end < s.start {
                        d.molecule().len() - s.start + 1 + s.end
                    } else {
                        s.len()
                    };
                    if best.is_none_or(|(b, ..)| len < b) {
                        best = Some((len, i, s.start, s.end));
                    }
                }
            }
        }
        if let Some((_, i, start, end)) = best {
            let n = d.molecule().len();
            let circular = d.molecule().topology.is_circular();
            // `saturating_sub`, because `start` can be 0. The SnapGene reader
            // parses `<Segment range="0-4"/>` with a bare `parse()` and
            // deliberately carries the zero through rather than dropping it —
            // `Molecule::rotate` carries a regression test named for the same
            // underflow — and nothing validates a molecule on the way into this
            // window. `start - 1` panicked in a debug build and, with overflow
            // checks off in release, wrapped to `u64::MAX`, which the clamp
            // then pulled down to `n`: double-clicking a feature covering bases
            // 1..4 selected bases 5..12, the ones it does not cover, and the
            // next Backspace deleted them. Raising 0 to 1 is what `rotate`'s own
            // remap does, and for the same reason.
            let sel = seqedit::Selection {
                anchor: start.saturating_sub(1),
                head: end,
                through_origin: end < start && circular,
            };
            let head = end.min(n);
            let d = self.bench.get_mut().expect("checked at the top");
            self.edit.set_selection(d, sel, head);
            self.selected = Some(i);
        }
    }

    /// Keyboard and clipboard for the sequence view.
    ///
    /// Events are consumed only while this tab is showing and nothing else has
    /// focus, so a keystroke meant for the feature filter or the library query
    /// never lands in the sequence.
    fn sequence_keys(&mut self, ui: &mut Ui, now: f64) {
        if ui.ctx().memory(|m| m.focused()).is_some() {
            return;
        }
        // A paste is waiting on an answer. `egui::Window` is not modal and
        // `Button` never takes keyboard focus, so without this the document
        // stays fully live underneath a dialog asking about it: arrow keys move
        // the caret, Backspace deletes, and a second Ctrl+V silently replaces
        // the question.
        //
        // The unsaved-changes guard and the `.dna` lossiness question need the
        // same stand-down and did not have it. `egui::Modal` blocks widget
        // interaction, not raw `ctx.input` reads, so with the close guard on
        // screen eight typed bases took the molecule from 8,120 to 8,128 bp and
        // the dialog's own sentence from "1 edit" to "3 edits" while it was
        // being read. The state answered about must be the state acted on.
        if self.asking() {
            return;
        }
        // Same reason, same rule. `egui::Window` is not modal and `Button`
        // never takes keyboard focus, so without this arrow keys move the caret
        // and Backspace deletes underneath a panel describing the document --
        // and the panel's snapshot would then name bases that are no longer
        // there.
        if self.design.is_some() {
            return;
        }
        // Same reason again. Without it, arrow keys move the caret and Backspace
        // deletes bases underneath a window whose coordinate boxes are describing
        // exactly those bases.
        if self.feature_edit.is_some() {
            return;
        }
        let events = ui.input(|i| i.events.clone());
        // The same number the renderer and the hit-test use, measured last
        // frame. Two different row widths in one frame is how Up/Down and a
        // click end up disagreeing about which base is under the pointer.
        let per_row = self.edit.per_row();
        let per_page = self.edit.visible_rows.max(1) * per_row;

        for ev in events {
            let Some(d) = self.bench.get_mut() else {
                return;
            };
            let n = d.molecule().len();
            match ev {
                egui::Event::Text(t) => self.edit.type_text(d, &t, now),
                // A paste that needs consent parks itself in `pending_paste`
                // and the dialog goes up at the end of this frame; one that
                // does not is already applied, as a single operation.
                egui::Event::Paste(t) => {
                    let _needs_consent = self.edit.paste(d, &t);
                }
                // COPY IS COPY. It used to read `modifiers.shift` off the frame
                // and hand back the REVERSE COMPLEMENT when shift was down,
                // because egui-winit's `is_copy_command` is `modifiers.command
                // && key == C` — it pushes `Copy` and RETURNS, so Ctrl+Shift+C
                // never produces a `Key::C` event and the frame's modifiers
                // were the only signal available.
                //
                // That signal cannot tell the two intents apart, and the app's
                // own selection idiom is Shift+arrow (see `Key::ArrowRight`
                // below). Select with Shift+Right, then Ctrl+C without letting
                // go of shift — the ordinary keyboard path — and the clipboard
                // silently got the other strand: plausible DNA, wrong bases,
                // with a small orange line at the foot of the panel as the only
                // warning. The reverse complement now has a chord of its own,
                // which egui-winit does not swallow.
                egui::Event::Copy => {
                    if let Some(s) = self.do_copy() {
                        ui.ctx().copy_text(s);
                    }
                }
                egui::Event::Cut => {
                    if let Some(s) = self.do_cut(now) {
                        ui.ctx().copy_text(s);
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let shift = modifiers.shift;
                    let cmd = modifiers.command;
                    match key {
                        egui::Key::A if cmd => {
                            let all = seqedit::Selection {
                                anchor: 0,
                                head: n,
                                through_origin: false,
                            };
                            self.edit.set_selection(d, all, n);
                        }
                        // R, not C, and deliberately. Ctrl+Shift+C cannot be
                        // told apart from Ctrl+C with shift still held — see
                        // `Event::Copy` above — and Shift is exactly the key a
                        // user is holding when they have just selected with
                        // Shift+arrow. Ctrl+Shift+R reaches this match arm
                        // intact, and a plain Ctrl+R is bound to nothing, so
                        // neither half of the chord misfires.
                        egui::Key::R if cmd && shift => {
                            if let Some(s) = self.do_copy_rc() {
                                ui.ctx().copy_text(s);
                            }
                        }
                        egui::Key::Backspace if !cmd => {
                            self.edit.backspace(d, now);
                        }
                        egui::Key::Delete if !cmd => {
                            self.edit.delete_forward(d, now);
                        }
                        egui::Key::ArrowLeft if !cmd => self.edit.step(d, -1, shift),
                        egui::Key::ArrowRight if !cmd => self.edit.step(d, 1, shift),
                        egui::Key::ArrowUp if !cmd => self.edit.step(d, -(per_row as i64), shift),
                        egui::Key::ArrowDown if !cmd => self.edit.step(d, per_row as i64, shift),
                        egui::Key::PageUp => self.edit.step(d, -(per_page as i64), shift),
                        egui::Key::PageDown => self.edit.step(d, per_page as i64, shift),
                        egui::Key::Home => {
                            let to = if cmd {
                                0
                            } else {
                                (self.edit.caret / per_row) * per_row
                            };
                            self.edit.place(d, to, shift);
                        }
                        egui::Key::End => {
                            let to = if cmd {
                                n
                            } else {
                                ((self.edit.caret / per_row + 1) * per_row).min(n)
                            };
                            self.edit.place(d, to, shift);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    fn file_tab(&mut self, ui: &mut Ui) {
        let d = self.bench.get().expect("checked by caller");
        let m = d.molecule();
        ui.add_space(6.0);

        egui::Grid::new("fileinfo")
            .num_columns(2)
            .spacing([12.0, 5.0])
            .show(ui, |ui| {
                let mut row = |k: &str, v: String| {
                    ui.label(RichText::new(k).color(pal(ui).muted).size(11.5));
                    ui.label(RichText::new(v).monospace().size(11.5));
                    ui.end_row();
                };
                row("format", d.format.name().into());
                row("length", format!("{} bp", fmt_int(m.span())));
                row("topology", m.topology.as_str().into());
                match m.double_stranded {
                    Some(true) => row("strands", "double".into()),
                    Some(false) => row("strands", "single".into()),
                    // The source did not record it, so neither do we.
                    None => row("strands", "not recorded".into()),
                }
                match m.gc_percent() {
                    Some(gc) => row("GC", format!("{gc:.1}%")),
                    None => row("GC", "n/a".into()),
                }
                let lower = m.seq.iter().filter(|b| b.is_ascii_lowercase()).count();
                if lower > 0 {
                    row(
                        "lowercase",
                        format!("{} bases (masked or low-confidence)", fmt_int(lower as u64)),
                    );
                }
                let amb = m.composition().other;
                if amb > 0 {
                    row("ambiguous", format!("{} outside ACGT", fmt_int(amb)));
                }
                row("features", fmt_int(m.features.len() as u64));
                if !m.primers.is_empty() {
                    let sites: usize = m.primers.iter().map(|p| p.sites.len()).sum();
                    row("primers", format!("{} ({sites} sites)", m.primers.len()));
                }
                let meth = [
                    (m.methylation.dam, "Dam"),
                    (m.methylation.dcm, "Dcm"),
                    (m.methylation.ecoki, "EcoKI"),
                ]
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, n)| *n)
                .collect::<Vec<_>>();
                if !meth.is_empty() {
                    row("methylation", meth.join(", "));
                }
            });

        if let Some(c) = &d.container {
            ui.add_space(12.0);
            ui.label(RichText::new("SnapGene container").strong().size(12.5));
            ui.add_space(4.0);
            let total = c.total_bytes().max(1);
            let derived = c.derived_bytes();
            ui.label(
                RichText::new(format!(
                    "{} bytes in {} blocks · {:.0}% is a regenerable cache of \
                     (sequence × enzyme set)",
                    fmt_int(total as u64),
                    c.blocks.len(),
                    100.0 * derived as f64 / total as f64
                ))
                .color(pal(ui).muted)
                .size(11.5),
            );
            if c.history_present {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(if c.history_compressed {
                        "A cloning history tree is present, xz-compressed."
                    } else {
                        "A cloning history tree is present."
                    })
                    .color(pal(ui).muted)
                    .size(11.5),
                );
            }
            // A statement about the file, so it belongs beside the history-tree
            // line and not in the notes grid below: these paths name parts of
            // block 6 that have no row in that grid precisely because the model
            // could not hold them. Deliberately not "nested deeper" — a
            // `Notes/Comments/text()` entry is a note's own text, not a nested
            // element, and the wording has to cover every form the channel
            // carries or it will describe two of the three falsely.
            if !c.unrepresentable_notes.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "{} part(s) of this file's notes block cannot be held by this model \
                         and are not shown: {}",
                        c.unrepresentable_notes.len(),
                        c.unrepresentable_notes.join(", ")
                    ))
                    .color(pal(ui).muted)
                    .size(11.5),
                );
            }
        }

        if !m.notes.is_empty() {
            ui.add_space(12.0);
            ui.label(RichText::new("Notes").strong().size(12.5));
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    egui::Grid::new("notes")
                        .num_columns(2)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            for n in &m.notes {
                                ui.label(RichText::new(&n.key).color(pal(ui).muted).size(11.0));
                                // Two columns, and the attributes go *under* the
                                // value rather than into a third column: most
                                // notes have none, so a third column would be
                                // empty on nearly every row. What must not
                                // happen is a synthesised "Created (UTC=22:0:0)"
                                // key or a "2022.12.13 UTC=22:0:0" value — that
                                // is a syntax this grid would render raw and
                                // that no file contains.
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&n.value).size(11.0));
                                    for (k, v) in &n.attrs {
                                        ui.horizontal(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(
                                                RichText::new(k).color(pal(ui).muted).size(10.5),
                                            );
                                            ui.label(RichText::new(v).size(10.5));
                                        });
                                    }
                                });
                                ui.end_row();
                            }
                        });
                });
        }
    }

    /// Make sure [`App::gel_cache`] holds this document's gel, seeded on first
    /// sight, and say why if it cannot.
    ///
    /// `Err` carries the sentence to show where the lanes would be. It is
    /// returned rather than left for the caller to reconstruct from
    /// `DigestState`, because the caller then had to write an arm for `Done`
    /// that this function could never produce — a branch whose string nobody
    /// could ever see, sitting exactly where the empty gel needed one.
    ///
    /// # It is a MEMO, and the comment it replaces was wrong
    ///
    /// This used to be rebuilt on every repaint, defended by "it is also cheap
    /// — a few hundred `Monotone::at` calls on fragment lengths already in
    /// hand — so there is nothing to cache and nothing to go stale". On a 4.6 Mb
    /// genome a frame ran `Gel::run` over ~14,000 fragments across seven lanes
    /// and formatted kilobytes of disclosure, and twelve mouse-moves over the
    /// pane cost 4.8x what the same twelve cost over the map. Nothing about a
    /// gel changes between two frames in which nothing changed, so the key
    /// below is the complete list of what a picture depends on.
    fn gel_ready(&mut self) -> Result<(), String> {
        let Some(d) = self.bench.get() else {
            self.gel_cache = None;
            return Err("no molecule is open".into());
        };
        match &d.digest {
            doc::DigestState::Running { .. } => {
                self.gel_cache = None;
                return Err("scanning for restriction sites…".into());
            }
            doc::DigestState::Unavailable(why) => {
                let why = why.clone();
                self.gel_cache = None;
                return Err(why);
            }
            doc::DigestState::Done(_) => {}
        }
        let results = d.digest.results();
        // The same table the Enzymes tab reads, so a row and a lane cannot give
        // opposite answers about one enzyme.
        let verdicts: Vec<Option<pl_enzymes::methylation::SiteEffect>> =
            (0..results.len()).map(|i| d.digest.verdict(i)).collect();
        if !self.gel.seeded {
            self.gel
                .seed(d.molecule(), results, &verdicts, self.enzyme_set);
        }
        let key = GelKey::of(self, &verdicts);
        if self.gel_cache.as_ref().is_some_and(|(k, _)| *k == key) {
            return Ok(());
        }
        let d = self.document().expect("checked above");
        let built = self.gel.build(
            d.molecule(),
            d.digest.results(),
            &verdicts,
            self.enzyme_set,
            &d.title,
        );
        self.gel_cache = Some((key, built));
        Ok(())
    }

    fn central(&mut self, ui: &mut Ui) {
        let error = self.error.clone();
        let selected = self.selected;
        let hot = self.hot_shown;
        let mut hovered_out = None;
        let mut clicked_out = None;
        let mut opened_out = None;
        let mut site_out: Option<Vec<(String, u64)>> = None;
        let mut pane_out: Option<egui::Rect> = None;
        // Straight from `selection_segment`, never re-derived from
        // `self.edit.sel` at the call site: that function is already the app's
        // single source of truth for which of the two arcs on a circle is meant.
        // It applies `Selection::canonical`, reads `through_origin` rather than
        // inferring it from the ordering, and does the caret-gap-to-1-based
        // conversion whose off-by-one is documented there. The app ships a
        // "take the other arc" button precisely because it refuses to guess.
        let sel_seg = self.selection_segment();
        let caret_at = self.document().map(|_| self.edit.caret);
        // ONE control, one answer. `self.enzyme_set` reached the Enzymes list,
        // the inline cut marks and the "N site(s) hidden" line, and the map was
        // called unfiltered — so narrowing to "Unique 6+" to pick a
        // linearisation site left the map, and the exported figure, showing
        // something else.
        //
        // INTERSECTION, not replacement, and the difference is the whole
        // reconciliation. `map.rs` argues, correctly, that its own rule must
        // survive as a floor: the Enzymes tab defaults to "All cutters", which
        // on pKoV is 40 enzymes and about 100 ticks — a map nobody can read,
        // arrived at without the user asking for it. Worked through all five
        // sets, `All`, `Unique` and `UniqueDual` intersect with the unique
        // filter to exactly today's picture; only `SixPlus` and `UniqueSixPlus`
        // are genuinely narrower, and those the map now follows.
        // The Features filter's ONE reach into the map: which names get the
        // label budget first. Never which features are drawn.
        let lit: Option<Vec<usize>> = (!self.filter.is_empty())
            .then(|| {
                let needle = self.filter.to_lowercase();
                self.document().map(|d| {
                    d.molecule()
                        .features
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| {
                            f.name.to_lowercase().contains(&needle)
                                || f.kind.to_lowercase().contains(&needle)
                                || f.qualifiers.iter().any(|q| {
                                    q.1.as_deref()
                                        .is_some_and(|v| v.to_lowercase().contains(&needle))
                                })
                        })
                        .map(|(i, _)| i)
                        .collect()
                })
            })
            .flatten();
        let enzyme_set = self.enzyme_set;

        // Asked here, outside the paint closure, and answered from a memo.
        // Which application owns the extension is *read*, never changed —
        // claiming .dna at install time is how two plasmid editors end up
        // fighting over double-click — but reading it live inside the closure
        // spawned a `cmd /C assoc .dna` child process on every repaint and
        // blocked the UI thread on it until cmd.exe exited.
        let association = if self.error.is_none() && self.document().is_none() {
            association_note(self.dna_owner())
        } else {
            String::new()
        };

        let notice = self.notice.clone();
        let mut dismiss = false;

        // BUILT BEFORE THE PANEL, not inside it. The gel needs `&self.document`
        // and `&mut self.gel` at once, and the paint closure already holds a
        // borrow of `self`; computing the picture here and letting the closure
        // only draw it keeps both honest. It is also memoised — see
        // `gel_ready` — so on all but the frame after a change this is a key
        // comparison and nothing else.
        let gel_state = (self.central_view == CentralView::Gel).then(|| self.gel_ready());
        // MOVED OUT rather than cloned, and put back after the closure. Cloning
        // it would undo the memo: a genome gel's `Scene` is thousands of items,
        // and copying them once a frame is the cost the cache exists to remove.
        let built = self.gel_cache.take();
        let view_was = self.central_view;
        let mut view_next = view_was;
        // The controls write into a COPY and are applied after the closure, for
        // the same borrow reason.
        let mut gel_next = GelControls::of(&self.gel);
        let mut methods = false;
        let mut show_all = false;

        egui::CentralPanel::default().show(ui, |ui| {
            self.recovery_banner(ui);
            // A refused *edit*, with the document still on screen.
            //
            // This used to go to `self.error`, which renders as a full-screen
            // takeover captioned "Could not read that file" and removes the
            // map: the application answered "that deletion would orphan two
            // features" by telling the user their file was unreadable and
            // hiding it. The message itself was always good — `OpError` names
            // the feature, its index and the numbers — the presentation was the
            // bug, and it already shipped for the four ops that were wired.
            if let Some(msg) = &notice {
                egui::Frame::NONE
                    .fill(pal(ui).selection())
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        // THE BUTTON IS RESERVED FIRST. Laid out after the
                        // label, a long message pushed Dismiss past the
                        // CentralPanel's right edge and it read "Dism" — on a
                        // banner whose only control is the way to make it go
                        // away. A right-to-left layout takes the button's width
                        // off the top, so the label wraps into what is left
                        // however long it gets.
                        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                            if ui.button("Dismiss").clicked() {
                                dismiss = true;
                            }
                            ui.vertical(|ui| {
                                ui.label(RichText::new(msg).color(pal(ui).ink));
                                // Literally true: `OpLog::apply` works on a
                                // clone and returns before touching `current`.
                                // Say it, because a user who sees an error
                                // assumes something half-happened and goes
                                // hunting for it.
                                ui.label(
                                    RichText::new("Nothing was changed.")
                                        .color(pal(ui).muted)
                                        .size(11.0),
                                );
                            });
                        });
                    });
            }
            if let Some(err) = &error {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new(format!("Could not read that file\n\n{err}"))
                            .color(pal(ui).warn)
                            .size(13.0),
                    );
                });
                return;
            }
            let Some(d) = self.bench.get() else {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "Drop a .dna, GenBank or FASTA file here\n\n\
                             Nothing leaves this machine.{association}"
                        ))
                        .color(pal(ui).muted)
                        .size(14.0),
                    );
                });
                return;
            };

            if d.molecule().annotation_span() == 0 {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("This file describes nothing to draw.").color(pal(ui).muted),
                    );
                });
                return;
            }

            // The fallback stays; only the extension goes. `map.rs` is right
            // that the `.dna` container carries no molecule name and that the
            // filename is the only thing left to print — but "pKoV with His
            // decR.dna" is what the *container* is called and "pKoV with His
            // decR" is what the plasmid is called. `Document::title` keeps the
            // whole filename, because the toolbar, the hover and the recovery
            // header all want the real one.
            let caption = if d.molecule().name.is_empty() {
                pl_fileio::caption_of(&d.title)
            } else {
                d.molecule().name.as_str()
            };
            if debug_geometry() {
                eprintln!(
                    "geometry: central max={:?} avail={:?} clip={:?}",
                    ui.max_rect(),
                    ui.available_rect_before_wrap(),
                    ui.clip_rect()
                );
            }
            // The switch, and the gel's conditions beside it when the gel is
            // up. It costs the map about 24 pt of vertical extent — roughly 3%
            // of its diameter at the default window — which is the price, and
            // stating it is better than letting somebody discover it. No new
            // geometry code is needed: `map::show` derives its rect from
            // `available_rect_before_wrap()` after whatever was laid out above
            // it, which is exactly how the recovery banner is accommodated.
            ui.horizontal_wrapped(|ui| {
                for (v, name) in [(CentralView::Map, "Map"), (CentralView::Gel, "Gel")] {
                    if ui.selectable_label(view_was == v, name).clicked() {
                        view_next = v;
                    }
                }
                if view_was == CentralView::Gel {
                    gel_controls(ui, &mut gel_next);
                }
            });
            if view_was == CentralView::Gel {
                // Mirrors the Enzymes tab exactly: the lanes ARE the digest, so
                // a gel cannot say more than the digest does — and the sentence
                // for each way it can say nothing comes from `gel_ready`, next
                // to the condition that produced it.
                match (&gel_state, &built) {
                    (Some(Ok(())), Some((_, b))) => {
                        gel_pane(ui, b, &mut methods, &mut show_all);
                    }
                    (Some(Err(why)), _) => {
                        ui.centered_and_justified(|ui| {
                            ui.label(RichText::new(why).color(pal(ui).muted));
                        });
                    }
                    _ => {}
                }
                return;
            }
            let r = map::show(
                ui,
                d.molecule(),
                caption,
                d.digest.results(),
                selected,
                hot,
                sel_seg,
                caret_at,
                enzyme_set,
                lit.as_deref(),
            );
            hovered_out = r.hovered;
            clicked_out = r.clicked;
            opened_out = r.double_clicked;
            site_out = r.hovered_site;
            pane_out = Some(r.pane);
        });

        // Back where it came from, so the next frame is a key comparison.
        self.gel_cache = built;
        if dismiss {
            self.notice = None;
        }
        self.central_view = view_next;
        gel_next.apply(&mut self.gel);
        if show_all {
            self.enzyme_set = pl_enzymes::EnzymeSet::All;
        }
        if methods {
            // The citable paragraph `pl-doc` already ships, reachable until now
            // only by typing `pl methods gel` in a terminal.
            let text = pl_doc::topic("gel")
                .map(pl_doc::methods)
                .unwrap_or_default();
            ui.ctx().copy_text(text);
            self.status = "the gel methods paragraph is on the clipboard".into();
        }
        // ONE assignment, after both panels have run, rather than two guarded
        // ones. `or` says the arbitration rule out loud, and it is safe and
        // order-independent because the side panel and the central panel are
        // disjoint rects and the pointer is in at most one of them. What it
        // accumulates is read by BOTH panels on the next frame — see
        // `App::hot_shown`.
        self.hot = hovered_out.or(self.hot);
        // The tooltip. `map.rs` returns WHAT is under the pointer; the words are
        // composed here, because this is the only place that has `DigestState`
        // and can therefore answer the methylation question with the SAME
        // `verdict` the Enzymes tab prints. A fourth surface with its own answer
        // to that question would widen the split-brain the review calls
        // finding 5.
        if let (Some(pane), Some(d)) = (pane_out, self.document()) {
            let tip = if let Some(sites) = &site_out {
                let mut lines: Vec<String> = Vec::new();
                for (name, pos) in sites {
                    // One line per enzyme carrying its OWN coordinate, never
                    // `XmaI/SmaI  6,917`: XmaI leaves a 4-base 5' overhang and
                    // SmaI is blunt, and `ring::Site::label` refuses to collapse
                    // the range for exactly that reason.
                    let mut l = format!("{name}  {}", fmt_int(*pos));
                    if let Some(e) = pl_enzymes::by_name(name) {
                        l.push_str(&format!("\n{}", e.site));
                    }
                    if let Some(i) = d
                        .digest
                        .results()
                        .iter()
                        .position(|x| x.enzyme.name == name.as_str())
                    {
                        // The SAME tag the Enzymes tab prints, from the same
                        // field. `verdict` exists because recomputing it cost 58
                        // full-molecule scans per frame.
                        if let Some(b) = d.digest.verdict(i) {
                            l.push_str(&format!(" · {} {}", b.methylase.name(), b.effect.as_str()));
                        }
                    }
                    lines.push(l);
                }
                Some(lines.join("\n"))
            } else {
                hovered_out
                    .and_then(|i| d.molecule().features.get(i))
                    .map(|f| feature_tip(f, d.molecule()))
            };
            if let Some(text) = tip {
                // AT THE POINTER, not on the pane.
                //
                // `Response::on_hover_text` anchors the tooltip to the widget's
                // rect, and this widget is the whole map. Measured in the
                // running app, three hovers at three points on three different
                // bands put the tooltip in the same place every time — the
                // bottom-right corner of the window, on top of the Features
                // list, up to 560 px from the cursor and outside the map pane
                // entirely. A tooltip that does not sit beside the thing it
                // describes is not an affordance for that thing.
                //
                // `pane` still has to be interacted with: without a hovered
                // response the map has no cursor icon and no hover state at
                // all.
                ui.interact(pane, ui.id().with("map-tip"), Sense::hover())
                    .on_hover_text_at_pointer(text);
            }
        }
        if let Some(i) = clicked_out {
            self.selected = if self.selected == Some(i) {
                None
            } else {
                Some(i)
            };
            self.tab = Tab::Features;
        }
        // After the click has been handled: egui delivers both, and a
        // double-click that left `selected` toggled off would open the editor on
        // a feature the panel is no longer highlighting. `open_feature_editor`
        // is where the highlight is put back, for every entry point at once.
        if let Some(i) = opened_out {
            self.open_feature_editor(Some(i));
        }
    }

    /// The one modal in the editing surface, and it only appears when a paste
    /// would either drop characters the user did not ask to lose or change the
    /// size of the document beyond recognition.
    ///
    /// A confirmation, never a refusal: somebody assembling a synthetic
    /// chromosome legitimately pastes megabases, and a hard cap makes the tool
    /// useless to them while a confirm costs one keypress. The real safety net
    /// is still undo — one paste is one operation, and the log forks rather
    /// than truncates, so the pasted branch stays reachable even afterwards.
    /// Open the design panel on the current selection.
    ///
    /// The commit comes first, and it is not tidiness. Between keystrokes the
    /// log is one run behind the screen (see `seqedit`'s module doc), so
    /// designing against the committed molecule while three more typed bases
    /// are visible would return primers for a sequence that is not on screen.
    fn open_design(&mut self) {
        self.settle();
        let Some(d) = self.bench.get() else {
            return;
        };
        let Some(sel) = self.edit.sel else { return };
        let mol = d.molecule();
        let title = d
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| mol.name.clone());
        match design::Panel::open(title, mol.len(), mol.topology.is_circular(), sel) {
            Ok(p) => {
                // The other direction of the same rule the feature editor
                // applies: only one non-modal window may guard the keyboard.
                self.close_feature_editor(
                    "the feature editor was closed: the design panel took the keyboard",
                );
                self.design = Some(p);
            }
            Err(e) => self.notice = Some(e),
        }
    }

    /// The document on screen.
    ///
    /// Named `document()` rather than `active()` so the 88 call sites this
    /// replaced read as they did — `self.document()` where they said
    /// `self.document.as_ref()`. Renaming them as well would have been churn on
    /// top of a container swap, and that stage was worth being reviewable as a
    /// no-behaviour-change diff.
    fn document(&self) -> Option<&Document> {
        self.bench.get()
    }

    /// The open documents, one row, above the panels.
    ///
    /// Hidden entirely at one tab. A strip that shows a single item teaches
    /// nothing and costs a row of a laptop screen, and this app's own map has
    /// already had the argument about spending vertical space on something that
    /// says nothing.
    ///
    /// The dot marks unsaved work, and it is the only place a user can see that
    /// a tab they are NOT looking at has edits in it — which is the whole reason
    /// the close guard had to change.
    fn tab_strip(&mut self, ui: &mut Ui) {
        let titles = self.bench.titles();
        if titles.len() < 2 {
            return;
        }
        let active = self.bench.active();
        let mut go: Option<usize> = None;
        let mut shut: Option<usize> = None;
        egui::Panel::top(egui::Id::new("tabs")).show(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                for (i, (title, unsaved)) in titles.iter().enumerate() {
                    // Bounded, for the reason the Features list is bounded: a
                    // 150-character /label must not be able to lay out the
                    // application. See `a_long_feature_name_cannot_lay_out_the_panel`.
                    let shown = pl_fileio::caption_of(title);
                    let label = format!("{}{}", if *unsaved { "• " } else { "" }, shown);
                    let r = ui
                        .add(
                            egui::Button::selectable(i == active, label)
                                .wrap_mode(egui::TextWrapMode::Truncate),
                        )
                        .on_hover_text(title);
                    if r.clicked() {
                        go = Some(i);
                    }
                    if r.middle_clicked() {
                        shut = Some(i);
                    }
                }
            });
            ui.add_space(2.0);
        });
        if let Some(i) = go {
            self.switch_tab(i);
        }
        if let Some(i) = shut {
            self.close_tab(i);
        }
    }

    /// Close tab `i`.
    ///
    /// NO GUARD HERE, and that is deliberate rather than an omission: a closed
    /// tab is kept and Ctrl+Shift+T puts it back, edits and undo history
    /// intact, so closing one destroys nothing. The question is asked once, at
    /// the point where work really does go away — closing the window — and
    /// asking it twice is how a guard becomes a reflex click.
    fn close_tab(&mut self, i: usize) {
        if i == self.bench.active() {
            self.settle();
            let v = self.take_view();
            self.bench.store(v);
        }
        if let Some(t) = self.bench.close(i) {
            self.closed.push(t);
            // The panels belonged to a molecule that is no longer on screen.
            self.close_design("the design panel was closed: its tab was closed");
            self.close_feature_editor("the feature editor was closed: its tab was closed");
            self.clone_panel = None;
            if let Some(v) = self.bench.take_active_view() {
                self.put_view(v);
            }
            self.doc_generation = self.doc_generation.wrapping_add(1);
        }
    }

    /// Lift the active tab's view off `App`, leaving a blank one behind.
    ///
    /// Written out field by field, with `put_view` its mirror, because the
    /// failure mode is silent: a field this forgets stays on `App` and then
    /// belongs to whichever tab you switch TO, so one molecule's caret,
    /// selection or feature filter appears over another's. Nothing crashes and
    /// nothing looks wrong — it just describes the wrong plasmid.
    /// `no_view_state_leaks_between_tabs` enumerates the fields independently so
    /// the two lists have to agree.
    fn take_view(&mut self) -> bench::DocView {
        bench::DocView {
            status: std::mem::take(&mut self.status),
            notice: self.notice.take(),
            edit: std::mem::replace(&mut self.edit, seqedit::SeqEdit::new()),
            selected: self.selected.take(),
            hot: self.hot.take(),
            hot_shown: self.hot_shown.take(),
            filter: std::mem::take(&mut self.filter),
            enz_strip: std::mem::take(&mut self.enz_strip),
            orf_strip: std::mem::take(&mut self.orf_strip),
            tr: std::mem::take(&mut self.tr),
            doc_code: self.doc_code,
            gel: std::mem::take(&mut self.gel),
            central_view: std::mem::replace(&mut self.central_view, CentralView::Map),
        }
    }

    fn put_view(&mut self, v: bench::DocView) {
        self.status = v.status;
        self.notice = v.notice;
        self.edit = v.edit;
        self.selected = v.selected;
        self.hot = v.hot;
        self.hot_shown = v.hot_shown;
        self.filter = v.filter;
        self.enz_strip = v.enz_strip;
        self.orf_strip = v.orf_strip;
        self.tr = v.tr;
        self.doc_code = v.doc_code;
        self.gel = v.gel;
        self.central_view = v.central_view;
    }

    /// Show tab `i`.
    ///
    /// The open typing run is SETTLED first, for the reason every durable action
    /// settles it: a run is uncommitted work living outside the op log, and
    /// carrying one across a switch would leave it to be committed against
    /// whichever molecule you land on.
    ///
    /// The three panels that belong to a molecule are closed, exactly as `adopt`
    /// closes them — the design panel writes coordinates into a named file, the
    /// feature editor holds an index into one feature list, and the religation
    /// panel holds a whole digest. Switching tabs invalidates all three as
    /// thoroughly as replacing the document did, and this is the second entrance
    /// to that hazard rather than a new one.
    fn switch_tab(&mut self, i: usize) {
        if i == self.bench.active() || i >= self.bench.len() {
            return;
        }
        self.settle();
        self.close_design("the design panel was closed: it was designed against another tab");
        self.close_feature_editor("the feature editor was closed: it was opened on another tab");
        self.clone_panel = None;
        let out = self.take_view();
        self.bench.store(out);
        if let Some(v) = self.bench.activate(i) {
            self.put_view(v);
        }
        // The digest, the ORF scan and the map all key off this.
        self.doc_generation = self.doc_generation.wrapping_add(1);
    }

    /// Draw the cut-and-religate panel and adopt a product if one was asked for.
    ///
    /// The product becomes a document THROUGH THE FILE PATH — serialised to
    /// GenBank and parsed straight back — rather than by hand-building a
    /// `Document`. That is a deliberate detour: it means a construct behaves
    /// exactly like one that was opened from disk, gets the same load report,
    /// the same digest worker and the same unsaved-changes protection, and it
    /// cannot drift into a second, subtly different way of being a document.
    /// `of_molecule` exists for the same job and is `#[cfg(test)]`, which is the
    /// right place for it.
    ///
    /// It arrives with no path, so it is unsaved by construction and the close
    /// guard already covers it. A product nobody saves is a product nobody
    /// loses by accident.
    fn clone_panel(&mut self, ctx: &egui::Context) {
        // Checked before the take, for the reason `design_panel` documents: a
        // panel that outlives its document is the state that writes one file's
        // answer into another.
        if self.document().is_none() {
            self.clone_panel = None;
            return;
        }
        let Some(mut panel) = self.clone_panel.take() else {
            return;
        };
        let dark = ctx.options(|o| o.theme_preference) != egui::ThemePreference::Light;
        let mol = self.document().expect("checked above").molecule().clone();
        let keep = clone::show(ctx, &mut panel, &mol, dark);

        if let Some(i) = panel.wanted.take() {
            if let Some(p) = panel.plan.as_ref().and_then(|pl| pl.prods.get(i)) {
                let title = format!("{} product", mol.name);
                let (bytes, _unwritable) =
                    pl_fileio::genbank::write_reporting(&p.mol, &title, today());
                match Document::from_bytes(bytes.as_bytes(), title, None) {
                    Ok(d) => {
                        let n = p.mol.seq.len();
                        let carried = p.carried;
                        let dropped = p.dropped;
                        // THROUGH `take_over`, NEVER `adopt`. 28e9d91 called
                        // `adopt` here directly, so a user who had edited their
                        // plasmid and then religated it lost those edits the
                        // moment they clicked Open — no prompt, no undo. That is
                        // the eighth path of the class cc36cf7 exists to close,
                        // and it was introduced by the commit that added this
                        // panel. The one funnel asks the question; the parked
                        // construct is adopted only if the answer is yes.
                        let status = format!(
                            "{n} bp construct — {carried} feature(s) carried over{}",
                            if dropped > 0 {
                                format!(", {dropped} left behind")
                            } else {
                                String::new()
                            }
                        );
                        self.take_over(d, status, None);
                    }
                    // The writer and the reader are both ours, so this is a bug
                    // rather than a bad file; say which of the two to look at.
                    Err(e) => {
                        self.error = Some(format!(
                            "the product could not be re-read after being written: {e}"
                        ))
                    }
                }
            }
        }
        if keep {
            self.clone_panel = Some(panel);
        }
    }

    fn design_panel(&mut self, ctx: &egui::Context) {
        // Checked *before* the take. Taking first and then returning on a
        // missing document dropped the panel, its constraints and its report
        // with nothing said — a failed load did it in the same frame as the
        // "could not read that file" takeover. `load` now closes the panel
        // deliberately and says so, which leaves this branch unreachable in
        // practice; it stays because a panel that outlives its document is
        // exactly the state that writes one file's primers into another.
        if self.document().is_none() {
            self.close_design(
                "the design panel was closed: the document it described is no longer open",
            );
            return;
        }
        let Some(mut panel) = self.design.take() else {
            return;
        };
        let dark = ctx.options(|o| o.theme_preference) != egui::ThemePreference::Light;
        let (seq, at) = {
            let Some(d) = self.bench.get() else {
                self.design = Some(panel);
                return;
            };
            (d.molecule().seq.clone(), d.log.cursor())
        };
        // Where the document stands this frame. `Panel::run` copies it onto the
        // report it produces, so the panel can tell whether the answer on
        // screen is still about the molecule on screen.
        panel.doc_at = at;
        let keep = design::show(ctx, &mut panel, &seq, dark);

        // Applied after the frame, because `App::edit` needs `&mut self` and
        // the panel is holding a borrow of it during the closure.
        if let Some(i) = panel.add_request.take() {
            // The window is not modal, so the toolbar and the Edit menu stayed
            // live between "Design" and "Add to document". A reverse complement
            // in between left the footprint coordinates naming unrelated bases,
            // with the length unchanged so `validate()` had nothing to say and
            // the WouldCorrupt gate accepted both features.
            if let Some(why) = panel.stale_reason() {
                self.notice = Some(why.to_string());
            } else if let Some(Ok(r)) = &panel.result {
                if let Some(p) = r.pairs.get(i) {
                    let stem = design::stem_of(&panel.title);
                    let fs = design::features(p, &stem, i + 1);
                    let names: Vec<String> = fs.iter().map(|f| f.name.clone()).collect();
                    let ok: Vec<bool> = fs
                        .into_iter()
                        .map(|f| {
                            self.edit(pl_core::OpKind::SetFeature {
                                index: None,
                                feature: Box::new(f),
                            })
                        })
                        .collect();
                    let added = ok.iter().filter(|x| **x).count();
                    // Counted, not assumed. Telling a user to press Ctrl+Z
                    // twice when only one feature landed undoes their previous,
                    // unrelated edit as well — the silent half-state ADR-2
                    // exists to prevent, produced by the sentence meant to
                    // prevent it. `App::edit` has already put the refusal
                    // itself in `notice`.
                    match added {
                        2 => {
                            panel.added.push(i);
                            self.status = format!(
                                "added 2 primer_bind features, {} and {} - Ctrl+Z twice to \
                                 undo both",
                                names[0], names[1]
                            );
                        }
                        1 => {
                            let which = if ok[0] { &names[0] } else { &names[1] };
                            self.status = format!(
                                "added 1 primer_bind feature, {which} - the other was \
                                 refused - Ctrl+Z once to undo it"
                            );
                        }
                        // Both refused: `notice` already says why, and the pair
                        // stays addable so the user can retry after fixing it.
                        _ => self.status = "no primer_bind feature was added".into(),
                    }
                }
            }
        }
        if keep {
            self.design = Some(panel);
        }
    }

    /// Run the feature editor for a frame, and service whatever it asked for.
    ///
    /// The shape is `design_panel`'s, including the check before the take: a
    /// panel that outlives its document is exactly the state that writes one
    /// file's coordinates into another.
    fn feature_editor(&mut self, ctx: &egui::Context) {
        if self.document().is_none() {
            self.close_feature_editor(
                "the feature editor was closed: the document it described is no longer open",
            );
            return;
        }
        let Some(mut panel) = self.feature_edit.take() else {
            return;
        };
        let dark = ctx.options(|o| o.theme_preference) != egui::ThemePreference::Light;
        let sel = self
            .selection_segment()
            .map(|s| (s.start, s.end, s.end < s.start));
        // Where the document stands THIS frame. The panel compares it against
        // where it stood on open, and refuses to commit through a moved index.
        panel.doc_at = self.document().and_then(|d| d.log.cursor());
        let mut keep = featedit::show(ctx, &mut panel, sel, dark);

        // After the frame: `App::edit` needs `&mut self`, which the draw closure
        // is holding.
        if std::mem::take(&mut panel.delete) {
            // Into the PANEL, not `self.notice`. `central` paints the notice
            // banner at the top-left of the map, which is where this window sits
            // by default: photographed, the explanation rendered behind the
            // editor with a sliver showing, so the user pressed a button,
            // nothing happened, and the reason was underneath the thing they
            // were looking at.
            if let Some(why) = panel.stale_reason() {
                panel.notice = Some(why.to_string());
            } else if let Some(i) = panel.index {
                self.remove_feature(i);
                keep = false;
            }
        }
        if std::mem::take(&mut panel.save) {
            // Asked again here, and not only where the button was drawn. The
            // button's disabled state is a claim made by one frame's layout;
            // this is the claim the document acts on, and the two must not be
            // able to disagree — the gate's own refusal names a feature index
            // that, for an add, does not exist after the refusal.
            let refusals = panel.refusals();
            if let Some(why) = panel.stale_reason() {
                panel.notice = Some(why.to_string());
            } else if let Some(first) = refusals.first() {
                panel.notice = Some(format!("{first} Nothing was changed."));
            } else if panel.is_noop() {
                // An operation that changes nothing still derives an id, spends
                // an undo step and dirties the document, and the title bar then
                // claims unsaved changes that do not exist. Harmless to the file,
                // corrosive to trust in the dirty flag.
                self.status = "nothing was changed, so nothing was recorded".into();
                keep = false;
            } else {
                let f = panel.to_feature();
                let (name, kind) = (f.name.clone(), f.kind.clone());
                let adding = panel.index.is_none();
                if self.edit(pl_core::OpKind::SetFeature {
                    index: panel.index,
                    feature: Box::new(f),
                }) {
                    if adding {
                        // `SetFeature { index: None }` pushes, so the new
                        // feature is the last one. Selecting it is what puts it
                        // under the highlight the user is already looking at.
                        let last = self
                            .document()
                            .map(|d| d.molecule().features.len())
                            .unwrap_or(0);
                        self.selected = last.checked_sub(1);
                    }
                    // The Features filter matches name and kind, so renaming
                    // `SacB` to `levansucrase` under a filter of "sac" makes the
                    // row vanish the instant Save lands. The filter is NOT
                    // cleared — the user set it — but a row disappearing without
                    // a word is the kind of thing people file bugs about.
                    let needle = self.filter.to_lowercase();
                    if !needle.is_empty()
                        && !name.to_lowercase().contains(&needle)
                        && !kind.to_lowercase().contains(&needle)
                    {
                        self.status = format!(
                            "{} — it no longer matches the filter \"{}\"",
                            self.status, self.filter
                        );
                    }
                    keep = false;
                }
            }
        }
        if keep {
            self.feature_edit = Some(panel);
        }
    }

    fn paste_dialog(&mut self, ctx: &egui::Context) {
        // The selection the question was asked about, not wherever the caret
        // has since got to. It was stored and then thrown away: with the
        // document still live behind a non-modal window, one click was enough
        // to make the paste land somewhere else while the dialog's own text
        // described the old target.
        let Some((report, target)) = self.edit.pending_paste.clone() else {
            return;
        };
        let n = self.document().map_or(0, |d| d.molecule().len());
        let added = report.bases.len() as u64;
        let mut go = false;
        let mut cancel = false;

        // `Modal`, not `Window`. A plain window is not modal in egui and
        // `Button` registers focus interest without ever taking focus, so the
        // document behind this one stayed fully live: the caret could be moved,
        // and the toolbar clicked, between the question and the answer.
        egui::Modal::new(egui::Id::new("pl-paste-consent")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.heading("Paste");
            ui.add_space(6.0);
            if seqedit::is_a_lot(added, n) {
                ui.label(RichText::new(paste_size_question(added, n)));
                ui.add_space(6.0);
            }
            if report.needs_consent() {
                ui.label(RichText::new(report.consent_question()).size(11.5));
                ui.add_space(6.0);
            }
            if !report.dropped.is_empty() {
                ui.label(
                    RichText::new(format!("Also dropped: {}", report.dropped.join(", ")))
                        .color(pal(ui).muted)
                        .size(11.0),
                );
                ui.add_space(6.0);
            }
            ui.horizontal(|ui| {
                // Cancel first, and it is what Escape does.
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                // The total, not the sum over the kinds that fit in the
                // dialog: the tally is capped and this number is a promise
                // about the whole paste.
                let dropping = report.rejected_total;
                let label = if dropping > 0 {
                    format!(
                        "Paste {} bases, discarding {dropping} characters",
                        fmt_int(added)
                    )
                } else {
                    format!("Paste {} bases", fmt_int(added))
                };
                if ui.button(label).clicked() {
                    go = true;
                }
            });
        });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if cancel {
            self.edit.pending_paste = None;
            self.edit.say("Paste cancelled. Nothing was changed.");
        } else if go {
            self.edit.pending_paste = None;
            if let Some(d) = self.bench.get_mut() {
                let target = target.unwrap_or_else(|| self.edit.target(d));
                self.edit.insert_paste(d, &report, target);
            }
        }
    }
}

/// The line under the welcome text naming whoever currently owns `.dna`.
///
/// Doing it unasked is worse than not doing it: say who owns the extension, and
/// leave the decision where it belongs. Silent when Polylinker already owns it,
/// and silent when nothing does — there is nothing to tell the user in either
/// case.
fn association_note(owner: Option<&str>) -> String {
    match owner {
        Some(h) if !h.contains("Polylinker") => format!(
            "\n\n.dna files currently open in {h}.\n\
             Polylinker does not change that."
        ),
        _ => String::new(),
    }
}

/// The large-paste question, in the units that make the mistake obvious.
///
/// There is no ratio when there is nothing to take a ratio of. `checked_div`
/// returned `None` on the zero divisor and `unwrap_or` handed back the absolute
/// length as if it were one, so pasting 80,000 bases into an empty document
/// read "the document becomes 80,000 bp — 80000x longer".
fn paste_size_question(added: u64, n: u64) -> String {
    let after = n + added;
    match n {
        0 => format!(
            "Paste {} bases into an empty document?
It becomes {} bp.",
            fmt_int(added),
            fmt_int(after)
        ),
        n => format!(
            "Paste {} bases into a {} bp molecule?
             The document becomes {} bp — {}x longer.",
            fmt_int(added),
            fmt_int(n),
            fmt_int(after),
            after / n
        ),
    }
}

/// What the clipboard did not get, when the sequence holds bytes that are not
/// nucleotide codes. Empty in the ordinary case, which is every real file.
fn not_copied(skipped: usize) -> String {
    if skipped == 0 {
        return String::new();
    }
    format!(
        " · {} byte(s) are not nucleotide codes and were left out",
        fmt_int(skipped as u64)
    )
}

/// The History tab's cursor on the current state, set `.monospace()`.
///
/// A constant so the test that asks the monospace face whether it HAS this glyph
/// and the label that draws it cannot name different characters. The same reason
/// `set_origin_path` became one: the refusal prose and the menu had drifted, and a
/// list of glyphs written out again in a test is a list that drifts the same way.
/// U+25B6 is in Hack and in both emoji fonts; Consolas has no such glyph, and
/// Consolas is in the candidate table the advance band prints.
pub const HISTORY_HERE: &str = "▶";

/// "There are more coordinates than the four shown", set `.monospace()`.
pub const MORE_MARK: &str = "…";

fn strand_glyph(s: Strand) -> &'static str {
    match s {
        Strand::Forward => "→",
        Strand::Reverse => "←",
        Strand::Both => "↔",
        Strand::Unoriented => "·",
    }
}

/// The same thing in words, for anywhere the arrows cannot be drawn.
///
/// Written when the proportional face was Ubuntu Light, which has no U+2190: a
/// `←` set in it came out a tofu box, and the features list got away with the
/// arrow only because it asks for `.monospace()`. Found by looking at the running
/// app, because `strand_glyphs_cover_every_variant` asserts the strings are
/// non-empty and a tofu box is a non-empty string.
///
/// **IBM PLEX SANS HAS U+2190, so that specific box is gone — and the words
/// stay.** The reason was never only the missing glyph. The hover readout is the
/// sequence view's non-colour channel, the one place a reverse feature's
/// direction is stated in the sequence at all, and "reverse" is legible to a
/// screen reader, in a monochrome screenshot and to someone who does not read
/// arrows as direction. Keeping this because a font swap happened to fix the
/// symptom would be reasoning from the defect instead of from the requirement.
/// The feature under the pointer, in the words the rest of the app uses.
///
/// Coordinates from `f.extent(span, circular)` — the SAME call the Features list
/// makes — so the two surfaces cannot print different coordinates for one
/// origin-crossing feature. Strand as the WORD and not the glyph alone:
/// `featedit.rs` builds every strand option as glyph AND word for a documented
/// reason, and pKoV has three `Unoriented` features whose "no strand" must be
/// printed rather than guessed at.
///
/// Line 3 is what makes the tab jump on click explicable rather than an
/// unexplained tab switch.
fn feature_tip(f: &pl_core::Feature, mol: &pl_core::Molecule) -> String {
    let span = mol.span();
    let circular = mol.topology.is_circular();
    let (fs, fe) = f.extent(span, circular).unwrap_or((f.start(), f.end()));
    let bp = if fs <= fe {
        fe - fs + 1
    } else {
        span - fs + 1 + fe
    };
    let mut second = format!(
        "{} · {}..{} · {} bp",
        f.kind,
        fmt_int(fs),
        fmt_int(fe),
        fmt_int(bp)
    );
    // The one place `(3n)` costs nothing, which is what review finding 12 is
    // really asking for: whether a CDS is a multiple of three is how an in-frame
    // fusion or a His-tag insertion is checked.
    //
    // From `feature_bp` and NOT from `bp` above, which is the extent and covers
    // the intron of a spliced CDS. The sequence view's hover line now prints
    // the same number, and two surfaces of one application must not give two
    // answers to "how long is this protein".
    if f.kind == "CDS" {
        if let Some(coding) = feature_bp(f, mol) {
            second.push_str(&if coding % 3 == 0 {
                format!(" ({} aa)", fmt_int(coding / 3))
            } else {
                format!(" · {} coding bp — NOT a multiple of 3", fmt_int(coding))
            });
        }
    }
    if f.segments.len() > 1 {
        second.push_str(&format!(" · {} segments", f.segments.len()));
    }
    second.push_str(&format!(" · {}", strand_word(f.strand)));
    format!(
        "{}\n{second}\nclick to select · double-click to edit",
        f.name
    )
}

/// How many bases a feature really covers, or `None` if it names none.
///
/// The sum of its segment lengths, with an origin-crossing segment counted the
/// way `Molecule::subseq` reads one. NOT `extent`, which is an outer bound and
/// covers the intron of a spliced CDS.
///
/// For a CDS the two facts that matter are this number and whether it is a
/// multiple of three -- that is how an in-frame fusion or a His-tag insertion
/// is checked, and it is exactly what this file is. Neither was available
/// anywhere in the application.
fn feature_bp(f: &pl_core::Feature, mol: &pl_core::Molecule) -> Option<u64> {
    let n = mol.len();
    let circular = mol.topology.is_circular();
    let mut total = 0u64;
    for s in &f.segments {
        if s.end < s.start {
            if !circular || s.start > n {
                continue;
            }
            total += n - (s.start - 1) + s.end.min(n);
        } else {
            total += s.end.min(n).saturating_sub(s.start.saturating_sub(1));
        }
    }
    (total > 0).then_some(total)
}

fn strand_word(s: Strand) -> &'static str {
    match s {
        Strand::Forward => "forward",
        Strand::Reverse => "reverse",
        Strand::Both => "both strands",
        Strand::Unoriented => "no strand",
    }
}

/// Everything a built gel depends on. Anything not named here cannot change
/// the picture, and anything that changes here rebuilds it.
///
/// The `f64` conditions are keyed by their BITS. The controls are range-limited
/// so no NaN can reach them, but a key comparing `f64` with `==` would rebuild
/// forever if one ever did, and `-0.0 == 0.0` would suppress a rebuild that a
/// hand-edited settings file could ask for.
#[derive(Clone, PartialEq, Eq)]
struct GelKey {
    doc: u64,
    seq: u64,
    picked: Vec<String>,
    arrangement: gel::Arrangement,
    ladder: &'static str,
    agarose: u64,
    run_mm: u64,
    band_mm: u64,
    inverted: bool,
    set: pl_enzymes::EnzymeSet,
    /// The methylation verdicts, which change what a lane may claim.
    blocked: Vec<Option<pl_enzymes::methylation::SiteEffect>>,
    /// The seeder's own disclosure line. It only ever changes in the same
    /// breath as `picked`, so it is here for completeness rather than because
    /// it is reachable on its own — and a key that is complete for a reason
    /// nobody has to remember is the kind that stays complete.
    seed_note: Option<String>,
}

impl GelKey {
    fn of(app: &App, verdicts: &[Option<pl_enzymes::methylation::SiteEffect>]) -> GelKey {
        let c = app.gel.conditions;
        GelKey {
            doc: app.doc_generation,
            seq: app.document().map_or(0, |d| d.seq_version),
            picked: app.gel.picked.iter().cloned().collect(),
            arrangement: app.gel.arrangement,
            ladder: app.gel.ladder,
            agarose: c.agarose_percent.to_bits(),
            run_mm: c.run_mm.to_bits(),
            band_mm: c.band_mm.to_bits(),
            inverted: app.gel.inverted,
            set: app.enzyme_set,
            blocked: verdicts.to_vec(),
            seed_note: app.gel.seed_note.clone(),
        }
    }
}

/// The gel's conditions, copied out of [`gel::View`] so the controls can be
/// drawn inside a closure that is already borrowing `self`.
#[derive(Clone, Copy, PartialEq)]
struct GelControls {
    arrangement: gel::Arrangement,
    ladder: &'static str,
    conditions: pl_gel::Conditions,
    inverted: bool,
}

impl GelControls {
    fn of(v: &gel::View) -> Self {
        GelControls {
            arrangement: v.arrangement,
            ladder: v.ladder,
            conditions: v.conditions,
            inverted: v.inverted,
        }
    }
    fn apply(self, v: &mut gel::View) {
        v.arrangement = self.arrangement;
        v.ladder = self.ladder;
        v.conditions = self.conditions;
        v.inverted = self.inverted;
    }
}

/// The gel's conditions. NOT its enzymes: those are ticked in the Enzymes tab,
/// which is the app's one enzyme control, and a second picker here would be a
/// second source of truth for the same question.
fn gel_controls(ui: &mut Ui, g: &mut GelControls) {
    ui.separator();
    for a in gel::Arrangement::ALL {
        if ui
            .selectable_label(g.arrangement == a, a.label())
            .on_hover_text(a.hover())
            .clicked()
        {
            g.arrangement = a;
        }
    }
    ui.separator();
    egui::ComboBox::from_id_salt("gel-ladder")
        .selected_text(g.ladder)
        .width(88.0)
        .show_ui(ui, |ui| {
            for l in pl_gel::LADDERS {
                ui.selectable_value(&mut g.ladder, l.name, l.name);
            }
        });
    ui.add(
        egui::DragValue::new(&mut g.conditions.agarose_percent)
            .speed(0.05)
            .range(0.3..=4.0)
            .suffix("% agarose"),
    );
    ui.add(
        egui::DragValue::new(&mut g.conditions.band_mm)
            .speed(0.1)
            .range(0.1..=20.0)
            .suffix(" mm band"),
    )
    // The CLI's own usage line for this number, because it is the dominant
    // uncertainty in the model and hiding it would be dishonest.
    .on_hover_text("This is what decides whether two fragments resolve.");
    if ui
        .selectable_label(g.inverted, "dark field")
        .on_hover_text("A stained gel as it photographs. The picture is still flat rectangles.")
        .clicked()
    {
        g.inverted = !g.inverted;
    }
}

/// The picture, and the strip beneath it that makes the picture citable.
///
/// The strip is ALWAYS present, not conditional on there being something to
/// warn about: a disclosure that is sometimes suppressed teaches the user that
/// its absence means "nothing to know". It also sits OUTSIDE the dark field,
/// because a bare dark rectangle with bands on it *is* a photograph, and a
/// photograph is evidence.
fn gel_pane(ui: &mut Ui, built: &gel::Built, methods: &mut bool, show_all: &mut bool) {
    // NAMES THE STATE AND THE CURE, like every other empty state in the app.
    // A lone ladder beside nothing reads as "this molecule has no sites", which
    // is the exact misreading the waiting state is written to avoid.
    if built.empty {
        ui.label(
            RichText::new(
                "Nothing is running on this gel — tick an enzyme on the Enzymes tab. \
                 The ladder below is the ruler, not a result.",
            )
            .size(12.0)
            .color(pal(ui).ink2),
        );
        ui.add_space(4.0);
    }
    let strip_h = (ui.available_height() * 0.34).clamp(96.0, 220.0);
    let pane = ui.available_rect_before_wrap();
    let picture = egui::Rect::from_min_size(
        pane.min,
        egui::vec2(pane.width(), (pane.height() - strip_h).max(40.0)),
    );
    // A FLOOR as well as a cap, and the floor is the load-bearing half. The
    // unplaced-fragment caption is 8.5 pt in scene units; below 0.85 it drops
    // under 7.2 pt, and a fragment the gel could not place stops being
    // readable — which is the whole point of drawing it. Rather than shrink
    // further the picture scrolls, because a picture that silently becomes
    // unreadable is worse than one that admits it needs more room.
    let raw = scene::fit_scale(&built.scene, picture.size() - egui::vec2(16.0, 16.0));
    let scale = raw.clamp(0.85, 2.0);
    let w = built.scene.width as f32 * scale;
    let h = built.scene.height as f32 * scale;
    // AND THE FLOOR SAYS SO WHEN IT BITES, in the direction it bit. It was
    // never asked what happens when scrolling cannot reach the lanes: a genome
    // gel came to 280,947 pt, the clamp held it at 0.85, and a 238,805 px
    // canvas in a 950 px pane put the first lane 4,283 px in with a sub-pixel
    // scrollbar thumb — a picture that is off-screen looked exactly like a
    // picture that is empty. The labels are capped now
    // (`pl_gel::MAX_LISTED`) so the extreme case is gone, and the ordinary one
    // is the recovery banner taking the room: naming the axis matters, because
    // "scroll right" on a picture that is too TALL is advice that does nothing.
    let lanes = built
        .scene
        .items
        .iter()
        .filter(|i| matches!(i, pl_draw::Item::Path { title: Some(t), .. } if t.ends_with(" well")))
        .count();
    if raw < 0.85 {
        let over_x = w > picture.width() - 16.0;
        let over_y = h > picture.height() - 8.0;
        let way = match (over_x, over_y) {
            (true, true) => "scroll around it",
            (true, false) => "scroll right",
            _ => "scroll down",
        };
        ui.label(
            RichText::new(format!(
                "The whole gel does not fit here at a readable size: {way}, or export it. \
                 {lanes} lane{}, {:.0} x {:.0} pt.",
                if lanes == 1 { "" } else { "s" },
                built.scene.width,
                built.scene.height
            ))
            .size(11.0)
            .color(pal(ui).warn),
        );
    }

    let mut skipped = 0usize;
    // BOTH AXES. It used to scroll horizontally only, so a picture taller than
    // the pane — which is what the recovery banner makes of an ordinary
    // seven-lane gel — was CLIPPED at the bottom, taking the disclosure strip
    // with it. A clipped picture and a scrolled one look the same until you
    // reach for the scrollbar that is not there.
    egui::ScrollArea::both()
        .id_salt("gel-scroll")
        .max_height(picture.height())
        .show(ui, |ui| {
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(w + 16.0, h + 8.0), egui::Sense::hover());
            let origin = egui::pos2(
                rect.min.x + (rect.width() - w).max(0.0) / 2.0,
                rect.min.y + 4.0,
            );
            let painted = scene::paint(ui.painter(), &built.scene, origin, scale);
            skipped = painted.skipped;
            if let Some(p) = resp.hover_pos() {
                if let Some(t) = painted.hover(p) {
                    let t = t.to_string();
                    resp.on_hover_text_at_pointer(t);
                }
            }
        });

    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("gel-disclosure")
        .max_height(strip_h)
        .show(ui, |ui| {
            for (i, line) in built.disclosure.iter().enumerate() {
                ui.label(
                    RichText::new(line)
                        .size(11.0)
                        // The calibration statement first and in the reading
                        // colour; the rest is supporting detail.
                        .color(if i == 0 { pal(ui).ink2 } else { pal(ui).muted }),
                );
            }
            // A DROPPED ITEM IS A HOLE, AND A SILENT HOLE IS THE BAD KIND.
            // `scene::paint` refuses a colour it cannot parse rather than
            // drawing it black — correctly, because black on a dark gel is
            // invisible — but until now the refusal reached nobody and the pane
            // simply looked emptier.
            if skipped > 0 {
                ui.label(
                    RichText::new(format!(
                        "{skipped} item(s) in this picture carry a colour the painter could \
                         not read and were not drawn. The exported file still has them; this \
                         is a bug in the application, not in your data."
                    ))
                    .size(11.0)
                    .color(pal(ui).warn),
                );
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("Methods…")
                    .on_hover_text(
                        "The citable paragraph: the model, the conditions actually used, \
                         and what it is not a basis for.",
                    )
                    .clicked()
                {
                    *methods = true;
                }
                if !built.suspended.is_empty() && ui.button("Show all").clicked() {
                    *show_all = true;
                }
            });
        });
}

/// One enzyme, with whatever qualifies the answer — and the ONE place an
/// enzyme is chosen for the gel.
///
/// `blocked` is the methylation verdict: `docs/PLAN.md` §7.1 requires such
/// sites be "struck through, not hidden". A site that will not cut is still a
/// site — it exists in the sequence, appears on everyone else's map, and cuts
/// the moment the plasmid goes through a dam- strain. Hiding it produces a map
/// that disagrees with every other tool for reasons the user cannot see.
///
/// `in_gel` is the whole gel picker. There is deliberately no second enzyme
/// control in the gel view: `App::enzyme_set` governs which enzymes can be
/// ticked, ticking governs which lane an enzyme is in, and the two answers
/// cannot disagree because there is only one of each. The map's own comment
/// ("ONE control, one answer") records what it cost the last time this project
/// had two. The two paragraphs belong on the same function for a reason the
/// review found the hard way: ticking a struck-through enzyme put its digest
/// on the gel as fact, one row giving two opposite answers. `gel::View` now
/// reads the same verdict this row draws.
#[allow(clippy::too_many_arguments)]
/// What one row says about the end its enzyme leaves.
///
/// Carried as text rather than as the enzyme, because the interesting half —
/// which OTHER enzymes leave an end this one can be ligated to — is a property
/// of the molecule on screen, not of the enzyme, and a row should not go looking
struct EndNote {
    /// `5' GATC`, `3' GTAC`, `blunt`, or `5' NNNN` when the sequence sets it.
    chip: String,
    hover: String,
}

/// What to say about the end `e` leaves, given the enzymes that cut this
/// molecule.
///
/// NARROWER THAN `pl ends` ON PURPOSE. The catalogue answer — every enzyme
/// anywhere that leaves `GATC` — is a reference lookup. In front of a plasmid
/// the question is "of the enzymes that cut THIS one, which are
/// interchangeable", because those are the alternatives its polylinker actually
/// offers. So the partner list is intersected with `cutters`.
///
/// A free function rather than a closure inside the tab, so it can be asserted
/// without standing up a document, a digest worker and a frame.
fn end_note(e: &pl_enzymes::Enzyme, cutters: &[&'static pl_enzymes::Enzyme]) -> EndNote {
    let side = if e.is_blunt() {
        "blunt".to_string()
    } else {
        let s = if e.is_five_prime_overhang() {
            "5'"
        } else {
            "3'"
        };
        match e.overhang_seq() {
            Some(o) => format!("{s} {o}"),
            None => format!("{s} {}", "N".repeat(e.overhang_len())),
        }
    };
    // A Type IIS end is not a fact about the enzyme, so it gets a sentence and
    // no partner list: "interchangeable with BsmBI" would be false in general.
    if e.overhang_seq().is_none() {
        return EndNote {
            chip: side,
            hover: format!(
                "{} cuts outside its own site, so the {} bases of the overhang come from the \
                 sequence and not from the enzyme. Two fragments join only where those bases \
                 match — which is what Golden Gate exploits.",
                e.name,
                e.overhang_len()
            ),
        };
    }
    let mates: Vec<&str> = cutters
        .iter()
        .filter(|o| o.name != e.name && e.ligates_with(o) == pl_enzymes::Compatibility::Always)
        .map(|o| o.name)
        .collect();
    let hover = if mates.is_empty() {
        format!(
            "Leaves {side}. Nothing else that cuts this molecule leaves an end it can be \
             ligated to."
        )
    } else {
        // The junction is the reason to prefer one partner over another: a seam
        // neither enzyme cuts cannot re-open.
        let seams: Vec<String> = mates
            .iter()
            .filter_map(|m| {
                let o = pl_enzymes::by_name(m)?;
                let j = e.junction(o)?;
                let recut = pl_enzymes::ENZYMES.iter().any(|x| {
                    !pl_enzymes::cut_positions(j.as_bytes(), pl_core::Topology::Linear, x)
                        .is_empty()
                });
                Some(format!(
                    "{}+{} = {j}{}",
                    e.name,
                    o.name,
                    if recut { "" } else { ", cut by neither" }
                ))
            })
            .collect();
        format!(
            "Leaves {side}, the same end as {}. A fragment cut with any of them can be ligated \
             here.\n{}",
            mates.join(", "),
            seams.join("\n")
        )
    };
    EndNote { chip: side, hover }
}

#[allow(clippy::too_many_arguments)]
fn enzyme_row(
    ui: &mut Ui,
    name: &str,
    site: &str,
    positions: &[u64],
    unique: bool,
    blocked: Option<pl_enzymes::methylation::SiteEffect>,
    poor_single_site: Option<&'static str>,
    end: &EndNote,
    in_gel: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.add(egui::Checkbox::without_text(in_gel))
            .on_hover_text("Run this enzyme on the gel.");
        let mut label = RichText::new(format!("{name:<9}"))
            .monospace()
            .size(11.5)
            .color(if unique { pal(ui).ink } else { pal(ui).ink2 });
        if blocked.is_some_and(|b| b.effect == pl_enzymes::methylation::Effect::Blocked) {
            label = label.strikethrough();
        }
        ui.label(label);
        ui.label(
            RichText::new(site)
                .monospace()
                .size(11.0)
                .color(pal(ui).muted),
        );
        if let Some(b) = blocked {
            ui.label(
                RichText::new(format!("{} {}", b.methylase.name(), b.effect.as_str()))
                    .size(10.5)
                    .color(pal(ui).warn),
            )
            .on_hover_text(
                "Methylation of this plasmid affects cleavage here. The site is real                  and is shown; it would cut in an unmethylated preparation.",
            );
        }
        if let Some(note) = poor_single_site {
            ui.label(RichText::new("1-site").size(10.5).color(pal(ui).warn))
                .on_hover_text(note);
        }
        // The end this cut leaves. Small and always present, because "which of
        // these can I swap for the one my polylinker actually has" is a question
        // you ask WHILE reading the list, and the answer used to live only in
        // `pl ends`.
        ui.label(
            RichText::new(&end.chip)
                .monospace()
                .size(10.5)
                .color(pal(ui).muted),
        )
        .on_hover_text(&end.hover);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let shown: Vec<String> = positions.iter().take(4).map(|p| fmt_int(*p)).collect();
            let more = if positions.len() > 4 { MORE_MARK } else { "" };
            ui.label(
                RichText::new(format!("{}{more}", shown.join(", ")))
                    .monospace()
                    .size(11.0)
                    .color(pal(ui).muted),
            );
        });
    });
}

/// The chromatogram palette for the field it will be painted on.
///
/// A named function rather than an `if` at the call site so the choice and the
/// background can be asserted against each other:
/// `the_chromatogram_is_legible_on_the_panel_it_is_painted_on` pairs this with
/// [`theme::panel_fill`], which is the only place the two facts meet.
fn trace_palette(dark: bool) -> pl_draw::trace::Palette {
    if dark {
        pl_draw::trace::Palette::AccessibleDark
    } else {
        pl_draw::trace::Palette::Accessible
    }
}

/// Enzymes that cleave poorly when the molecule has only one site.
///
/// Two of our fifty, verified against NEB and REBASE rather than taken from
/// `docs/PLAN.md` §7.1 — whose list of fifteen turns out to be the assay panel
/// from one 2006 paper rather than a catalogue, and disagrees with NEB in both
/// directions. Only shown when the digest actually returns a single site,
/// because that is the only case in which it changes what anyone should do.
fn poor_single_site_note(name: &str, sites: usize) -> Option<&'static str> {
    if sites != 1 {
        return None;
    }
    match name {
        "SacII" => Some(
            "Cleaves poorly at a single site. NEB flags SacII as requiring two or more              sites for optimal cleavage, and says the mechanism is not fully understood.              Do not add more enzyme — excess makes it worse; titrate down instead.",
        ),
        "XmaI" => Some(
            "Reported to behave as a multi-site enzyme (NEB groups it with its              equischizomer Cfr9I), so a single-site digest may be slow or incomplete.              NEB does not carry its multi-site icon for XmaI — a caution, not a warning.",
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hand_rolled_calendar_returns_a_sane_date() {
        // A wrong LOCUS date silently corrupts every file the app writes.
        let (d, m, y) = today();
        assert!((1..=31).contains(&d), "{d}");
        assert!(m < 12, "{m}");
        assert!(y >= 2026, "{y}");
    }

    /// PROVEN TO FAIL before `Palette::AccessibleDark`: the app opens in the
    /// dark theme and painted the G channel at `#000000` on `#161a1d`.
    ///
    /// 1.20:1. Not "hard to read" — the trace and the base letters under it
    /// were both invisible, so a four-channel chromatogram genuinely had three,
    /// and the panel's own comment that "the letters under the peaks are the
    /// second channel and are never turned off" failed in exactly the place the
    /// first channel did. `pl_draw::trace::to_scene` emits no background
    /// rectangle, so the caller's panel IS the field, and this is the only
    /// place in the codebase where the palette and that panel are both known.
    #[test]
    fn the_chromatogram_is_legible_on_the_panel_it_is_painted_on() {
        use pl_draw::contrast::{parse_hex, ratio};
        for dark in [true, false] {
            let p = trace_palette(dark);
            let bg = theme::panel_fill(dark);
            let bg = (bg.r(), bg.g(), bg.b());
            for base in b"ACGT" {
                let c = parse_hex(p.color(*base)).expect("a palette colour parses");
                // 3:1 is the graphical-object threshold this project applies to
                // Okabe-Ito — `pl_draw::contrast`'s own test names the three
                // members that fail it on white and settles on that standard.
                assert!(
                    ratio(c, bg) >= 3.0,
                    "{p:?} {} on the {} panel: {:.2}:1",
                    *base as char,
                    if dark { "dark" } else { "light" },
                    ratio(c, bg)
                );
            }
            for at_least in [true, false] {
                let c = parse_hex(p.quality(at_least)).expect("a palette colour parses");
                assert!(
                    ratio(c, bg) >= 3.0,
                    "{p:?} quality bar on the {} panel: {:.2}:1",
                    if dark { "dark" } else { "light" },
                    ratio(c, bg)
                );
            }
        }
        // THE CONTROL, so this cannot pass by asserting nothing: the palette
        // the app no longer uses on a dark panel is the one that fails there.
        let bg = theme::panel_fill(true);
        let g = parse_hex(pl_draw::trace::Palette::Accessible.color(b'G')).expect("hex");
        assert!(
            ratio(g, (bg.r(), bg.g(), bg.b())) < 1.5,
            "the old palette's G was invisible, and that is why this test exists"
        );
    }

    #[test]
    fn strand_glyphs_cover_every_variant() {
        for s in [
            Strand::Forward,
            Strand::Reverse,
            Strand::Both,
            Strand::Unoriented,
        ] {
            assert!(!strand_glyph(s).is_empty());
            // In words as well, and ASCII. The original reason was that the
            // hover readout is set in the proportional face and Ubuntu Light had
            // no U+2190, so the arrow rendered as an empty box; IBM Plex Sans has
            // it, and the words stay anyway — see `strand_word`. Non-empty was
            // never enough to ask.
            let w = strand_word(s);
            assert!(!w.is_empty());
            assert!(w.is_ascii(), "{w} needs a font we cannot count on");
        }
    }

    // -----------------------------------------------------------------------
    // typography: the faces have to be able to draw what we hand them
    // -----------------------------------------------------------------------

    /// Whether `c` set in `family` at `size` really draws as a tofu box.
    ///
    /// **`Fonts::has_glyphs` is not this question and must not be used for it.**
    /// epaint 0.35 implements it as `resolve_face(c) != replacement_face_key`
    /// (`font.rs:722`), with its own `TODO` beside it admitting a false negative —
    /// and the false negative is not an edge case here, it is the whole primary
    /// face. `CachedFamily::new` picks the replacement face by asking which face in
    /// the chain first has U+25FB `◻`; for Monospace that is **Hack**, so
    /// `has_glyphs` answers *false* for every character Hack owns. Measured through
    /// it: U+2192, U+2190, U+00B7, U+2026 and U+2014 all report "missing" from the
    /// monospace face that in fact draws all five, while U+26A0 — which Hack really
    /// does lack — reports present. It is inverted for exactly the family ITEM 2
    /// changes.
    ///
    /// It fails the other way too. In Proportional the replacement face comes out
    /// NotoEmoji, so `has_glyphs` calls U+26A0 missing when NotoEmoji draws it, and
    /// U+26A0 is the warning banner's marker. A gate built on it raises false alarms
    /// and, worse, misses.
    ///
    /// So ask the ATLAS instead, through the real layout path. A character no face
    /// supports is rendered as `replacement_char` — `Font::glyph_info` falls back to
    /// it explicitly — and the substitute therefore lands on the same rasterised
    /// glyph in the font texture. Comparing `uv_rect` against `◻`'s own is exact,
    /// needs no font table parsing, and follows the fallback chain wherever it goes.
    /// Verified against the four embedded faces' `cmap`s with fontTools: this agrees
    /// with them on all twelve characters tried, where `has_glyphs` disagrees on six.
    fn renders_as_tofu(ctx: &egui::Context, family: egui::FontFamily, size: f32, c: char) -> bool {
        let uv = |s: &str| {
            let job = egui::text::LayoutJob::simple_singleline(
                s.to_string(),
                egui::FontId::new(size, family.clone()),
                egui::Color32::WHITE,
            );
            let g = ctx.fonts_mut(|f| f.layout_job(job));
            let r = g.rows[0].glyphs[0].uv_rect;
            // The atlas rect as plain numbers, so this needs no path to `UvRect`
            // (epaint 0.35 does not re-export it) and reads as what it is.
            (r.min, r.max, r.offset, r.size)
        };
        // U+25FB is `CachedFamily::new`'s `PRIMARY_REPLACEMENT_CHAR`. Nothing in this
        // app draws it, so a match can only mean substitution.
        uv(&c.to_string()) == uv("\u{25FB}")
    }

    #[test]
    fn the_tofu_oracle_answers_both_ways_before_anything_relies_on_it() {
        let ctx = test_ctx();
        // A CJK ideograph is in none of the four embedded faces, in either family.
        for fam in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
            assert!(
                renders_as_tofu(&ctx, fam.clone(), 11.0, '\u{4E2D}'),
                "{fam:?}: U+4E2D is in no embedded face and must read as tofu"
            );
            assert!(
                !renders_as_tofu(&ctx, fam.clone(), 11.0, 'A'),
                "{fam:?}: 'A' reads as tofu, so the oracle is stuck on yes"
            );
        }
        // And the two families genuinely differ, which is the reason the monospace
        // gate cannot be inferred from the proportional one.
        //
        // THIS ASKED U+2192 UNTIL THE PLEX SWAP AND HAD TO CHANGE, which is worth
        // recording because the reason is not that the test was wrong. U+2192 was
        // in Hack and in nothing else `default_fonts` embeds, and Hack is not in
        // the proportional chain — so the arrow was the cheapest character that
        // separated the two families. IBM Plex Sans HAS U+2192 (checked in its own
        // cmap), so prepending it to the proportional chain made the old
        // assertion false while changing nothing about the property being tested.
        // U+25B8 replaces it: absent from BOTH Plex faces and from Ubuntu-Light,
        // present in Hack, and Hack is still monospace-only. Measured from the
        // four cmaps rather than assumed, because picking a character that turned
        // out to be in Plex Sans as well would have left this passing for the
        // wrong reason and taken the oracle's calibration with it.
        assert!(!renders_as_tofu(
            &ctx,
            egui::FontFamily::Monospace,
            11.0,
            '\u{25B8}'
        ));
        assert!(renders_as_tofu(
            &ctx,
            egui::FontFamily::Proportional,
            11.0,
            '\u{25B8}'
        ));
    }

    /// The band of monospace advances that leaves `DEF_PANEL`, `MIN_PANEL` and
    /// every per-row expectation exactly where 0ebaa41 calibrated them.
    ///
    /// COMPILE-ONLY at 0ebaa41: nothing here existed. It asserts no new
    /// behaviour, and that is deliberate — it is the instrument the font swap
    /// needs and could not have, because "does this face still reach sixty at the
    /// 500 pt default" was previously answerable only by installing the face and
    /// looking. Stated as a band, it is answerable from the face's `hmtx`.
    ///
    /// The model: `per_row = fit_per_row(P - C - gutter_w(n, 11.0 * ratio),
    /// 11.5 * ratio)`, where `C` is the chrome, the scrollbar and the gutter's
    /// own air -- everything with no font in it. `C` is not written down as a
    /// literal; it is MEASURED from the real painter below and then asserted to
    /// reproduce all three of
    /// `the_default_split_reaches_sixty_and_takes_no_more_than_it_needs`'s cases.
    /// A hard-coded C is a number that rots the first time egui changes a margin;
    /// a measured one cannot.
    ///
    /// What the band bought, and this is now history rather than a forecast: it
    /// predicted that IBM Plex Mono at 0.600 em would reach sixty at `DEF_PANEL`
    /// with no constant moved, and the swap then moved none. Fira Code is 0.615385
    /// and breaks `DEF_PANEL - 12`; Iosevka is 0.500 and breaks `DEF_PANEL - 40` in
    /// the other direction. Whoever picks a face outside the band must move
    /// `DEF_PANEL` -- not edit the expectation, because the "and it is not padded"
    /// half of that test is what stops the details panel quietly eating the map
    /// pane.
    ///
    /// **THE BAND IS HORIZONTAL AND SEES NOTHING VERTICAL, which the swap proved
    /// the hard way.** Plex Mono's line box is 1.300 em against Hack's 1.164, so at
    /// the map's 10 pt label size the drawn height went from 11.64 pt to exactly
    /// 13.00 — into `map.rs`'s then-pinned `LINE_H = 13.0`, leaving a column of
    /// stacked enzyme labels with zero gap and `Rect::intersects` counting touching
    /// as overlapping. A face can sit in the middle of this band and still collide
    /// its own labels. `map::line_h` is the answer and derives the pitch from the
    /// face; do not read a green band here as clearance.
    #[test]
    fn the_advance_band_that_keeps_every_per_row_expectation() {
        const LEN: u64 = 8_117; // `seq_app`'s molecule, so a 5-character gutter
        let per_row = |p: f32, ratio: f32, c: f32| -> u64 {
            let g = seqedit::gutter_w(LEN, 11.0 * ratio);
            seqedit::fit_per_row(p - c - g, 11.5 * ratio)
        };

        // --- measure C, and the face we have, from the real painter -----------
        let ctx = test_ctx();
        let mono = ctx.fonts_mut(|f| f.glyph_width(&egui::FontId::monospace(11.5), 'A')) / 11.5;
        // IBM Plex Mono 2.005 is 600/1000, a terminating rational and not a
        // rounding: read from the vendored file's own `hmtx`, where all 95
        // printable ASCII codepoints share one advance of 600 units over a
        // unitsPerEm of 1000. If this moves, the numbers in the doc above are
        // about a different font than the one in the binary.
        //
        // The tolerance is NOT widened when the number changes. This assertion
        // exists to break on a face swap, and 1e-4 is what makes it able to: at
        // 1e-2 it would have accepted Hack's 0.602051 and Plex's 0.600000 alike
        // and told the next reader nothing.
        assert!(
            (mono - 0.600_000).abs() < 1e-4,
            "the monospace advance ratio is {mono}, not IBM Plex Mono's 0.600000"
        );

        // The smallest panel that reaches sixty, from the painter itself.
        // Bisected rather than stepped: `fit_per_row` is monotonic in the width,
        // and a 0.5 pt linear sweep cost this test 27 seconds of a suite that
        // people have to be willing to run.
        let reaches = |p: f32| -> bool {
            let ctx = test_ctx();
            let mut app = seq_app();
            app.layout.panel_w = Some(p);
            paint(&mut app, &ctx, window());
            app.edit.per_row() >= 60
        };
        assert!(reaches(700.0), "sixty is unreachable below a 700 pt panel");
        let (mut lo_p, mut hi_p) = (App::MIN_PANEL, 700.0f32);
        while hi_p - lo_p > 0.25 {
            let mid = 0.5 * (lo_p + hi_p);
            if reaches(mid) {
                hi_p = mid;
            } else {
                lo_p = mid;
            }
        }
        let sixty = hi_p;
        let c = sixty - 60.0 * (11.5 * mono) - seqedit::gutter_w(LEN, 11.0 * mono);
        // A quarter of a point of quantisation from the 0.5 pt search, no more.
        assert!(
            (0.0..64.0).contains(&c),
            "the font-independent chrome came out {c} pt, which is not a chrome"
        );

        // The model has to reproduce the painter before it is used to predict.
        for (panel, want) in [
            (App::DEF_PANEL, 60u64),
            (App::DEF_PANEL - 12.0, 60),
            (App::DEF_PANEL - 40.0, 50),
        ] {
            assert_eq!(
                per_row(panel, mono, c),
                want,
                "the model disagrees with the painter at {panel} pt for the face in \
                 the binary; C = {c}"
            );
        }

        // --- the band ---------------------------------------------------------
        let ok = |ratio: f32| -> bool {
            per_row(App::DEF_PANEL, ratio, c) == 60
                && per_row(App::DEF_PANEL - 12.0, ratio, c) == 60
                && per_row(App::DEF_PANEL - 40.0, ratio, c) == 50
                && per_row(App::MIN_PANEL, ratio, c) >= 10
        };
        const STEP: f32 = 0.000_25;
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        let mut r = 0.40f32;
        while r <= 0.80 {
            if ok(r) {
                lo = lo.min(r);
                hi = hi.max(r);
            }
            r += STEP;
        }
        assert!(
            lo < hi,
            "no advance ratio satisfies the expectations at all"
        );
        // The interval is CLOSED, and saying so is the difference between a face at
        // the edge being reported inside or outside on no evidence: `lo` and `hi` are
        // both ratios `ok` returned true for.
        assert!(ok(lo) && ok(hi), "the endpoints are inside the band");
        assert!(
            !ok(lo - STEP) && !ok(hi + STEP),
            "one step outside either end must fail, or the search did not find an edge"
        );
        // Printed, not pinned: the numbers belong in a commit message and in
        // whatever the next person measures a candidate face against, and pinning
        // them would make this test fail for the healthy reason that `DEF_PANEL`
        // moved. Visible under `cargo test -- --nocapture`.
        //
        // **CLOSED, and stated to the resolution it was found at.** `lo` is a `min`
        // and `hi` a `max` over ratios where `ok(r)` is TRUE, so both endpoints
        // satisfy the expectations — writing it `(lo, hi]` said the opposite about
        // the low end and excluded a ratio that passes. And the sweep steps by
        // 0.00025, so six decimals claim about a thousand times the precision the
        // search has: the true upper edge is somewhere in `[hi, hi + STEP)`. Both
        // were reproduced verbatim in the report that came with this test and then
        // reasoned from, which is how a soft boundary becomes a hard number.
        eprintln!(
            "advance band: [{lo:.5}, {hi:.5}] em, resolved to +/-{STEP}  |  \
             sixty at {sixty:.2} pt  |  chrome C = {c:.2} pt  |  \
             face in the binary = {mono:.6} em"
        );
        // Measured with fontTools on this machine, except where marked.
        for (name, ratio, inside) in [
            // Read from the file committed at bins/pl-gui/fonts, not from a table:
            // all 95 printable ASCII codepoints are 600/1000. It sits 0.00400 em
            // below the upper edge where Hack sat 0.00195 below it, so the swap
            // moved the shipped face FURTHER from the edge that loses sixty.
            ("IBM Plex Mono 2.005 (shipped)", 0.600f32, true),
            // Still in the binary, one place down the Monospace chain, and still
            // the supplier of U+25B6. Kept in this table because a fallback that
            // served a base would silently change the grid pitch.
            ("Hack 3.003 (fallback)", 0.602_051, true),
            ("JetBrains Mono", 0.600, true),
            ("Cascadia Mono 2404.023", 0.585_938, true),
            ("DejaVu Sans Mono", 0.602, true),
            ("Fira Code 6.002", 0.615_385, false),
            ("Consolas 7.01", 0.549_805, false),
            ("Iosevka", 0.500, false),
        ] {
            assert_eq!(
                ok(ratio),
                inside,
                "{name} at {ratio} em: the band is [{lo:.5}, {hi:.5}]; \
                 60/60/50 at {}/{}/{} pt gave {}/{}/{}",
                App::DEF_PANEL,
                App::DEF_PANEL - 12.0,
                App::DEF_PANEL - 40.0,
                per_row(App::DEF_PANEL, ratio, c),
                per_row(App::DEF_PANEL - 12.0, ratio, c),
                per_row(App::DEF_PANEL - 40.0, ratio, c),
            );
        }
    }

    /// PROVEN TO FAIL at 0ebaa41 on both refusals: they contained U+25B8 `▸`.
    ///
    /// Both are drawn PROPORTIONALLY — `sequence_tab` sets them with
    /// `RichText::new(msg).size(11.0)` and no `.monospace()` — and U+25B8 is
    /// present in Hack and in NOTHING else compiled into this binary. Measured
    /// with fontTools over the four faces `eframe`'s `default_fonts` embeds:
    ///
    ///   U+25B8  Ubuntu-Light MISSING · Hack yes · NotoEmoji MISSING · emoji-icon MISSING
    ///
    /// egui's Proportional fallback chain is Ubuntu-Light, NotoEmoji,
    /// emoji-icon-font — no Hack — so the app drew a tofu box in the middle of a
    /// sentence explaining a refusal. Exactly the trap `strand_word` was written
    /// for, one family up, and `strand_glyphs_cover_every_variant` above is its
    /// sibling.
    ///
    /// Asked of the FACE and not of an allow-list of characters, through
    /// [`renders_as_tofu`], so this keeps holding when the chain changes — which is
    /// the whole point of asking it in a typography pass that intends to change the
    /// chain. It used to ask `Fonts::has_glyphs`, which happened to give the right
    /// answer for these two strings and is not the question; see
    /// [`renders_as_tofu`] for the six characters it gets wrong.
    #[test]
    fn the_caret_refusals_use_only_glyphs_the_proportional_face_has() {
        use seqedit::SeqEdit;
        const NOW: f64 = 100.0;
        let mut before = doc::Document::of_molecule(pl_core::Molecule {
            seq: b"ACGTACGTACGT".to_vec(),
            topology: pl_core::Topology::Circular,
            ..Default::default()
        });
        let mut e = SeqEdit::new();
        e.caret = 0;
        e.backspace(&mut before, NOW);
        let at_start = e.notice.clone().expect("a refusal at base 1");

        let mut after = doc::Document::of_molecule(pl_core::Molecule {
            seq: b"ACGTACGTACGT".to_vec(),
            topology: pl_core::Topology::Circular,
            ..Default::default()
        });
        let mut e = SeqEdit::new();
        e.caret = 12;
        e.delete_forward(&mut after, NOW);
        let at_end = e.notice.clone().expect("a refusal past the last base");

        let ctx = test_ctx();
        for msg in [&at_start, &at_end] {
            // The path a user is sent down has to be the one that is there.
            assert!(
                msg.contains(&set_origin_path()),
                "{msg:?} does not name {:?}",
                set_origin_path()
            );
            for c in msg.chars() {
                assert!(
                    !renders_as_tofu(&ctx, egui::FontFamily::Proportional, 11.0, c),
                    "U+{:04X} {c:?} has no glyph in the face that draws this message, so it \
                     renders as a tofu box: {msg:?}",
                    c as u32
                );
            }
        }
    }

    /// The mirror of the refusal test above, for the family a monospace swap
    /// actually replaces.
    ///
    /// COMPILE-ONLY at 0ebaa41 (`HISTORY_HERE` and `MORE_MARK` did not exist), and
    /// it asserts nothing new about today's binary — Hack has all six glyphs. It is
    /// here because the gate it completes had a hole exactly where the work was
    /// pointed: every glyph-coverage assertion in the workspace asked
    /// `FontId::proportional`, inside
    /// `the_caret_refusals_use_only_glyphs_the_proportional_face_has` — while
    /// ITEM 2's subject is the MONOSPACE face, and six non-ASCII characters are set
    /// in it.
    ///
    /// Measured against the four embedded faces' `cmap`s with fontTools, and the
    /// first two lines are why this is not a formality:
    ///
    ///   U+2192 `→`  Plex Mono yes · Plex Sans yes · Hack yes · the rest MISSING
    ///   U+2190 `←`  Plex Mono yes · Plex Sans yes · Hack yes · the rest MISSING
    ///   U+2194 `↔`  Plex Mono yes · Plex Sans yes · Hack yes · NotoEmoji yes
    ///   U+25B6 `▶`  Plex Mono MISSING · Plex Sans MISSING · Hack yes · both emoji fonts yes
    ///
    /// Re-measured after the swap, from the two vendored files' own `cmap`s. The
    /// arrows gained two suppliers and are no longer a single point of failure.
    /// **U+25B6 lost its primary one:** `HISTORY_HERE`, the History tab's cursor on
    /// the current state, is now served by Hack as a FALLBACK, one place down the
    /// Monospace chain. That is fine and it is not invisible — the test below asks
    /// the chain, so it stays green — but it means the Ubuntu Font Licence decision
    /// that kept `default_fonts` is what keeps this glyph on screen. Drop
    /// `default_fonts` and the History tab draws a box.
    ///
    /// The forward and reverse arrows used to have exactly ONE supplier in the
    /// binary, and it was the face that got swapped. They are `strand_glyph`'s two commonest values,
    /// drawn `.monospace()` in the Features panel's coordinate column, and that
    /// column is where a reverse feature's direction is stated without colour. Swap
    /// to a face without them and the panel fills with tofu boxes — which is not
    /// hypothetical in this codebase: it is the U+25B8 defect the pass just fixed one
    /// family up, and [`renders_as_tofu`] confirms U+25B8 still reads as tofu in the
    /// proportional face today.
    ///
    /// `strand_glyphs_cover_every_variant` above cannot see this. It asserts the
    /// strings are non-empty and that the WORD is ASCII, and its own comment says a
    /// tofu box is a non-empty string.
    ///
    /// `HISTORY_HERE` and `MORE_MARK` are constants rather than characters written
    /// out here for the reason `set_origin_path` is one: a list of glyphs restated
    /// in a test drifts from the label that draws them, silently, and the drift is
    /// invisible until someone looks at the running app.
    #[test]
    fn the_monospace_face_has_every_glyph_the_app_sets_in_it() {
        let ctx = test_ctx();
        // Every character the app hands the monospace family, with where from.
        let mut want: Vec<(String, &str)> = vec![
            (
                HISTORY_HERE.to_string(),
                "the History tab's cursor on the current state",
            ),
            (
                MORE_MARK.to_string(),
                "enzyme_row's 'more coordinates than shown'",
            ),
        ];
        for s in [
            Strand::Forward,
            Strand::Reverse,
            Strand::Both,
            Strand::Unoriented,
        ] {
            want.push((
                strand_glyph(s).to_string(),
                "strand_glyph, the Features panel's coordinate column",
            ));
        }
        // And the printable ASCII the grid, the gutter, the two rulers, the enzyme
        // names, the sites and `op.id.short()` are all drawn from. `row_text` can
        // emit any of 0x21..=0x7E, so the whole range and not a hand-picked subset.
        for b in 0x21u8..=0x7E {
            want.push(((b as char).to_string(), "printable ASCII, via row_text"));
        }
        // The sizes the app really asks for: 9.0 is the map's ruler, 9.5 the
        // sequence view's, 10.0 every enzyme site label on the map
        // (`map::label_font`), 11.0 and 11.5 everything else.
        //
        // 10.0 WAS MISSING FROM THIS LIST while the comment above it claimed the
        // list was what the app asks for. Nothing was broken by the omission —
        // coverage does not vary with size — but the sibling test
        // `the_sequence_grid_has_one_advance_at_every_size_it_is_drawn` does include
        // 10.0, so the two lists disagreed about the same application, and the one
        // that was wrong was also the one that stated it in prose.
        for size in [9.0f32, 9.5, 10.0, 11.0, 11.5] {
            for (s, why) in &want {
                for c in s.chars() {
                    assert!(
                        !renders_as_tofu(&ctx, egui::FontFamily::Monospace, size, c),
                        "at {size} pt U+{:04X} {c:?} has no glyph in the MONOSPACE face, so \
                         {why} renders as a tofu box",
                        c as u32
                    );
                }
            }
        }
    }

    /// THE LIGATURE GATE. A row of bases must SHAPE to one glyph per base, all on
    /// one pitch.
    ///
    /// This is the check the pass was asked for and did not have. Its sibling below
    /// measures `Fonts::glyph_width`, which reads one character's `hmtx` advance
    /// (`fonts.rs:851` -> `FontFace::advance_width_unscaled` -> skrifa) and never
    /// shapes anything — so it cannot detect a ligature, and a standalone probe
    /// confirmed that it PASSES for Fira Code 6.002, the face the advance band's own
    /// table asserts must be rejected. A candidate at 0.600 em that collapses
    /// clusters would walk through the whole gate.
    ///
    /// The row is painted as ONE shaped run: `row_text` builds all sixty characters
    /// and `main.rs` draws them in a single `painter.text` call, which goes
    /// `layout_no_wrap` -> `layout_job` -> `shape_text` -> `shaper.shape(buffer, &[])`
    /// — an EMPTY user-feature list, so harfrust's defaults govern and `liga`,
    /// `clig`, `calt`, `rlig`, `rclt` and `kern` are all on. There is no knob to turn
    /// them off: `TextOptions`, `FontTweak` and `TextFormat` carry no feature list
    /// (`coords` is variable-font axes, not GSUB), so the only route is a face that
    /// does not ligate. Verified at source in epaint 0.35. `layout_job` here is the
    /// same entry point the painter uses, so this asks the shaper the app's own
    /// question.
    ///
    /// Four properties, because a ligature can break the grid three different ways
    /// and one of them leaves the count intact:
    ///
    ///  1. one glyph per character — a shaper that drops a cluster shortens the list;
    ///  2. each glyph is still the character it came from, in order;
    ///  3. glyph `k` sits at `x0 + k * advance` — the mapping `seqedit` rests on;
    ///  4. the whole row is `n * advance` wide — the direct form of "two glyphs
    ///     collapsed into one advance", which is invisible to 1 and 2 because
    ///     `emit_continuation_glyphs` appends zero-advance glyphs to keep
    ///     `glyphs.len() == char_count`.
    ///
    /// PASSES at 0ebaa41, and this says so rather than dressing it up: Hack-Regular
    /// 3.003 cannot shape — one non-zero advance in the whole font, no GPOS, no
    /// legacy `kern`, no GSUB lookup reachable from the default-on features. Worth
    /// recording that the brief's premise is measurably too strong for the actual
    /// candidates: Fira Code's and Cascadia Code's ligatures are advance-PRESERVING,
    /// and a 60-character row of `--`, `**`, `..`, `->`, `=>`, `<=`, `!=`, `::`,
    /// `//`, `<>` and `++` drifts at most 0.87 pt in either — the same pixel-snapping
    /// sawtooth Hack shows. The hole in the gate is real; the danger from today's
    /// shortlist is smaller than either document claimed.
    ///
    /// The fixture is a real `Molecule` through the real `row_text` so the alphabet
    /// is what the app can actually paint: `row_text` pushes `b as char` for every
    /// `is_ascii_graphic` byte, which is all 94 printable ASCII, and `?` otherwise.
    ///
    /// AND IT IS RUN IN BOTH CASES, which it was not until this was reviewed. The
    /// row below is uppercase, `Molecule::seq` is case-preserved, and the user's own
    /// pKoV renders entirely lowercase on screen — so the gate was looking at an
    /// alphabet the application mostly does not draw. Measured: the committed IBM
    /// Plex Mono with one LigatureSubst rule `a + c -> A` appended and `hmtx` left
    /// alone passes this test with the uppercase row and fails it with the lowercase
    /// one. Half the ligature-bait alphabet was unwatched.
    ///
    /// **THIS IS THE LOAD-BEARING GUARD ON THE PHOSPHOR INSTALL, AND IT IS
    /// BEHAVIOURAL RATHER THAN STRUCTURAL, WHICH IS WHY IT IS THE ONE TO TRUST.**
    /// It never mentions a font name or a `font_data` key: it measures the row the
    /// painter would draw. So it reddens whichever way the icon face reaches the
    /// grid — prepended to `families[Monospace]`, appended under a second key,
    /// smuggled in by `egui_phosphor::add_to_fonts`, or brought in by a future
    /// face swap that ligates. Demonstrated rather than argued: with Phosphor Bold
    /// inserted at Monospace index 0 this test fails property 3 at glyph 8 with
    /// `('-') shaped to x=40.000, -5.600 pt off the 5.7000 pt grid — -0.98 cells`.
    ///
    /// AND IT IS THE ONLY ONE THAT CATCHES THE BYTES CHANGING UNDER A NAME. Its
    /// structural sibling,
    /// `the_icon_face_is_in_its_own_family_and_in_neither_text_chain`, asserts the
    /// spelling of both chains; register the icon face under the existing
    /// `"IBMPlexMono"` key and that test and the byte-level one both stay green
    /// while this one goes red. Measured, not argued. The converse also holds and
    /// is why the sibling is kept: append Phosphor to the END of the Monospace
    /// chain and this test stays green, because nothing in a sequence row ever
    /// resolves that far down.
    ///
    /// It says nothing about the grid's CHOICE of family, and that gap is real:
    /// every assertion here asks about `FontFamily::Monospace`, and if the sequence
    /// view were ever moved to a named family of its own the greenness below would
    /// be a fact about a family nothing paints.
    #[test]
    fn a_sequence_row_shapes_to_one_glyph_per_base_on_one_pitch() {
        // Ligature-prone pairs that survive `is_ascii_graphic`, padded with bases.
        //
        // THE BASE RUN AT THE FRONT IS `ATCATAG` AND IT IS NOT ARBITRARY. The
        // fixture used to open `ACGT--` and repeat, and scanning it there was no
        // `a` immediately followed by a `t` anywhere in sixty cells — so it caught
        // a zero-advance face by property 3 and could not have caught a LIGATING
        // one at all. `at`, `cat` and `tag` are the three rules Phosphor Regular
        // spells from the DNA alphabet, and `ATCATAG` lowercases to a string
        // holding all three. The count is preserved exactly: 7 + 2 for the first
        // run, eight runs of `ACGT` + pair, then `A<>` — sixty, with all ten pairs
        // still present.
        let mol = pl_core::Molecule {
            seq: b"ATCATAG--ACGT**ACGT..ACGT->ACGT=>ACGT<=ACGT!=ACGT::ACGT//A<>".to_vec(),
            topology: pl_core::Topology::Circular,
            ..Default::default()
        };
        let e = seqedit::SeqEdit::new();
        let mut row = String::new();
        e.row_text(&mol, 0, 60, &mut row);
        assert_eq!(row.chars().count(), 60, "sixty cells: {row:?}");
        for pair in ["--", "**", "..", "->", "=>", "<=", "!=", "::", "//", "<>"] {
            assert!(
                row.contains(pair),
                "{pair:?} did not survive row_text, so this proves nothing about \
                 ligatures: {row:?}"
            );
        }
        // The same sixty cells with the bases in the case the app actually shows
        // them in. `to_ascii_lowercase` leaves every punctuation pair above alone,
        // so this is the same fixture with one variable changed.
        let lower = row.to_ascii_lowercase();
        assert_ne!(
            lower, row,
            "the lowercase twin is identical to the row, so it is not testing a \
             second alphabet -- the fixture must contain letters"
        );
        // And the LOWERCASE twin is what has to carry the ligature bait, because a
        // ligature rule is spelled in lowercase and `Molecule::seq` is
        // case-preserved — the user's own pKoV renders entirely lowercase. Checked
        // against the string `row_text` actually emitted rather than against the
        // literal above, for the reason the pair loop already exists: a future
        // change to `row_text` could stop emitting these and the test would go on
        // claiming to watch them.
        for bait in ["at", "cat", "tag"] {
            assert!(
                lower.contains(bait),
                "{bait:?} is not in the lowercase row, and it is a Phosphor ligature \
                 name — without it this test watches the zero-advance failure only \
                 and not the ligature one: {lower:?}"
            );
        }

        let ctx = test_ctx();
        // One device pixel. `epaint` snaps every glyph's x to a whole device pixel
        // (`text_layout.rs`: `glyph.pos.x = round_to_pixel(glyph.pos.x)`), so a
        // correct face still shows a bounded sawtooth of about 0.87 pt at this
        // metric. A collapsed cluster is out by a whole advance, seven times this.
        let tol = 1.01 / ctx.pixels_per_point();

        // All four properties as one function returning WHICH one broke, so the same
        // code can be pointed at a row that must pass and a row that must fail.
        //
        // THE REFERENCE ADVANCE IS ALWAYS MONOSPACE'S, WHATEVER FAMILY THE ROW IS
        // LAID OUT IN, and that is what stops the icon-family case below from
        // passing vacuously. `advance` models the number the GRID computes, and
        // `main.rs:2877` computes it from `glyph_width(.., 'A')` — uppercase A,
        // which is absent from Phosphor's cmap in all five variants. So in a build
        // where Phosphor had reached the Monospace chain, the arithmetic would
        // still read Plex Mono's 6.90 while the painted lowercase row collapsed;
        // that is precisely the failure, and pairing a family's own broken advance
        // with its own broken layout would hide it. Asking the icons family for
        // `glyph_width('A')` returns 0.00 — with a zero advance every glyph sits
        // at `x0 + k*0`, properties 3 and 4 hold trivially, and the check could
        // never fail.
        let check = |text: &str, size: f32, family: egui::FontFamily| -> Result<(), String> {
            let chars: Vec<char> = text.chars().collect();
            let advance = ctx.fonts_mut(|f| f.glyph_width(&egui::FontId::monospace(size), 'A'));
            let job = egui::text::LayoutJob::simple_singleline(
                text.to_string(),
                egui::FontId::new(size, family),
                egui::Color32::WHITE,
            );
            let g = ctx.fonts_mut(|f| f.layout_job(job));
            if g.rows.len() != 1 {
                return Err(format!("the row wrapped into {} rows", g.rows.len()));
            }
            let glyphs = &g.rows[0].glyphs;
            // 1 — one glyph per character. A shaper that drops a cluster shortens
            // the list, and every column past it names the wrong base.
            if glyphs.len() != chars.len() {
                return Err(format!(
                    "the shaper returned {} glyphs for {} characters",
                    glyphs.len(),
                    chars.len()
                ));
            }
            for (k, (gl, c)) in glyphs.iter().zip(&chars).enumerate() {
                // 2 — and it is still the character it came from, in order.
                if gl.chr != *c {
                    return Err(format!(
                        "glyph {k} carries {:?} where the row has {c:?}",
                        gl.chr
                    ));
                }
                // 3 — on the grid `seqedit` computes every x from.
                let want = glyphs[0].pos.x + k as f32 * advance;
                if (gl.pos.x - want).abs() > tol {
                    return Err(format!(
                        "glyph {k} ({c:?}) shaped to x={:.3}, {:.3} pt off the {advance:.4} pt \
                         grid — {:.2} cells; a click in that column lands on the wrong base",
                        gl.pos.x,
                        gl.pos.x - want,
                        (gl.pos.x - want) / advance
                    ));
                }
            }
            // 4 — and the row is as wide as its cells. This is the failure the first
            // three can miss: `emit_continuation_glyphs` appends ZERO-ADVANCE glyphs
            // to keep `glyphs.len() == char_count`, so a collapsed cluster keeps the
            // count and the characters and only shortens the row.
            let want_w = chars.len() as f32 * advance;
            if (g.size().x - want_w).abs() > tol + advance * 0.5 {
                return Err(format!(
                    "the row shaped {:.2} pt wide against {} cells of {advance:.4} = \
                     {want_w:.2}, a difference of {:.2} cells",
                    g.size().x,
                    chars.len(),
                    (g.size().x - want_w) / advance
                ));
            }
            Ok(())
        };

        for (case, text) in [("upper", &row), ("lower", &lower)] {
            for size in [9.5f32, 11.0, 11.5] {
                check(text, size, egui::FontFamily::Monospace).unwrap_or_else(|e| {
                    panic!(
                        "at {size} pt the {case}case sequence row does not shape to a \
                         grid: {e}"
                    )
                });
            }
        }

        // AND THE SAME ROW IN THE ICON FAMILY, WITH THE POLARITY INVERTED. This is
        // the half that makes the half above mean something. Green in Monospace is
        // a fact about the CHAIN only if the face kept out of that chain is
        // genuinely capable of destroying it; without this, somebody could quietly
        // swap in an icon face with no `liga` and no zero advances, the isolation
        // would stop mattering, and nothing would say so.
        //
        // The BASES out of the same lowercase row, and not the whole row, because
        // the whole row also fails property 1 here — Phosphor's cmap has no `!`,
        // `=`, `<`, `>`, `:`, `*` or `/`, and with no replacement face behind it
        // those characters produce no glyph at all, so sixty characters come back
        // as forty-three. That is a true failure but a less interesting one; the
        // failure this design exists to prevent is the one where every character IS
        // covered and the cells still collapse. Filtering to a-z gives exactly that
        // case, and it is derived from the fixture rather than written out, so it
        // cannot drift away from the row above.
        let bases: String = lower.chars().filter(|c| c.is_ascii_lowercase()).collect();
        assert!(
            bases.len() >= 40 && bases.contains("at") && bases.contains("cat"),
            "the base-only twin has to be a real run of lowercase bases: {bases:?}"
        );
        for size in [9.5f32, 11.0, 11.5] {
            let err = check(&bases, size, ICON_FAMILY.clone()).expect_err(
                "a run of lowercase bases laid out in the ICON family satisfied the grid \
                 properties, which would mean the face this whole design keeps out of the \
                 Monospace chain is harmless — either Phosphor was replaced by something \
                 with real advances and no ligatures, in which case the isolation is no \
                 longer load-bearing and every comment saying it is has become false, or \
                 this check has stopped measuring what it says it does",
            );
            // It must break by COLLAPSE — properties 3 and 4 — and not by absence.
            // Measured: a two-glyph subset of this same face fails property 1 here
            // instead ("the shaper returned 0 glyphs for 40 characters"), because
            // subsetting deletes a-z outright. That is a different world and the
            // message has to say so, since a subsetted icon face in the Monospace
            // chain would be harmless and the isolation would have stopped
            // mattering. See NOTICE, "NOT SUBSETTED", for why that trade is refused.
            assert!(
                err.contains("off the") || err.contains("wide against"),
                "at {size} pt the icon family broke the bases by the wrong property: \
                 {err}. Property 3 or 4 -- a collapse -- is what a zero-advance or \
                 ligating face does. A count failure instead means the face no longer \
                 covers a-z at all, which most likely means it has been subsetted; a \
                 subsetted icon face cannot destroy the grid, so the isolation would no \
                 longer be load-bearing and the comments saying it is would be false."
            );
        }
        // And the concrete form of it, so the number is in the failure message: every
        // base is covered, every glyph is emitted, and the whole run is nothing wide.
        let icon_job = egui::text::LayoutJob::simple_singleline(
            bases.clone(),
            egui::FontId::new(11.5, ICON_FAMILY.clone()),
            egui::Color32::WHITE,
        );
        let icon_row = ctx.fonts_mut(|f| f.layout_job(icon_job));
        assert_eq!(
            icon_row.rows[0].glyphs.len(),
            bases.chars().count(),
            "the icon family dropped a base, so the width below is not measuring a \
             collapse — it is measuring an absence"
        );
        let mono_w = ctx.fonts_mut(|f| f.glyph_width(&egui::FontId::monospace(11.5), 'A'))
            * bases.chars().count() as f32;
        assert!(
            icon_row.size().x < mono_w * 0.25,
            "{} lowercase bases in the icon family came out {:.2} pt against the grid's \
             {mono_w:.2} pt; the point of this assertion is that they come out at \
             essentially nothing",
            bases.chars().count(),
            icon_row.size().x
        );

        // THE CHECK CAN FAIL, in the SHIPPED monospace face, on exactly the failure a
        // ligating face produces. `A` followed by U+0301 COMBINING ACUTE shapes in
        // Hack to three glyphs whose second and third both sit at x=7.0: the count is
        // right, the characters are right, the middle cell has no advance, and the row
        // comes out 13.84 pt where three cells of 6.9236 say 20.77. Measured, not
        // supposed. That is a cluster collapse in the face the app uses today, which
        // makes assertions 3 and 4 demonstrably live rather than arguably live.
        //
        // `row_text` can never emit it — it substitutes `?` for every byte that is not
        // `is_ascii_graphic`, and U+0301 is not ASCII — so this is a probe, not a live
        // defect.
        let collapsing = "A\u{0301}A";
        let err = check(collapsing, 11.5, egui::FontFamily::Monospace)
            .expect_err("a zero-advance combining mark passed every assertion above");
        assert!(
            err.contains("off the") || err.contains("wide against"),
            "the collapse was caught by the wrong property: {err}"
        );
        // And per-character advances stay uniform right through it, which is why the
        // sibling below cannot stand in for this test: both `A`s are 0.602 em by
        // `hmtx` while the pair `A` + mark occupies one cell on the page. A shaping
        // decision is a property of a PAIR and `glyph_width` only ever sees one
        // character. (U+0301's own `hmtx` advance is 0, so the sibling would catch
        // *this* probe if a combining mark were in its alphabet — a real ligature is
        // the harder case, because both members carry a full advance in `hmtx` and the
        // substitution still puts one glyph where two cells were. That is the case
        // measured on Fira Code, where nothing in the workspace asks the question.)
        let a = ctx.fonts_mut(|f| f.glyph_width(&egui::FontId::monospace(11.5), 'A'));
        assert!(a > 0.0, "the advance of a base is not what changed");
        let job = egui::text::LayoutJob::simple_singleline(
            collapsing.to_string(),
            egui::FontId::monospace(11.5),
            egui::Color32::WHITE,
        );
        let g = ctx.fonts_mut(|f| f.layout_job(job));
        let xs: Vec<f32> = g.rows[0].glyphs.iter().map(|q| q.pos.x).collect();
        assert_eq!(
            xs.len(),
            3,
            "the count survived the collapse, which is the point: {xs:?}"
        );
        assert!(
            (xs[2] - xs[1]).abs() < 0.01,
            "the third glyph must sit on top of the second for this to be a collapse: {xs:?}"
        );
    }

    /// The sequence grid rests on `x(base) = x0 + col * advance`, so the face it
    /// is set in must have ONE advance.
    ///
    /// PASSES at 0ebaa41 and this says so: Hack-Regular 3.003 has exactly one
    /// non-zero advance in the whole font (1233/2048 = 0.6020508), no GPOS, no
    /// legacy `kern`, and no GSUB lookup reachable from the shaper's default-on
    /// features. The arithmetic holds because the face cannot shape, not because
    /// anything checked — which is what makes this a prerequisite for changing the
    /// face and not a formality.
    ///
    /// **What this does NOT test is shaping, and it used to claim it did.** It calls
    /// `Fonts::glyph_width`, which is `FontFace::advance_width_unscaled` — one
    /// character's `hmtx` entry, out of skrifa, with no shaper anywhere in the path.
    /// The paragraph that stood here explained how a ligature breaks the column
    /// mapping and then measured `hmtx`, which cannot see one: a standalone probe
    /// confirmed this assertion PASSES for Fira Code 6.002, the face the advance
    /// band's own table asserts must be rejected. The shaping question is asked by
    /// `a_sequence_row_shapes_to_one_glyph_per_base_on_one_pitch` above, and that is
    /// the gate a font swap has to clear; this one is the narrower property its
    /// arithmetic sibling needs — a single advance to multiply by — and it is worth
    /// having as itself.
    ///
    /// THE CHECK CAN FAIL, demonstrated rather than asserted: the same loop over
    /// `FontFamily::Proportional` fails immediately, because Ubuntu-Light has 325
    /// distinct advances (measured). That case is exercised below so the
    /// demonstration ships with the test instead of living in a commit message.
    #[test]
    fn the_sequence_grid_has_one_advance_at_every_size_it_is_drawn() {
        // Every character `row_text` can put in a cell, and not a hand-picked
        // subset of it. `row_text` pushes `b as char` for every `is_ascii_graphic`
        // byte — 0x21..=0x7E, ninety-four characters — and `?` for the rest, so a
        // file whose reader kept a `@` or a `#` paints one. The 46-character
        // alphabet this used to list left those outside the gate for no reason:
        // measured, all 94 are uniform in Hack, Cascadia Mono, Cascadia Code and
        // Fira Code alike, so asking the whole range costs nothing and matches what
        // the function under test can actually emit.
        let ctx = test_ctx();
        for size in [9.0f32, 9.5, 10.0, 11.0, 11.5] {
            let id = egui::FontId::monospace(size);
            let want = ctx.fonts_mut(|f| f.glyph_width(&id, 'A'));
            for c in (0x21u8..=0x7E).map(char::from) {
                let got = ctx.fonts_mut(|f| f.glyph_width(&id, c));
                // Bit-identical, not a tolerance: these come from `hmtx`
                // integers scaled by one factor, so any difference at all is a
                // face that cannot carry a column grid.
                assert_eq!(
                    got, want,
                    "at {size} pt {c:?} advances {got} against {want} for 'A'; \
                     x(base) = x0 + col * advance is not true in this face"
                );
            }
        }
        // And the same question of a face that fails it, so the assertion above
        // is known to be able to say no. Ubuntu-Light has 325 distinct advances.
        let id = egui::FontId::proportional(11.5);
        let a = ctx.fonts_mut(|f| f.glyph_width(&id, 'i'));
        let b = ctx.fonts_mut(|f| f.glyph_width(&id, 'W'));
        assert_ne!(
            a, b,
            "the proportional face suddenly has one advance, so the check above \
             proves nothing about monospace"
        );
    }

    // -----------------------------------------------------------------------
    // the vendored faces
    // -----------------------------------------------------------------------

    /// **THE PROOF THAT EVERY OTHER FONT TEST IN THIS FILE IS ABOUT THE SHIPPED
    /// BINARY.**
    ///
    /// PROVEN TO FAIL at 0aa0f88: `install_fonts`, `test_ctx` and the vendored
    /// files did not exist, so this does not compile there. That is a
    /// compile-only failure and it is stated as one — but the assertion it makes
    /// is not vacuous, and this is where the danger was.
    ///
    /// The install has to happen in `App::new`, because that is the only place
    /// with a `CreationContext`. Tests never call `App::new`; all thirty-odd
    /// font-touching tests build their own `Context`. Put those two facts
    /// together and the swap could have shipped with the binary drawing Plex, the
    /// suite measuring Hack, and the advance band's pin on 0.602051 — the one
    /// assertion written to break on a face change — still green. So this
    /// measures the same glyph both ways and asserts they DISAGREE. If someone
    /// later drops the install from `test_ctx`, or the fonts stop reaching the
    /// context, this goes red rather than the whole suite going quietly stale.
    ///
    /// It also pins the deferred-effect trap. `set_fonts` does not install
    /// anything: it parks the definitions in `Memory::new_font_definitions`
    /// (`context.rs:2038`) for the next pass to pick up. The third measurement
    /// below is a context that was handed the fonts and then measured with NO
    /// pass in between, and it still reports Hack's advance.
    #[test]
    fn the_test_context_installs_the_faces_the_binary_ships() {
        let id = egui::FontId::monospace(11.5);
        let width = |ctx: &egui::Context| ctx.fonts_mut(|f| f.glyph_width(&id, 'A')) / 11.5;

        let bare = egui::Context::default();
        let _ = bare.run_ui(egui::RawInput::default(), |_| {});
        let before = width(&bare);
        let after = width(&test_ctx());

        assert!(
            (before - 0.602_051).abs() < 1e-4,
            "a context with no fonts installed should measure Hack's 0.602051, not {before} \
             -- if this moved, `default_fonts` changed and the rest of this test is \
             comparing two unknowns"
        );
        assert!(
            (after - 0.600_000).abs() < 1e-4,
            "`test_ctx` should measure IBM Plex Mono's 0.600000, not {after}"
        );
        assert_ne!(
            before, after,
            "`test_ctx` measures the same face as a bare Context, so it installs \
             nothing and every font test in this file is about a font the binary \
             does not ship"
        );

        // And the pass is what makes it take effect, not the call. One pass first,
        // because a context that has never run has no font set at all and
        // `glyph_width` panics rather than answering — which is its own small proof
        // that measuring and installing are separate events.
        let deferred = egui::Context::default();
        let _ = deferred.run_ui(egui::RawInput::default(), |_| {});
        install_fonts(&deferred);
        let no_pass = width(&deferred);
        assert!(
            (no_pass - 0.602_051).abs() < 1e-4,
            "a `set_fonts` with no pass after it measured {no_pass}, so it took effect \
             immediately -- egui changed, and `test_ctx`'s `run_ui` is no longer what \
             is keeping the tests honest. Check `Memory::new_font_definitions` before \
             deleting anything."
        );
    }

    /// THE LIGATURE GUARD, asked of the bytes in the repository.
    ///
    /// PROVEN TO FAIL at 0aa0f88 by mutation, and the mutation is recorded here
    /// because a guard nobody has seen go red is a guess: pointing `PLEX_MONO` at
    /// `IBMPlexSans-Regular.ttf` — a one-word edit, and exactly the kind of edit
    /// a hurried maintainer makes — turns this red with
    /// `IBM Plex Mono advertises liga, which harfrust applies by default`.
    ///
    /// This is the guard that replaces the one that could not fail. The old check
    /// asserted `Fonts::glyph_width` was bit-identical across the alphabet;
    /// `glyph_width` reads `hmtx`, a ligature is a GSUB substitution, and `hmtx`
    /// is untouched by one — so it measured a quantity a ligature cannot move and
    /// would have passed a fully ligating face. This reads the FeatureList.
    ///
    /// Three things it deliberately does, each of which the obvious version gets
    /// wrong:
    ///
    ///  1. It asks which rules can FIRE — not which lookups exist, and not which
    ///     features are advertised. Plex Mono ships two LigatureSubst lookups (34
    ///     fraction rules behind `frac`, 13 mark-stacking rules behind `ccmp`), so
    ///     a "no LookupType 4" test goes red on the healthy shipped face; and it
    ///     advertises `ccmp`, `locl` and GPOS `mark`, all three of which harfrust
    ///     turns on, so a "no default-on feature" test goes red on it too.
    ///  2. It checks GPOS as well as GSUB. Kerning moves x without touching GSUB
    ///     and would break the column grid invisibly to a GSUB-only guard.
    ///  3. It is scoped to the MONOSPACE face. Plex Sans has a live `f + i -> fi`
    ///     and is shipped anyway — see
    ///     `the_proportional_face_ligates_and_that_is_recorded_not_denied`.
    ///
    /// WHY IT IS NO LONGER THE TAG TEST IT WAS. Until this was reviewed,
    /// `SHAPER_DEFAULTS` held six tags and this asserted the intersection was
    /// empty. harfrust turns on FOURTEEN: `ccmp`, `locl`, `mark`, `mkmk`, `abvm`,
    /// `blwm`, `curs` and `dist` were all missing from the premise, so the guard
    /// was blind to the place a ligature would most plausibly hide. `ccmp` is the
    /// normal home for composition rules and this very face already keeps thirteen
    /// LigatureSubst rules there. The fix could not be to widen the list and keep
    /// the assertion — that is red on IBM Plex Mono — so the question became
    /// reachability: can any rule behind a default-on feature be SPELLED from the
    /// characters `row_text` emits.
    ///
    /// What it cannot see, so nobody mistakes it for total: it reads the file in
    /// the repository, so Hack — still in the chain — is out of reach, and it
    /// takes harfrust's default set as a premise. If egui gains user-feature
    /// control and someone enables `frac`, Plex Mono's 34 fraction ligatures
    /// become live and `1/2` in a coordinate would collapse; `SHAPER_DEFAULTS` is
    /// what would have to widen.
    #[test]
    fn the_monospace_face_advertises_no_feature_the_shaper_turns_on() {
        // Not vacuous: this face advertises 23 distinct features, so the parser is
        // demonstrably reading a real FeatureList and still saying "none of them".
        // Without this, a parser that returned an empty list for every input would
        // pass the assertion below and prove nothing.
        let all = sfnt::feature_tags(PLEX_MONO, b"GSUB")
            .expect("the vendored monospace face parses")
            .expect("IBM Plex Mono has a GSUB table");
        let mut distinct: Vec<_> = all.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert!(
            distinct.len() >= 20,
            "only {} distinct GSUB features in the monospace face ({}); the parser is \
             probably not reading the FeatureList, which would make the check below \
             pass for the wrong reason",
            distinct.len(),
            sfnt::show(&distinct)
        );

        // The tags a text face must not carry at all. `liga`, `clig`, `calt`,
        // `rlig` and `rclt` substitute and `kern` and `dist` move x, all of them
        // FOR the alphabet, so a face advertising one is not a thing to
        // investigate — it is the wrong face.
        for table in [b"GSUB", b"GPOS"] {
            let on = sfnt::default_on_features(PLEX_MONO, table)
                .expect("the vendored monospace face parses");
            let banned: Vec<_> = on
                .iter()
                .copied()
                .filter(|t| sfnt::NEVER_IN_A_MONOSPACE_TEXT_FACE.contains(t))
                .collect();
            assert!(
                banned.is_empty(),
                "the monospace face advertises {} in {}, which harfrust applies by \
                 default and egui 0.35 cannot switch off. A ligature collapses two \
                 glyphs into one advance, so x(base) = x0 + col * advance stops being \
                 true and a click lands on the wrong base.",
                sfnt::show(&banned),
                String::from_utf8_lossy(table)
            );
        }

        // THE VERDICT: the tags it does carry must have nothing behind them a
        // sequence row can spell.
        for table in [b"GSUB", b"GPOS"] {
            let live = sfnt::ascii_reachable_default_on(PLEX_MONO, table)
                .expect("the vendored monospace face parses");
            assert!(
                live.is_empty(),
                "in {} the monospace face has a rule behind {} that can fire on a run \
                 of printable ASCII. harfrust turns that feature on and egui 0.35 \
                 cannot switch it off, so the shaper may substitute or reposition \
                 inside a sequence row: x(base) = x0 + col * advance stops being true \
                 and a click lands on the wrong base.",
                String::from_utf8_lossy(table),
                sfnt::show(&live)
            );
        }

        // AND THE WALK IS NOT VACUOUS EITHER, which is the assertion the six-tag
        // version had no way to make. Plex Mono really does advertise three
        // features harfrust turns on, so the empty answer above is the result of
        // walking real lookups and ruling every one of them out: 13 ligature rules
        // and 5 chain contexts behind `ccmp`, 4 SingleSubsts behind `locl`, 4
        // MarkBasePos behind `mark` — each needing a combining mark `row_text`
        // cannot emit. If either of these ever reads EMPTY, the reachability check
        // above has stopped being exercised and is passing without looking at
        // anything.
        assert_eq!(
            sfnt::show(&sfnt::default_on_features(PLEX_MONO, b"GSUB").expect("it parses")),
            "ccmp locl",
            "the monospace face should still advertise exactly `ccmp` and `locl` among \
             the default-on features -- those are what give the reachability walk \
             something to reject"
        );
        assert_eq!(
            sfnt::show(&sfnt::default_on_features(PLEX_MONO, b"GPOS").expect("it parses")),
            "mark",
            "the monospace face should still advertise GPOS `mark`, the only default-on \
             positioning feature the walk gets to reject"
        );
    }

    /// The guard's positive control, on a face this repository actually ships.
    ///
    /// PROVEN TO FAIL at 0aa0f88: compile-only there, but the assertion is a
    /// measurement of a real file and not a tautology — it says the detector says
    /// YES when handed something that ligates. Without it,
    /// `the_monospace_face_advertises_no_feature_the_shaper_turns_on` is a check
    /// whose sensitivity nobody has ever observed, which is the shape of every
    /// defect this area has produced.
    ///
    /// IBM Plex Sans advertises `liga` — one rule, `f + i -> fi` — and GPOS
    /// `kern`. Both fire under egui 0.35, on ordinary words: "purification",
    /// "modified", "verification". That is recorded rather than fixed, because it
    /// is not a correctness bug HERE and the reason is checkable: no proportional
    /// text in this app carries a position-to-index mapping. The sequence grid,
    /// both gutters, the strand column and the map's site labels are all
    /// monospace (`map::label_font` is `FontId::monospace(10.0)`), and every
    /// proportional width is taken from a real egui layout — `width_of`, `cut_to`,
    /// `layout_no_wrap` — which shapes, so a ligature is already inside the
    /// measurement rather than missing from it.
    ///
    /// The line to hold: a ligating face is acceptable in Proportional and is not
    /// acceptable in Monospace. If anyone ever puts Plex Sans, or any face this
    /// test reports as ligating, into the Monospace chain, the guard above is what
    /// catches it.
    #[test]
    fn the_proportional_face_ligates_and_that_is_recorded_not_denied() {
        let gsub = sfnt::default_on_features(PLEX_SANS, b"GSUB").expect("Plex Sans parses");
        assert_eq!(
            sfnt::show(&gsub),
            "ccmp liga locl",
            "IBM Plex Sans should advertise `liga` among the default-on features, \
             alongside the same `ccmp` and `locl` the monospace face carries. If \
             `liga` has gone the detector may have gone blind and the monospace \
             guard proves nothing; if it lists MORE, the proportional face changed \
             and the reasoning above needs redoing."
        );
        let gpos = sfnt::default_on_features(PLEX_SANS, b"GPOS").expect("Plex Sans parses");
        assert_eq!(
            sfnt::show(&gpos),
            "kern mark",
            "Plex Sans kerns; it is proportional"
        );

        // AND THE REACHABILITY WALK — the part the monospace guard's verdict rests
        // on — SAYS YES ABOUT THIS FACE. It is the same code path, on a file this
        // repository ships, giving the opposite answer: `f + i -> fi` is a
        // LigatureSubst whose first glyph and whose one component are both
        // printable ASCII, so it is reported; `ccmp` and `locl` carry the same
        // mark-only rules here as in the monospace face and are correctly NOT
        // reported. A walk that answered "reachable" for every default-on tag would
        // fail this line, and so would one that answered "unreachable" for all of
        // them — which is the whole difficulty with a guard of this shape.
        assert_eq!(
            sfnt::show(&sfnt::ascii_reachable_default_on(PLEX_SANS, b"GSUB").expect("it parses")),
            "liga",
            "the walk should find Plex Sans's `f + i -> fi` reachable from printable \
             ASCII and nothing else in GSUB. EMPTY means it cannot see a live \
             ligature at all and the monospace verdict is worthless; MORE means it \
             is over-reporting and would eventually reject a healthy face."
        );
        assert_eq!(
            sfnt::show(&sfnt::ascii_reachable_default_on(PLEX_SANS, b"GPOS").expect("it parses")),
            "kern",
            "Plex Sans's `kern` reaches ASCII -- that is what kerning is -- and its \
             `mark` lookups do not, for the same reason the monospace face's do not"
        );
    }

    /// The icon face CAN ligate on the alphabet the grid paints — which is the
    /// premise the whole isolation rests on, asserted rather than assumed.
    ///
    /// This is the byte-level guard: no egui, no shaper, no `Context`. It reads
    /// the same `sfnt` walk the monospace guard's verdict comes from and points it
    /// at Phosphor, where the answer must be the OPPOSITE. Stated as one sentence:
    /// *this face can substitute inside a run of printable ASCII, therefore it must
    /// not be in the family the sequence grid is laid out in.* If it ever reads
    /// empty, the isolation has stopped being load-bearing and every comment in
    /// this file that explains why it exists has quietly become false — which is a
    /// thing to notice deliberately, not to discover.
    ///
    /// Plex Mono's empty answer is restated beside it so the two are read together,
    /// and so this test carries its own positive-and-negative control: the same
    /// function, the same run, opposite verdicts on two files the binary embeds.
    ///
    /// ITS BLIND SPOT, stated because it is exactly complementary to the shaping
    /// gate's: this is a statement about the FONT and not about the INSTALL. A
    /// build that vendors a perfectly dangerous Phosphor and then puts it straight
    /// into the Monospace chain passes this test with flying colours.
    /// `a_sequence_row_shapes_to_one_glyph_per_base_on_one_pitch` is what catches
    /// that. Neither substitutes for the other.
    ///
    /// PROVEN TO FAIL at 7ce59c1 only by not compiling — `phosphor()` did not exist
    /// there and no icon face was in the tree, so there is nothing at that commit
    /// for this to be run against. Said plainly rather than dressed up as a
    /// behavioural proof.
    ///
    /// IT IS SHOWN TO FAIL ON A REAL FILE INSTEAD. Point `phosphor()` at a
    /// two-glyph subset of the very same face — 1,100 bytes, built with fontTools,
    /// which is the ~5 KB alternative NOTICE names and rejects — and this test goes
    /// red at the vacuity check with "the icon face advertises no GSUB feature at
    /// all". That is the intended behaviour and not a nuisance: a subsetted face is
    /// harmless in any chain, so its arrival means the isolation has stopped being
    /// load-bearing and every comment explaining it has to be rewritten or the
    /// isolation deliberately relaxed.
    #[test]
    fn the_icon_face_can_ligate_on_the_alphabet_the_grid_paints() {
        // Not vacuous: Phosphor advertises a FeatureList the parser really reads.
        let all = sfnt::feature_tags(phosphor(), b"GSUB")
            .expect("the icon face parses")
            .expect("Phosphor has a GSUB table");
        assert!(
            !all.is_empty(),
            "the icon face advertises no GSUB feature at all, so the verdict below \
             would be about a file the parser did not read"
        );

        let live =
            sfnt::ascii_reachable_default_on(phosphor(), b"GSUB").expect("the icon face parses");
        assert!(
            live.contains(b"liga"),
            "the icon face no longer has a `liga` rule reachable from printable ASCII \
             (the walk found {}). That is the premise `font_definitions` is built on: \
             this face can substitute inside a run of the characters `row_text` emits, \
             so it must never be in the Monospace or Proportional chain. If the face \
             really has become harmless, the isolation is no longer load-bearing and \
             the comments claiming it is must be rewritten -- do not simply delete \
             this assertion.",
            sfnt::show(&live)
        );

        // The same walk on the face the grid IS set in, so the two verdicts sit side
        // by side and a walk that answered the same thing about both would fail here.
        assert!(
            sfnt::ascii_reachable_default_on(PLEX_MONO, b"GSUB")
                .expect("the monospace face parses")
                .is_empty(),
            "the monospace face now has a live ASCII-reachable substitution, which is \
             the existing font-swap alarm and is checked in full by \
             `the_monospace_face_advertises_no_feature_the_shaper_turns_on`"
        );
    }

    /// Phosphor is registered as its own family and is in neither text chain.
    ///
    /// **THE WEAKEST OF THE THREE GUARDS ON THIS CHANGE, AND KEPT ANYWAY** because
    /// it is the one whose failure message can name the invariant in one line, and
    /// because it is the only one that can catch the install being safe BY ORDERING.
    /// Measured, not asserted: appending `"Phosphor"` to the end of the Monospace
    /// chain leaves `a_sequence_row_shapes_to_one_glyph_per_base_on_one_pitch`
    /// perfectly green — Plex Mono covers everything a row can hold, so nothing
    /// resolves to the icon face and the row still lays out on one pitch — and this
    /// test goes red. That is exactly the arrangement `egui_phosphor::add_to_fonts`
    /// ships and exactly the arrangement `font_definitions` refuses.
    ///
    /// **AND HERE IS WHAT IT CANNOT SEE, MEASURED THE SAME WAY.** It asserts the
    /// SPELLING of the two chains, not the bytes behind the names in them: register
    /// the icon face under the existing `"IBMPlexMono"` key and this test passes,
    /// `the_icon_face_can_ligate_on_the_alphabet_the_grid_paints` passes, and the
    /// grid is destroyed — only the shaping gate goes red. The two are complements
    /// and neither substitutes for the other. (The narrower blind spot one might
    /// expect — the same bytes registered under a SECOND key and pushed onto
    /// Monospace — is closed here, but by the chain equality below rather than by
    /// the name check above, so it is closed as a side effect and should not be
    /// leant on.)
    ///
    /// It reads [`font_definitions`]'s return value — the value the binary installs
    /// — and not a copy built here. A structural test that rebuilds the definitions
    /// itself proves nothing at all about the application.
    ///
    /// The equality against `FontDefinitions::default()` is doing two jobs at once:
    /// it says nothing was added to either chain, and it says nothing was REMOVED.
    /// Plex Mono has no U+25B6 (the History tab's cursor, served by Hack) and
    /// neither Plex face has U+26A0 (the hidden-cut warning, served by Noto Emoji),
    /// so a chain that lost its tail would draw two live controls as tofu. Comparing
    /// against the upstream default rather than against a hard-coded list of face
    /// names keeps that true across an epaint bump.
    ///
    /// PROVEN TO FAIL at 7ce59c1 only by not compiling: `font_definitions` and
    /// `ICON_FAMILY` do not exist there, and neither does the icon face. Said
    /// plainly rather than dressed up. Against this commit it reddens on both
    /// mutations above.
    #[test]
    fn the_icon_face_is_in_its_own_family_and_in_neither_text_chain() {
        let defs = font_definitions();
        let base = egui::FontDefinitions::default();

        for (family, ours) in [
            (egui::FontFamily::Monospace, "IBMPlexMono"),
            (egui::FontFamily::Proportional, "IBMPlexSans"),
        ] {
            let got = defs
                .families
                .get(&family)
                .unwrap_or_else(|| panic!("{family:?} is not bound to any fonts"));
            assert!(
                !got.iter().any(|n| n == "Phosphor"),
                "{family:?} resolves to {got:?}, which includes the icon face. Phosphor \
                 covers a-z with a zero `hmtx` advance and carries 1,513 ligature rules; \
                 in a text chain it takes a 60-base lowercase row from 414.00 pt to \
                 115.00 pt and every click in the sequence view lands on the wrong base. \
                 It belongs in {:?} and nowhere else.",
                *ICON_FAMILY
            );
            let mut want = base
                .families
                .get(&family)
                .expect("egui's own defaults bind both text families")
                .clone();
            want.insert(0, ours.to_owned());
            assert_eq!(
                got, &want,
                "{family:?} is no longer the vendored face followed by egui's own \
                 defaults. Nothing may be appended to these chains and nothing may be \
                 dropped from them: Plex Mono has no U+25B6 and neither Plex face has \
                 U+26A0, so a shortened chain draws the History cursor and the \
                 hidden-cut warning as tofu boxes."
            );
        }

        assert_eq!(
            defs.families.get(&ICON_FAMILY.clone()).map(Vec::as_slice),
            Some(["Phosphor".to_owned()].as_slice()),
            "the icons family must hold exactly the one icon face. A text face behind \
             it would turn a stray label from a visible hole into a slightly-wrong \
             word that ships; a missing family is worse still, because \
             `FontsImpl::font` panics rather than falling back."
        );
        assert!(
            defs.font_data.contains_key("Phosphor"),
            "the icons family names a face that was never registered as font data, \
             which panics on the first paint that asks for it"
        );
    }

    /// A screen reader hears "Undo", not U+E08A.
    ///
    /// **THE THIRD OF NOTICE'S FOUR REASONS FOR REJECTING AN ICON FONT WAS THIS
    /// ONE, AND UNLIKE THE FIRST IT IS NOT ANSWERED BY THE FAMILY ISOLATION AT
    /// ALL.** `Atoms::text()` (egui 0.35 `atomics/atoms.rs:51`) concatenates every
    /// text atom with a space and `Button` hands the result to
    /// `WidgetInfo::labeled` (`widgets/button.rs:401`), so the obvious spelling —
    /// `ui.button((ICON_UNDO, "Undo"))` — names the control `"\u{E08A} Undo"` for
    /// accesskit, which is on in this binary. [`button_with_icon`] reserves the
    /// space with an empty `Atom` and paints the glyph instead, and this is what
    /// says that actually worked rather than a comment claiming it.
    ///
    /// PROVEN TO FAIL: swap the empty `Atom` in [`button_with_icon`] for `icon`
    /// and the PUA assertion goes red with the name `"\u{e08a} Undo"`. Measured.
    ///
    /// The PUA sweep is over EVERY node, not just the two buttons, because the
    /// failure it guards against is a private codepoint reaching assistive
    /// technology from anywhere — and the range is checked rather than the two
    /// literals, so a third icon added later is covered without anyone
    /// remembering to extend this.
    #[test]
    fn the_icon_buttons_read_as_their_word_and_not_as_a_private_use_codepoint() {
        let ctx = test_ctx();
        ctx.enable_accesskit();
        // Two passes: the first turns accesskit on, the second is the one that
        // carries a tree. A single pass yields None and the assertions below
        // would then be about nothing.
        let mut update = None;
        for _ in 0..2 {
            let out = ctx.run_ui(egui::RawInput::default(), |ui| {
                button_with_icon(ui, ICON_UNDO, "Undo");
                button_with_icon(ui, ICON_REDO, "Redo");
            });
            update = out.platform_output.accesskit_update;
        }
        let update = update.expect("accesskit is enabled, so a tree is produced");

        let labels: Vec<String> = update
            .nodes
            .iter()
            .filter_map(|(_, n)| n.label().map(str::to_owned))
            .collect();
        assert!(
            !labels.is_empty(),
            "the accessibility tree has no labelled node at all, so the assertions \
             below would pass on an empty set"
        );
        for want in ["Undo", "Redo"] {
            assert!(
                labels.iter().any(|l| l == want),
                "no control is named exactly {want:?}; the labels are {labels:?}. If one \
                 of them reads \"\\u{{E08A}} Undo\" the icon has been passed as a text \
                 atom, and `Atoms::text()` concatenated it into the accessible name."
            );
        }
        for l in &labels {
            for c in l.chars() {
                assert!(
                    !('\u{E000}'..='\u{F8FF}').contains(&c),
                    "the accessible name {l:?} contains U+{:04X}, a Private Use Area \
                     codepoint. A screen reader has no name for it, so a user is told \
                     nothing or told a number. Icons are painted, never passed as text \
                     atoms -- see `button_with_icon`.",
                    c as u32
                );
            }
        }
    }

    /// The guard must FAIL CLOSED, or it is the same defect wearing a parser.
    ///
    /// PROVEN TO FAIL at 0aa0f88: compile-only. The assertions are real.
    ///
    /// "No GSUB table, therefore no ligatures" is true of a font and false of
    /// everything else. Without the container check, a renamed JPEG, a WOFF, a
    /// font collection or a truncated download would all sail through the guard
    /// above reporting no ligatures — and the guard would be green precisely when
    /// the file was unusable. Each case below is a thing that could plausibly end
    /// up at `bins/pl-gui/fonts/` after a bad merge or a mangled checkout.
    #[test]
    fn the_ligature_guard_refuses_to_pass_a_file_it_cannot_read() {
        let cases: [(&str, Vec<u8>); 5] = [
            ("empty", Vec::new()),
            (
                "a JPEG renamed to .ttf",
                vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 16, 0, 0],
            ),
            ("a WOFF", b"wOFF\x00\x01\x00\x00\x00\x00\x00\x10".to_vec()),
            (
                "a font COLLECTION, whose faces are one level down",
                b"ttcf\x00\x01\x00\x00\x00\x00\x00\x02".to_vec(),
            ),
            // A real sfnt header claiming more tables than the file holds: the
            // shape a truncated download takes.
            ("a truncated face", {
                let mut v = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x09];
                v.extend_from_slice(&[0u8; 6]);
                v.extend_from_slice(b"head");
                v
            }),
        ];
        for (what, bytes) in cases {
            let got = sfnt::default_on_features(&bytes, b"GSUB");
            assert!(
                got.is_err(),
                "{what} was accepted, and reported {:?} ligature features. A guard that \
                 answers \"no ligatures\" about a file that is not a font is green \
                 exactly when it should be loudest.",
                got.map(|t| sfnt::show(&t))
            );
        }

        // And the other direction, so the refusals above are known not to be a
        // parser that rejects everything: a hand-built sfnt carrying a GSUB whose
        // FeatureList holds `liga` must be READ, and reported.
        let synthetic = synthetic_face_advertising(b"liga");
        assert_eq!(
            sfnt::show(&sfnt::default_on_features(&synthetic, b"GSUB").expect("it parses")),
            "liga",
            "the parser could not find `liga` in a FeatureList built to contain it, \
             so its silence about the real faces means nothing"
        );
        // The same blob with a benign tag, so the filter is doing the filtering
        // rather than the parser finding `liga` in every input.
        let benign = synthetic_face_advertising(b"frac");
        assert!(
            sfnt::default_on_features(&benign, b"GSUB")
                .expect("it parses")
                .is_empty(),
            "`frac` was reported as default-on; it is not, and Plex Mono has 34 \
             fraction ligatures behind it that would then read as a defect"
        );
    }

    /// THE HOLE THE SIX-TAG GUARD HAD, HELD OPEN AS A RUNNING TEST.
    ///
    /// PROVEN TO FAIL by construction, and this is the one place in the change
    /// where the failing case is a permanent fixture rather than a mutation
    /// somebody applied once and reverted. Two faces differing in ONE GLYPH ID —
    /// the second component of a single ligature rule — must get opposite answers.
    ///
    /// Both advertise `ccmp` and nothing else. Under the list this module shipped
    /// with for one review cycle, `ccmp` was not even in `SHAPER_DEFAULTS`, so
    /// BOTH read as clean; harfrust has turned `ccmp` on the whole time
    /// (`ot_shape.rs:87`, `F_GLOBAL`), and `ccmp` is where composition rules
    /// normally live — IBM Plex Mono keeps thirteen LigatureSubst rules there. A
    /// ligating face only had to spell its rule `ccmp` instead of `liga` to walk
    /// past the guard entirely.
    ///
    /// Widening the list is not by itself the fix, and the second assertion here
    /// is what says so: Plex Mono advertises `ccmp` too, so a widened TAG test is
    /// red on the healthy shipped face. Only asking whether the rule can be
    /// SPELLED from printable ASCII separates these two.
    #[test]
    fn a_ccmp_ligature_on_ascii_is_caught_and_one_on_marks_is_not() {
        // a + c -> A. Every glyph in it is one `row_text` can emit, so this is the
        // exact failure the sequence grid cannot survive: two columns become one
        // advance and every base after it is named wrong.
        let dangerous = synthetic_ccmp_ligature(GID_C);
        // a + U+0301 COMBINING ACUTE -> A. Structurally identical, one glyph id
        // different, and unreachable because `row_text` substitutes `?` for every
        // byte that is not `is_ascii_graphic`. This is the shape of all thirteen
        // of Plex Mono's real `ccmp` rules.
        let harmless = synthetic_ccmp_ligature(GID_ACUTE);

        // First: the shortcut the guard applies before the walk cannot tell these
        // apart, and must not be mistaken for the thing that does.
        for (what, face) in [("dangerous", &dangerous), ("harmless", &harmless)] {
            let on = sfnt::default_on_features(face, b"GSUB").expect("it parses");
            assert_eq!(
                sfnt::show(&on),
                "ccmp",
                "the {what} fixture should advertise exactly `ccmp`"
            );
            assert!(
                !on.iter()
                    .any(|t| sfnt::NEVER_IN_A_MONOSPACE_TEXT_FACE.contains(t)),
                "the {what} fixture tripped the never-in-a-text-face list, so this \
                 test is no longer about the reachability walk"
            );
        }

        // Then: the walk, which is the part that has to be right.
        assert_eq!(
            sfnt::show(&sfnt::ascii_reachable_default_on(&dangerous, b"GSUB").expect("it parses")),
            "ccmp",
            "a LigatureSubst spelled `a` + `c` -> `A`, behind `ccmp`, was NOT reported \
             as reachable. Both glyphs are printable ASCII and harfrust turns `ccmp` \
             on globally, so this collapses two columns of a sequence row into one \
             advance and every click past it lands on the wrong base."
        );
        assert!(
            sfnt::ascii_reachable_default_on(&harmless, b"GSUB")
                .expect("it parses")
                .is_empty(),
            "a `ccmp` rule whose second component is a combining mark was reported as \
             reachable. `row_text` cannot emit one, so this is over-reporting -- and \
             it would reject IBM Plex Mono, whose thirteen real `ccmp` ligature rules \
             all have exactly this shape."
        );
    }

    // The glyph ids `synthetic_ccmp_ligature` assigns. Named because the whole
    // point of the fixture is that ONE of them is the difference between a face
    // that breaks the grid and one that does not.
    const GID_A_LOWER: u16 = 1;
    const GID_C: u16 = 2;
    const GID_A_UPPER: u16 = 3;
    const GID_ACUTE: u16 = 4;

    /// A face advertising `ccmp` with one LigatureSubst rule behind it:
    /// `a` + `component` -> `A`.
    ///
    /// Hand-assembled, with every offset written out, because the point is to
    /// control exactly one variable — the second component's glyph id — and to
    /// exercise the reader's real path: FeatureList -> Feature -> LookupList ->
    /// Lookup -> LigatureSubst -> Coverage -> LigatureSet -> Ligature, plus a cmap
    /// the walk has to read to know which glyph ids are printable at all.
    ///
    /// The cmap is format 12 rather than the format 4 both Plex faces carry, which
    /// is deliberate: it means the two cmap readers in `sfnt` are each exercised by
    /// something, format 4 by the real faces and format 12 by this.
    fn synthetic_ccmp_ligature(component: u16) -> Vec<u8> {
        // cmap format 12: four single-codepoint groups, sorted by codepoint as the
        // format requires. 'A' -> 3, 'a' -> 1, 'c' -> 2, U+0301 -> 4.
        let mut sub: Vec<u8> = Vec::new();
        sub.extend_from_slice(&12u16.to_be_bytes()); // format
        sub.extend_from_slice(&0u16.to_be_bytes()); // reserved
        sub.extend_from_slice(&(16u32 + 4 * 12).to_be_bytes()); // length
        sub.extend_from_slice(&0u32.to_be_bytes()); // language
        sub.extend_from_slice(&4u32.to_be_bytes()); // numGroups
        for (cp, gid) in [
            (0x41u32, GID_A_UPPER),
            (0x61, GID_A_LOWER),
            (0x63, GID_C),
            (0x0301, GID_ACUTE),
        ] {
            sub.extend_from_slice(&cp.to_be_bytes());
            sub.extend_from_slice(&cp.to_be_bytes());
            sub.extend_from_slice(&(gid as u32).to_be_bytes());
        }
        let mut cmap: Vec<u8> = Vec::new();
        cmap.extend_from_slice(&0u16.to_be_bytes()); // version
        cmap.extend_from_slice(&1u16.to_be_bytes()); // numTables
        cmap.extend_from_slice(&3u16.to_be_bytes()); // platformID: Windows
        cmap.extend_from_slice(&10u16.to_be_bytes()); // encodingID: full repertoire
        cmap.extend_from_slice(&12u32.to_be_bytes()); // subtableOffset
        cmap.extend_from_slice(&sub);

        // GSUB, laid out at fixed offsets so every Offset16 below is a constant
        // that can be checked by eye against the spec.
        //
        //   0  header (10 bytes)          10 ScriptList: count 0
        //  12  FeatureList                20 Feature
        //  26  LookupList                 30 Lookup
        //  38  LigatureSubst              46 Coverage
        //  52  LigatureSet                56 Ligature
        let mut g: Vec<u8> = Vec::new();
        g.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]); // version 1.0
        g.extend_from_slice(&10u16.to_be_bytes()); // scriptListOffset
        g.extend_from_slice(&12u16.to_be_bytes()); // featureListOffset
        g.extend_from_slice(&26u16.to_be_bytes()); // lookupListOffset
        g.extend_from_slice(&0u16.to_be_bytes()); // @10 scriptCount = 0
        g.extend_from_slice(&1u16.to_be_bytes()); // @12 featureCount
        g.extend_from_slice(b"ccmp");
        g.extend_from_slice(&8u16.to_be_bytes()); // -> 12 + 8 = 20
        g.extend_from_slice(&0u16.to_be_bytes()); // @20 featureParams
        g.extend_from_slice(&1u16.to_be_bytes()); // lookupIndexCount
        g.extend_from_slice(&0u16.to_be_bytes()); // lookupListIndices[0]
        g.extend_from_slice(&1u16.to_be_bytes()); // @26 lookupCount
        g.extend_from_slice(&4u16.to_be_bytes()); // -> 26 + 4 = 30
        g.extend_from_slice(&4u16.to_be_bytes()); // @30 lookupType = LigatureSubst
        g.extend_from_slice(&0u16.to_be_bytes()); // lookupFlag
        g.extend_from_slice(&1u16.to_be_bytes()); // subTableCount
        g.extend_from_slice(&8u16.to_be_bytes()); // -> 30 + 8 = 38
        g.extend_from_slice(&1u16.to_be_bytes()); // @38 substFormat
        g.extend_from_slice(&8u16.to_be_bytes()); // coverageOffset -> 38 + 8 = 46
        g.extend_from_slice(&1u16.to_be_bytes()); // ligatureSetCount
        g.extend_from_slice(&14u16.to_be_bytes()); // -> 38 + 14 = 52
        g.extend_from_slice(&1u16.to_be_bytes()); // @46 coverageFormat
        g.extend_from_slice(&1u16.to_be_bytes()); // glyphCount
        g.extend_from_slice(&GID_A_LOWER.to_be_bytes()); // the ligature's first glyph
        g.extend_from_slice(&1u16.to_be_bytes()); // @52 ligatureCount
        g.extend_from_slice(&4u16.to_be_bytes()); // -> 52 + 4 = 56
        g.extend_from_slice(&GID_A_UPPER.to_be_bytes()); // @56 ligatureGlyph
        g.extend_from_slice(&2u16.to_be_bytes()); // componentCount, first included
        g.extend_from_slice(&component.to_be_bytes()); // the one variable

        assemble_face(&[
            (b"GSUB", &g),
            (b"cmap", &cmap),
            (b"head", &[]),
            (b"hmtx", &[]),
            (b"maxp", &[]),
        ])
    }

    /// Wrap hand-built tables in an sfnt table directory.
    fn assemble_face(tables: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        // The directory must be sorted by tag, as the format requires, even though
        // this reader does not depend on it.
        let mut sorted = tables.to_vec();
        sorted.sort_by_key(|(t, _)| **t);

        let mut out = vec![0x00, 0x01, 0x00, 0x00];
        out.extend_from_slice(&(sorted.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0u8; 6]); // searchRange, entrySelector, rangeShift
        let mut at = 12 + 16 * sorted.len();
        let mut body = Vec::new();
        for (tag, data) in &sorted {
            out.extend_from_slice(*tag);
            out.extend_from_slice(&0u32.to_be_bytes()); // checkSum, unchecked
            out.extend_from_slice(&(at as u32).to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            at += data.len();
            body.extend_from_slice(data);
        }
        out.extend_from_slice(&body);
        out
    }

    /// A minimal but structurally valid TrueType face whose GSUB advertises one
    /// feature, for [`the_ligature_guard_refuses_to_pass_a_file_it_cannot_read`].
    ///
    /// Hand-assembled rather than subsetted from a real font, because the point is
    /// to control exactly one variable — the feature tag — and a subsetting tool
    /// decides too much. The four required tables are present and empty: the guard
    /// checks that they EXIST, which is what stops a non-font passing, and reads
    /// none of them.
    fn synthetic_face_advertising(tag: &[u8; 4]) -> Vec<u8> {
        // GSUB: version 1.0, then three Offset16s. Script and Lookup lists point
        // at an empty count; the FeatureList holds one record.
        let mut gsub = vec![0x00, 0x01, 0x00, 0x00];
        gsub.extend_from_slice(&10u16.to_be_bytes()); // scriptList -> empty count
        gsub.extend_from_slice(&12u16.to_be_bytes()); // featureList
        gsub.extend_from_slice(&10u16.to_be_bytes()); // lookupList -> empty count
        gsub.extend_from_slice(&0u16.to_be_bytes()); // the shared empty count at 10
        gsub.extend_from_slice(&1u16.to_be_bytes()); // featureCount, at 12
        gsub.extend_from_slice(tag);
        gsub.extend_from_slice(&0u16.to_be_bytes()); // featureOffset, unfollowed

        assemble_face(&[
            (b"GSUB", &gsub),
            (b"cmap", &[]),
            (b"head", &[]),
            (b"hmtx", &[]),
            (b"maxp", &[]),
        ])
    }

    /// The disclosure caret is UI chrome, so SC 1.4.11 applies: 3:1, against what
    /// it is actually drawn on.
    ///
    /// PROVEN TO FAIL at 0aa0f88: compile-only there, `CARET_W` and the caret did
    /// not exist. The assertion is a real measurement.
    ///
    /// **AGAINST THE BUTTON FILL, NOT THE PANEL, and that distinction is the whole
    /// point.** `ring.rs` already records the trap of choosing a palette role that
    /// clears the panel background and then fails against the thing it is painted
    /// over; a toolbar button has its own `bg_fill`, which in dark mode is lighter
    /// than the panel and in light mode darker. Checking the easy background would
    /// have produced a passing number about the wrong pair of colours.
    ///
    /// The negative half is not decoration either: `faint` and `line` are the two
    /// palette roles a reasonable person would reach for when drawing a hairline
    /// triangle, and both fail. If a later edit picks one, the assertion above
    /// catches it; this half proves the assertion is capable of catching it.
    #[test]
    fn the_disclosure_caret_clears_three_to_one_on_the_button_it_is_drawn_on() {
        use pl_draw::contrast::{ratio, Kind};
        let min = Kind::Graphic.min_ratio();

        // FIRST, and this half is why the test is not a tautology: the carets are
        // actually painted, and there are three of them.
        //
        // Everything below this block is a statement about a palette role that
        // predates the caret, and on its own it would pass unchanged at 0aa0f88
        // where no caret exists — a check that cannot fail, for the fourth time in
        // this project. So find the real polygons in the real toolbar, by their
        // shape, and count them.
        let ctx = test_ctx();
        let mut app = seq_app();
        let win = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 840.0),
            )),
            ..Default::default()
        };
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let out = ctx.run_ui(win.clone(), |ui| {
                app.top_bar(ui);
            });
            shapes = flat_shapes(&out.shapes);
        }
        let ink = Palette::of(ctx.theme() == egui::Theme::Dark).ink;
        // Found by GEOMETRY ALONE — three points at the caret's own size — and
        // asked as a shape rather than by an `Id`, because what has to be true is
        // that a user sees a triangle.
        //
        // COLOUR IS DELIBERATELY NOT IN THIS FILTER, and it was. Painting the
        // caret in `faint` then dropped the count to zero and the test reported
        // "0 disclosure carets painted in the toolbar", which is the wrong
        // diagnosis for a contrast defect: it sends the next reader to the
        // toolbar when the fault is in the palette role. The count says the
        // triangles are there, the assertion under it says what they are filled
        // with, and only then does the ratio below have a subject.
        let carets: Vec<&egui::epaint::PathShape> = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Path(p) => {
                    let b = egui::Rect::from_points(&p.points);
                    (p.points.len() == 3
                        && (b.width() - CARET_W).abs() < 0.51
                        && (b.height() - CARET_H).abs() < 0.51)
                        .then_some(p)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            carets.len(),
            3,
            "{} disclosure carets painted in the toolbar, not 3. The three menus \
             are Save, Export map and Molecule; a button that opens a menu and does not \
             say so is the defect `menu_with_caret` exists to fix.",
            carets.len()
        );
        for c in &carets {
            assert_eq!(
                c.fill, ink,
                "a caret is filled {:?}, not the palette's `ink` {ink:?}. Every ratio \
                 asserted below is about `ink`; a caret drawn in anything else is not \
                 covered by any of them, and `faint` and `line` are exactly the two \
                 roles a hairline triangle invites.",
                c.fill
            );
        }

        for dark in [true, false] {
            let mut v = if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            };
            theme::apply(&mut v);
            let p = Palette::of(dark);
            let mode = if dark { "dark" } else { "light" };
            // Every surface a menu button presents while it is on screen: at rest,
            // hovered, and open. An open menu's button is drawn `active`, which is
            // exactly when the caret matters most.
            for (state, bg) in [
                ("at rest", v.widgets.inactive.bg_fill),
                ("hovered", v.widgets.hovered.bg_fill),
                ("open", v.widgets.active.bg_fill),
                ("the panel behind it", theme::panel_fill(dark)),
            ] {
                let bg = (bg.r(), bg.g(), bg.b());
                let fg = (p.ink.r(), p.ink.g(), p.ink.b());
                let got = ratio(fg, bg);
                assert!(
                    got >= min,
                    "the caret on a {state} button in {mode} mode is {got:.2}:1, under \
                     the {min}:1 SC 1.4.11 asks of UI chrome"
                );
            }
            // And the roles that must NOT be used, so the check above is known to be
            // able to say no.
            //
            // `faint` fails in BOTH themes; `line` fails in light only and clears
            // 3:1 in dark. Stated per theme rather than as one loop, because
            // writing it as "these two always fail" is false, and a negative
            // control that is false about its own subject is worth less than none —
            // it would have to be loosened the first time someone ran it, and
            // loosening is how a demonstration turns back into an assumption.
            let bg = theme::panel_fill(dark);
            let bg = (bg.r(), bg.g(), bg.b());
            let mut failing = vec![("faint", p.faint)];
            if !dark {
                failing.push(("line", p.line));
            }
            for (what, c) in failing {
                let fg = (c.r(), c.g(), c.b());
                let got = ratio(fg, bg);
                assert!(
                    got < min,
                    "`{what}` now reaches {got:.2}:1 in {mode} mode, so this test no \
                     longer demonstrates that the threshold can fail. Pick another \
                     failing role rather than deleting the demonstration."
                );
            }
        }
    }

    /// The vendored files are the ones NOTICE says they are.
    ///
    /// PROVEN TO FAIL at 0aa0f88: compile-only, and the assertion is a real
    /// measurement of the committed bytes.
    ///
    /// NOTICE records a sha256 for each face so a recipient can check the shipped
    /// binary against IBM's own release archive. A hash in a text file that
    /// nothing compares is a hash that goes stale the first time someone
    /// re-downloads a face and forgets — and then the provenance chain the "do not
    /// subset" decision was made to protect is broken, silently, in the direction
    /// of looking fine. This is the comparison.
    ///
    /// The lengths are here as well because they are the cheap half: a truncated
    /// checkout gets caught by the byte count before anyone reads the hash.
    ///
    /// **AND IT READS NOTICE, WHICH IT DID NOT UNTIL THIS WAS REVIEWED.** The
    /// first version compared each file against a string literal in this test, and
    /// NOTICE claimed in its own prose that the comparison protected IT. It did
    /// not: the hash existed twice, in two files, with nothing joining them, so a
    /// mistyped or stale digit in NOTICE stayed green forever. That is the "a
    /// check that cannot fail proves nothing" defect applied to the wrong axis —
    /// it could fail if somebody swapped a font and could not fail if somebody
    /// mistyped the record OF the font. `include_str!` is the join.
    ///
    /// THE LICENCE TEXTS ARE IN HERE TOO, and they are the reason the count is
    /// five rather than two. They are not linked into the binary — they travel
    /// beside it, copied by `tools/release.ps1` — so nothing else in the build
    /// would notice if one were truncated, re-wrapped by an editor, or replaced
    /// with the wrong face's licence. A licence text that has quietly become the
    /// wrong bytes is worse than a missing one, because the package still looks
    /// complete.
    #[test]
    fn the_vendored_faces_are_the_files_notice_records() {
        // The record itself. Compiled in so that a change to either side has to
        // be a change to both.
        const NOTICE_TEXT: &str = include_str!("../../../NOTICE");

        for (what, bytes, len, want) in [
            (
                "IBM Plex Mono 2.005 Regular",
                PLEX_MONO,
                173_052usize,
                "7c6fbddca4b700be918f5f6183d9bd4464fa427fe435f0b480d77fe2bb8c5a43",
            ),
            (
                "IBM Plex Sans 3.005 Regular",
                PLEX_SANS,
                200_500,
                "975dcda37d80f038dcd143c22e33ca2d97a0cc5a929aace1c749153b0fe1afa5",
            ),
            // The four licence texts vendored on 2026-07-30 for the faces
            // `default_fonts` embeds. Each is byte-identical to the copy in
            // epaint_default_fonts-0.35.0/fonts/, which is what makes a recipient
            // able to check it against the crate.
            (
                "the Hack MIT + Bitstream Vera text",
                include_bytes!("../fonts/Hack-MIT-and-BitstreamVera.txt"),
                3_734,
                "47c0cccbeec7e8614548cc485588b28149e7874188df5f41b36efebcee285c87",
            ),
            (
                "the Ubuntu Font Licence 1.0 text",
                include_bytes!("../fonts/Ubuntu-UFL.txt"),
                4_673,
                "2f0015108d68627bd788d313f529c21ff4da2c2c42a5e1f3883acc83480f9002",
            ),
            (
                "the Noto Emoji OFL text",
                include_bytes!("../fonts/NotoEmoji-OFL.txt"),
                4_301,
                "6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2",
            ),
            (
                "the emoji-icon-font MIT text",
                include_bytes!("../fonts/emoji-icon-font-MIT.txt"),
                1_069,
                "b9d2c1d909aa149996fd4c91dcb92b2362a04431640c1d200959da94caf8cde1",
            ),
            // The icon face, added 2026-07-30. It arrives through a crate like the
            // four above, but unlike them it is a face this project CHOSE, and the
            // `=0.13.0` pin in Cargo.toml exists so that this digest and the bytes
            // cannot drift apart. If `cargo update` were ever able to move it, this
            // assertion is what would notice.
            (
                "Phosphor Icons 2.1 Bold",
                phosphor(),
                495_308,
                "10a0a1cb4f8156a420f9f84cf34c4e9871e58ed2ddea1f6a8079ad07243a7fb2",
            ),
            // Its licence text, which unlike the four above could NOT be copied out
            // of the crate: egui-phosphor ships a licence for its Rust wrapper and
            // none for the typeface. Transcribed from the URL in the font's own
            // `name` ID 14; NOTICE says so, and says byte-for-byte identity with
            // upstream was not established.
            (
                "the Phosphor MIT text",
                include_bytes!("../fonts/Phosphor-MIT.txt"),
                1_071,
                "6918b72504641180600cbbd4a86b0dfa9dfccf788775694325b71b9a029f6eb4",
            ),
        ] {
            assert_eq!(
                bytes.len(),
                len,
                "{what} is {} bytes, not {len}",
                bytes.len()
            );
            let got = pl_core::sha256::sha256_hex(bytes);
            assert_eq!(
                got, want,
                "{what} hashes to {got}, and this test says {want}. Either the file \
                 was replaced without updating the record — in which case the licence \
                 record and the trademark notice may now describe something else — or \
                 the expectation is wrong. Do not edit the expectation without \
                 re-deriving it from the upstream source."
            );
            assert!(
                NOTICE_TEXT.contains(got.as_str()),
                "NOTICE does not contain {got}, the sha256 of {what}. NOTICE is what a \
                 recipient checks the shipped bytes against, so a hash that is only \
                 correct in this file is a hash nobody will ever use. Update the \
                 entry in NOTICE."
            );
        }

        // The embedded faces whose copyright cannot be read out of their own `name`
        // table — IDs 0, 7 and 13 are all empty in emoji-icon-font.ttf, and in
        // Phosphor ID 0 holds the family name "Phosphor Icons" where the copyright
        // should be and ID 7 is absent — so NOTICE is the ONLY place their MIT
        // notices can travel. Named explicitly because a hash check would not
        // notice one going missing.
        for owed in [
            "John Slegers",
            "Canonical Ltd",
            "Google Inc",
            "Bold Monday",
            "Phosphor Icons",
        ] {
            assert!(
                NOTICE_TEXT.contains(owed),
                "NOTICE no longer names {owed:?}. Every embedded face's copyright \
                 holder has to appear here: four of the six licences require the \
                 copyright notice in each copy, and the crate's generic OFL.txt and \
                 UFL.txt name no holder at all."
            );
        }
    }

    // -----------------------------------------------------------------------
    // autosave
    // -----------------------------------------------------------------------

    /// An app with a recovery file of its own, in the temp directory.
    fn app_with_recovery(name: &str) -> (App, PathBuf) {
        let dir = std::env::temp_dir().join(format!("pl-gui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp directory");
        let path = dir.join(format!("{name}.recover"));
        let _ = std::fs::remove_file(&path);
        let mut app = App::blank();
        app.recovery = Some(path.clone());
        (app, path)
    }

    /// A document holding `seq`, circularised, so it has exactly one edit.
    fn edited_doc(name: &str, seq: &str) -> Document {
        let mut d =
            Document::from_bytes(format!(">x\n{seq}\n").as_bytes(), name.into(), None).unwrap();
        d.apply(pl_core::OpKind::SetTopology(pl_core::Topology::Circular))
            .unwrap();
        d
    }

    /// What the recovery file actually holds, read back as a molecule.
    fn autosaved(path: &std::path::Path) -> (pl_core::Molecule, String) {
        let text = std::fs::read_to_string(path).expect("a recovery file");
        let snap = recover::decode(&text).expect("a readable header");
        let (mol, _, _) =
            pl_fileio::load_with_report(snap.genbank.as_bytes()).expect("a readable body");
        (mol, snap.title)
    }

    #[test]
    fn an_undo_then_a_different_edit_is_not_mistaken_for_the_autosaved_state() {
        // The op *count* is not a document identity. Circularise (one op on the
        // path), undo (none), reverse-complement (one again) — and the old
        // `ops == autosaved_at_ops` gate returned on every frame from then on,
        // so the recovery file kept the abandoned circular branch while the
        // reverse-complemented molecule on screen was never written. The
        // Recover banner showed a matching op count, so it looked right.
        const SEQ: &str = "AAAACCCCGGGGTTTTAAGG";
        let (mut app, path) = app_with_recovery("fork");
        app.bench.set(edited_doc("x.fa", SEQ));
        app.autosave(false);
        assert_eq!(
            autosaved(&path).0.topology,
            pl_core::Topology::Circular,
            "the premise: the first edit was written"
        );

        let d = app.bench.get_mut().unwrap();
        d.undo().unwrap();
        d.apply(pl_core::OpKind::ReverseComplement).unwrap();
        assert_eq!(d.log.path().len(), 1, "the collision this test is about");
        // The thirty-second throttle is a separate question. Clear it so this
        // is about identity and nothing else.
        app.last_autosave = None;
        app.autosave(false);

        let (mol, _) = autosaved(&path);
        assert_eq!(
            mol.seq.to_ascii_uppercase(),
            pl_core::reverse_complement(SEQ.as_bytes()),
            "the molecule on screen, not the branch that was abandoned"
        );
        assert_eq!(mol.topology, pl_core::Topology::Linear);
    }

    #[test]
    fn opening_a_second_document_does_not_inherit_the_first_ones_autosave_state() {
        // Both documents are circularised from their base, and the log is
        // content-addressed, so both cursors are the *same* OpId — which is
        // why the identity carries the title and path as well. Before, editing
        // A once and then B once left the single recovery file holding A's
        // molecule under A's title, and B's work was never written at all.
        let (mut app, path) = app_with_recovery("swap");
        app.bench.set(edited_doc("a.fa", "AAAACCCCGGGGTTTTAAGG"));
        app.autosave(false);
        assert_eq!(autosaved(&path).1, "a.fa", "the premise");

        app.bench.set(edited_doc("b.fa", "GGGGGGGGTTTTTTTTAACC"));
        app.last_autosave = None;
        app.autosave(false);

        let (mol, title) = autosaved(&path);
        assert_eq!(title, "b.fa", "the recovery file follows the open document");
        assert_eq!(
            mol.seq.to_ascii_uppercase(),
            b"GGGGGGGGTTTTTTTTAACC".to_vec()
        );
    }

    #[test]
    fn an_idle_window_does_not_rewrite_the_same_bytes() {
        // The control, and the reason the gate exists: `autosave` runs on every
        // frame. Nothing changed, so nothing may be written.
        let (mut app, path) = app_with_recovery("idle");
        app.bench.set(edited_doc("x.fa", "AAAACCCCGGGGTTTTAAGG"));
        app.autosave(false);
        assert!(path.exists());
        std::fs::remove_file(&path).unwrap();
        app.last_autosave = None;
        for _ in 0..100 {
            app.autosave(false);
        }
        assert!(!path.exists(), "an unchanged document was rewritten");
    }

    #[test]
    fn merely_opening_another_file_does_not_discard_an_unsaved_draft() {
        // The other half of "an unedited document has nothing to protect". A
        // file that has only been *looked at* must not overwrite somebody's
        // unsaved edits, however stale the identity check thinks they are.
        let (mut app, path) = app_with_recovery("browse");
        app.bench.set(edited_doc("a.fa", "AAAACCCCGGGGTTTTAAGG"));
        app.autosave(false);

        // `Some(path)`, and the path is what the case is about: "only looked
        // at" means read FROM a file. A pathless document is unsaved by
        // definition and now IS autosaved — see
        // `a_restored_draft_with_no_path_is_autosaved`.
        app.bench.set(
            Document::from_bytes(b">b\nTTTTTTTT\n", "b.fa".into(), Some("b.fa".into())).unwrap(),
        );
        app.last_autosave = None;
        for _ in 0..10 {
            app.autosave(false);
        }
        assert_eq!(autosaved(&path).1, "a.fa", "the draft was thrown away");
    }

    #[test]
    fn undoing_back_to_the_base_leaves_the_recovery_file_showing_the_base() {
        // The case the "unedited" gate must not swallow: this is the same
        // document, and the base really is what is on screen, so the recovery
        // file must stop offering the branch the user has just stepped off.
        const SEQ: &str = "AAAACCCCGGGGTTTTAAGG";
        let (mut app, path) = app_with_recovery("rewound");
        app.bench.set(edited_doc("x.fa", SEQ));
        app.autosave(false);
        assert_eq!(autosaved(&path).0.topology, pl_core::Topology::Circular);

        app.bench.get_mut().unwrap().undo().unwrap();
        app.last_autosave = None;
        app.autosave(false);
        assert_eq!(autosaved(&path).0.topology, pl_core::Topology::Linear);
    }

    #[test]
    fn an_unedited_document_is_not_autosaved_at_all() {
        // The other control. The user's own file already holds this, and a
        // recovery file that exists is this program's only record of an
        // unclean exit.
        let (mut app, path) = app_with_recovery("unedited");
        app.bench.set(
            Document::from_bytes(b">x\nAAAACCCCGGGG\n", "x.fa".into(), Some("x.fa".into()))
                .unwrap(),
        );
        for _ in 0..10 {
            app.autosave(false);
        }
        assert!(!path.exists(), "nothing had been edited");
    }

    // -----------------------------------------------------------------------
    // The map's disclosure, under a filter that is not `All`
    // -----------------------------------------------------------------------

    /// A digest shaped like the user's own pKoV: 40 cutters, of which 22 cut
    /// once, 12 cut twice and 6 cut more than twice.
    ///
    /// `pkov_cutters` gives every enzyme ONE position, so under it `dual` and
    /// `multi` are both zero and no assertion about either can fail. That, plus
    /// four call sites all passing `EnzymeSet::All`, is why the disclosure
    /// regression below survived 1,336 tests.
    fn pkov_mixed_digest() -> Vec<pl_enzymes::Digest> {
        let mut out: Vec<pl_enzymes::Digest> = pkov_cutters();
        assert_eq!(out.len(), 22, "the unique half");
        let taken: Vec<&str> = out.iter().map(|d| d.enzyme.name).collect();
        let rest: Vec<&'static pl_enzymes::Enzyme> = pl_enzymes::ENZYMES
            .iter()
            .filter(|e| !taken.contains(&e.name))
            .collect();
        assert!(rest.len() >= 18, "the table has only {} spare", rest.len());
        for (i, e) in rest.into_iter().take(18).enumerate() {
            let n = if i < 12 { 2 } else { 3 };
            out.push(pl_enzymes::Digest {
                enzyme: e,
                positions: (0..n).map(|k| 100 + 137 * (i as u64) + 40 * k).collect(),
            });
        }
        out
    }

    /// PROVEN TO FAIL against the working tree as handed over: two features
    /// whose names truncate to the same string drew two identical labels with
    /// nothing on the picture to tell them apart. On stock pET28a at the pane
    /// its long `/label` forces, the map drew "T7 ..." twice — T7 promoter and
    /// T7 terminator, which sit at opposite ends of the insert and are the two a
    /// cloner most needs to distinguish — while the exported SVG kept both names
    /// in full. The screen was the worse of the two.
    ///
    /// The names here differ only at character 45, so the swatch's two
    /// characters cannot separate them and the second half of the rule is what
    /// is exercised: whatever still collides is COUNTED, in the note, rather
    /// than left for the reader to notice.
    #[test]
    fn two_features_never_quietly_draw_the_same_label() {
        let cutters = pkov_cutters();
        for file in ["pET28a", "pUC19"] {
            let mut mol = plasmid(file);
            mol.features[4].name = "T7 transcription regulatory element, initiation".into();
            mol.features[5].name = "T7 transcription regulatory element, termination".into();
            // The two panes where the note still has room for the clause. At
            // 440 pt and below it falls to `tiny()`, which names nothing at all,
            // and that is a width-tier question and not this one.
            for (w, h) in [(706.0f32, 756.0f32), (560.0, 900.0)] {
                let (shapes, _) = paint_map(&mol, file, &cutters, w, h);
                let drawn: Vec<String> = texts_in(&shapes, 10.0, egui::FontFamily::Monospace)
                    .into_iter()
                    .map(|(t, _)| t)
                    .filter(|t| t.starts_with("T7 trans"))
                    .collect();
                assert_eq!(
                    drawn.len(),
                    2,
                    "{file} {w}x{h}: the premise — both names must reach the ring: {drawn:?}"
                );
                let note = texts_in(&shapes, 10.0, egui::FontFamily::Proportional)
                    .into_iter()
                    .map(|(t, _)| t)
                    .collect::<Vec<_>>()
                    .join(" / ");
                if drawn[0] == drawn[1] {
                    assert!(
                        note.contains("2 alike"),
                        "{file} {w}x{h}: {drawn:?} are one string and the map says nothing: \
                         {note:?}"
                    );
                } else {
                    assert!(
                        !note.contains("alike"),
                        "{file} {w}x{h}: {drawn:?} differ and the note claims otherwise: {note:?}"
                    );
                }
            }
        }
    }

    /// PROVEN TO FAIL against the working tree as handed over, on `Unique`,
    /// `Unique & dual` and `Unique 6+ base`: `map::show` folded every
    /// filter-excluded enzyme into `dual` so that `Disclosure::closes()` would
    /// still close, and it closed while lying. On the user's own pKoV — 22
    /// unique, 12 dual, 6 multi — the note read "18 dual, 0 multi not drawn"
    /// while the picture did not change by a single pixel.
    ///
    /// "0 multi not drawn" reads to a plasmid biologist as "nothing else cuts
    /// this more than twice", which is the class of hidden-cut misinformation
    /// `docs/PLAN.md` item 33 is written about, arriving through the very
    /// sentence that exists to prevent it.
    #[test]
    fn the_map_note_reports_the_molecules_own_cutter_classes_under_every_filter() {
        let mol = pkov();
        let digest = pkov_mixed_digest();
        assert_eq!(digest.iter().filter(|d| d.count() > 0).count(), 40);
        assert_eq!(digest.iter().filter(|d| d.count() == 2).count(), 12);
        assert_eq!(digest.iter().filter(|d| d.count() > 2).count(), 6);

        for set in pl_enzymes::EnzymeSet::ALL {
            let (shapes, _) = paint_map_with(&mol, "pKoV", &digest, 1100.0, 900.0, None, set);
            let note: String = texts_in(&shapes, 10.0, egui::FontFamily::Proportional)
                .into_iter()
                .map(|(t, _)| t)
                .collect::<Vec<_>>()
                .join(" / ");
            assert!(
                note.contains("12 dual, 6 multi not drawn"),
                "{:?}: the note must state the molecule's own classes, not the filter's \
                 leftovers: {note:?}",
                set.label()
            );
        }
    }

    /// PROVEN TO FAIL against the working tree as handed over: appending the
    /// feature clause pushed the enzyme sentence off its `long()` tier, so a
    /// dense molecule that printed "0 of 58 cutters labelled · 1 dual, 57 multi
    /// not drawn" before this work printed "0/58 cutters · 62 of 9000 names"
    /// after it. The clause naming what the map hid was silenced to make room
    /// for the clause naming what the map hid.
    #[test]
    fn a_dense_molecule_keeps_both_disclosures() {
        let mut mol = pkov();
        // More features than the label budget, so the feature clause is
        // non-trivial and the trade is forced.
        mol.features.clear();
        let span = mol.seq.len() as u64;
        for i in 0..300u64 {
            let mut f = pl_core::Feature::new(format!("feature {i}"), "misc_feature");
            let start = 1 + (span - 200) * i / 300;
            f.segments = vec![pl_core::Segment::new(start, start + 150)];
            mol.features.push(f);
        }
        // 780x770 is the pane where the trade is forced and only there: the
        // enzyme sentence's long form fits alone, the feature clause fits alone,
        // and the two do not fit side by side. On a wider pane both fit on one
        // line and the old code was right by accident; on the shipped 706x756
        // the long form does not fit at all and the tiers decide, which is a
        // different question. Measured, then chosen.
        let (shapes, _) = paint_map(&mol, "dense", &pkov_mixed_digest(), 780.0, 770.0);
        let note: Vec<String> = texts_in(&shapes, 10.0, egui::FontFamily::Proportional)
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        let all = note.join(" / ");
        assert!(
            all.contains("dual") && all.contains("multi"),
            "the enzyme disclosure lost the clause naming what it hid: {note:?}"
        );
        assert!(
            all.contains("names"),
            "the feature disclosure is missing: {note:?}"
        );
        assert!(
            note.len() <= 2,
            "the note may take a second line and no more: {note:?}"
        );
    }

    /// PROVEN TO FAIL against the working tree as handed over: the budget ranked
    /// by span alone with no notion of where a feature is, so on a molecule
    /// whose features are all one length and in coordinate order the survivors
    /// were a contiguous index block. The map drew 62 names from 0.72% of a
    /// 200 kb molecule as one column of near-parallel leaders and left the other
    /// 99.3% of the ring unnamed, which reads as a statement about the molecule
    /// and is not one.
    #[test]
    fn the_label_budget_spreads_round_the_ring() {
        let mut mol = pkov();
        mol.features.clear();
        let span = mol.seq.len() as u64;
        let n = 3_000u64;
        for i in 0..n {
            // Every span identical, in coordinate order: the degenerate input.
            let mut f = pl_core::Feature::new(format!("f{i:04}"), "misc_feature");
            let start = 1 + (span - 20) * i / n;
            f.segments = vec![pl_core::Segment::new(start, start + 12)];
            mol.features.push(f);
        }
        let (shapes, _) = paint_map(&mol, "many", &[], 1100.0, 900.0);
        let drawn: Vec<u64> = texts_in(&shapes, 10.0, egui::FontFamily::Monospace)
            .into_iter()
            .filter_map(|(t, _)| {
                t.strip_prefix('f')
                    .filter(|d| d.len() == 4)
                    .and_then(|d| d.parse::<u64>().ok())
            })
            .collect();
        assert!(
            drawn.len() >= 20,
            "the premise: names must reach the ring at all ({} drawn)",
            drawn.len()
        );
        assert!(
            drawn.len() < n as usize,
            "the premise: the budget must actually bind"
        );
        // Indices run with the coordinate, so an index bucket IS an angular
        // sector. Min-to-max is NOT the measure — one stray survivor on the far
        // side makes a contiguous block look like a spread — so this counts how
        // many eighths of the ring carry a name at all.
        let mut sectors = [false; 8];
        for i in &drawn {
            sectors[(i * 8 / n).min(7) as usize] = true;
        }
        let hit = sectors.iter().filter(|x| **x).count();
        assert!(
            hit >= 6,
            "the {} chosen names occupy {hit} of 8 sectors: {drawn:?}",
            drawn.len()
        );
    }

    /// What naming the features costs the ring, measured against a baseline
    /// where the names are one character each, on all five plasmids.
    ///
    /// `map.rs` said this cost was zero — "the screen … keeps the radius" — and
    /// it is not: measured on the user's own pKoV it is 4.9%, and on pUC19,
    /// whose "CAP binding site" is exactly the 16-character cap, 8.3%.
    /// `a_long_feature_name_does_not_shrink_the_ring` cannot see any of it,
    /// because both of its measurements are taken from molecules that already
    /// carry feature names: it compares 33 characters against 12 and never
    /// against none.
    ///
    /// The bound is what the cap buys. Past 16 characters more name is free, so
    /// the whole feature contribution to the reserve is bounded however the file
    /// is annotated — which is the property that makes the trade acceptable, and
    /// the one worth pinning.
    #[test]
    fn feature_names_cost_the_ring_no_more_than_the_cap() {
        let cutters = pkov_cutters();
        for file in ["pKoV .dna", "pkov.gb", "pET28a", "pACYC184", "pUC19"] {
            let mol = if file == "pKoV .dna" {
                pkov()
            } else {
                plasmid(file)
            };
            // Same features, same lanes, same everything except the widths the
            // reserve is computed from.
            let mut tiny = mol.clone();
            for f in &mut tiny.features {
                f.name = "x".into();
            }
            for (w, h) in [(706.0f32, 756.0f32), (880.0, 620.0), (560.0, 900.0)] {
                let r_of =
                    |m: &pl_core::Molecule| backbone(&paint_map(m, file, &cutters, w, h).0).1;
                let (base, named) = (r_of(&tiny), r_of(&mol));
                assert!(
                    named >= base * 0.85,
                    "{file} {w}x{h}: naming the features took the ring from {base} to {named}"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // .dna out: the DESTINATION, not only the source
    // -----------------------------------------------------------------------

    /// PROVEN TO FAIL against the working tree as handed over, and it is the
    /// worst defect that tree had: the lossiness gate was computed from the OPEN
    /// document's container, so a GenBank molecule saved over an existing `.dna`
    /// took the fast path. Driven live, that turned a 17-block 75,795 B file
    /// carrying a cloning history tree, five history nodes and nine typed
    /// primers into a 4-block 14,928 B one, with no modal, and a status line
    /// naming the one thing that was NOT lost — the regenerable cache.
    #[test]
    fn writing_over_someone_elses_dna_says_what_it_destroys() {
        let dir = std::env::temp_dir().join(format!("pl-gui-dest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let victim = dir.join("victim.dna");

        // A destination shaped like a real SnapGene file: a valid container
        // carrying the blocks a rewrite cannot carry.
        let mut rich =
            pl_fileio::snapgene::read_blocks(&pl_fileio::snapgene::from_molecule(&pkov())).unwrap();
        for (kind, n) in [
            (7u8, 8_107usize),
            (11, 7_720),
            (11, 2_199),
            (8, 295),
            (13, 345),
        ] {
            rich.push(pl_fileio::snapgene::Block {
                kind,
                payload: vec![b'x'; n],
            });
        }
        std::fs::write(&victim, pl_fileio::snapgene::write_blocks(&rich)).unwrap();

        // And a document that came from GenBank, so its own container is None
        // and its own report is empty: the case that took the fast path.
        let mut app = App::blank();
        let gb = pl_fileio::genbank::write(&pkov(), "plain", today());
        app.bench
            .set(Document::from_bytes(gb.as_bytes(), "plain.gb".into(), None).unwrap());
        assert!(
            app.document().unwrap().container.is_none(),
            "the premise: nothing about the source says a .dna is at risk"
        );

        let pending = app.plan_dna(victim.clone(), None).expect("a plan");
        assert!(
            pending.asks(),
            "replacing a .dna holding a cloning history must raise the question"
        );
        let losing = pending.losing();
        let kinds: Vec<u8> = losing.iter().map(|d| d.kind).collect();
        for k in [7u8, 11, 8, 13] {
            assert!(
                kinds.contains(&k),
                "block {k} is destroyed and unnamed: {kinds:?}"
            );
        }
        assert!(
            pending.history,
            "the destination's cloning history is replaced with none, and must be said"
        );
        let said = pl_fileio::snapgene::dropped_summary(&losing).unwrap();
        assert!(
            said.contains("cloning history tree") && said.contains("2 × a cloning history node"),
            "the sentence must count the nodes: {said:?}"
        );
        // And nothing here may be described as rebuildable.
        assert!(losing.iter().all(|d| !d.derived), "{losing:?}");

        let _ = std::fs::remove_file(&victim);
    }

    /// The control, and the one that must stay cheap: a molecule with nothing
    /// behind it, written where there is nothing, asks nothing.
    #[test]
    fn an_ordinary_dna_save_to_an_empty_path_asks_nothing() {
        let dir = std::env::temp_dir().join(format!("pl-gui-dest2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fresh = dir.join("fresh.dna");
        let _ = std::fs::remove_file(&fresh);

        let mut app = App::blank();
        let gb = pl_fileio::genbank::write(&pkov(), "plain", today());
        app.bench
            .set(Document::from_bytes(gb.as_bytes(), "plain.gb".into(), None).unwrap());
        let pending = app.plan_dna(fresh, None).expect("a plan");
        assert!(!pending.asks(), "{:?}", pending.losing());
    }

    // -----------------------------------------------------------------------
    // The guard: what it does with an answer, and what it stops meanwhile
    // -----------------------------------------------------------------------

    /// PROVEN TO FAIL against the working tree as handed over: `resolve_guard`
    /// set `abandoned_unsaved` on BOTH answers, so a user who clicked "Save as
    /// GenBank…", watched the file appear on disk and let the app exit was
    /// greeted on every subsequent launch by "You closed Polylinker with unsaved
    /// changes", naming work that is saved. Crying wolf on the one answer the
    /// guard exists to encourage is worse than not asking at all.
    #[test]
    fn answering_the_guard_by_saving_leaves_no_abandoned_draft() {
        let (mut app, recovery) = app_with_recovery("saved-answer");
        app.bench.set(edited_doc("x.fa", "AAAACCCCGGGGTTTTAAGG"));
        app.closing = true;
        app.resolve_guard(Losing::Close, true);
        assert!(
            !app.abandoned_unsaved,
            "a saved document was recorded as abandoned"
        );
        assert!(
            !recovery.exists(),
            "a recovery draft was kept for work that is in a file"
        );
        assert!(app.close_now, "and the window still closes");
    }

    /// PROVEN TO FAIL against the working tree as handed over: the guard latched
    /// `closing` and never re-read the predicate, so undoing back to the opening
    /// state under the dialog left it up — changing its own sentence to "has not
    /// been saved to a file" — and answering it left a 0-edit recovery draft of
    /// an untouched file plus a false "did not close cleanly" on the next launch.
    #[test]
    fn the_guard_stands_down_when_the_document_stops_being_at_risk() {
        let ctx = test_ctx();
        let (mut app, recovery) = app_with_recovery("undone");
        // FROM A FILE, not `edited_doc`: a document that has never been written
        // anywhere is unsaved at its own base and correctly stays so, which is a
        // different case and not this one.
        let file = temp_file("undone", "fa", PLASMID_A);
        app.load(file.clone());
        app.bench
            .get_mut()
            .unwrap()
            .apply(pl_core::OpKind::SetTopology(pl_core::Topology::Circular))
            .unwrap();
        assert!(app.document().unwrap().unsaved(), "the premise");
        app.closing = true;
        // The user undoes back to the base while the question is on screen.
        app.bench.get_mut().unwrap().undo().unwrap();
        assert!(
            !app.document().unwrap().unsaved(),
            "the premise: undoing to the opening state is clean"
        );
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let c = ui.ctx().clone();
            app.unsaved_modal(&c);
        });
        assert!(!app.closing, "the question answered itself");
        assert!(app.close_now, "and the close the user asked for happened");
        assert!(
            !app.abandoned_unsaved && !recovery.exists(),
            "nothing was at risk, so nothing may be recorded as abandoned"
        );
        let _ = std::fs::remove_file(&file);
    }

    /// PROVEN TO FAIL against the working tree as handed over: `global_shortcuts`
    /// stood the document down only for the paste question, so Ctrl+Z, Ctrl+S,
    /// Ctrl+O and every printable key stayed live underneath the unsaved-changes
    /// guard and the `.dna` lossiness modal. Driven live, eight bases typed with
    /// the close guard on screen took the molecule from 8,120 to 8,128 bp and
    /// the dialog's own sentence from "1 edit" to "3 edits" while it was being
    /// read.
    ///
    /// `egui::Modal` blocks widget interaction; it does not block raw
    /// `ctx.input` reads, and both key handlers run before a widget exists.
    /// PROVEN TO FAIL against the shipped handler: plain Ctrl+C copied the
    /// REVERSE COMPLEMENT whenever Shift was still down.
    ///
    /// `egui::Event::Copy` carries no modifiers, so the handler read
    /// `modifiers.shift` off the frame — and egui-winit's `is_copy_command` is
    /// `modifiers.command && key == C`, which fires for Ctrl+Shift+C too and
    /// then RETURNS, so no `Key::C` event exists to tell the two apart. Shift
    /// is exactly where this app's own selection idiom leaves the user's hand:
    /// `Key::ArrowRight` with `shift` is how you select. Select with
    /// Shift+Right, press Ctrl+C without letting go, and the clipboard silently
    /// held the other strand — plausible DNA, wrong bases.
    #[test]
    fn ctrl_c_copies_the_selection_even_with_shift_still_held() {
        let seq = "CTAAGCCTTTGGGGCCCC";
        let mut app = App::blank();
        app.bench.set(
            Document::from_bytes(format!(">x\n{seq}\n").as_bytes(), "x.fa".into(), None).unwrap(),
        );
        app.tab = Tab::Sequence;
        app.edit.sel = Some(seqedit::Selection {
            anchor: 0,
            head: 10,
            through_origin: false,
        });
        let want: String = seq[..10].to_string();
        let rc = pl_core::reverse_complement(want.as_bytes());
        let rc = String::from_utf8(rc).expect("ASCII");
        assert_ne!(want, rc, "the fixture must be able to tell them apart");

        // The gesture, exactly as winit delivers it: `Event::Copy`, with the
        // frame's modifiers still carrying Shift from the selection.
        let shifted = egui::Modifiers {
            shift: true,
            command: true,
            ctrl: true,
            ..Default::default()
        };
        let copied = |app: &mut App, modifiers: egui::Modifiers, events: Vec<egui::Event>| {
            let ctx = test_ctx();
            let out = ctx.run_ui(
                egui::RawInput {
                    events,
                    modifiers,
                    ..Default::default()
                },
                |ui| app.sequence_keys(ui, 1.0),
            );
            out.platform_output
                .commands
                .iter()
                .find_map(|c| match c {
                    egui::OutputCommand::CopyText(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_default()
        };
        assert_eq!(
            copied(&mut app, shifted, vec![egui::Event::Copy]),
            want,
            "Ctrl+C with Shift held gave the other strand"
        );
        // And with Shift released, which used to be the only safe way to copy.
        assert_eq!(
            copied(&mut app, egui::Modifiers::COMMAND, vec![egui::Event::Copy]),
            want
        );

        // The reverse complement still has a way in — Ctrl+Shift+R, which
        // egui-winit does not swallow, so it arrives as a real key event.
        let r = vec![egui::Event::Key {
            key: egui::Key::R,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: shifted,
        }];
        assert_eq!(copied(&mut app, shifted, r), rc);
        // ...and the chord's other half does nothing on its own.
        let plain_r = vec![egui::Event::Key {
            key: egui::Key::R,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }];
        assert_eq!(
            copied(&mut app, egui::Modifiers::COMMAND, plain_r),
            String::new()
        );
    }

    /// PROVEN TO FAIL against the loop this replaces, on both halves.
    ///
    /// It opened every argument as a path, so `polylinker --help` presented a
    /// file called `--help` as unreadable; and `load_failed` assigns
    /// `self.notice`, so three arguments of which two fail reported only the
    /// last. Neither was reachable before this work, because only `argv[1]` was
    /// ever opened — which is exactly why the new loop has to answer for them.
    #[test]
    fn the_command_line_skips_flags_and_reports_every_file_that_failed() {
        use std::ffi::OsString;
        let good = temp_file("argv-good", "fa", PLASMID_A);
        let bad1 = temp_file("argv-bad1", "txt", "this is not a sequence file at all");
        let bad2 = temp_file("argv-bad2", "gb", "neither is this");

        let mut app = App::blank();
        app.open_argv([
            OsString::from("--help"),
            OsString::from(&good),
            OsString::from(&bad1),
            OsString::from(&bad2),
        ]);
        // The flag never became a file: nothing in the notice mentions it, and
        // the good molecule is open.
        assert!(app.document().is_some(), "{:?}", app.error);
        assert!(app.error.is_none(), "the screen was not taken over");
        let notice = app.notice.clone().unwrap_or_default();
        assert!(!notice.contains("--help"), "{notice}");
        assert!(
            app.status.contains("takes file names, not flags"),
            "{}",
            app.status
        );

        // BOTH failures, not just the last, and each names its own file.
        assert!(
            notice.starts_with("2 file(s) could not be opened"),
            "{notice}"
        );
        for f in [&bad1, &bad2] {
            let name = f.file_name().expect("a name").to_string_lossy().to_string();
            assert!(notice.contains(&name), "{name} is missing from {notice}");
        }

        // `--` ends the flags, so a file really called `-x` can still be named.
        let mut app = App::blank();
        app.open_argv([OsString::from("--"), OsString::from(&good)]);
        assert!(app.document().is_some());
        assert!(app.notice.is_none(), "{:?}", app.notice);

        // And ONE failure reads as itself rather than as a list of one.
        let mut app = App::blank();
        app.open_argv([OsString::from(&bad1)]);
        let said = app.error.clone().unwrap_or_default();
        assert!(!said.contains("1 file(s)"), "{said}");
        assert!(said.contains("unrecognised format"), "{said}");

        for f in [good, bad1, bad2] {
            let _ = std::fs::remove_file(f);
        }
    }

    #[test]
    fn no_shortcut_reaches_the_document_while_a_question_is_on_screen() {
        let ctx = test_ctx();
        let mut app = App::blank();
        assert!(!app.asking(), "the control");
        let press = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            modifiers: egui::Modifiers::COMMAND,
            ..Default::default()
        };
        // Every gesture that raises a question about the document, and the
        // control beside it.
        let dna = || PendingDna {
            path: PathBuf::from("x.dna"),
            bytes: Vec::new(),
            unwritable: Vec::new(),
            history: false,
            notes: Vec::new(),
            overwriting_source: false,
            dest_lost: Vec::new(),
            source_lost: Vec::new(),
            then: None,
        };
        let mut fired: Vec<(&str, bool)> = Vec::new();
        let _ = ctx.run_ui(press, |ui| {
            let c = ui.ctx().clone();
            fired.push(("nothing asked", app.global_shortcuts(&c).undo));
            app.closing = true;
            fired.push(("the close guard", app.global_shortcuts(&c).undo));
            app.closing = false;
            app.pending_dna = Some(dna());
            fired.push(("the .dna question", app.global_shortcuts(&c).undo));
            app.pending_dna = None;
        });
        assert_eq!(
            fired,
            vec![
                ("nothing asked", true),
                ("the close guard", false),
                ("the .dna question", false),
            ],
            "a shortcut reached the document from under a question"
        );
    }

    /// PROVEN TO FAIL against the working tree as handed over: `resolve_guard`
    /// forced the final autosave by clearing `self.autosaved`, which also
    /// destroyed the `same_document` escape hatch the base-cursor guard reads —
    /// so for a document sitting at its own base the forced write returned
    /// before writing anything, the `exit: unsaved` flag never reached the file,
    /// and the next launch greeted a deliberate quit with "A previous session
    /// did not close cleanly".
    #[test]
    fn the_forced_final_autosave_writes_even_at_the_base_of_the_log() {
        let (mut app, path) = app_with_recovery("forced-base");
        app.bench.set(edited_doc("x.fa", "AAAACCCCGGGGTTTTAAGG"));
        app.autosave(false);
        assert!(path.exists(), "the premise");
        app.bench.get_mut().unwrap().undo().unwrap();
        assert!(
            app.document().unwrap().log.cursor().is_none(),
            "the premise: the cursor is at the base"
        );
        app.resolve_guard(Losing::Close, false);
        let snap = recover::decode(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(
            snap.abandoned,
            "the deliberate quit did not reach the file, so the next launch calls it a crash"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The recovery banner's one decision-support line must not point at the
    /// worse copy.
    ///
    /// PROVEN TO FAIL against the working tree as handed over: a 0-edit draft of
    /// an untouched `.dna` was advertised as "newer than the file on disk" —
    /// true of the two mtimes, false of the two contents — and taking it costs
    /// the container, the typed primers and the methylation flags.
    #[test]
    fn a_draft_holding_no_edits_is_not_advertised_as_newer() {
        let zero = draft_age(2_000, 2_000, Some(1_000), true, 0);
        assert!(
            !zero.contains("newer"),
            "a 0-edit draft was called newer: {zero:?}"
        );
        assert!(zero.contains("holds no edits"), "{zero:?}");
        // With edits in it the comparison is still made, in the file's own terms
        // rather than in a word that reads as "better".
        let some = draft_age(2_000, 2_000, Some(1_000), true, 3);
        assert!(some.contains("written after the file"), "{some:?}");
        assert!(!some.contains("newer than"), "{some:?}");
        // The other direction is unchanged.
        assert!(draft_age(1_000, 2_000, Some(3_000), true, 3).contains("on disk is newer"));
    }

    /// A second window must not be able to delete the first window's LIVE draft.
    ///
    /// PROVEN TO FAIL against the working tree as handed over: `stale` returns
    /// every `*.recover` that is not this process's and never asks whether the
    /// process that owns it is running — there is no lock file and no PID probe
    /// anywhere in the module. So a draft written seconds ago by a window that
    /// is still open was listed under "A previous session did not close cleanly"
    /// and its Discard permanently removed it. Two clicks in the second window
    /// destroyed the first window's only crash copy while its user was typing
    /// into the document it holds.
    ///
    /// **Driven through the banner, and both halves clicked.** Asserting only
    /// that the live draft survives would pass against a click that missed the
    /// button entirely, so the crashed draft in the same list is clicked the
    /// same way, at coordinates found the same way, and must be gone. One file
    /// survives and one does not, from the same gesture — that is the whole
    /// claim, and neither half can be true by accident.
    #[test]
    fn a_draft_another_window_is_still_writing_cannot_be_discarded() {
        let ctx = test_ctx();
        let dir = std::env::temp_dir().join(format!("pl-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp directory");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap();
        let write = |name: &str, saved_at: u64| {
            let p = dir.join(format!("{name}.recover"));
            let s = recover::Snapshot {
                original: None,
                title: name.into(),
                saved_at,
                ops: 3,
                abandoned: false,
                genbank: "LOCUS x 4 bp DNA linear UNK\nORIGIN\n 1 acgt\n//\n".into(),
            };
            std::fs::write(&p, recover::encode(&s)).unwrap();
            p
        };
        // One draft written a moment ago — the running window's — and one from a
        // session that really did die, an hour back. `stale` sorts newest first,
        // so the live one is row 0 either way and the coordinates below name the
        // same row before and after the fix.
        let live = write("still-typing", now.saturating_sub(5));
        let dead = write("really-crashed", now.saturating_sub(3_600));
        let mut app = App::blank();
        app.stale = recover::stale(&dir);
        assert_eq!(
            app.stale.len(),
            2,
            "the premise: two drafts to choose between"
        );

        let frame = |app: &mut App, input: egui::RawInput| {
            ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.recovery_banner(ui));
            })
        };
        // The nth Discard button's centre, measured off the galley actually
        // painted rather than computed from a layout this test does not own.
        let spot = |out: &egui::FullOutput, n: usize| -> egui::Pos2 {
            let hits: Vec<egui::Pos2> = flat_shapes(&out.shapes)
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Text(t) if t.galley.text().starts_with("Discard") => {
                        Some(egui::Rect::from_min_size(t.pos, t.galley.size()).center())
                    }
                    _ => None,
                })
                .collect();
            assert!(
                hits.len() > n,
                "only {} Discard button(s) drawn",
                hits.len()
            );
            hits[n]
        };
        // Two clicks, which is everything the banner offers: one to arm, one to
        // confirm. The armed label is wider and this layout is right-aligned, so
        // it grows leftwards and the same point stays inside it.
        let two_clicks = |app: &mut App, at: egui::Pos2| {
            for _ in 0..2 {
                frame(app, pointer_to(at));
                frame(app, pointer_button(at, true));
                frame(app, pointer_button(at, false));
            }
        };

        let out = frame(&mut app, window());
        two_clicks(&mut app, spot(&out, 0));
        assert!(
            live.exists(),
            "a second window deleted the draft a running window is still writing"
        );

        // ...and the same gesture on the row below it, so the one above cannot
        // have survived by missing.
        let out = frame(&mut app, window());
        two_clicks(&mut app, spot(&out, 1));
        assert!(
            !dead.exists(),
            "the click never reached a Discard button, so the assertion above proves nothing"
        );

        let _ = std::fs::remove_file(&live);
        let _ = std::fs::remove_file(&dead);
    }

    /// The dialog must agree with itself about number. It did not: "1 edit that
    /// is not in any file." was followed by "Closing Polylinker discards them."
    #[test]
    fn the_guards_two_sentences_agree_about_number() {
        assert!(Losing::Close.consequence(true).contains("discards it."));
        assert!(Losing::Close.consequence(false).contains("discards them."));
    }

    /// PROVEN TO FAIL against the working tree as handed over: `figure_options`
    /// hardcoded `single: 0` under a comment saying the site filter "never turns
    /// a single cutter away", which stopped being true when this work
    /// intersected that filter with the user's enzyme set. It stayed harmless
    /// only because every enzyme in the shipped table has a 6-base or longer
    /// site — and `debug_assert!` is compiled OUT of the release binary, so the
    /// exported SVG, PDF and EPS would carry a note whose numbers do not add up.
    #[test]
    fn the_exported_figures_note_accounts_for_what_the_filter_turned_away() {
        // A four-base cutter the shipped table does not have, so the 6+ sets
        // genuinely discriminate. The whole defect is invisible without one.
        const SHORT: pl_enzymes::Enzyme = pl_enzymes::Enzyme {
            name: "AluI",
            site: "AGCT",
            fst5: 2,
            ovhg: 0,
        };
        let mut results = pkov_mixed_digest();
        results.push(pl_enzymes::Digest {
            enzyme: &SHORT,
            positions: vec![4_242],
        });
        let told = App::figure_disclosure(&results, pl_enzymes::EnzymeSet::UniqueSixPlus);
        assert_eq!(
            told.single, 1,
            "the four-base unique cutter the filter turned away is in no bucket"
        );
        assert_eq!(told.dual, 12, "a fact about the molecule, not the filter");
        assert_eq!(told.multi, 6);
        // `labelled` is filled by the layout pass; the ring names all 22 that
        // survive the filter, and with that the five buckets have to close.
        let mut told = told;
        told.labelled = 22;
        assert!(told.closes(), "{told:?} does not account for every cutter");
    }

    // -----------------------------------------------------------------------
    // The document lifecycle: dirty state, the guard, and .dna out
    // -----------------------------------------------------------------------

    /// A file on disk holding `text`, cleaned up by the caller.
    fn temp_file(tag: &str, ext: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pl-gui-lc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp directory");
        let p = dir.join(format!("{tag}.{ext}"));
        std::fs::write(&p, text).expect("a writable temp file");
        p
    }

    const PLASMID_A: &str = ">a\nAAAACCCCGGGGTTTTAAGGCCTTAAAACCCCGGGGTTTT\n";
    const PLASMID_B: &str = ">b\nTTTTGGGGCCCCAAAATTGGCCAATTTTGGGGCCCCAAAA\n";

    /// An app with `PLASMID_A` open, read from a real file so it starts clean.
    fn app_with_a(tag: &str) -> (App, PathBuf) {
        let path = temp_file(tag, "fa", PLASMID_A);
        let mut app = App::blank();
        app.load(path.clone());
        assert!(app.document().is_some(), "the premise: {tag} opened");
        (app, path)
    }

    /// PROVEN TO FAIL at 528dcd9, where `Document` had no notion of a write at
    /// all and the only predicate — `edited()` — was
    /// `!log.all_ops().is_empty()`: true forever after the first keystroke,
    /// including after an undo back to the base. A guard on that fires when
    /// nothing has changed, which is exactly how a guard becomes a reflex click.
    #[test]
    fn undoing_back_to_the_opening_state_is_not_unsaved() {
        let (mut app, path) = app_with_a("undo-clean");
        let d = app.bench.get_mut().unwrap();
        assert!(!d.unsaved(), "a file just opened is on disk, by definition");

        d.apply(pl_core::OpKind::SetTopology(pl_core::Topology::Circular))
            .unwrap();
        assert!(d.unsaved(), "and an edit is not");
        assert_eq!(d.unsaved_ops(), Some(1));

        d.undo().unwrap();
        assert!(
            !d.unsaved(),
            "back at the base the file holds what is on screen"
        );
        assert_eq!(d.unsaved_ops(), Some(0));

        d.redo().unwrap();
        assert!(d.unsaved(), "and forward again it does not");
        let _ = std::fs::remove_file(&path);
    }

    /// PROVEN TO FAIL at 528dcd9: no close guard existed anywhere —
    /// `grep -rn "close_requested"` over `bins/` and `crates/` returned nothing
    /// — so eframe closed on the title-bar X with no prompt and `on_exit` then
    /// deleted the autosaved draft as well.
    #[test]
    fn closing_with_unsaved_changes_holds_the_window_and_closing_without_does_not() {
        let ctx = test_ctx();
        let close = || egui::RawInput {
            viewports: std::iter::once((
                egui::ViewportId::ROOT,
                egui::ViewportInfo {
                    events: vec![egui::ViewportEvent::Close],
                    ..Default::default()
                },
            ))
            .collect(),
            ..Default::default()
        };

        // Clean: no question, and nothing held back.
        let (mut app, path) = app_with_a("close-clean");
        let _ = ctx.run_ui(close(), |_| {});
        app.close_request(&ctx);
        assert!(!app.closing, "an unedited document must not prompt");

        // Dirty: held.
        app.bench
            .get_mut()
            .unwrap()
            .apply(pl_core::OpKind::SetTopology(pl_core::Topology::Circular))
            .unwrap();
        let _ = ctx.run_ui(close(), |_| {});
        app.close_request(&ctx);
        assert!(app.closing, "an edited document must prompt");
        let _ = std::fs::remove_file(&path);
    }

    /// PROVEN TO FAIL at 528dcd9, which had neither latch. The trap is that
    /// egui-winit pushes `ViewportEvent::Close` into the viewport info when the
    /// command is sent, so the NEXT frame sees `close_requested` again — and a
    /// guard still armed on that frame cancels its own close and the window can
    /// never be shut.
    #[test]
    fn answering_close_without_saving_actually_closes() {
        let ctx = test_ctx();
        let (mut app, path) = app_with_a("close-go");
        app.bench
            .get_mut()
            .unwrap()
            .apply(pl_core::OpKind::SetTopology(pl_core::Topology::Circular))
            .unwrap();
        app.closing = true;
        app.resolve_guard(Losing::Close, false);
        assert!(!app.closing, "the question is answered");
        assert!(app.close_now, "and the window is asked to go");

        // The frame that sends `Close`, and the frame after it, in which the
        // event comes back round.
        app.close_request(&ctx);
        let echo = egui::RawInput {
            viewports: std::iter::once((
                egui::ViewportId::ROOT,
                egui::ViewportInfo {
                    events: vec![egui::ViewportEvent::Close],
                    ..Default::default()
                },
            ))
            .collect(),
            ..Default::default()
        };
        let _ = ctx.run_ui(echo, |_| {});
        app.close_request(&ctx);
        assert!(
            !app.closing,
            "the guard must not cancel the close it just asked for"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// PROVEN TO FAIL at 528dcd9: `on_exit` called `recover::clear` on every
    /// exit, so answering "close without saving" destroyed the copy the dialog
    /// had just promised.
    #[test]
    fn abandoning_unsaved_work_keeps_the_recovery_draft() {
        let (mut app, recovery) = app_with_recovery("abandoned");
        app.bench.set(edited_doc("x.fa", "AAAACCCCGGGGTTTTAAGG"));
        app.resolve_guard(Losing::Close, false);
        assert!(
            recovery.exists(),
            "the forced final autosave must have written it"
        );
        eframe::App::on_exit(&mut app, None);
        assert!(
            recovery.exists(),
            "and the clean exit must not have deleted it"
        );
        // ...and the next launch must not call this a crash.
        let text = std::fs::read_to_string(&recovery).unwrap();
        let snap = recover::decode(&text).expect("a readable header");
        assert!(snap.abandoned, "the exit was deliberate and says so");
        let _ = std::fs::remove_file(&recovery);
    }

    /// PROVEN TO FAIL against this change's own first draft, which cleared
    /// `last_autosave` and not `autosaved`.
    ///
    /// `autosave` returns early when the recovery file already holds this
    /// (original, title, cursor), and that memo knows nothing about the exit
    /// flag — so a document autosaved on its ordinary thirty-second clock and
    /// then deliberately abandoned wrote nothing further, and the header kept
    /// saying the session crashed. Caught in the running application, which is
    /// why it is pinned here.
    #[test]
    fn the_abandoned_flag_survives_an_already_current_recovery_file() {
        let (mut app, path) = app_with_recovery("flag");
        app.bench.set(edited_doc("x.fa", "AAAACCCCGGGGTTTTAAGG"));
        // The ordinary periodic write, which leaves the memo current.
        app.autosave(false);
        let first = std::fs::read_to_string(&path).unwrap();
        assert!(
            !recover::decode(&first).unwrap().abandoned,
            "the premise: an ordinary autosave is not an abandonment"
        );

        app.resolve_guard(Losing::Close, false);
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            recover::decode(&after).unwrap().abandoned,
            "the deliberate quit must reach the file, or the next launch calls it a crash"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// PROVEN TO FAIL at ea436aa — the commit that claimed to have closed this.
    ///
    /// `close_request` was taught to arm on `bench.any_unsaved()`, and
    /// `unsaved_modal`'s early-out was left reading the document on screen, so
    /// it disarmed the guard one frame later. Edit plasmid A, open plasmid B in
    /// a new tab, close the window: B is clean, the modal resolved with
    /// `preserved = true`, and the app exited. A's edits were gone with no
    /// dialog — and no recovery draft either, because `preserved` is precisely
    /// the answer that says none is needed.
    ///
    /// A guard with two halves needs both to agree, and only one of them was
    /// changed. That is the whole lesson: `any_unsaved` in one place and
    /// `document()` in the other reads as a fix and behaves as a hole.
    #[test]
    fn a_dirty_background_tab_still_stops_the_window_closing() {
        let (mut app, a) = app_with_a("bg-dirty");
        app.bench
            .get_mut()
            .unwrap()
            .apply(pl_core::OpKind::SetTopology(pl_core::Topology::Circular))
            .unwrap();
        assert!(app.document().unwrap().unsaved(), "tab A must be dirty");

        // A second, CLEAN tab in front of it.
        let b = temp_file("bg-clean", "fa", PLASMID_B);
        app.load(b.clone());
        app.bench.get_mut().unwrap().mark_saved();
        assert!(!app.document().unwrap().unsaved(), "tab B must be clean");
        assert_eq!(app.bench.len(), 2);

        // The window close is requested and the guard latches.
        app.closing = true;
        let ctx = test_ctx();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.unsaved_modal(ui.ctx());
        });

        // The app must NOT be on its way out.
        assert!(
            !app.close_now && !app.let_it_go,
            "the window closed with an edited tab behind the active one"
        );
        assert!(app.closing, "the guard disarmed itself");
        // And it moved to the tab it is asking about, because a dialog that
        // names a clean file while discarding a dirty one is worse than none.
        assert!(
            app.document().unwrap().unsaved(),
            "the dialog is describing a clean document"
        );
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }

    /// PROVEN TO FAIL at 528dcd9: `load`'s Ok arm called `adopt` directly, with
    /// no dirty check anywhere on the path, so opening a second file over an
    /// edited one destroyed it silently.
    #[test]
    fn opening_a_second_file_keeps_the_edited_one_in_its_own_tab() {
        let (mut app, a) = app_with_a("swap-a");
        app.bench
            .get_mut()
            .unwrap()
            .apply(pl_core::OpKind::SetTopology(pl_core::Topology::Circular))
            .unwrap();
        let b = temp_file("swap-b", "fa", PLASMID_B);

        app.load(b.clone());

        // Two tabs, the new one in front. 528dcd9 made this ASK first, because
        // `load` called `adopt` directly and destroyed the edited file; the
        // bench answers it better by not replacing anything, so the property to
        // pin is no longer "the user was warned" but "the edit is still there"
        // — which is what the warning existed to achieve.
        assert_eq!(app.bench.len(), 2, "opening replaced instead of adding");
        assert_eq!(app.document().unwrap().title, "swap-b.fa");
        assert!(
            app.bench.any_unsaved(),
            "the edit vanished when the second file was opened"
        );
        assert_eq!(app.bench.unsaved_count(), 1);

        // And going back finds it exactly as it was left.
        app.switch_tab(0);
        assert_eq!(app.document().unwrap().title, "swap-a.fa");
        assert!(app.document().unwrap().molecule().topology.is_circular());
        assert!(app.document().unwrap().unsaved());
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }

    /// Closing the tab you just opened puts you back where you were, with the
    /// edit intact.
    ///
    /// This used to be "cancelling the question keeps the document", which was
    /// the same property expressed through a dialog that no longer exists:
    /// opening adds a tab, so the way to undo an open is to close it. Ctrl+W is
    /// unguarded precisely because this holds.
    #[test]
    fn closing_the_tab_you_just_opened_returns_you_to_the_edited_one() {
        let (mut app, a) = app_with_a("cancel-a");
        app.bench
            .get_mut()
            .unwrap()
            .apply(pl_core::OpKind::SetTopology(pl_core::Topology::Circular))
            .unwrap();
        let b = temp_file("cancel-b", "fa", PLASMID_B);
        app.load(b.clone());
        assert_eq!(app.bench.len(), 2);

        app.close_tab(app.bench.active());
        assert_eq!(app.bench.len(), 1);
        assert_eq!(app.document().unwrap().title, "cancel-a.fa");
        assert!(app.document().unwrap().unsaved(), "still dirty");
        assert!(
            app.document().unwrap().molecule().topology.is_circular(),
            "the edit itself did not survive"
        );
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }

    /// PROVEN TO FAIL at 528dcd9: `wrote()` set a transient status and nothing
    /// else, so there was no record anywhere that a save had happened. A guard
    /// keyed on the old predicate would have fired immediately after the user
    /// did exactly what it asked — the fastest possible way to teach someone the
    /// dialog is noise.
    #[test]
    fn a_faithful_save_clears_the_dirty_state_and_a_lossy_one_does_not() {
        // GenBank, on a molecule with features: faithful.
        let mut d = Document::from_bytes(
            b">x\nAAAACCCCGGGGTTTTAAGGCCTT\n",
            "x.fa".into(),
            Some("x.fa".into()),
        )
        .unwrap();
        d.apply(pl_core::OpKind::SetTopology(pl_core::Topology::Circular))
            .unwrap();
        assert!(d.unsaved());
        d.mark_saved();
        assert!(!d.unsaved(), "the state at the cursor is on disk");
        d.apply(pl_core::OpKind::SetFeature {
            index: None,
            feature: Box::new({
                let mut f = pl_core::Feature::new("probe", "misc_feature");
                f.segments = vec![pl_core::Segment::new(2, 6)];
                f
            }),
        })
        .unwrap();
        assert!(d.unsaved(), "and an edit after the save is not");
        d.undo().unwrap();
        assert!(
            !d.unsaved(),
            "undoing back to the saved point is clean again"
        );
    }

    /// PROVEN TO FAIL at 528dcd9: the Save menu offered GenBank and FASTA only.
    /// `snapgene::from_molecule` existed, tested, with no GUI entry point at
    /// all, so a user who opens `.dna` all day could not write one back.
    #[test]
    fn a_molecule_written_as_dna_reloads_with_its_features_primers_and_notes() {
        let mut mol = pkov();
        mol.notes
            .push(pl_core::Note::new("Description", "a round trip"));
        mol.primers.push(pl_core::Primer {
            name: "F_his colony PCR".into(),
            seq: "ACGTACGTACGTACGTAC".into(),
            description: String::new(),
            sites: vec![pl_core::BindingSite {
                start: 100,
                end: 117,
                strand: Strand::Forward,
                tm: None,
            }],
        });

        let (bytes, unwritable) = pl_fileio::snapgene::from_molecule_reporting(&mol);
        assert!(
            unwritable.is_empty(),
            "nothing here is unrepresentable: {unwritable:?}"
        );
        let back = pl_fileio::snapgene::parse(&bytes).expect("a readable .dna");
        assert_eq!(
            back.molecule
                .features
                .iter()
                .map(|f| f.name.clone())
                .collect::<Vec<_>>(),
            mol.features
                .iter()
                .map(|f| f.name.clone())
                .collect::<Vec<_>>(),
            "every feature name survived"
        );
        assert_eq!(back.molecule.primers.len(), 1, "and the primer");
        assert_eq!(back.molecule.primers[0].name, "F_his colony PCR");
        assert!(
            back.molecule
                .notes
                .iter()
                .any(|n| n.value == "a round trip"),
            "and the notes"
        );
        assert_eq!(back.molecule.seq, mol.seq);
        assert_eq!(back.molecule.topology, mol.topology);

        // Blocks 2, 3 and 7 are the three the writer will not synthesise, and
        // the user must be told rather than discover it later.
        let kinds: Vec<u8> = back.blocks.iter().map(|b| b.kind).collect();
        for (block, what) in [(2u8, "cut positions"), (3, "enzyme table"), (7, "history")] {
            assert!(
                !kinds.contains(&block),
                "block {block} ({what}) must not be synthesised"
            );
        }
        assert!(!back.history_present, "and no history is claimed");
    }

    /// The .dna lossiness modal fires when, and only when, there is something to
    /// say. Blocks 2 and 3 are caches and announcing them every time is how the
    /// dialog stops being read.
    #[test]
    fn the_dna_question_is_asked_only_when_something_is_really_lost() {
        // A molecule that came from no .dna at all: nothing to say.
        let mol = pkov();
        let (_, unwritable) = pl_fileio::snapgene::from_molecule_reporting(&mol);
        let quiet = unwritable.is_empty();
        assert!(quiet, "an ordinary molecule saves in one click");

        // A binding site starting before base 1 has no 0-based `location` form,
        // which is the case the writer's own report exists for.
        let mut noisy = pkov();
        // Start 0 is before base 1, which has no 0-based `location` form at
        // all — the case `from_molecule_reporting`'s own docstring exists for.
        noisy.primers.push(pl_core::Primer {
            name: "overhang".into(),
            seq: "ACGTACGTACGT".into(),
            description: String::new(),
            sites: vec![pl_core::BindingSite {
                start: 0,
                end: 12,
                strand: Strand::Forward,
                tm: None,
            }],
        });
        let (_, report) = pl_fileio::snapgene::from_molecule_reporting(&noisy);
        assert!(
            !report.is_empty(),
            "a location the format cannot hold must be named"
        );
    }

    /// PROVEN TO FAIL at 528dcd9: `settle()` committed a typing run and never
    /// assigned `self.status`, so after Ctrl+A then Delete the toolbar still
    /// read "add feature UX probe feature — Ctrl+Z to undo" beside a molecule
    /// with no bases left in it.
    #[test]
    fn the_status_after_a_typing_run_names_the_typing_run() {
        let mut app = App::blank();
        app.bench.set(
            Document::from_bytes(
                b">x\nAAAACCCCGGGGTTTTAAGG\n",
                "x.fa".into(),
                Some("x.fa".into()),
            )
            .unwrap(),
        );
        // A discrete edit first, so the bar has something else in it to be
        // wrong about — the shape of the photographed defect.
        app.edit(pl_core::OpKind::SetTopology(pl_core::Topology::Circular));
        assert!(
            app.status.contains("make circular"),
            "the premise: {}",
            app.status
        );

        app.edit.caret = 4;
        let d = app.bench.get_mut().unwrap();
        app.edit.type_text(d, "gg", 0.0);
        app.settle();
        assert!(
            app.status.starts_with("insert 2 bp at 5"),
            "the status must name the run that just landed, not the edit before \
             it: {:?}",
            app.status
        );
        assert!(app.status.contains("Ctrl+Z to undo"));
    }

    /// PROVEN TO FAIL at 528dcd9: `autosave`'s early return read
    /// `here.cursor.is_none()` as "the user's own file already holds it", and
    /// that premise is false for every document with no path. A draft restored
    /// from the recovery banner (the restore path drops the path deliberately)
    /// and a payload dropped in from a browser both sit at the base of an empty
    /// log with nothing on disk behind them — and this branch refused to write
    /// either of them. The unsaved-changes guard's forced final autosave walks
    /// straight into it, so the dialog's promise of a kept copy would have been
    /// a lie.
    #[test]
    fn a_restored_draft_with_no_path_is_autosaved() {
        let (mut app, path) = app_with_recovery("restored");
        app.bench
            .set(Document::from_bytes(b">r\nAAAACCCCGGGG\n", "r.fa".into(), None).unwrap());
        assert!(
            app.document().unwrap().unsaved(),
            "the premise: nothing on disk holds this"
        );
        app.autosave(false);
        assert!(
            path.exists(),
            "a document with no file behind it must be written"
        );
    }

    // -----------------------------------------------------------------------
    // methylation verdicts, and the welcome screen
    // -----------------------------------------------------------------------

    #[test]
    fn a_site_wrapping_the_origin_keeps_its_methylation_verdict() {
        // ApaI's GGGCCC starts at 0-based 15 on this 20 bp circle and runs off
        // the end, so `cut_positions` reports the cut at 1. Recovering the site
        // start with `saturating_sub` clamped 1 - 1 - 5 to 0 — five bases to
        // the right of the real site — and `site_effect` there finds nothing,
        // so the Enzymes row showed a clean unique cutter for a site Dcm
        // blocks: no strikethrough, no "Dcm blocked" label.
        const SEQ: &[u8] = b"CAAAAAAAAAAACCAGGGCC";
        let apai = pl_enzymes::by_name("ApaI").expect("ApaI ships");
        let cuts = pl_enzymes::cut_positions(SEQ, pl_core::Topology::Circular, apai);
        assert_eq!(cuts, vec![1], "the premise: one cut, on the origin");

        // Asked of `cut_sites`, which is the mapping the app now uses, so this
        // pins the live path rather than a second copy of the arithmetic.
        let site = pl_enzymes::cut_sites(SEQ, pl_core::Topology::Circular, apai)
            .into_iter()
            .next()
            .expect("one site");
        assert_eq!(site.position, 1);
        let start = (site.site_start - 1) as usize;
        assert_eq!(start, 15, "the site starts where the site starts");

        let meth = pl_core::Methylation {
            dcm: true,
            ..Default::default()
        };
        let effect = pl_enzymes::methylation::site_effect(
            apai,
            SEQ,
            start,
            pl_core::Topology::Circular,
            &meth,
        )
        .expect("Dcm blocks this site");
        assert_eq!(effect.effect, pl_enzymes::methylation::Effect::Blocked);
        assert_eq!(effect.methylase, pl_enzymes::methylation::Methylase::Dcm);
    }

    #[test]
    fn a_site_that_does_not_wrap_is_recovered_exactly_as_before() {
        // The control. Modular arithmetic must not move a site that never
        // needed it, on either topology.
        const SEQ: &[u8] = b"AAAAGGGCCCAAAAAAAAAA";
        let apai = pl_enzymes::by_name("ApaI").unwrap();
        for topo in [pl_core::Topology::Circular, pl_core::Topology::Linear] {
            let cuts = pl_enzymes::cut_positions(SEQ, topo, apai);
            assert_eq!(cuts, vec![10], "{topo:?}");
            let site = pl_enzymes::cut_sites(SEQ, topo, apai)
                .into_iter()
                .next()
                .expect("one site");
            assert_eq!(site.site_start, 5, "1-based, so 0-based 4  ({topo:?})");
        }
        // A molecule with no bases has no site to report, and inventing one
        // would be a claim about a sequence there is nothing to say about.
        assert!(pl_enzymes::cut_sites(b"", pl_core::Topology::Circular, apai).is_empty());
    }

    #[test]
    fn the_file_association_is_read_once_and_not_on_every_repaint() {
        // It was read live inside the welcome screen's paint closure, so every
        // repaint spawned `cmd /C assoc .dna` and blocked the UI thread on
        // `.output()` until cmd.exe exited. Pointer motion over the empty
        // window drove dozens of those a second. The answer is a machine-wide
        // registry setting; it cannot change between frames.
        let mut app = App::blank();
        let reads = std::cell::Cell::new(0);
        for _ in 0..50 {
            let owner = app.dna_owner_with(|| {
                reads.set(reads.get() + 1);
                Some("SnapGene.Document".to_string())
            });
            assert_eq!(owner, Some("SnapGene.Document"));
        }
        assert_eq!(reads.get(), 1, "one process, not fifty");
    }

    #[test]
    fn the_large_paste_question_does_not_divide_by_an_empty_document() {
        let q = paste_size_question(80_000, 0);
        assert!(q.contains("empty document"), "{q}");
        assert!(!q.contains("80000x"), "there is no ratio to state: {q}");
        // The ordinary case is unchanged.
        let q = paste_size_question(6_000, 3_000);
        assert!(q.contains("9,000 bp"), "{q}");
        assert!(q.contains("3x longer"), "{q}");
    }

    #[test]
    fn the_welcome_screen_names_another_owner_and_stays_quiet_otherwise() {
        // Saying who owns the extension is the whole point of reading it;
        // saying it about ourselves, or about nobody, is noise.
        let note = association_note(Some("SnapGene.Document"));
        assert!(note.contains("SnapGene.Document"), "{note}");
        assert!(note.contains("does not change that"), "{note}");
        assert!(association_note(Some("Polylinker.dna")).is_empty());
        assert!(association_note(None).is_empty());
    }

    // -----------------------------------------------------------------------
    // the frame loop, which is where coalescing was lost
    // -----------------------------------------------------------------------

    /// One frame's worth of what `App::ui` does around a keystroke.
    ///
    /// Not the whole frame — `ui` needs an `eframe::Frame` nobody can build
    /// outside eframe — but the two calls whose ORDER is the defect: the
    /// frame-top settle rule, and then `autosave`.
    fn frame(app: &mut App, typed: Option<&str>, now: f64) {
        let idle = app.edit.run().is_some_and(|r| r.is_idle(now));
        if idle {
            app.settle();
        }
        app.autosave(false);
        if let (Some(t), Some(d)) = (typed, app.bench.get_mut()) {
            app.edit.type_text(d, t, now);
        }
    }

    /// PROVEN TO FAIL at 5ef1c08: `Document::path` was assigned once, at
    /// construction, and never again — a grep for `.path = ` across
    /// bins/pl-gui returned nothing. A document written by Save As was marked
    /// clean and left pointing nowhere.
    ///
    /// It is clean and it is nowhere, and both halves cost something today,
    /// before any workspace exists: every later Ctrl+S reopens the picker
    /// because there is no path to write to, and `autosave`'s base-cursor guard
    /// tests `here.original.is_some()`, so a saved-but-pathless document keeps
    /// writing recovery drafts of a file that is already on disk.
    ///
    /// Driven through `write_dna`, which takes an explicit path — `export`
    /// raises a native picker and cannot be reached from a test at all.
    #[test]
    fn a_written_document_records_where_its_bytes_went() {
        let mut app = App::blank();
        let mol = pl_core::Molecule {
            name: "construct".into(),
            seq: b"ACGTACGTACGTACGT".to_vec(),
            topology: pl_core::Topology::Circular,
            ..Default::default()
        };
        let title = "construct".to_string();
        let (bytes, _) = pl_fileio::genbank::write_reporting(&mol, &title, today());
        app.adopt(Document::from_bytes(bytes.as_bytes(), title, None).expect("re-read"));

        // The state a religation product arrives in: real work, no path.
        assert!(app.document().unwrap().path.is_none());
        assert!(app.document().unwrap().unsaved());

        let path = temp_file("where-it-went", "dna", "");
        let (dna, unwritable) =
            pl_fileio::snapgene::from_molecule_reporting(app.document().unwrap().molecule());
        app.write_dna(PendingDna {
            path: path.clone(),
            bytes: dna,
            unwritable,
            history: false,
            notes: Vec::new(),
            overwriting_source: false,
            dest_lost: Vec::new(),
            source_lost: Vec::new(),
            then: None,
        });

        assert!(
            !app.document().unwrap().unsaved(),
            "the write did not clear the dirty state"
        );
        assert_eq!(
            app.document().unwrap().path.as_deref(),
            Some(path.as_path()),
            "the document was marked clean and left pointing nowhere"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// PROVEN TO FAIL at 4ca407b: nothing scheduled the frame on which an
    /// autosave would become due, so on an idle app it never became due.
    ///
    /// `autosave` declines inside `AUTOSAVE_EVERY`, and eframe waits for an
    /// event rather than spinning:
    ///
    ///     14:00:00  an autosave lands
    ///     14:00:17  a 1,240 bp deletion — one frame runs, autosave declines
    ///     14:00:18  the user goes to lunch
    ///     14:20:00  the machine loses power
    ///
    /// The deletion was in no file anywhere. The throttle is right; "another
    /// frame will come along" was the assumption, and the one case it fails is
    /// the idle app — which is the exact case a crash-recovery file exists for.
    #[test]
    fn an_idle_app_with_unsaved_work_still_schedules_its_next_autosave() {
        let (mut app, _path) = app_with_recovery("wake");
        // Nothing open: nothing at risk, and the app is allowed to sleep.
        assert_eq!(app.autosave_due_in(), None, "an empty bench must not wake");

        app.adopt(Document::from_bytes(b">x\nAAAACCCCGGGGTTTT\n", "x.fa".into(), None).unwrap());
        // Opened from bytes with no path, so it counts as unsaved work at once.
        assert!(app.document().unwrap().unsaved());
        let due = app
            .autosave_due_in()
            .expect("unsaved work must schedule a wake-up");
        assert!(
            due <= App::AUTOSAVE_EVERY,
            "the wake-up is further away than the throttle itself: {due:?}"
        );

        // `adopt` starts the clock, so the wake is in the future rather than
        // immediate — the point is that one is asked for AT ALL.
        app.last_autosave = Some(std::time::Instant::now());
        let due = app.autosave_due_in().expect("still at risk");
        assert!(due > std::time::Duration::ZERO && due <= App::AUTOSAVE_EVERY);

        // And a bench with nothing at risk sleeps again.
        app.bench.get_mut().unwrap().mark_saved();
        assert!(!app.bench.any_unsaved());
        assert_eq!(
            app.autosave_due_in(),
            None,
            "a saved bench must not hold the CPU awake every thirty seconds"
        );
    }

    #[test]
    fn a_typing_run_survives_the_autosave_that_runs_on_every_frame() {
        // THE defect this whole surface turned on. `autosave` began with an
        // unconditional `settle`, and `App::ui` calls `autosave` every frame,
        // so a run opened in frame N was committed at the top of frame N+1 —
        // before the next keystroke was even read. Every keystroke became its
        // own `InsertAt`. Measured in the running application on the 4.6 Mb
        // genome: 300 typed characters gave "300 edit(s)" in the History tab
        // and one Ctrl+Z gave back one base, so undoing a 300 bp cassette
        // needed 300 presses; 23 s of typing burned 45 s of CPU and 60 MB.
        //
        // The throttle was thirty lines below the settle, so it throttled the
        // disk write and never the commit.
        let (mut app, _path) = app_with_recovery("coalesce");
        app.adopt(Document::from_bytes(b">x\nAAAACCCCGGGGTTTT\n", "x.fa".into(), None).unwrap());

        let mut t = 500.0;
        for _ in 0..40 {
            frame(&mut app, Some("a"), t);
            t += 0.05; // well inside Run::IDLE_SECONDS
        }
        let d = app.document().unwrap();
        assert_eq!(
            d.log.path().len(),
            0,
            "forty keystrokes inside one second are still one open run"
        );
        assert_eq!(app.edit.run().unwrap().inserted.len(), 40);

        // And when the typing stops, the run closes on its own and becomes
        // exactly one operation — one Ctrl+Z for the lot.
        frame(&mut app, None, t + seqedit::Run::IDLE_SECONDS);
        let d = app.document().unwrap();
        assert_eq!(d.log.path().len(), 1);
        assert_eq!(d.molecule().len(), 16 + 40);
        app.do_undo();
        assert_eq!(app.document().unwrap().molecule().len(), 16);
    }

    #[test]
    fn an_autosave_that_writes_never_leaves_out_the_open_run() {
        // The other half, and the reason the settle is there at all: a recovery
        // file written from `log.current()` mid-run is missing the user's last
        // keystrokes. Moving the throttle above the settle must not cost this.
        let (mut app, path) = app_with_recovery("midrun");
        app.adopt(Document::from_bytes(b">x\nAAAACCCCGGGGTTTT\n", "x.fa".into(), None).unwrap());
        let d = app.bench.get_mut().unwrap();
        app.edit.caret = 16;
        app.edit.type_text(d, "gggg", 500.0);
        assert!(app.edit.run().is_some(), "the premise: a run is open");

        // The autosave falls due.
        app.last_autosave = None;
        app.autosave(false);

        let (mol, _) = autosaved(&path);
        assert_eq!(
            String::from_utf8(mol.seq).unwrap().to_ascii_uppercase(),
            "AAAACCCCGGGGTTTTGGGG",
            "the file holds what is on screen"
        );
        assert!(
            app.edit.run().is_none(),
            "and the run was settled to get it"
        );
    }

    // -----------------------------------------------------------------------
    // Ctrl+X
    // -----------------------------------------------------------------------

    /// A circular document holding `seq`, with the named feature over `a..b`.
    fn circle_with(seq: &str, name: &str, a: u64, b: u64) -> Document {
        let mut mol = pl_core::Molecule {
            seq: seq.as_bytes().to_vec(),
            topology: pl_core::Topology::Circular,
            ..Default::default()
        };
        let mut f = pl_core::Feature::new(name, "misc_feature");
        f.segments.push(pl_core::Segment::new(a, b));
        mol.features.push(f);
        Document::from_bytes(
            pl_fileio::genbank::write(&mol, "x", (1, 0, 2026)).as_bytes(),
            "x.gb".into(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn cutting_across_the_origin_still_says_the_plasmid_was_renumbered() {
        // The Cut arm assigned the notice unconditionally, so the one edit in
        // this surface that silently renumbers the whole molecule was the one
        // that said nothing about it: `apply_gesture` set "the plasmid was
        // renumbered ... Ctrl+Z twice" and the next line replaced it with
        // "cut 4 bases". The identical delete by Backspace reported both.
        let mut app = App::blank();
        app.bench.set(circle_with("ABCDEFGHIJKL", "inner", 5, 8));
        app.edit.sel = Some(seqedit::Selection {
            anchor: 10,
            head: 2,
            through_origin: true,
        });

        let clip = app.do_cut(500.0).expect("four bases on the clipboard");
        assert_eq!(clip, "KLAB", "read across the origin, in reading order");
        assert_eq!(app.document().unwrap().molecule().seq, b"CDEFGHIJ".to_vec());
        let said = app.edit.notice.clone().unwrap_or_default();
        assert!(said.contains("cut 4 bases"), "said {said:?}");
        assert!(said.contains("renumbered"), "said {said:?}");
    }

    #[test]
    fn cutting_a_whole_feature_away_still_names_it() {
        let mut app = App::blank();
        app.bench
            .set(circle_with("AAAACCCCGGGGTTTTAAGG", "AmpR", 5, 8));
        app.edit.sel = Some(seqedit::Selection {
            anchor: 4,
            head: 8,
            through_origin: false,
        });

        app.do_cut(500.0).expect("four bases");
        assert!(app.document().unwrap().molecule().features.is_empty());
        let said = app.edit.notice.clone().unwrap_or_default();
        assert!(said.contains("cut 4 bases"), "said {said:?}");
        assert!(said.contains("AmpR"), "said {said:?}");
    }

    #[test]
    fn a_cut_with_nothing_selected_removes_nothing_and_says_so() {
        let mut app = App::blank();
        app.bench
            .set(circle_with("AAAACCCCGGGGTTTTAAGG", "f", 1, 4));
        app.edit.caret = 5;
        assert_eq!(app.do_cut(500.0), None);
        assert_eq!(app.document().unwrap().molecule().len(), 20);
        assert!(app.edit.notice.as_deref().unwrap().contains("Nothing"));
    }

    // -----------------------------------------------------------------------
    // undo over a two-operation gesture
    // -----------------------------------------------------------------------

    #[test]
    fn undoing_an_origin_crossing_cut_puts_the_origin_back_too() {
        // Deleting across the origin is a rotate and then a range op. One
        // Ctrl+Z over the range op alone gave back all twelve bases with the
        // origin still moved — "KLABCDEFGHIJ", a numbering that matches neither
        // the state before the edit nor the state after it, with the feature
        // sitting at 6..10 instead of 4..8. That is not a partial undo anyone
        // can recognise; it is a plausible plasmid that is wrong.
        let mut app = App::blank();
        app.bench.set(circle_with("ABCDEFGHIJKL", "inner", 5, 8));
        app.edit.sel = Some(seqedit::Selection {
            anchor: 10,
            head: 2,
            through_origin: true,
        });
        app.do_cut(500.0).unwrap();
        let d = app.document().unwrap();
        assert_eq!(d.log.path().len(), 2, "the premise: two operations");
        assert_eq!(d.molecule().seq, b"CDEFGHIJ".to_vec());

        app.do_undo();
        let d = app.document().unwrap();
        assert_eq!(
            d.molecule().seq,
            b"ABCDEFGHIJKL".to_vec(),
            "one press, and the numbering is the one the file had"
        );
        assert_eq!(d.molecule().features[0].start(), 5, "and so is the feature");
        assert!(app.status.contains("origin"), "status {:?}", app.status);

        // And forward again, both halves together.
        app.do_redo();
        let d = app.document().unwrap();
        assert_eq!(d.molecule().seq, b"CDEFGHIJ".to_vec());
    }

    // -----------------------------------------------------------------------
    // a document arriving with coordinates the readers deliberately keep
    // -----------------------------------------------------------------------

    #[test]
    fn double_clicking_a_feature_that_starts_at_base_zero_selects_the_feature() {
        // `<Segment range="0-4"/>` is parsed verbatim by the SnapGene reader,
        // which deliberately carries the zero rather than dropping it, and
        // nothing validates a molecule on the way into this window. `start - 1`
        // panicked in a debug build; in release, with overflow checks off, it
        // wrapped to u64::MAX and clamped to n, so double-clicking a feature
        // covering bases 1..4 selected bases 5..12 — the ones it does not cover
        // — and the next Backspace deleted them.
        let mut mol = pl_core::Molecule {
            seq: b"ABCDEFGHIJKL".to_vec(),
            topology: pl_core::Topology::Circular,
            ..Default::default()
        };
        let mut f = pl_core::Feature::new("zero", "misc_feature");
        f.segments.push(pl_core::Segment::new(0, 4));
        mol.features.push(f);

        let mut app = App::blank();
        app.bench.set(Document::of_molecule(mol));
        app.select_feature_under(2);

        let s = app.edit.sel.expect("the feature under the pointer");
        assert!(!s.through_origin);
        assert_eq!(
            (s.lo(), s.hi()),
            (0, 4),
            "the bases it names, not their complement"
        );
    }

    #[test]
    fn a_recovered_document_does_not_inherit_the_previous_documents_caret() {
        // `load` reset the editor with the comment "a caret from the previous
        // document names bases this one does not have". The Recover banner and
        // a dropped byte payload assigned `self.document` without it, so a
        // selection made on a 5 kb plasmid survived onto a 200 bp recovered
        // document as a highlight of its tail — and Backspace deletes what is
        // highlighted.
        let mut app = App::blank();
        app.bench
            .set(Document::from_bytes(b">a\nAAAACCCCGGGGTTTT\n", "a.fa".into(), None).unwrap());
        app.edit.caret = 16;
        app.edit.sel = Some(seqedit::Selection {
            anchor: 8,
            head: 16,
            through_origin: false,
        });
        app.selected = Some(0);

        app.adopt(Document::from_bytes(b">b\nACGT\n", "b.fa".into(), None).unwrap());
        assert_eq!(app.edit.caret, 0);
        assert_eq!(app.edit.sel, None);
        assert_eq!(app.selected, None);
    }

    // -----------------------------------------------------------------------
    // the toolbar's title block
    // -----------------------------------------------------------------------

    /// PROVEN TO FAIL against the working tree as handed over, on the first
    /// assertion: `elide` opened with `if room <= 0.0 || fits { return s }`, so
    /// non-positive room paid out the WHOLE string.
    ///
    /// That inverts the documented degradation order at exactly the moment it
    /// matters. `room` is `available_width - state_w - 12`, so a long status
    /// string drives it negative, and the filename then gave nothing back while
    /// both it and the un-elided status overran the reserved right-hand cluster —
    /// the theme switch painted through the letters and the path cut mid-token at
    /// the window edge with no ellipsis. It is not a synthetic path length: the
    /// user's own genome files sit 160 characters deep in OneDrive.
    #[test]
    fn no_room_elides_to_nothing_and_not_to_everything() {
        let ctx = test_ctx();
        ctx.begin_pass(egui::RawInput::default());
        egui::Area::new(egui::Id::new("t")).show(&ctx, |ui| {
            for room in [0.0, -1.0, -400.0] {
                assert_eq!(
                    elide(ui, "pKoV with His decR.dna", room),
                    "",
                    "room {room} paid out the whole name"
                );
            }
            // Small but positive: something, ending in an ellipsis, and shorter
            // than what was asked for.
            let some = elide(ui, "pKoV with His decR.dna", 40.0);
            assert!(some.ends_with("..."), "{some:?}");
            assert!(some.len() < "pKoV with His decR.dna".len());
            // Ample: untouched, with no ellipsis invented.
            assert_eq!(
                elide(ui, "pKoV with His decR.dna", 4_000.0),
                "pKoV with His decR.dna"
            );
            // Not even one character and an ellipsis: empty, never a bare "...",
            // which would claim a name is there when none of it can be read.
            assert_eq!(elide(ui, "pKoV", 3.0), "");
        });
        let _ = ctx.end_pass();
    }

    /// The whole toolbar inside the window, and nothing over the theme switch.
    ///
    /// Item 4 shipped with no automated coverage at all: every measured claim
    /// about the bar rested on screenshots, and `elide` — one caller, no test —
    /// held the defect above. This paints real frames at the app's own
    /// `min_inner_size` and asks the two questions the screenshots were asked.
    #[test]
    fn the_toolbar_stays_inside_the_window_however_long_the_status_is() {
        for status in [
            "SnapGene .dna · 8,117 bp · circular",
            // What a real export writes: a 99-character consequence in front of
            // a 150-character path.
            "FASTA keeps only the bases; this drops 9 feature(s) and the topology (it will \
             reopen as linear)  —  wrote C:\\Users\\alf22\\AppData\\Local\\Temp\\claude\\\
             C--Users-alf22-Zotero\\bb0d8734-3b4e-44e4-86d3-d1d2dcab7b48\\scratchpad\\vout\\seq.fa",
            // And a pathological one, to establish there is no length at which
            // the bar gives up quietly.
            &"x".repeat(600),
        ] {
            // 880 x 560 is `min_inner_size`; 1280 x 840 is the default.
            for (w, h) in [(880.0f32, 560.0f32), (1280.0, 840.0)] {
                let ctx = test_ctx();
                let mut app = seq_app();
                app.status = status.to_string();
                let win = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(w, h),
                    )),
                    ..Default::default()
                };
                // The toolbar alone. `paint_out` drives `side_panel` and a filler
                // CentralPanel and never calls `top_bar`, which is exactly the gap
                // this test exists to close.
                let mut shapes = Vec::new();
                for _ in 0..2 {
                    app.status = status.to_string();
                    let out = ctx.run_ui(win.clone(), |ui| {
                        app.top_bar(ui);
                    });
                    shapes = flat_shapes(&out.shapes);
                }
                // Every text in the window, found by content rather than by a
                // guessed y-band: the toolbar's own height is not this test's
                // business and getting it wrong makes the test about the wrong
                // widgets.
                let texts: Vec<(String, egui::Rect)> = shapes
                    .iter()
                    .filter_map(|s| match s {
                        egui::Shape::Text(t) => Some((
                            t.galley.text().to_string(),
                            egui::Rect::from_min_size(t.pos, t.galley.size()),
                        )),
                        _ => None,
                    })
                    .collect();
                assert!(texts.len() >= 8, "{w}x{h}: only {} texts", texts.len());
                for (text, r) in &texts {
                    assert!(
                        r.right() <= w + 0.5 && r.left() >= -0.5,
                        "{w}x{h}: {text:?} is drawn at {r:?}, outside a {w} pt window"
                    );
                }
                // The status is drawn whole, or drawn with a mark saying it was
                // cut. Never cut in silence.
                //
                // This is the assertion that distinguishes a fix from no fix. "Is
                // it inside the window" holds either way, because with no elision
                // of our own egui truncates the galley itself and the rect stays
                // put; what it does not do is leave a mark. The bar simply read
                // `wrote C:\Users\alf22\AppData\Local\Temp\claude\C--Users*lf2`
                // and stopped, mid-token, with no ellipsis and no hover, so a
                // reader could not tell there was more — and what was cut off was
                // `FASTA keeps only the bases; this drops 9 feature(s) and the
                // topology`. A silently truncated warning is the same defect as a
                // silently dropped label.
                let head: String = status.chars().take(10).collect();
                let drawn: Vec<&String> = texts
                    .iter()
                    .map(|(t, _)| t)
                    // `!t.is_empty()`, because every string starts with "" and
                    // the elided filename beside the status is legitimately empty.
                    .filter(|t| {
                        !t.is_empty() && (t.starts_with(&head) || status.starts_with(t.as_str()))
                    })
                    .collect();
                assert!(
                    !drawn.is_empty(),
                    "{w}x{h}: the status is not on screen at all"
                );
                for t in &drawn {
                    assert!(
                        *t == status || t.ends_with("..."),
                        "{w}x{h}: a {}-character status was cut to {t:?} with nothing saying so",
                        status.len()
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // the three application-wide shortcuts, and the design panel's lifetime
    // -----------------------------------------------------------------------

    /// A raw input carrying one Ctrl+`key` press.
    fn ctrl(key: egui::Key) -> egui::RawInput {
        let modifiers = egui::Modifiers {
            command: true,
            ctrl: true,
            ..Default::default()
        };
        egui::RawInput {
            modifiers,
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        }
    }

    /// What `global_shortcuts` decides, with a REAL text box optionally holding
    /// focus. `focused` names the box for the failure message only.
    ///
    /// It has to be a real one. This helper used to fake it with
    /// `ctx.memory_mut(|m| m.request_focus(egui::Id::new(name)))`, and a bare
    /// id has no widget behind it — no `TextEditState`, nothing to say what
    /// kind of thing holds the focus. That is why it could not distinguish a
    /// text box from a button, and so could not have caught the defect that
    /// `a_focused_button_does_not_disable_the_application_shortcuts` covers.
    ///
    /// `global_shortcuts` is called BEFORE the box is built, which is the order
    /// `update` uses and the order that matters: a focused `TextEdit` handles
    /// Ctrl+Z itself and consumes the event, so building first would leave this
    /// asserting that the event was eaten rather than that the guard held.
    fn shortcuts_with(app: &App, key: egui::Key, focused: Option<&str>) -> Shortcuts {
        fn build(ui: &mut Ui, id: Option<egui::Id>, text: &mut String) {
            if let Some(id) = id {
                ui.add(egui::TextEdit::singleline(text).id(id));
            }
        }
        let ctx = test_ctx();
        let mut text = String::new();
        let id = focused.map(egui::Id::new);
        // One pass to build the box, so the id has `TextEditState` behind it,
        // and only then focus it.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| build(ui, id, &mut text));
        if let Some(id) = id {
            ctx.memory_mut(|m| m.request_focus(id));
        }
        let mut out = Shortcuts::default();
        let _ = ctx.run_ui(ctrl(key), |ui| {
            out = app.global_shortcuts(ui.ctx());
            build(ui, id, &mut text);
        });
        out
    }

    /// PROVEN TO FAIL at dfd6ac9 (see the report): with the block as it stood,
    /// `undo` is true while the Features filter holds focus, so Ctrl+Z after a
    /// typo in a search box undoes the *molecule* as well as the typo.
    #[test]
    fn a_shortcut_typed_into_a_focused_text_box_does_not_reach_the_document() {
        let mut app = App::blank();
        app.bench
            .set(Document::from_bytes(b">a\nAAAACCCCGGGGTTTT\n", "a.fa".into(), None).unwrap());

        // The control: nothing focused, so the shortcuts are the app's.
        assert!(shortcuts_with(&app, egui::Key::Z, None).undo);
        assert!(shortcuts_with(&app, egui::Key::Y, None).redo);
        assert!(shortcuts_with(&app, egui::Key::O, None).open);

        assert!(shortcuts_with(&app, egui::Key::S, None).save);

        // The Features tab's filter box, the Library query and the design
        // panel's Spacer field are all plain `TextEdit`s, and egui gives Ctrl+Z
        // to the focused one without consuming it. So are the feature editor's
        // Name box, its free-text Type box, its colour box and every qualifier
        // value — Ctrl+Z typed into one of those must undo the typo, not the
        // molecule.
        for who in [
            "features filter",
            "library query",
            "design spacer",
            "feature name",
            "feature qualifier value",
        ] {
            let k = shortcuts_with(&app, egui::Key::Z, Some(who));
            assert!(!k.undo, "Ctrl+Z reached the document from {who}");
            assert!(!shortcuts_with(&app, egui::Key::Y, Some(who)).redo, "{who}");
            assert!(
                !shortcuts_with(&app, egui::Key::O, Some(who)).open,
                "Ctrl+O popped a file dialog out of {who}"
            );
            // Ctrl+S is the most reflexive keystroke there is, and a file dialog
            // popping out of a search box is the same surprise as a file dialog
            // popping out of it for Ctrl+O.
            assert!(
                !shortcuts_with(&app, egui::Key::S, Some(who)).save,
                "Ctrl+S opened a Save dialog out of {who}"
            );
        }
    }

    /// A raw input carrying one unmodified `key` press.
    fn plain(key: egui::Key) -> egui::RawInput {
        egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            }],
            ..Default::default()
        }
    }

    /// Tab through a REAL `Button` and a REAL `TextEdit` with REAL Tab presses,
    /// then press Ctrl+`key` and report what `global_shortcuts` decided —
    /// together with where the focus actually went.
    ///
    /// The pair is reported so the assertions cannot pass vacuously. `undo`
    /// being true proves nothing if the Tab landed nowhere, which is exactly
    /// how `shortcuts_with` above is blind here: it fakes focus with
    /// `request_focus(Id::new("features filter"))`, and a bare id has no widget
    /// behind it, so nothing in that helper can tell a text box from a button.
    /// It could not have caught this defect and cannot verify this fix.
    fn shortcuts_after_tabbing(app: &App, key: egui::Key, tabs: usize) -> (Shortcuts, bool, bool) {
        fn build(ui: &mut Ui, text: &mut String) {
            let _ = ui.button("a toolbar button");
            let _ = ui.text_edit_singleline(text);
        }
        let ctx = test_ctx();
        let mut text = String::new();
        // One pass to register the widgets, then one pass per Tab: egui moves
        // focus in creation order, so the first lands on the button.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| build(ui, &mut text));
        for _ in 0..tabs {
            let _ = ctx.run_ui(plain(egui::Key::Tab), |ui| build(ui, &mut text));
        }
        // Before the widgets, as `update` does: see `shortcuts_with`.
        let mut out = Shortcuts::default();
        let _ = ctx.run_ui(ctrl(key), |ui| {
            out = app.global_shortcuts(ui.ctx());
            build(ui, &mut text);
        });
        (
            out,
            ctx.memory(|m| m.focused()).is_some(),
            ctx.text_edit_focused(),
        )
    }

    /// PROVEN TO FAIL at 694c4b3: the guard read `m.focused().is_some()`, and
    /// the comment justifying it said "`Button` never takes keyboard focus".
    /// egui 0.35 builds `Button` with `.sense(Sense::click())`, which is
    /// `CLICK | FOCUSABLE`, so one Tab into the toolbar killed Ctrl+Z, Ctrl+Y,
    /// Ctrl+O and Ctrl+S for the rest of the session, silently.
    #[test]
    fn a_focused_button_does_not_disable_the_application_shortcuts() {
        let mut app = App::blank();
        app.bench
            .set(Document::from_bytes(b">a\nAAAACCCCGGGGTTTT\n", "a.fa".into(), None).unwrap());

        // One Tab: the button. The two booleans are the control — something
        // holds focus, and it is not a text box — so "undo still fires" cannot
        // be true merely because the Tab went nowhere.
        for (key, got) in [
            (egui::Key::Z, "undo"),
            (egui::Key::Y, "redo"),
            (egui::Key::O, "open"),
            (egui::Key::S, "save"),
        ] {
            let (k, focused, text) = shortcuts_after_tabbing(&app, key, 1);
            assert!(focused, "the Tab focused nothing, so {got} proves nothing");
            assert!(!text, "the Tab landed on a text box, not the button");
            let fired = match key {
                egui::Key::Z => k.undo,
                egui::Key::Y => k.redo,
                egui::Key::O => k.open,
                _ => k.save,
            };
            assert!(fired, "a focused button swallowed {got}");
        }

        // Two Tabs: the text box, where the stand-down is the whole point. The
        // same helper, so the two outcomes are compared on one mechanism.
        let (k, focused, text) = shortcuts_after_tabbing(&app, egui::Key::Z, 2);
        assert!(focused && text, "the second Tab did not reach the text box");
        assert!(!k.undo, "Ctrl+Z in a text box reached the document");
    }

    /// A 900 bp document with a selection, and the design panel open on it.
    fn app_designing() -> App {
        let seq: String = (0..900u32)
            .scan(12_345u64, |s, _| {
                *s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                Some(b"ACGT"[((*s >> 24) & 3) as usize] as char)
            })
            .collect();
        let mut app = App::blank();
        // `Some(path)` so the document counts as saved: these tests are about
        // the design panel, not about the unsaved-changes guard, and a pathless
        // fixture would be parked by `take_over` before `adopt` ever ran.
        app.bench.set(
            Document::from_bytes(
                format!(">a\n{seq}\n").as_bytes(),
                "a.fa".into(),
                Some("a.fa".into()),
            )
            .unwrap(),
        );
        app.edit.sel = Some(seqedit::Selection {
            anchor: 300,
            head: 600,
            through_origin: false,
        });
        app.open_design();
        assert!(app.design.is_some(), "the panel opened");
        app
    }

    /// Run one frame of the design panel, which is what refreshes `doc_at` and
    /// services an `add_request`.
    fn design_frame(app: &mut App) {
        let ctx = test_ctx();
        ctx.begin_pass(egui::RawInput::default());
        app.design_panel(&ctx);
        let _ = ctx.end_pass();
    }

    #[test]
    fn an_undo_is_not_taken_while_the_design_panel_is_open() {
        let app = app_designing();
        assert!(!shortcuts_with(&app, egui::Key::Z, None).undo);
        assert!(!shortcuts_with(&app, egui::Key::Y, None).redo);
        // Opening another file is still allowed; it closes the panel and says so.
        assert!(shortcuts_with(&app, egui::Key::O, None).open);
        // And so is saving, deliberately. The panel guard exists because an undo
        // underneath the panel changes the bases the panel is describing; saving
        // changes nothing, and writing the file you are looking at while a primer
        // report is open is a reasonable thing to want. Pinned here because the
        // doc block on `Shortcuts` claimed all three guards applied and this one
        // never did — either answer is defensible, an undocumented one is not.
        assert!(
            shortcuts_with(&app, egui::Key::S, None).save,
            "Ctrl+S is deliberately NOT gated on the design panel; if that changed, \
             the doc on `Shortcuts::save` has to change with it"
        );
    }

    /// PROVEN TO FAIL at dfd6ac9: both `primer_bind` features land, at the
    /// pre-reverse-complement coordinates, on the reverse-complemented molecule
    /// — the length is unchanged so `validate()` reports nothing and the
    /// WouldCorrupt gate accepts them.
    #[test]
    fn a_report_computed_before_an_edit_cannot_be_added_after_it() {
        let mut app = app_designing();
        design_frame(&mut app);
        let seq = app.document().unwrap().molecule().seq.clone();
        app.design.as_mut().unwrap().run(&seq);
        assert!(
            matches!(app.design.as_ref().unwrap().result, Some(Ok(_))),
            "the premise: a report to add"
        );

        // The toolbar stayed live behind a non-modal window.
        app.edit(pl_core::OpKind::ReverseComplement);
        assert!(app.document().unwrap().molecule().features.is_empty());

        app.design.as_mut().unwrap().add_request = Some(0);
        design_frame(&mut app);
        assert!(
            app.document()
                .as_ref()
                .unwrap()
                .molecule()
                .features
                .is_empty(),
            "a report about the previous molecule must not be written onto this one"
        );
        assert!(
            app.notice
                .as_deref()
                .unwrap_or_default()
                .contains("changed"),
            "{:?}",
            app.notice
        );

        // Pressing Design again makes it addable, against the molecule as it
        // now stands.
        let seq = app.document().unwrap().molecule().seq.clone();
        app.design.as_mut().unwrap().run(&seq);
        app.design.as_mut().unwrap().add_request = Some(0);
        design_frame(&mut app);
        assert_eq!(
            app.document().unwrap().molecule().features.len(),
            2,
            "and a current report still adds two features"
        );
        assert!(app.status.contains("Ctrl+Z twice"), "{}", app.status);
    }

    /// PROVEN TO FAIL at 528dcd9: `load`'s error arm assigned
    /// `self.document = None`, so choosing a `.ab1`, a folder-that-is-a-file or
    /// anything unparseable destroyed the open document and replaced the whole
    /// screen with the "could not read that file" takeover. The user lost their
    /// work AND got nothing, because there was no new document to have traded
    /// it for.
    ///
    /// This test used to assert the opposite — that the failed load closed the
    /// design panel — which was the right answer while the document was being
    /// destroyed underneath it. With the document left alone the panel is still
    /// describing the molecule that is still open, so closing it would now be
    /// the defect. The original silent-drop it was written for is unreachable:
    /// `design_panel`'s early return needs `self.document` to be `None`, and
    /// nothing on this path sets it.
    #[test]
    fn a_failed_load_leaves_the_open_document_and_its_panel_alone() {
        let mut app = app_designing();
        let seq = app.document().unwrap().molecule().seq.clone();
        design_frame(&mut app);
        app.design.as_mut().unwrap().run(&seq);

        let bad = std::env::temp_dir().join(format!("pl-gui-notafile-{}.dna", std::process::id()));
        std::fs::write(&bad, b"this is not a sequence file at all").unwrap();
        app.load(bad.clone());
        let _ = std::fs::remove_file(&bad);

        assert!(app.document().is_some(), "the document survived");
        assert!(app.error.is_none(), "and the screen was not taken over");
        assert!(app.design.is_some(), "so the panel is still valid");
        assert!(
            app.notice
                .as_deref()
                .unwrap_or_default()
                .contains("unrecognised format"),
            "and the failure was reported: {:?}",
            app.notice
        );
    }

    /// PROVEN TO FAIL at 78a46f2: `load_as` there called `Document::open`
    /// unconditionally, so a `.ab1` reached the molecule readers, came back
    /// `unrecognised format`, and the only outcome available was the error arm.
    /// A chromatogram could not be opened at all, and this test asserts the
    /// three cases together because they are one contract.
    ///
    /// The contract is cc36cf7's and it must not be regressed: whatever a load
    /// does, an open EDITED document survives it — same op cursor, same bases —
    /// and the unsaved-changes question is never raised, because opening a read
    /// does not close a document. Asking "discard and open?" when a user drops
    /// a chromatogram onto their plasmid is exactly the false positive that
    /// trains people to click through the guard.
    #[test]
    fn loading_a_trace_never_touches_the_open_document() {
        // A minimal ABIF, an SCF wearing `.ab1`, and the 68 bytes the indexer
        // classifies as "not a sequence file". All three are BUILT: see
        // `reads::tests::truncated_ab1` for why the third stopped being read
        // off disk.
        let good = reads::tests::ab1(b"ACGTACGTACGTACGTACGT", &[40u8; 20]);
        let scf = {
            let mut v = b".scf".to_vec();
            v.extend_from_slice(&[0u8; 64]);
            v
        };
        let truncated = reads::tests::truncated_ab1();

        for (i, (what, bytes)) in [
            ("a parseable trace", good),
            ("an SCF named .ab1", scf),
            ("a truncated .ab1", truncated),
        ]
        .into_iter()
        .enumerate()
        {
            let mut app = seq_app();
            // One applied operation, so `unsaved()` is true and the guard would
            // fire if anything on this path reached it.
            let d = app.bench.get_mut().expect("a document");
            d.apply(pl_core::OpKind::InsertAt {
                at: 1,
                seq: "AAAA".to_string(),
            })
            .expect("an ordinary insert");
            let cursor = d.log.cursor();
            let seq = d.molecule().seq.clone();
            assert!(d.unsaved(), "the fixture depends on there being an edit");

            let path =
                std::env::temp_dir().join(format!("pl-gui-trace-{}-{i}.ab1", std::process::id()));
            std::fs::write(&path, &bytes).unwrap();
            app.load(path.clone());
            let _ = std::fs::remove_file(&path);

            let d = app.document().unwrap_or_else(|| {
                panic!("{what}: the document was destroyed");
            });
            assert_eq!(d.log.cursor(), cursor, "{what}: the history moved");
            assert_eq!(d.molecule().seq, seq, "{what}: the bases changed");
            assert!(app.error.is_none(), "{what}: the screen was taken over");
            // A chromatogram is attached to the open document; it must not
            // arrive as a tab of its own. The old form of this asserted that no
            // unsaved-changes question was raised, which said the same thing
            // through machinery that no longer exists.
            assert_eq!(
                app.bench.len(),
                1,
                "{what}: taking a read opened a second tab"
            );
            if i == 0 {
                assert_eq!(app.reads.len(), 1, "{what}: the read was not kept");
                assert!(app.tab == Tab::Reads, "{what}: the Reads tab did not open");
                // No molecule was compared against here, but one IS open, so
                // the comparison is armed rather than left saying "no
                // reference".
                assert!(
                    !matches!(app.reads[0].state, reads::CompareState::NoReference),
                    "{what}: a read taken while a document is open must be compared to it"
                );
            } else {
                assert!(app.reads.is_empty(), "{what}: a damaged file became a read");
                assert!(
                    app.notice.is_some(),
                    "{what}: the failure was not reported at all"
                );
                // And it names the format rather than saying "parse error",
                // which is what sends the user to the right tool.
                if i == 1 {
                    let n = app.notice.clone().unwrap_or_default();
                    assert!(n.contains("SCF"), "{what}: {n}");
                }
            }
        }
    }

    /// PROVEN TO FAIL at 78a46f2 for the same reason as the test above: there
    /// is no `reads` field to survive anything.
    ///
    /// With NO molecule open a trace still opens, and every number that would
    /// be about a pair is absent — because there is no pair. Then opening a
    /// molecule compares what is already held.
    #[test]
    fn a_trace_opens_with_no_molecule_and_claims_nothing_about_one() {
        let ctx = test_ctx();
        let mut app = App::blank();
        assert!(app.document().is_none());

        let path = std::env::temp_dir().join(format!("pl-gui-lonely-{}.ab1", std::process::id()));
        std::fs::write(
            &path,
            reads::tests::ab1(b"ACGTACGTACGTACGTACGTACGT", &[40u8; 24]),
        )
        .unwrap();
        app.load(path.clone());
        let _ = std::fs::remove_file(&path);

        assert_eq!(app.reads.len(), 1);
        assert!(app.document().is_none(), "no document was invented");
        // `error` renders as a full-screen takeover and is documented as right
        // for "there is no document" and wrong for anything else. A trace that
        // loaded fine is not an error.
        assert!(app.error.is_none());
        let r = &app.reads[0];
        assert!(matches!(r.state, reads::CompareState::NoReference));
        let v = r.verdict(0);
        assert!(v.contains("No construct is open"), "{v}");
        // NOT ONE NUMBER ABOUT A PAIR.
        assert!(!v.contains('%'), "{v}");
        assert!(!v.contains("identity"), "{v}");
        assert!(!v.contains("covers"), "{v}");
        // But the facts about the FILE are all there.
        let h = r.header().join(" | ");
        assert!(h.contains("24 bases"), "{h}");
        assert!(r.reliable().is_some(), "the Mott window needs no reference");

        // And the panel paints in that state rather than refusing.
        app.tab = Tab::Reads;
        let _ = paint_out(&mut app, &ctx, window());

        // Now a molecule arrives and the held read is compared to it.
        let mol = std::env::temp_dir().join(format!("pl-gui-late-{}.fa", std::process::id()));
        std::fs::write(&mol, ">x\nACGTACGTACGTACGTACGTACGTTTTTTTTTTTTT\n").unwrap();
        app.load(mol.clone());
        let _ = std::fs::remove_file(&mol);
        assert_eq!(
            app.reads.len(),
            1,
            "the read survived the document arriving"
        );
        assert!(
            !matches!(app.reads[0].state, reads::CompareState::NoReference),
            "it was not re-armed against the document that arrived"
        );
    }

    /// PROVEN TO FAIL against the MUTATION of dropping `seq_version` from
    /// `App::reads_for`: a report computed before an edit stays on screen
    /// beside a chromatogram that never changes, and reads as current.
    ///
    /// It is compile-only against 78a46f2, where there are no reads at all.
    #[test]
    fn an_edit_re_arms_every_read_rather_than_leaving_a_stale_report() {
        let ctx = test_ctx();
        let mut app = seq_app();
        let path = std::env::temp_dir().join(format!("pl-gui-stale-{}.ab1", std::process::id()));
        let seq = app.document().unwrap().molecule().seq.clone();
        // A read that really is from this molecule, so there is a report to go
        // stale in the first place.
        let read: Vec<u8> = seq.iter().copied().take(120).collect();
        std::fs::write(&path, reads::tests::ab1(&read, &[45u8; 120])).unwrap();
        app.load(path.clone());
        let _ = std::fs::remove_file(&path);
        assert_eq!(app.reads.len(), 1);
        for _ in 0..400 {
            app.refresh_reads();
            let _ = paint_out(&mut app, &ctx, window());
            if matches!(app.reads[0].state, reads::CompareState::Done(_)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let reads::CompareState::Done(before) = &app.reads[0].state else {
            panic!("the first comparison never finished: {:?}", app.reads_for);
        };
        let covered = before.covered;

        // Now move every base the read covers, by inserting in front of them.
        let d = app.bench.get_mut().expect("a document");
        d.apply(pl_core::OpKind::InsertAt {
            at: 1,
            seq: "GGGGGGGGGG".to_string(),
        })
        .expect("an ordinary insert");
        // The moment the sequence moved, the old report must be gone — either
        // re-running or already replaced, and never the one computed before.
        app.refresh_reads();
        assert!(
            !matches!(app.reads[0].state, reads::CompareState::Done(_)),
            "the report survived an edit that moved every base it describes"
        );
        for _ in 0..400 {
            app.refresh_reads();
            if matches!(app.reads[0].state, reads::CompareState::Done(_)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let reads::CompareState::Done(after) = &app.reads[0].state else {
            panic!("the re-comparison never finished");
        };
        assert_eq!(
            after.covered,
            (covered.0 + 10, covered.1 + 10),
            "the report still describes where the bases used to be"
        );
    }

    /// PROVEN TO FAIL at dfd6ac9: `adopt` reset the caret, the selection and the
    /// highlight but not `self.design`, so the panel survived a document swap
    /// holding the old file's title, length and report — and "Add to document"
    /// wrote file A's primer coordinates, under file A's name, into file B.
    #[test]
    fn opening_another_file_closes_the_design_panel_rather_than_reusing_it() {
        let mut app = app_designing();
        let seq = app.document().unwrap().molecule().seq.clone();
        design_frame(&mut app);
        app.design.as_mut().unwrap().run(&seq);

        let other: String = (0..900u32)
            .scan(999u64, |s, _| {
                *s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                Some(b"ACGT"[((*s >> 24) & 3) as usize] as char)
            })
            .collect();
        let path = std::env::temp_dir().join(format!("pl-gui-other-{}.fa", std::process::id()));
        std::fs::write(&path, format!(">b\n{other}\n")).unwrap();
        app.load(path.clone());
        let _ = std::fs::remove_file(&path);

        assert!(
            app.document().is_some(),
            "the premise: the second file opened"
        );
        assert!(app.design.is_none(), "the panel is gone");
        assert!(
            app.notice
                .as_deref()
                .unwrap_or_default()
                .contains("previous file"),
            "and it was said: {:?}",
            app.notice
        );
    }

    /// PROVEN TO FAIL at dfd6ac9: `GUI_TEMPLATE_LIMIT` was tested against
    /// `panel.bp`, the length snapshotted when the panel opened, rather than
    /// against the template the scan would read.
    #[test]
    fn the_template_limit_measures_the_sequence_it_is_about_to_scan() {
        let mut app = app_designing();
        let big = vec![b'A'; design::GUI_TEMPLATE_LIMIT as usize + 1];
        app.design.as_mut().unwrap().run(&big);
        let msg = match &app.design.as_ref().unwrap().result {
            Some(Err(e)) => e.clone(),
            other => panic!("expected a refusal, got {other:?}"),
        };
        assert!(msg.contains("stop responding"), "{msg}");
        assert!(msg.contains(&fmt_int(big.len() as u64)), "{msg}");
    }

    // -----------------------------------------------------------------------
    // The feature editor
    //
    // Everything here goes through `App::edit` -> `Document::apply` ->
    // `OpLog::apply`, which is the whole point: nothing writes
    // `Molecule::features`, so undo, the annotation remap and the WouldCorrupt
    // gate are inherited rather than reimplemented.
    // -----------------------------------------------------------------------

    /// The one feature that carries everything a form can silently destroy.
    /// Mirrors `featedit::tests::fixture`, and pKoV's `SacB` before it.
    fn rich_feature() -> pl_core::Feature {
        let mut f = pl_core::Feature::new("SacB", "CDS");
        f.strand = Strand::Reverse;
        f.segments = vec![
            pl_core::Segment {
                start: 100,
                end: 200,
                color: Some("#993366".into()),
                translated: true,
                kind: "standard".into(),
            },
            pl_core::Segment {
                start: 201,
                end: 260,
                color: Some("#993366".into()),
                translated: true,
                kind: "standard".into(),
            },
        ];
        f.set_qualifier("codon_start", "1");
        f.set_flag_qualifier("pseudo");
        f.set_qualifier("transl_table", "11");
        f.set_qualifier("replace", "");
        f
    }

    /// A 400 bp circular document with `rich_feature` already on it.
    fn app_with_feature() -> App {
        let seq: String = (0..400u32)
            .scan(4_242u64, |s, _| {
                *s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                Some(b"ACGT"[((*s >> 24) & 3) as usize] as char)
            })
            .collect();
        let mut app = App::blank();
        app.bench.set(
            Document::from_bytes(format!(">p\n{seq}\n").as_bytes(), "p.fa".into(), None).unwrap(),
        );
        assert!(app.edit(pl_core::OpKind::SetTopology(pl_core::Topology::Circular)));
        assert!(app.edit(pl_core::OpKind::SetFeature {
            index: None,
            feature: Box::new(rich_feature()),
        }));
        app
    }

    /// One frame of the feature editor, which is what refreshes `doc_at` and
    /// services a Save or a Delete.
    fn feature_frame(app: &mut App) {
        let ctx = test_ctx();
        ctx.begin_pass(egui::RawInput::default());
        app.feature_editor(&ctx);
        let _ = ctx.end_pass();
    }

    /// PROVEN TO FAIL at 04afbb6: `App` had no `feature_edit` field and no
    /// `open_feature_editor`, so there was no way to add a feature from the GUI
    /// at all — `SetFeature` was constructed at exactly one place, the primer
    /// path.
    #[test]
    fn a_feature_added_from_a_selection_covers_the_bases_that_were_selected() {
        let mut app = app_with_feature();
        // Carets sit BETWEEN bases: this is bases 10..29 inclusive.
        app.edit.sel = Some(seqedit::Selection {
            anchor: 9,
            head: 29,
            through_origin: false,
        });
        app.open_feature_editor(None);
        let p = app.feature_edit.as_mut().expect("the editor opened");
        assert_eq!(
            (p.segments[0].start, p.segments[0].end),
            (10, 29),
            "one base out at either end is a feature that is legal and in the \
             wrong place, and nothing anywhere would contradict it"
        );
        p.name = "myPromoter".into();
        p.kind = "promoter".into();
        p.save = true;
        feature_frame(&mut app);

        assert!(app.feature_edit.is_none(), "the window closed on success");
        let mol = app.document().unwrap().molecule();
        assert_eq!(mol.features.len(), 2);
        let f = mol.features.last().unwrap();
        assert_eq!(f.name, "myPromoter");
        assert_eq!(f.kind, "promoter");
        assert_eq!(f.segments.len(), 1);
        assert_eq!((f.segments[0].start, f.segments[0].end), (10, 29));
        assert_eq!(
            app.selected,
            Some(1),
            "and the new feature is the selected one"
        );

        // ONE operation, so ONE undo.
        app.do_undo();
        assert_eq!(
            app.document().unwrap().molecule().features.len(),
            1,
            "one undo removes it"
        );
    }

    /// PROVEN TO FAIL at 04afbb6: `App::edit` cleared `edit.sel` after every
    /// successful operation, including the two that move no bases. Select 900 bp,
    /// add the CDS, and the highlight vanished on a molecule where nothing had
    /// moved — so the promoter and the RBS that come next had to be re-dragged
    /// by hand.
    #[test]
    fn an_annotation_edit_keeps_the_selection_and_a_base_edit_does_not() {
        let mut app = app_with_feature();
        let sel = seqedit::Selection {
            anchor: 9,
            head: 29,
            through_origin: false,
        };

        app.edit.sel = Some(sel);
        assert!(app.edit(pl_core::OpKind::SetFeature {
            index: None,
            feature: Box::new(rich_feature()),
        }));
        assert_eq!(
            app.edit.sel,
            Some(sel),
            "SetFeature moves no bases, so the arc is exactly where it was"
        );

        assert!(app.edit(pl_core::OpKind::RemoveFeature { index: 1 }));
        assert_eq!(app.edit.sel, Some(sel), "and neither does RemoveFeature");

        // The ops that DO move bases must still collapse it: the arc they name
        // may no longer exist.
        app.edit.sel = Some(sel);
        assert!(app.edit(pl_core::OpKind::InsertAt {
            at: 5,
            seq: "AAAA".into(),
        }));
        assert_eq!(app.edit.sel, None, "an insertion still clears it");
        app.edit.sel = Some(sel);
        assert!(app.edit(pl_core::OpKind::ReverseComplement));
        assert_eq!(app.edit.sel, None, "and so does a reverse complement");
    }

    /// PROVEN TO FAIL at 04afbb6: no feature editor to edit through.
    ///
    /// The App-level half of `featedit::tests::renaming_a_feature_changes_only_
    /// its_name`: the same claim, but made about what actually landed in the
    /// document through `OpKind::SetFeature` and the corruption gate.
    #[test]
    fn renaming_through_the_editor_keeps_every_qualifier_colour_and_segment() {
        let mut app = app_with_feature();
        let before = app.document().unwrap().molecule().features[0].clone();

        app.open_feature_editor(Some(0));
        let p = app.feature_edit.as_mut().expect("the editor opened");
        p.name = "levansucrase".into();
        p.save = true;
        feature_frame(&mut app);

        let after = &app.document().unwrap().molecule().features[0];
        assert_eq!(after.name, "levansucrase");
        assert_eq!(after.segments.len(), 2, "still two segments");
        for (i, (a, b)) in after.segments.iter().zip(&before.segments).enumerate() {
            assert_eq!(a.start, b.start, "segment {i} start");
            assert_eq!(a.end, b.end, "segment {i} end");
            assert_eq!(a.color, b.color, "segment {i} colour");
            assert_eq!(a.translated, b.translated, "segment {i} translated");
            assert_eq!(a.kind, b.kind, "segment {i} kind");
        }
        assert!(after.has_qualifier("pseudo"), "the flag qualifier survived");
        assert_eq!(after.qualifier("pseudo"), None, "and is still valueless");
        assert_eq!(after.qualifier("replace"), Some(""), "empty is still empty");
        assert_eq!(
            after.qualifiers, before.qualifiers,
            "order, repeats and all"
        );
        assert_eq!(after.strand, before.strand);

        // One operation, one undo, and the History line names the feature.
        let last = app.document().unwrap().log.path().len();
        assert_eq!(last, 3, "SetTopology, SetFeature, and this one");
        app.do_undo();
        assert_eq!(app.document().unwrap().molecule().features[0].name, "SacB");
    }

    /// PROVEN TO FAIL at 04afbb6: no feature editor.
    ///
    /// Struct equality after `SetFeature` does not prove the WRITER kept
    /// anything, and the writers are where the valueless qualifier and the
    /// per-segment colour actually go missing.
    #[test]
    fn a_feature_added_here_survives_a_round_trip_through_dna_and_genbank() {
        let mut app = app_with_feature();
        app.edit.sel = Some(seqedit::Selection {
            anchor: 299,
            head: 340,
            through_origin: false,
        });
        app.open_feature_editor(None);
        let p = app.feature_edit.as_mut().expect("the editor opened");
        p.name = "decR".into();
        p.kind = "CDS".into();
        p.kind_other = false;
        p.color = featedit::ColorMode::One("#993366".into());
        p.segments[0].translated = true;
        p.quals.push(featedit::QualRow {
            key: "pseudo".into(),
            has_value: false,
            value: String::new(),
        });
        p.quals.push(featedit::QualRow {
            key: "codon_start".into(),
            has_value: true,
            value: "1".into(),
        });
        p.save = true;
        feature_frame(&mut app);

        let mol = app.document().unwrap().molecule().clone();
        assert_eq!(mol.features.len(), 2, "the premise: it landed");

        // .dna
        let bytes = pl_fileio::snapgene::from_molecule(&mol);
        let (back, _) = pl_fileio::load(&bytes).expect("the .dna reads");
        let f = back
            .features
            .iter()
            .find(|f| f.name == "decR")
            .expect(".dna kept the feature");
        assert_eq!((f.segments[0].start, f.segments[0].end), (300, 340));
        assert_eq!(f.color(), Some("#993366"), ".dna kept the colour");
        assert!(f.segments[0].translated, ".dna kept the translated flag");
        assert!(f.has_qualifier("pseudo"), ".dna kept /pseudo");
        assert_eq!(f.qualifier("pseudo"), None, ".dna kept it VALUELESS");
        assert_eq!(f.qualifier("codon_start"), Some("1"));

        // GenBank
        let (text, unwritable) = pl_fileio::genbank::write_reporting(&mol, "p", today());
        assert!(unwritable.is_empty(), "{unwritable:?}");
        let back = pl_fileio::genbank::parse(&text);
        let f = back
            .features
            .iter()
            .find(|f| f.name == "decR")
            .expect(".gb kept the feature");
        assert_eq!((f.segments[0].start, f.segments[0].end), (300, 340));
        assert_eq!(f.color(), Some("#993366"), ".gb kept the colour");
        assert!(f.has_qualifier("pseudo"), ".gb kept /pseudo");
        assert_eq!(
            f.qualifier("pseudo"),
            None,
            ".gb wrote it BARE — a /pseudo=\"\" here is a pseudogene exported as \
             an ordinary protein-coding one"
        );
        assert_eq!(f.qualifier("codon_start"), Some("1"));
    }

    /// PROVEN TO FAIL at 04afbb6: no feature editor.
    #[test]
    fn an_origin_crossing_feature_added_here_still_crosses_the_origin() {
        let mut app = app_with_feature();
        // The other arc: from base 380 forwards through base 1 to base 40.
        app.edit.sel = Some(seqedit::Selection {
            anchor: 40,
            head: 379,
            through_origin: true,
        });
        app.open_feature_editor(None);
        let p = app.feature_edit.as_mut().expect("the editor opened");
        assert_eq!((p.segments[0].start, p.segments[0].end), (380, 40));
        assert!(p.segments[0].wraps, "read from the selection, not guessed");
        assert_eq!(p.row_bases(&p.segments[0]), 400 - 380 + 1 + 40);
        p.name = "across".into();
        p.save = true;
        feature_frame(&mut app);

        let mol = app.document().unwrap().molecule();
        let f = mol.features.iter().find(|f| f.name == "across").unwrap();
        assert_eq!((f.segments[0].start, f.segments[0].end), (380, 40));
        assert_eq!(f.extent(400, true), Some((380, 40)));

        // Through GenBank it comes back as the two-part join that is INSDC's only
        // spelling, and `extent` still reads it as the same wrap. That is not a
        // loss and must not be re-merged on load.
        let text = pl_fileio::genbank::write(mol, "p", today());
        let back = pl_fileio::genbank::parse(&text);
        let f = back.features.iter().find(|f| f.name == "across").unwrap();
        assert_eq!(f.segments.len(), 2, "join(380..400,1..40)");
        assert_eq!(f.extent(400, true), Some((380, 40)));
    }

    /// PROVEN TO FAIL at 04afbb6: no feature editor, so no guard to test.
    #[test]
    fn an_undo_is_not_taken_while_the_feature_editor_is_open() {
        let mut app = app_with_feature();
        app.open_feature_editor(Some(0));
        assert!(app.feature_edit.is_some(), "the premise");
        assert!(!shortcuts_with(&app, egui::Key::Z, None).undo);
        assert!(!shortcuts_with(&app, egui::Key::Y, None).redo);
        // Opening another file is still allowed; it closes the editor and says so.
        assert!(shortcuts_with(&app, egui::Key::O, None).open);
        // And saving, for the same reason as the design panel: an undo changes
        // what the window is describing, and a save changes nothing.
        assert!(shortcuts_with(&app, egui::Key::S, None).save);
    }

    /// PROVEN TO FAIL at 04afbb6: no feature editor.
    ///
    /// Two non-modal windows each guarding the keyboard for a reason the other
    /// does not know about is the state the design panel's own comment warns
    /// against.
    #[test]
    fn the_feature_editor_and_the_design_panel_are_never_both_open() {
        let mut app = app_designing();
        app.open_feature_editor(None);
        assert!(app.feature_edit.is_some());
        assert!(app.design.is_none(), "the design panel gave way");

        app.edit.sel = Some(seqedit::Selection {
            anchor: 300,
            head: 600,
            through_origin: false,
        });
        app.open_design();
        assert!(app.design.is_some());
        assert!(app.feature_edit.is_none(), "and the other way round");
    }

    /// PROVEN TO FAIL at 04afbb6: no feature editor.
    ///
    /// An operation that changes nothing still derives an id, spends an undo
    /// step and dirties the document, and the title bar then claims unsaved
    /// changes that do not exist.
    #[test]
    fn a_save_that_changes_nothing_records_no_operation() {
        let mut app = app_with_feature();
        let before = app.document().unwrap().log.all_ops().len();
        app.open_feature_editor(Some(0));
        app.feature_edit.as_mut().unwrap().save = true;
        feature_frame(&mut app);
        assert_eq!(
            app.document().unwrap().log.all_ops().len(),
            before,
            "nothing was recorded"
        );
        assert!(app.feature_edit.is_none(), "and the window still closed");
        assert!(app.status.contains("nothing was changed"), "{}", app.status);
    }

    /// PROVEN TO FAIL at 04afbb6: no feature editor.
    ///
    /// The user must never be able to press OK into a `WouldCorrupt`. Its
    /// message is pitched at the whole molecule — "feature 9 'PhoP' segment 0:
    /// 9000 is past the 8,117 bp molecule" — and for an add the index names a
    /// feature that does not exist after the refusal.
    #[test]
    fn a_save_with_a_refusal_outstanding_never_reaches_the_document() {
        // A type with a space: the writer's key column swallows the rest as
        // coordinates and the feature vanishes on the next open, with exit 0.
        // `validate()` has nothing to say about it, so only the form can.
        let mut app = app_with_feature();
        let ops = app.document().unwrap().log.all_ops().len();
        app.open_feature_editor(Some(0));
        app.feature_edit.as_mut().unwrap().kind = "signal peptide".into();
        app.feature_edit.as_mut().unwrap().save = true;
        feature_frame(&mut app);
        assert_eq!(
            app.document().unwrap().molecule().features[0].kind,
            "CDS",
            "nothing landed"
        );
        assert_eq!(app.document().unwrap().log.all_ops().len(), ops);
        assert!(
            app.feature_edit.is_some(),
            "and the window stayed open, with every box as the user left it"
        );
        // IN THE PANEL, not in `App::notice`. `central` paints that banner at
        // the top-left of the map, which is where this window sits: the
        // explanation rendered behind the thing the user was looking at.
        let notice = app
            .feature_edit
            .as_ref()
            .unwrap()
            .notice
            .clone()
            .unwrap_or_default();
        assert!(notice.contains("space"), "{notice}");
        assert!(
            app.notice.is_none(),
            "and not behind the window: {:?}",
            app.notice
        );

        // A COORDINATE PAST THE END IS REFUSED, NOT TRUNCATED. This assertion
        // used to read the other way — "the control clamped it to the molecule;
        // 9,000 was never committed" — and that was the defect written down as
        // a virtue. The clamp it was describing is `DragValue`'s
        // `clamp_existing_to_range`, which does not only stop the user TYPING
        // 9,000: it rewrites a coordinate the FILE carried, on a plain layout
        // pass with no input at all. See the comment at that DragValue for what
        // it cost on this repository's own `odd.gb`. The form's own sentence is
        // the answer, and it names the box.
        app.feature_edit.as_mut().unwrap().notice = None;
        app.feature_edit.as_mut().unwrap().kind = "CDS".into();
        app.feature_edit.as_mut().unwrap().segments[0].end = 9_000;
        app.feature_edit.as_mut().unwrap().save = true;
        feature_frame(&mut app);
        let mol = app.document().unwrap().molecule();
        assert_eq!(
            mol.features[0].segments[0].end, 200,
            "nothing landed: the feature is still the one the document had"
        );
        assert_eq!(app.document().unwrap().log.all_ops().len(), ops);
        let notice = app
            .feature_edit
            .as_ref()
            .unwrap()
            .notice
            .clone()
            .unwrap_or_default();
        assert!(
            notice.contains("9,000") && notice.contains("400 bp"),
            "the refusal names the number and the molecule: {notice}"
        );
        assert!(
            !notice.contains("inconsistent"),
            "and it is the FORM's sentence, not the gate's whole-molecule one: {notice}"
        );
    }

    /// One frame of the feature editor, keeping the shapes.
    ///
    /// A real `screen_rect`, unlike `feature_frame`: with `RawInput::default()`
    /// an `egui::Window` has no screen to be placed on and the pass emits no
    /// shapes at all, so a test reading them would pass by seeing nothing.
    /// The same `ctx` across calls, because a window's size is learnt from the
    /// pass before.
    fn feature_frame_out(app: &mut App, ctx: &egui::Context) -> egui::FullOutput {
        ctx.begin_pass(window());
        app.feature_editor(ctx);
        ctx.end_pass()
    }

    /// The colour a piece of text was drawn in, if it was drawn.
    fn text_colour(out: &egui::FullOutput, needle: &str) -> Option<egui::Color32> {
        out.shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == needle => {
                Some(t.galley.job.sections.first()?.format.color)
            }
            _ => None,
        })
    }

    /// PROVEN TO FAIL against the working code as delivered: `Delete feature`
    /// was drawn outside the `refusals.is_empty() && stale_reason().is_none()`
    /// that gates Save, so on a stale form the two footer buttons made
    /// contradictory claims about one state — and the only one greyed out was
    /// the safe one. Pressing it produced a notice instead of the action it
    /// advertised, and that notice went behind the window.
    #[test]
    fn a_stale_form_greys_the_delete_button_as_well_as_save() {
        let mut app = app_with_feature();
        let mut other = pl_core::Feature::new("AmpR", "CDS");
        other.segments.push(pl_core::Segment::new(300, 350));
        assert!(app.edit(pl_core::OpKind::SetFeature {
            index: None,
            feature: Box::new(other),
        }));
        app.open_feature_editor(Some(1));

        // Live first, so this cannot pass by the button never being drawn.
        let ctx = test_ctx();
        let mut out = feature_frame_out(&mut app, &ctx);
        for _ in 0..2 {
            out = feature_frame_out(&mut app, &ctx);
        }
        let pal = theme::Palette::of(true);
        assert_eq!(
            text_colour(&out, "Delete feature"),
            Some(pal.warn),
            "the premise: on a live form it is offered"
        );

        // The window is not modal: the toolbar stayed live behind it.
        assert!(app.edit(pl_core::OpKind::RemoveFeature { index: 0 }));
        let out = feature_frame_out(&mut app, &ctx);
        assert!(
            app.feature_edit.as_ref().unwrap().stale_reason().is_some(),
            "the premise: the document moved"
        );
        assert_eq!(
            text_colour(&out, "Delete feature"),
            Some(pal.muted),
            "and now it is drawn as unavailable, like Save beside it"
        );
    }

    /// PROVEN TO FAIL against the working code as delivered: the per-segment
    /// colour box used `desired_width(72.0)` inside an `egui::Grid` and
    /// photographed as `#993`, four of seven characters — in the one control
    /// whose entire job is the exact colour, on the code path that exists so
    /// disagreeing segment colours are not silently flattened. `#993366` and
    /// `#9933ff` are the same box until the last two characters are on screen.
    ///
    /// It is the trap the qualifier key box already documents and fixes: a
    /// `TextEdit` sizes to `min(desired, available)`, and inside a Grid
    /// `available` is last frame's column width, which never grows for a widget
    /// that only ever asks for what is available.
    #[test]
    fn the_per_segment_colour_box_shows_a_whole_colour() {
        let mut app = app_with_feature();
        app.open_feature_editor(Some(0));
        app.feature_edit.as_mut().unwrap().color = featedit::ColorMode::PerSegment;
        let ctx = test_ctx();
        let mut out = feature_frame_out(&mut app, &ctx);
        for _ in 0..3 {
            out = feature_frame_out(&mut app, &ctx);
        }

        // The galley is laid out whole and CUT by the widget's clip rect, so the
        // text is present either way and only the geometry tells the truth.
        let mut seen = 0;
        for cs in &out.shapes {
            if let egui::Shape::Text(t) = &cs.shape {
                if t.galley.text() != "#993366" {
                    continue;
                }
                seen += 1;
                let b = egui::Rect::from_min_size(t.pos, t.galley.size());
                let cut = (cs.clip_rect.left() - b.left())
                    .max(b.right() - cs.clip_rect.right())
                    .max(0.0);
                assert!(cut < 1.0, "{cut:.0} pt of #993366 is cut off its own box");
            }
        }
        assert_eq!(
            seen, 2,
            "the premise: two segment rows, each with the colour in a box"
        );
    }

    /// PROVEN TO FAIL against the working code as delivered: `open_row` never
    /// set `selected`, so double-clicking a Features row opened the editor with
    /// the row unhighlighted, the map arc unhighlighted and the toolbar's
    /// Edit…/Duplicate/Remove all disabled — the window title was the only clue
    /// which feature was open. The map path had the line; the list path did not.
    #[test]
    fn opening_the_editor_highlights_the_feature_it_opened_on() {
        let mut app = app_with_feature();
        // What a double-click leaves behind: egui delivers `clicked()` on the
        // first press, and every row handler in this file toggles.
        app.selected = Some(0);
        app.selected = None;
        app.open_feature_editor(Some(0));
        assert_eq!(
            app.selected,
            Some(0),
            "the list, the map and the toolbar all read this"
        );

        // An add highlights nothing until it lands — there is no index yet.
        app.feature_edit = None;
        app.selected = None;
        app.open_feature_editor(None);
        assert_eq!(app.selected, None);
    }

    /// PROVEN TO FAIL against the WORKING CODE AS DELIVERED, on this
    /// repository's own fixture, with no input at all.
    ///
    /// `egui::DragValue` defaults to `clamp_existing_to_range(true)`, which
    /// rewrites a value it was merely asked to draw. `tests/library-fixture/
    /// odd.gb` is a 7 bp circular record carrying `CDS 1..9` and `misc_feature
    /// 10..15` — coordinates the app's own title bar reports as problems the
    /// moment the file opens, which is exactly the file a user opens the editor
    /// ON. Measured before the fix: one frame turned `10..15` into `7..7`, so
    /// renaming "spacer" committed a feature five of whose six bases were gone
    /// and which had moved three bases, with `notice == None` and a status line
    /// that said only "edit feature 1". The `WouldCorrupt` gate cannot help,
    /// because the edit REDUCES the `PastEnd` count.
    ///
    /// Every form-level test in `featedit` is blind to this class: none of them
    /// draws.
    #[test]
    fn drawing_the_form_does_not_move_a_coordinate_the_file_carried() {
        let gb = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/library-fixture/odd.gb"
        ))
        .expect("the library fixture is in the tree");
        let mut app = App::blank();
        app.adopt(Document::from_bytes(&gb, "odd.gb".into(), None).unwrap());

        let before = app.document().unwrap().molecule().features.clone();
        // NON-VACUITY: this test is worthless on a molecule whose features all
        // fit. `odd.gb` is in the tree precisely because they do not.
        assert!(
            app.document()
                .as_ref()
                .unwrap()
                .molecule()
                .validate()
                .iter()
                .any(|x| x.kind() == "past the end"),
            "the fixture has nothing to lose: {before:?}"
        );

        for (i, want) in before.iter().enumerate() {
            app.open_feature_editor(Some(i));
            feature_frame(&mut app);
            let p = app.feature_edit.as_ref().expect("the editor stayed open");
            assert_eq!(
                &p.to_feature(),
                want,
                "one layout pass, no input, and feature {i} came back different"
            );
            assert!(
                !p.dirty(),
                "an untouched form is not unsaved work: closing it would raise a \
                 false warning, and `is_noop` would let a Save spend an undo step"
            );
            assert!(
                !p.refusals().is_empty(),
                "and the number IS refused, so Save is disabled and the user is told why"
            );
            app.feature_edit = None;
        }

        // The rename a user would actually do, all the way through: refused,
        // because the coordinate the file carries is still on screen and still
        // wrong. Nothing is committed and nothing is truncated.
        let ops = app.document().unwrap().log.all_ops().len();
        app.open_feature_editor(Some(1));
        feature_frame(&mut app);
        app.feature_edit.as_mut().unwrap().name = "spacer2".into();
        app.feature_edit.as_mut().unwrap().save = true;
        feature_frame(&mut app);
        assert_eq!(
            app.document().unwrap().molecule().features,
            before,
            "nothing landed"
        );
        assert_eq!(app.document().unwrap().log.all_ops().len(), ops);
        let notice = app
            .feature_edit
            .as_ref()
            .and_then(|p| p.notice.clone())
            .unwrap_or_default();
        assert!(notice.contains("past the"), "{notice}");
    }

    /// PROVEN TO FAIL against the working code as delivered.
    ///
    /// `genbank::parse` stores `/ApEinfo_fwdcolor="cyan"` verbatim, and every
    /// ApE- and Benchling-authored plasmid in circulation carries a name rather
    /// than a hex triple. Refusing it meant Save was permanently greyed on a
    /// feature the user had never touched, with a hover blaming a box they had
    /// not filled in — and the one escape, "from its type", discards the file's
    /// colour. A rename must not cost a colour.
    #[test]
    fn a_colour_the_file_carried_does_not_block_a_rename_and_is_not_discarded() {
        let gb = "LOCUS       x                      400 bp    DNA     circular SYN 01-JAN-2026\n\
                  FEATURES             Location/Qualifiers\n     \
                  CDS             100..200\n                     \
                  /label=\"AmpR\"\n                     \
                  /ApEinfo_fwdcolor=\"cyan\"\n\
                  ORIGIN\n//\n";
        let mut app = App::blank();
        app.adopt(Document::from_bytes(gb.as_bytes(), "x.gb".into(), None).unwrap());
        assert_eq!(
            app.document().unwrap().molecule().features[0].segments[0].color,
            Some("cyan".into()),
            "the premise: the reader keeps it verbatim"
        );

        app.open_feature_editor(Some(0));
        feature_frame(&mut app);
        {
            let p = app.feature_edit.as_mut().unwrap();
            assert!(
                p.refusals().is_empty(),
                "Save must not be disabled on a file the user has not touched: {:?}",
                p.refusals()
            );
            assert!(
                p.warnings().iter().any(|w| w.contains("cyan")),
                "but the user is told the map cannot draw it: {:?}",
                p.warnings()
            );
            p.name = "bla".into();
            p.save = true;
        }
        feature_frame(&mut app);
        let f = &app.document().unwrap().molecule().features[0];
        assert_eq!(f.name, "bla", "the rename landed");
        assert_eq!(
            f.segments[0].color,
            Some("cyan".into()),
            "and the file's own colour is still the file's own colour"
        );
    }

    /// PROVEN TO FAIL at 04afbb6: `describe()` reads "remove feature 0" and the
    /// Molecule-menu path cleared `selected` but left `hot`, so the pointer's
    /// feature index survived a removal that shifted every later index — a
    /// different feature, drawn highlighted on the map.
    #[test]
    fn removing_a_feature_names_it_and_clears_the_hot_row() {
        let mut app = app_with_feature();
        app.selected = Some(0);
        app.hot = Some(0);
        app.open_feature_editor(Some(0));
        app.feature_edit.as_mut().unwrap().delete = true;
        feature_frame(&mut app);

        assert!(app.document().unwrap().molecule().features.is_empty());
        assert!(
            app.status.contains("SacB"),
            "the line the user reads names the feature: {}",
            app.status
        );
        assert_eq!(app.selected, None);
        assert_eq!(
            app.hot, None,
            "the pointer's index would name another feature"
        );
        assert!(app.feature_edit.is_none());
    }

    /// PROVEN TO FAIL at 04afbb6: no feature editor.
    ///
    /// `RemoveFeature` shifts every later index, so a form holding `Some(1)`
    /// across one writes its feature over whatever is now at index 1.
    #[test]
    fn an_edit_underneath_the_editor_refuses_the_save_rather_than_writing_through() {
        let mut app = app_with_feature();
        let mut other = pl_core::Feature::new("AmpR", "CDS");
        other.segments.push(pl_core::Segment::new(300, 350));
        assert!(app.edit(pl_core::OpKind::SetFeature {
            index: None,
            feature: Box::new(other),
        }));

        app.open_feature_editor(Some(1));
        feature_frame(&mut app);
        app.feature_edit.as_mut().unwrap().name = "renamed".into();

        // The window is not modal: the toolbar stayed live behind it.
        assert!(app.edit(pl_core::OpKind::RemoveFeature { index: 0 }));

        app.feature_edit.as_mut().unwrap().save = true;
        feature_frame(&mut app);
        assert_eq!(
            app.document().unwrap().molecule().features[0].name,
            "AmpR",
            "the feature that moved into index 1's old place was not overwritten"
        );
        // In the panel, where the button that was refused is.
        assert!(
            app.feature_edit
                .as_ref()
                .and_then(|p| p.notice.as_deref())
                .unwrap_or_default()
                .contains("changed"),
            "{:?}",
            app.feature_edit.as_ref().and_then(|p| p.notice.clone())
        );
    }

    /// PROVEN TO FAIL at 04afbb6: no `duplicate_feature`.
    #[test]
    fn duplicating_a_feature_copies_everything_the_model_holds() {
        let mut app = app_with_feature();
        app.duplicate_feature(0);
        let mol = app.document().unwrap().molecule();
        assert_eq!(mol.features.len(), 2);
        let (a, b) = (&mol.features[0], &mol.features[1]);
        assert_eq!(b.name, "SacB copy");
        assert_eq!(b.segments, a.segments, "both segments, both colours");
        assert_eq!(b.qualifiers, a.qualifiers, "including the valueless one");
        assert_eq!(b.strand, a.strand);
        assert_eq!(app.selected, Some(1));
    }

    /// PROVEN TO FAIL at 04afbb6: the two menu items did not exist.
    #[test]
    fn the_molecule_menu_names_the_feature_editor() {
        // Consts rather than literals, for the reason `SET_ORIGIN_ITEM` is one:
        // prose elsewhere points at these paths and drifts silently otherwise.
        assert!(ADD_FEATURE_ITEM.ends_with('…'), "it opens a window");
        assert!(EDIT_FEATURE_ITEM.ends_with('…'));
        assert_ne!(ADD_FEATURE_ITEM, EDIT_FEATURE_ITEM);
    }

    /// PROVEN TO FAIL at the first cut of this window, which was photographed:
    /// the row buttons were `✕` and the qualifier disclosure was `▾`/`▸`, and
    /// all three came out as empty boxes on screen. Same trap as
    /// `menu_with_caret`'s U+25BE and `strand_word`'s U+2190 — the third time
    /// this project has paid for asking a font for chrome, and the first time a
    /// test catches it instead of a screenshot.
    #[test]
    fn the_feature_editors_own_glyphs_are_in_the_face_that_draws_them() {
        let ctx = test_ctx();
        // The premise: the oracle really does say no to the characters that were
        // wrong. Without this the test could pass by being blind.
        for bad in ['\u{2715}', '\u{25BE}', '\u{25B8}', '\u{21C5}'] {
            assert!(
                renders_as_tofu(&ctx, egui::FontFamily::Proportional, 11.0, bad),
                "U+{:04X} is drawable after all, so this test proves nothing",
                bad as u32
            );
        }

        // A panel wound up to say as much as it can: every refusal and every
        // warning, which is where the prose — and any character in it — lives.
        let mut f = rich_feature();
        f.strand = Strand::Unoriented;
        f.segments.push(pl_core::Segment::new(150, 260));
        f.segments.push(pl_core::Segment::new(120, 130));
        f.set_qualifier("label", "x");
        f.set_qualifier("note", "clone #1a2b3c from the -80");
        let mut p = featedit::Panel::open(Some(0), f, 400, false, None).unwrap();
        p.kind = "signal peptide and a very long one".into();
        p.color = featedit::ColorMode::One("#abc".into());
        p.name.clear();

        let mut said: Vec<String> = vec![featedit::UP.into(), featedit::DELETE.into()];
        said.extend(p.refusals());
        said.extend(p.warnings());
        said.extend(featedit::KINDS.iter().map(|k| (*k).to_string()));
        assert!(said.len() > 8, "the premise: it had plenty to say");

        for s in &said {
            for c in s.chars() {
                assert!(
                    !renders_as_tofu(&ctx, egui::FontFamily::Proportional, 11.0, c),
                    "U+{:04X} {c:?} has no glyph in the face that draws the feature \
                     editor, so it renders as an empty box: {s:?}",
                    c as u32
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // The split, and the grid it changes the shape of
    //
    // These drive real frames. A layout verified only by reading the code is
    // not verified: the whole complaint being answered here is about where
    // things ended up on screen.
    // -----------------------------------------------------------------------

    /// The user's window, at the size the screenshots were taken at.
    fn window() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 840.0),
            )),
            ..Default::default()
        }
    }

    /// The same window with the clock set.
    ///
    /// egui decides a press is a drag rather than a click once it has moved
    /// more than `max_click_dist` (6 pt) OR been held longer than
    /// `max_click_duration` (0.8 s). Six points is less than one cell, so a
    /// test that starts a drag by *moving* cannot start it on a named column —
    /// it starts on the next one. Holding still and advancing the clock starts
    /// the drag exactly where the press landed, which is what a person's hand
    /// does anyway.
    fn window_at(t: f64) -> egui::RawInput {
        egui::RawInput {
            time: Some(t),
            ..window()
        }
    }

    fn pointer_to(to: egui::Pos2) -> egui::RawInput {
        egui::RawInput {
            events: vec![egui::Event::PointerMoved(to)],
            ..window()
        }
    }

    fn pointer_to_at(to: egui::Pos2, t: f64) -> egui::RawInput {
        egui::RawInput {
            events: vec![egui::Event::PointerMoved(to)],
            ..window_at(t)
        }
    }

    fn pointer_button(at: egui::Pos2, pressed: bool) -> egui::RawInput {
        egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..window()
        }
    }

    fn pointer_button_at(at: egui::Pos2, pressed: bool, t: f64) -> egui::RawInput {
        egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..window_at(t)
        }
    }

    /// How far past the bottom of the WINDOW the worst-placed thing in this
    /// frame was drawn.
    ///
    /// The clipping defect measured directly rather than inferred. Deliberately
    /// the window edge and not each shape's own clip rect: a `ScrollArea`
    /// clips its own content on purpose, and by up to a whole row, so "outside
    /// its clip rect" would flag ordinary scrolling. Nothing may be laid out
    /// below the window, and at bd96e5b the readout was — by about 40 pt,
    /// taking the origin warning's second half and the Design primers button
    /// with it.
    fn drawn_below_the_window(out: &egui::FullOutput, window_h: f32) -> f32 {
        out.shapes
            .iter()
            .map(|cs| {
                let b = cs.shape.visual_bounding_rect();
                if b.is_finite() && b.is_positive() {
                    (b.bottom() - window_h).max(0.0)
                } else {
                    0.0
                }
            })
            .fold(0.0f32, f32::max)
    }

    /// The worst horizontal clipping of any text laid out in the band `y`, as
    /// `(points cut, the text)`. A zero first element means nothing is cut.
    ///
    /// Measured against each shape's OWN `clip_rect`, which is the rectangle
    /// egui will really cut it against. Comparing against the panel's response
    /// rect instead is circular, and was tried first: an overflowing row makes
    /// that rect wider, so the band grows with the defect and the check passes
    /// over it. Measured on the unwrapped row, `seq_readout` came back 350 pt
    /// wide inside a 284 pt clip, with `take the other arc (8,090 bp)` starting
    /// 54 pt left of it.
    ///
    /// Text only, and deliberately: egui clips at paint time, so a galley that
    /// does not fit is still laid out at full width and simply has its left end
    /// cut. That is the failure — a label reading "her arc (8,090 bp)" — and the
    /// shape list is where it is visible. Backgrounds and separators
    /// legitimately span their whole clip rect.
    fn text_clipped_horizontally(out: &egui::FullOutput, y: egui::Rangef) -> (f32, String) {
        out.shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::Shape::Text(t) => {
                    let b = egui::Rect::from_min_size(t.pos, t.galley.size());
                    if !b.is_finite() || !b.is_positive() || !y.contains(b.center().y) {
                        return None;
                    }
                    let cut = (cs.clip_rect.left() - b.left())
                        .max(b.right() - cs.clip_rect.right())
                        .max(0.0);
                    Some((cut, t.galley.text().to_string()))
                }
                _ => None,
            })
            .fold(
                (0.0f32, String::new()),
                |acc, x| {
                    if x.0 > acc.0 {
                        x
                    } else {
                        acc
                    }
                },
            )
    }

    /// One frame of the whole details panel.
    fn paint(app: &mut App, ctx: &egui::Context, input: egui::RawInput) {
        // The same shape `eframe::App::ui` is handed: a root `Ui` over the
        // whole window, with the details panel taking its share of it and a
        // CentralPanel behind for whatever is left. Driving `side_panel` inside
        // an `Area` instead would give it an unbounded width, and the width is
        // the entire question here.
        let _ = paint_out(app, ctx, input);
    }

    fn paint_out(app: &mut App, ctx: &egui::Context, input: egui::RawInput) -> egui::FullOutput {
        ctx.run_ui(input, |ui| {
            app.side_panel(ui);
            egui::CentralPanel::default().show(ui, |ui| {
                ui.allocate_space(ui.available_size());
            });
        })
    }

    /// A plasmid the size of the user's, open on the Sequence tab.
    /// PROVEN TO FAIL at a79a276: the name was a bare `ui.label` allocated
    /// before the coordinates, and a bare label asks for the width of its whole
    /// string. docs/UX-REVIEW-2026-07-31.md finding 7 measured what one
    /// 150-character `/label` then did to the pane that carries every other
    /// view: the tab strip read `ce  History  File`, the button row was gone,
    /// the coordinates were off the right edge of the window, and the splitter
    /// would not move.
    ///
    /// Asserted against a SHORT-NAMED control rather than against fixed
    /// numbers, so it measures the name's influence on the layout rather than
    /// the layout — which is what must be zero, and what no absolute bound
    /// would pin down.
    #[test]
    fn a_long_feature_name_cannot_lay_out_the_panel() {
        // Position, text and DRAWN WIDTH. The width is the point: `galley.text()`
        // returns the original string whether or not it was truncated, because
        // truncation is a property of the galley's size, so text alone cannot
        // tell a cut-down name from a full one.
        fn panel_texts(name: &str) -> Vec<(egui::Pos2, String, f32)> {
            let mut app = seq_app();
            app.tab = Tab::Features;
            let d = app.bench.get_mut().expect("a document");
            let mut f = pl_core::Feature::new(name, "CDS");
            f.strand = pl_core::Strand::Forward;
            f.segments.push(pl_core::Segment::new(400, 1_400));
            d.apply(pl_core::OpKind::SetFeature {
                index: None,
                feature: Box::new(f),
            })
            .expect("adding a feature");
            let ctx = test_ctx();
            let win = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 840.0),
                )),
                ..Default::default()
            };
            // Twice: the panel's width is restored from the previous pass.
            let mut out = Vec::new();
            for _ in 0..2 {
                let full = ctx.run_ui(win.clone(), |ui| app.side_panel(ui));
                out = full
                    .shapes
                    .iter()
                    .filter_map(|cs| match &cs.shape {
                        egui::Shape::Text(t) => {
                            Some((t.pos, t.galley.text().to_string(), t.galley.size().x))
                        }
                        _ => None,
                    })
                    .collect();
            }
            out
        }

        let long = "a".repeat(150);
        let short = panel_texts("araC");
        let wide = panel_texts(&long);

        // The tab strip is the review's own symptom. Every tab must be drawn,
        // and drawn at the same place as with a short name.
        for tab in ["Features", "Library", "Enzymes", "Sequence", "History"] {
            let at = |v: &[(egui::Pos2, String, f32)]| {
                v.iter().find(|(_, t, _)| t == tab).map(|(p, _, _)| *p)
            };
            let a = at(&short).unwrap_or_else(|| panic!("{tab} is missing from the control"));
            let b = at(&wide)
                .unwrap_or_else(|| panic!("a 150-character name pushed {tab} off the panel"));
            assert!(
                (a.x - b.x).abs() < 0.5 && (a.y - b.y).abs() < 0.5,
                "a 150-character name moved the {tab} tab from {a:?} to {b:?}"
            );
        }

        // The coordinates a cloner reads off the row survive it.
        // The review's second symptom, on the same mechanism: "the whole
        // New… / Edit… / Duplicate / Remove row is gone".
        for b in ["New…", "Edit…", "Duplicate", "Remove"] {
            assert!(
                wide.iter()
                    .any(|(p, t, _)| t == b && (0.0..1_280.0).contains(&p.x)),
                "a 150-character name took the {b} button off the panel"
            );
        }

        // Position only, not position plus width: the coordinates sit in a
        // right-to-left group, where the galley's own size is not the extent it
        // is drawn into, and the control shows it — `400..1,400` is laid out at
        // x=1257 with a 66 pt galley inside a 1,280 pt window. Where it STARTS
        // is the thing the defect moves, from inside the pane to past the edge
        // of the screen.
        let coords = |v: &[(egui::Pos2, String, f32)]| {
            v.iter()
                .any(|(p, t, _)| t == "400..1,400" && (0.0..1_280.0).contains(&p.x))
        };
        assert!(coords(&short), "the control has no coordinates to lose");
        assert!(
            coords(&wide),
            "a 150-character name pushed the coordinates out of the window"
        );

        // And the name is laid out into the room it has, not at its own width.
        let (at, w) = wide
            .iter()
            .find(|(_, t, _)| *t == long)
            .map(|(p, _, w)| (*p, *w))
            .expect("the name is not drawn at all");
        assert!(
            at.x + w <= 1_280.5,
            "the name is laid out {w:.0} pt wide and runs to x={:.0} in a 1,280 pt \
             window, so one /label still sets the width of the pane",
            at.x + w
        );
    }

    /// The Enzymes tab's end chip and its hover, over the enzymes that cut.
    ///
    /// The narrowing is the point, and it is what a catalogue-wide answer gets
    /// wrong: `BamHI` is interchangeable with `BclI` and `BglII` in the table,
    /// but if only `BglII` cuts the plasmid in front of you then `BclI` is not
    /// an alternative you have, and offering it sends somebody looking for a
    /// site that is not there.
    #[test]
    fn the_end_chip_names_only_the_alternatives_this_molecule_offers() {
        let cut = |names: &[&str]| -> Vec<&'static pl_enzymes::Enzyme> {
            names
                .iter()
                .map(|n| pl_enzymes::by_name(n).unwrap())
                .collect()
        };
        let bam = pl_enzymes::by_name("BamHI").unwrap();

        // All three cut: both partners are real options here.
        let all = end_note(bam, &cut(&["BamHI", "BglII", "BclI", "EcoRI"]));
        assert_eq!(all.chip, "5' GATC");
        assert!(
            all.hover.contains("BglII") && all.hover.contains("BclI"),
            "{}",
            all.hover
        );
        // EcoRI leaves AATT and must not be offered.
        assert!(!all.hover.contains("EcoRI"), "{}", all.hover);
        // The junction, and the reason to choose it.
        assert!(all.hover.contains("BamHI+BglII = GGATCT"), "{}", all.hover);
        assert!(all.hover.contains("cut by neither"), "{}", all.hover);

        // Only BglII present: BclI is compatible in the CATALOGUE and is not an
        // option in this molecule, so it must not be named.
        let some = end_note(bam, &cut(&["BamHI", "BglII"]));
        assert!(some.hover.contains("BglII"), "{}", some.hover);
        assert!(
            !some.hover.contains("BclI"),
            "BclI does not cut this molecule and must not be offered: {}",
            some.hover
        );

        // Nothing compatible cuts it: say so rather than show an empty list.
        let alone = end_note(bam, &cut(&["BamHI", "EcoRI", "HindIII"]));
        assert!(alone.hover.contains("Nothing else"), "{}", alone.hover);
        assert!(!alone.hover.contains("same end as"), "{}", alone.hover);

        // Polarity, in the surface a user actually reads: KpnI is 3' and BsrGI
        // 5', both GTAC, and neither may be offered as the other's partner.
        let kpn = end_note(
            pl_enzymes::by_name("KpnI").unwrap(),
            &cut(&["KpnI", "BsrGI"]),
        );
        assert_eq!(kpn.chip, "3' GTAC");
        assert!(kpn.hover.contains("Nothing else"), "{}", kpn.hover);

        // A Type IIS end gets a sentence, not a partner list, and the chip shows
        // the length rather than bases it does not have.
        let bsa = end_note(
            pl_enzymes::by_name("BsaI").unwrap(),
            &cut(&["BsaI", "BsmBI"]),
        );
        assert_eq!(bsa.chip, "5' NNNN");
        assert!(
            bsa.hover.contains("cuts outside its own site"),
            "{}",
            bsa.hover
        );
        assert!(
            !bsa.hover.contains("BsmBI"),
            "a Type IIS end must not be advertised as interchangeable: {}",
            bsa.hover
        );

        // Blunt reads as blunt, not as an empty overhang.
        let ecorv = end_note(
            pl_enzymes::by_name("EcoRV").unwrap(),
            &cut(&["EcoRV", "SmaI"]),
        );
        assert_eq!(ecorv.chip, "blunt");
        assert!(ecorv.hover.contains("SmaI"), "{}", ecorv.hover);
    }

    /// PROVEN TO FAIL at 28e9d91: the clone panel's product path called
    /// `self.adopt(d)` directly, with no unsaved check on it. Edit a plasmid,
    /// religate it, click Open, and the edits were gone — no prompt, no undo.
    ///
    /// It is the EIGHTH path of the class cc36cf7 was written to close, and it
    /// arrived in the commit that added the panel, which is the honest reason
    /// this test exists: a guard that was exhaustive when it was written stops
    /// being exhaustive the moment somebody adds a route past it, and nothing
    /// structural stopped that here.
    #[test]
    fn opening_a_religated_product_cannot_silently_discard_an_edited_document() {
        let mut app = seq_app();
        let d = app.bench.get_mut().expect("a document");
        d.apply(pl_core::OpKind::InsertAt {
            at: 1,
            seq: "AAAA".to_string(),
        })
        .expect("an ordinary insert");
        assert!(d.unsaved(), "the fixture depends on there being an edit");
        let before = d.molecule().seq.clone();
        let cursor = d.log.cursor();

        // DRIVEN THROUGH THE PANEL, not by calling `take_over` directly. The
        // defect was never in the funnel — it was that this path did not use
        // it — so a test that calls the funnel itself asserts the wrong thing
        // and passes against the broken code. Checked: with the call site put
        // back to `adopt`, the version below goes red and the direct one did
        // not.
        let picked: std::collections::BTreeSet<String> = ["BamHI".to_string()].into();
        let seq_mol = app.document().expect("open").molecule().clone();
        let mut panel = clone::Panel::new(&picked);
        panel.plan = Some(clone::plan(&seq_mol, &picked, true));
        panel.stale = false;
        assert!(
            panel.plan.as_ref().is_some_and(|p| !p.prods.is_empty()),
            "the fixture must produce a construct, or nothing is being opened"
        );
        panel.wanted = Some(0);
        app.clone_panel = Some(panel);
        let ctx = test_ctx();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let _ = ui;
            app.clone_panel(ui.ctx());
        });

        // The construct arrives in ITS OWN TAB and the edited document is
        // untouched behind it. 28e9d91 called `adopt` here and destroyed the
        // edit; fe89c5c parked it behind a question; the bench needs neither,
        // because nothing is replaced. The property is the same one all three
        // were reaching for and is now checkable without any dialog: the edit is
        // still there.
        assert_eq!(
            app.bench.len(),
            2,
            "the construct did not open in a new tab"
        );
        app.switch_tab(0);
        let d = app.document().expect("still open");
        assert_eq!(d.molecule().seq, before, "the edited molecule changed");
        assert_eq!(d.log.cursor(), cursor, "the edit history moved");
        assert!(d.unsaved(), "the edit stopped counting as unsaved work");

        // And the construct is genuinely there to go back to.
        app.switch_tab(1);
        assert_ne!(
            app.document().expect("the construct").molecule().seq,
            before,
            "the second tab is not the construct"
        );
    }

    /// PROVEN TO FAIL at 28e9d91: `adopt` closed the design panel and the
    /// feature editor because each holds something belonging to the molecule it
    /// was opened on, and did not close the cut-and-religate panel, which holds
    /// a whole digest of one.
    ///
    /// Left open across a swap it showed plasmid A's fragments while B was on
    /// screen, and "Open" built A's construct under A's name — a construct from
    /// a file that is no longer open, presented as if it came from the one in
    /// front of you. That is the failure mode this program can least afford.
    #[test]
    fn a_document_swap_closes_the_religation_panel_it_invalidates() {
        let mut app = seq_app();
        let picked: std::collections::BTreeSet<String> = ["BamHI".to_string()].into();
        let first = app.document().expect("open").molecule().clone();
        let mut panel = clone::Panel::new(&picked);
        panel.plan = Some(clone::plan(&first, &picked, true));
        panel.stale = false;
        app.clone_panel = Some(panel);
        assert!(app.clone_panel.is_some(), "the fixture needs a panel open");

        // A different molecule takes over.
        let other = pl_core::Molecule {
            name: "somethingElse".into(),
            seq: b"ACGTACGTACGTACGTACGTACGT".to_vec(),
            topology: pl_core::Topology::Circular,
            ..Default::default()
        };
        let title = "other".to_string();
        let (bytes, _) = pl_fileio::genbank::write_reporting(&other, &title, today());
        app.adopt(Document::from_bytes(bytes.as_bytes(), title, None).expect("re-read"));

        assert!(
            app.clone_panel.is_none(),
            "the panel outlived the molecule it digested, so Open would build the wrong construct"
        );
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|n| n.contains("previous file")),
            "closing it silently is the same surprise as leaving it open: {:?}",
            app.notice
        );
    }

    /// The whole path a user walks: digest, religate, open the product.
    ///
    /// The step this covers that `clone::tests` cannot is the LAST one — the
    /// product going out through the GenBank writer and coming back in through
    /// `Document::from_bytes`. A molecule that plans correctly and then cannot
    /// be re-read is still a feature nobody can use, and that hand-off is
    /// exactly where a construct would lose its topology or its features.
    #[test]
    fn a_religated_product_survives_becoming_a_document() {
        let seq = "AAAAGGATCCTTTTGCGCGCATATATCCCGGGAAAATTTTCCCC";
        let mut m = pl_core::Molecule {
            name: "pTest".into(),
            seq: seq.as_bytes().to_vec(),
            topology: pl_core::Topology::Circular,
            ..Default::default()
        };
        let mut f = pl_core::Feature::new("a gene", "CDS");
        f.strand = pl_core::Strand::Forward;
        f.segments.push(pl_core::Segment::new(15, 30));
        m.features.push(f);

        let picked: std::collections::BTreeSet<String> = ["BamHI".to_string()].into();
        let plan = clone::plan(&m, &picked, false);
        assert_eq!(plan.prods.len(), 1, "{:?}", plan.note);
        let p = &plan.prods[0];

        let title = "pTest product".to_string();
        let (bytes, unwritable) = pl_fileio::genbank::write_reporting(&p.mol, &title, today());
        assert!(
            unwritable.is_empty(),
            "the product could not be fully written: {unwritable:?}"
        );
        let d = Document::from_bytes(bytes.as_bytes(), title, None).expect("re-read");

        // The construct survives the trip intact.
        assert_eq!(d.molecule().seq.len(), seq.len(), "length changed");
        assert!(
            d.molecule().topology.is_circular(),
            "a circular product came back linear, so the plasmid became a fragment"
        );
        assert_eq!(
            d.molecule().features.len(),
            1,
            "the feature did not survive"
        );
        assert_eq!(d.molecule().features[0].name, "a gene");

        // No path, so it is unsaved by construction and the close guard covers
        // it. A product nobody saves must not be a product silently lost.
        assert!(d.path.is_none());
        assert!(d.unsaved(), "the product must count as unsaved work");
    }

    fn seq_app() -> App {
        let mut s = 0x2545_F491_4F6C_DD1Du64;
        let seq: String = (0..8_117)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                b"ACGT"[(s >> 33) as usize & 3] as char
            })
            .collect();
        let mut d =
            Document::from_bytes(format!(">p\n{seq}\n").as_bytes(), "p.fa".into(), None).unwrap();
        d.apply(pl_core::OpKind::SetTopology(pl_core::Topology::Circular))
            .unwrap();
        let mut app = App::blank();
        app.adopt(d);
        app.tab = Tab::Sequence;
        app
    }

    // -----------------------------------------------------------------------
    // the amino-acid track
    // -----------------------------------------------------------------------

    /// A molecule of `n` bases with a CDS every `every` bases, alternating
    /// strands, opened on the Sequence tab.
    fn perf_app(n: usize, every: usize) -> App {
        let mut s = 0x2545_F491_4F6C_DD1Du64;
        let mut seq = String::with_capacity(n + 8);
        for _ in 0..n {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            seq.push(b"ACGT"[(s >> 33) as usize & 3] as char);
        }
        let mut d =
            Document::from_bytes(format!(">p\n{seq}\n").as_bytes(), "p.fa".into(), None).unwrap();
        d.apply(pl_core::OpKind::SetTopology(pl_core::Topology::Circular))
            .unwrap();
        // Straight into the log's molecule rather than one `SetFeature` each:
        // 4,600 operations would measure `OpLog::apply`, which is not what this
        // is about.
        let mut mol = d.molecule().clone();
        let mut at = 1u64;
        let mut i = 0;
        while at + every as u64 <= n as u64 {
            let mut f = pl_core::Feature::new(format!("g{i}"), "CDS");
            f.strand = if i % 2 == 0 {
                Strand::Forward
            } else {
                Strand::Reverse
            };
            f.segments
                .push(pl_core::Segment::new(at, at + every as u64 - 30));
            mol.features.push(f);
            at += every as u64;
            i += 1;
        }
        let mut d2 = Document::of_molecule(mol);
        d2.digest.cancel();
        std::mem::swap(&mut d, &mut d2);
        let mut app = App::blank();
        app.adopt(d);
        app.tab = Tab::Sequence;
        app
    }

    /// The mean wall time of one painted frame, over `n` frames, scrolling.
    ///
    /// SCROLLING, not resting, and the difference is the whole measurement: at
    /// rest every row's galley is in egui's cache and the frame costs almost
    /// nothing, while a scrolling view lays out a screenful of new text every
    /// frame. A number taken at rest would say this feature is free and would
    /// be measuring the cache.
    fn frame_ms(app: &mut App, ctx: &egui::Context, n: usize) -> f64 {
        // The pointer has to be over the grid or egui delivers the wheel
        // somewhere else and the view never moves.
        let over = {
            for _ in 0..3 {
                paint(app, ctx, window());
            }
            let g = app.seq_grid.expect("painted");
            egui::pos2(g.x0 + 10.0, g.top + g.row_h * 2.0)
        };
        let first_before = app.seq_grid.expect("painted").first_row;
        let input = |k: usize| -> egui::RawInput {
            let mut i = window();
            i.events.push(egui::Event::PointerMoved(over));
            i.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -240.0 - (k % 3) as f32),
                modifiers: egui::Modifiers::default(),
                phase: egui::TouchPhase::Move,
            });
            i
        };
        let t = std::time::Instant::now();
        for k in 0..n {
            paint(app, ctx, input(k));
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        let first_after = app.seq_grid.expect("painted").first_row;
        assert!(
            first_after > first_before + 10,
            "the premise: the view actually scrolled ({first_before} -> {first_after})"
        );
        ms
    }

    /// MEASURED wall time for the ORF scan, at both ends of the corpus, and it
    /// can fail: the scan must not be on the UI thread, and the assertion is
    /// that the FRAME the user gets while it runs is not the scan.
    ///
    /// Run with `--nocapture` for the numbers. The comparison that decides the
    /// design is the enzyme digest, which this application already runs on a
    /// worker on exactly the same trigger: measured on this machine, ORFs over
    /// a 4.6 Mb molecule are a fraction of that scan, so there is no argument
    /// for a different mechanism.
    #[test]
    fn the_orf_scan_is_off_the_ui_thread_at_both_ends_of_the_corpus() {
        let ctx = test_ctx();
        for (n, label) in [(8_117usize, "plasmid"), (4_641_652, "genome")] {
            let mut app = perf_app(n, 900);
            app.layout.orf_track = true;
            let t = std::time::Instant::now();
            app.refresh_orfs();
            let spawned = t.elapsed().as_secs_f64() * 1000.0;
            // A frame WHILE it runs. This is the number that matters: a scan
            // done synchronously would show up here in full.
            let f = std::time::Instant::now();
            paint(&mut app, &ctx, window());
            let during = f.elapsed().as_secs_f64() * 1000.0;

            // Wait for the ANSWER, not for a state change this loop happens to
            // observe itself. `poll_orfs` returns true only for the transition,
            // and the `paint` above also polls — so on the plasmid, whose scan
            // is shorter than one cold frame, that frame collects the result and
            // this loop would spin to its deadline over a scan that finished
            // before it started. It only ever passed because the code under test
            // cancelled and respawned the worker on that very frame, which is
            // the defect: a test whose green depended on the bug.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            loop {
                app.bench.get_mut().unwrap().poll_orfs();
                if app.document().unwrap().orfs.done().is_some() {
                    break;
                }
                assert!(std::time::Instant::now() < deadline, "the ORF worker hung");
                std::thread::yield_now();
            }
            let total = t.elapsed().as_secs_f64() * 1000.0;
            let d = app.document().unwrap();
            let o = d.orfs.done().expect("finished");
            eprintln!(
                "PERF ORFs {label} {} bp: spawn {spawned:.3} ms on the UI thread,                  {total:.1} ms wall to the answer, {} ORFs at table {} >={} aa;                  a frame while it ran took {during:.3} ms",
                fmt_int(n as u64),
                fmt_int(o.orfs.len() as u64),
                o.code,
                o.min_aa
            );
            // RATIOS, not absolutes, because this test has to mean the same
            // thing in a debug build as in a release one: an unoptimised
            // profile moves every number here together, and an absolute
            // millisecond ceiling would fail for the profile rather than for
            // the defect. The claim is "the scan is not on the UI thread", and
            // that is a ratio.
            assert!(
                spawned * 20.0 < total,
                "{label}: asking for the scan cost {spawned:.3} ms against {total:.1} ms of scan"
            );
            if n > 1_000_000 {
                // Only where the scan is long enough for the comparison to say
                // anything. At plasmid scale the whole scan is shorter than one
                // cold frame, which is itself the answer.
                assert!(
                    during * 4.0 < total,
                    "{label}: a frame during the scan cost {during:.3} ms of its {total:.1} ms"
                );
            }
        }
    }

    /// MEASURED, and it can fail: the marginal cost of the whole track — one
    /// residue lane per strand, the complement row and the ORF strip — is
    /// asserted to stay under a millisecond a frame at plasmid scale.
    ///
    /// It is not a microbenchmark for its own sake. The design of this feature
    /// rests on the claim that COMPUTE IS NOT THE CONSTRAINT and vertical space
    /// is, and a claim like that decays silently: the obvious wrong
    /// implementations (a `translate()` allocation per row, a per-row
    /// `reverse_complement`, a `layout_no_wrap` per residue) are all still
    /// correct on screen and all several times this cost.
    ///
    /// Run with `--nocapture` for the numbers.
    #[test]
    fn the_track_costs_a_fraction_of_a_frame_at_plasmid_scale() {
        let ctx = test_ctx();
        let mut app = perf_app(8_117, 900);
        app.layout.aa_track = aa::TrackMode::Off;
        app.layout.complement = Some(false);
        app.layout.orf_track = false;
        let off = frame_ms(&mut app, &ctx, 40);

        app.layout.aa_track = aa::TrackMode::File;
        app.layout.complement = Some(true);
        let on = frame_ms(&mut app, &ctx, 40);
        let g = app.seq_grid.expect("painted");
        assert!(g.strips.aa_fwd > 0 && g.strips.aa_rev > 0, "the premise");

        eprintln!(
            "PERF plasmid 8,117 bp / {} features: tracks off {off:.3} ms/frame, \
             on {on:.3} ms/frame, marginal {:.3} ms; row_h {:.2} -> {:.2}",
            app.document().unwrap().molecule().features.len(),
            on - off,
            32.88,
            g.row_h
        );
        // A RATIO, so this means the same thing in a debug build as in a
        // release one. It still catches every shape the design rejected: a
        // `translate()` allocation per row is about 4x, a per-row
        // `reverse_complement` about 10x, and a `layout_no_wrap` per residue
        // about 30x.
        assert!(
            on < off * 2.5,
            "the track took the frame from {off:.3} ms to {on:.3} ms"
        );
    }

    /// The virtualisation promise, MEASURED with the track on: the sequence
    /// view costs the same on a 4.6 Mb genome as on an 8 kb plasmid, because
    /// only the visible rows are built and a residue's coordinates come out of
    /// a path rather than out of a materialised protein.
    ///
    /// It can fail, and the failure it is aimed at is real: materialising the
    /// residues would be about 1.3 million entries for a genome this size, and
    /// scanning every path per row instead of reading the row's own interval
    /// query would be 4,600 extent tests times forty rows every frame.
    ///
    /// Run with `--nocapture` for the numbers.
    #[test]
    fn the_track_costs_the_same_on_a_genome_as_on_a_plasmid() {
        let ctx = test_ctx();
        let mut small = perf_app(8_117, 900);
        small.layout.aa_track = aa::TrackMode::File;
        small.layout.complement = Some(true);
        let a = frame_ms(&mut small, &ctx, 30);

        let mut big = perf_app(4_641_652, 1_000);
        big.layout.aa_track = aa::TrackMode::Off;
        big.layout.complement = Some(false);
        let b_off = frame_ms(&mut big, &ctx, 30);
        big.layout.aa_track = aa::TrackMode::File;
        big.layout.complement = Some(true);
        let b = frame_ms(&mut big, &ctx, 30);
        let feats = big.document().unwrap().molecule().features.len();
        eprintln!(
            "PERF genome 4,641,652 bp / {feats} features: tracks off {b_off:.3} ms/frame, \n             on {b:.3} ms/frame; the plasmid with tracks on is {a:.3} ms/frame, ratio {:.2}x",
            b / a.max(1e-6)
        );
        assert!(feats > 4_000, "the premise: a genome's worth of CDSs");
        assert!(
            b < a * 5.0,
            "the genome cost {b:.3} ms a frame against the plasmid's {a:.3}"
        );
        let _ = b_off;
    }

    /// A plasmid whose first 300 bases are one forward CDS, open on Sequence
    /// with the track and the complement strand on.
    ///
    /// 300 rather than the whole molecule so there are rows with a translation
    /// on them and rows without, which is the whole of the row-pitch question.
    fn aa_app(reverse: bool, mode: aa::TrackMode) -> App {
        let mut app = seq_app();
        let mut f = pl_core::Feature::new("gene", "CDS");
        f.strand = if reverse {
            Strand::Reverse
        } else {
            Strand::Forward
        };
        f.segments.push(pl_core::Segment::new(1, 300));
        assert!(app.edit(pl_core::OpKind::SetFeature {
            index: None,
            feature: Box::new(f),
        }));
        app.layout.aa_track = mode;
        app.layout.complement = Some(true);
        app
    }

    /// Every `Shape::Text` drawn, as `(pos, text)`.
    fn texts(out: &egui::FullOutput) -> Vec<(egui::Pos2, String)> {
        out.shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::Shape::Text(t) => Some((t.pos, t.galley.text().to_string())),
                _ => None,
            })
            .collect()
    }

    /// PROVEN TO FAIL against cc36cf7: there is no amino-acid track there at
    /// all, so no row of residues is ever drawn and this finds nothing. It is a
    /// behavioural failure, not a compile-only one — `aa::TrackMode` does not
    /// exist at cc36cf7 either, so the fixture would also have to be written
    /// differently to build.
    ///
    /// What it really asserts is the ONE rule that keeps the track honest: the
    /// residue lane is a `per_row`-length string painted at `cx(0)`, in the same
    /// `FontId` as the bases, so a residue at column `c` occupies exactly
    /// `[cx(c), cx(c+1))` and its codon's three cells are `[cx(c-1), cx(c+2))`,
    /// symmetric about it BY CONSTRUCTION. There is no second producer of an x.
    ///
    /// Asserted at the LAST codon of the row, column 58, where an error is
    /// largest: any formula that inserts a gap, centres on a measured galley,
    /// or uses a smaller aa font is right at column 1 and wrong by whole cells
    /// at column 58.
    ///
    /// AND IT IS MEASURED OFF THE GALLEY, not recomputed from `g.x0` and
    /// `g.advance`. The first version of this test read only the two galleys'
    /// ANCHORS — both painted at `cx(0)`, so both trivially `g.x0` — and then
    /// asserted relations among numbers it had just derived from `g.advance`
    /// itself: `assert!((glyph_x - (codon_lo + g.advance)).abs() < 1e-3)` is
    /// `|0| < 1e-3`, true for any painter whatsoever. A reviewer built the exact
    /// mutation the paragraph above names — `FontId::monospace(font.size * 0.85)`
    /// for the residue lane only — and the whole suite stayed green while the
    /// row's last residue drifted to cell 48.8, ten cells left of its codon,
    /// with the boundary ticks visibly detached from the letters. Glyph
    /// positions come from the laid-out text, so they move when the face does.
    #[test]
    fn a_residue_sits_over_exactly_its_own_three_bases_at_the_last_codon_of_a_row() {
        let ctx = test_ctx();
        let mut app = aa_app(false, aa::TrackMode::File);
        let out = paint_out(&mut app, &ctx, window());
        let g = app.seq_grid.expect("the grid was painted");
        assert_eq!(g.per_row, 60, "the premise: a full-width row");
        assert_eq!(g.strips.aa_fwd, 1, "the premise: one forward residue lane");

        // The absolute x of character `i` of a painted string, off the galley
        // egui actually laid out. THE MEASUREMENT: a smaller face, a different
        // family or any inserted gap changes these and changes nothing the
        // arithmetic above can see.
        let glyph_x = |t: &str, i: usize| -> f32 {
            let s = out
                .shapes
                .iter()
                .find_map(|cs| match &cs.shape {
                    egui::Shape::Text(sh) if sh.galley.text() == t => Some(sh),
                    _ => None,
                })
                .expect("the string was painted");
            let row = s.galley.rows.first().expect("a single-line galley");
            s.pos.x + row.pos.x + row.row.glyphs[i].pos.x
        };

        let ts = texts(&out);
        // Row 0's bases, and row 0's residues. The CDS begins at coordinate 0,
        // so residue k covers 3k, 3k+1, 3k+2 and its MIDDLE base is 3k+1: the
        // letters land at columns 1, 4, ..., 58.
        let bases = ts
            .iter()
            .find(|(pos, t)| t.len() == 60 && !t.contains(' ') && pos.y > g.top)
            .expect("row 0's bases");
        let aa_row = ts
            .iter()
            .find(|(_, t)| {
                t.len() == 60
                    && t.starts_with(' ')
                    && t.chars().enumerate().all(|(i, c)| {
                        if i % 3 == 1 {
                            c.is_ascii_uppercase() || c == '*'
                        } else {
                            c == ' '
                        }
                    })
            })
            .expect("row 0's residues");

        // One x, shared. Not "close to": the same number, because both come
        // out of `RowLayout::col_x(0)`.
        assert_eq!(
            aa_row.0.x, bases.0.x,
            "the residue lane and the bases start at the same column 0"
        );
        assert_eq!(aa_row.0.x, g.x0);
        // Above the strand it reads, and above the complement below that.
        assert!(aa_row.0.y < bases.0.y, "forward residues sit above");

        // THE MEASUREMENT, at the last codon of the row. The residue's glyph
        // sits at column 58; its codon's three cells are 57, 58, 59.
        //
        // Every number below is read off a laid-out galley. The residue glyph
        // and the BASE glyph at the same column must be the same x — not close,
        // the same — because both are character 58 of a `per_row`-length string
        // laid out in one `FontId` at one anchor. A residue lane in a smaller
        // face has an identical anchor and an identical string and fails here by
        // whole cells.
        let res_58 = glyph_x(&aa_row.1, 58);
        let base_58 = glyph_x(&bases.1, 58);
        assert!(
            (res_58 - base_58).abs() < 1e-3,
            "residue 19's glyph is at {res_58} and its middle base's at {base_58}"
        );
        // And that x is the one `RowLayout::col_x` promises, so the codon ticks
        // and the marks — which ARE drawn from `cx` — cannot drift from the
        // letters they belong to.
        //
        // One PIXEL of tolerance and not 1e-3, because epaint snaps a laid-out
        // glyph to the pixel grid on purpose (`PlacedRow::pos` is "rounded to
        // the closest pixel in order to produce crisp text"). At the default
        // 1 px/pt of the test context that is at most 1.0 here, against the
        // 6.9 pt cell it has to stay inside and the ~60 pt a wrong font face
        // moves it.
        let px = 1.0;
        assert!(
            (res_58 - (g.x0 + 58.0 * g.advance)).abs() <= px,
            "residue 19's glyph is at {res_58}, not cx(58) = {}",
            g.x0 + 58.0 * g.advance
        );
        // The cell pitch is the base font's advance over the whole row, so the
        // error cannot be zero at column 0 and grow along it.
        let res_1 = glyph_x(&aa_row.1, 1);
        assert!(
            ((res_58 - res_1) - 57.0 * g.advance).abs() <= px,
            "the residue lane's own pitch is {} per cell, not {}",
            (res_58 - res_1) / 57.0,
            g.advance
        );
        // The GLYPH is inside the band. Not its codon: a reading whose middle
        // bases land at column 59 has that codon's third base on the NEXT row,
        // which is exactly what the middle-base rule is for, so asserting the
        // codon fits would teach a rule the code does not keep.
        assert!(
            res_58 + g.advance <= g.x0 + g.per_row as f32 * g.advance + px,
            "the last residue is inside the band"
        );
        // And the 59th character of the lane really is a residue.
        let ch = aa_row.1.as_bytes()[58];
        assert!(
            ch.is_ascii_uppercase() || ch == b'*',
            "column 58 holds residue 19, not {:?}",
            ch as char
        );
        // The three bases under it are the codon `pl_core` translates.
        let mol = app.document().unwrap().molecule();
        let codon = &mol.seq[57..60];
        let want = pl_core::translate::table(11).unwrap().codon(codon);
        assert_eq!(ch, want, "the letter is what pl_core::translate says");
    }

    /// PROVEN TO FAIL against cc36cf7: `RowStrips` does not exist there, so
    /// this is a COMPILE-ONLY failure at that commit — said plainly, because a
    /// test that cannot build proves less than one that runs and fails.
    ///
    /// What it exercises is the hazard the whole design turns on. `show_rows`
    /// maps a scroll offset to a row index by dividing, so every row must be
    /// the same height. Here rows 0-4 carry a CDS and rows 5 on do not, and the
    /// gutter coordinates — which are drawn from the same `y_text` the letters
    /// use — must sit on an exact arithmetic progression of `row_h`.
    #[test]
    fn the_row_pitch_is_the_same_on_a_row_with_a_translation_and_one_without() {
        let ctx = test_ctx();
        let mut app = aa_app(false, aa::TrackMode::File);
        let out = paint_out(&mut app, &ctx, window());
        let g = app.seq_grid.expect("painted");
        assert!(g.strips.aa_fwd > 0, "the premise: the lane is reserved");

        // The row coordinates, in row order: "1", "61", "121", ...
        let ts = texts(&out);
        let mut ys: Vec<(u64, f32)> = Vec::new();
        for row in 0..8u64 {
            let want = fmt_int(row * 60 + 1);
            let (pos, _) = ts
                .iter()
                .find(|(_, t)| *t == want)
                .unwrap_or_else(|| panic!("row {row}'s gutter coordinate {want}"));
            ys.push((row, pos.y));
        }
        // Rows 0-4 hold the CDS (bases 0..300); rows 5+ hold none of it.
        for w in ys.windows(2) {
            let step = w[1].1 - w[0].1;
            assert!(
                (step - g.row_h).abs() < 0.01,
                "rows {} -> {} stepped {step}, not {}",
                w[0].0,
                w[1].0,
                g.row_h
            );
        }

        // And uniformity is not enough on its own: the DRAWING has to sit where
        // `RowStrips` says it does, or the painter and the hit-test have drifted
        // apart inside a row while the pitch between rows still looks perfect.
        // The gutter coordinate is drawn at `y_text`, the strand it labels.
        for (row, y) in &ys {
            let want = g.top + (row - g.first_row) as f32 * g.row_h + g.strips.y_text();
            assert!(
                (y - want).abs() < 0.01,
                "row {row}'s letters were drawn at {y}, and the hit-test reads {want}"
            );
        }
        // The bottom strand is exactly one line of letters below the top one,
        // and the forward residue lane exactly one above.
        let base_row = ys[0].1;
        assert!((g.strips.y_comp() - g.strips.y_text() - g.strips.text_h).abs() < 0.01);
        assert!((g.strips.y_text() - g.strips.y_aa_fwd(0) - g.strips.text_h).abs() < 0.01);
        let _ = base_row;
    }

    /// PROVEN TO FAIL against cc36cf7 for the same compile-only reason
    /// (`RowStrips`, `GridGeom::strips`), and behaviourally against the obvious
    /// wrong implementation of this change: with a residue lane above the
    /// letters and a complement row below them, a hit-test that knows only the
    /// row maps any y in the band onto the letters, so a click meant for a
    /// residue moves the caret — and one meant for the caret at the last column
    /// of a row lands wherever the extra height put it.
    #[test]
    fn the_caret_still_lands_on_the_last_column_of_a_row_with_the_tracks_on() {
        let ctx = test_ctx();
        let mut app = aa_app(false, aa::TrackMode::File);
        paint(&mut app, &ctx, window());
        let g = app.seq_grid.expect("painted");
        assert_eq!(g.per_row, 60);
        assert!(
            g.strips.aa_fwd > 0 && g.strips.complement,
            "the premise: the row is taller than the letters"
        );

        let row = 2u64;
        let col = 59u64;
        // A NAMED band — the middle of the top strand's own letters — not
        // `row_h * 0.5`, which with these strips is the complement row.
        let y = g.top
            + (row - g.first_row) as f32 * g.row_h
            + g.strips.y_text()
            + g.strips.text_h * 0.5;
        let at = egui::pos2(g.x0 + (col as f32 + 0.2) * g.advance, y);
        paint(&mut app, &ctx, pointer_to(at));
        paint(&mut app, &ctx, pointer_button(at, true));
        paint(&mut app, &ctx, pointer_button(at, false));
        assert_eq!(app.edit.caret, row * 60 + col);

        // And the COMPLEMENT row is the same coordinate space: a click on it
        // places the caret on the same base, because the bottom strand is a
        // read-only mirror and not a second coordinate system.
        let y = g.top
            + (row - g.first_row) as f32 * g.row_h
            + g.strips.y_comp()
            + g.strips.text_h * 0.5;
        let at = egui::pos2(g.x0 + (col as f32 + 0.2) * g.advance, y);
        paint(&mut app, &ctx, pointer_to(at));
        paint(&mut app, &ctx, pointer_button(at, true));
        paint(&mut app, &ctx, pointer_button(at, false));
        assert_eq!(
            app.edit.caret,
            row * 60 + col,
            "a click on the bottom strand names the same base"
        );
    }

    /// PROVEN TO FAIL against cc36cf7: compile-only there (no `aa` module), and
    /// behaviourally against any version that routes an aa-lane click through
    /// `hit`, which would move the caret instead of selecting the codon.
    #[test]
    fn a_click_on_a_residue_selects_its_three_bases() {
        let ctx = test_ctx();
        let mut app = aa_app(false, aa::TrackMode::File);
        paint(&mut app, &ctx, window());
        let g = app.seq_grid.expect("painted");

        // Residue 19 of row 0: coordinates 57, 58, 59, glyph at column 58.
        let y = g.top + g.strips.y_aa_fwd(0) + g.strips.text_h * 0.5;
        let at = egui::pos2(g.x0 + 58.4 * g.advance, y);
        paint(&mut app, &ctx, pointer_to(at));
        paint(&mut app, &ctx, pointer_button(at, true));
        paint(&mut app, &ctx, pointer_button(at, false));
        let s = app
            .edit
            .sel
            .expect("a click on a residue selects its codon");
        assert_eq!((s.lo(), s.hi()), (57, 60), "the codon, not the base");
        let notice = app.edit.notice.clone().unwrap_or_default();
        assert!(notice.contains("residue 20"), "{notice}");
    }

    /// PROVEN TO FAIL before the fix, and the failure is the one with no honest
    /// reading: with both forward lanes already spoken for, the ad-hoc selection
    /// was painted into lane 1, on top of a file translation, and the two
    /// proteins came out interleaved a column apart — `M K RA GV CA M* KN ...`,
    /// where the `M*` is one protein's methionine beside another's stop. Nothing
    /// was counted in the row's `+N`, because the selection's lane index was
    /// clamped to `MAX_AA_LANES - 1` and the reservation to `MAX_AA_LANES`, so
    /// `1 < 2` and the over-cap escape never fired.
    ///
    /// Restoring either clamp turns this red. The invariant is asserted
    /// directly, on the drawing: no two residue strings share a y.
    #[test]
    fn the_selection_translation_never_shares_a_lane_with_a_file_translation() {
        let ctx = test_ctx();
        let mut app = seq_app();
        // Two overlapping forward CDSs, which is what makes the strand need
        // both lanes — a vector plus a tagged variant, or any stretch of
        // MG1655. pKoV does not hit it; that is why nothing caught this.
        for (name, lo, hi) in [("cdsA", 1u64, 300u64), ("cdsB", 150, 420)] {
            let mut f = pl_core::Feature::new(name, "CDS");
            f.strand = Strand::Forward;
            f.segments.push(pl_core::Segment::new(lo, hi));
            assert!(app.edit(pl_core::OpKind::SetFeature {
                index: None,
                feature: Box::new(f),
            }));
        }
        app.layout.aa_track = aa::TrackMode::Selection;
        app.layout.complement = Some(true);
        app.edit.sel = Some(seqedit::Selection {
            anchor: 190,
            head: 232,
            through_origin: false,
        });
        app.edit.caret = 232;

        let out = paint_out(&mut app, &ctx, window());
        let g = app.seq_grid.expect("painted");
        assert_eq!(app.tr.fwd_lanes, 2, "the premise: both file lanes are used");
        assert_eq!(
            g.strips.aa_fwd, 3,
            "the selection is reserved a lane of its own, above the two"
        );

        // Every residue lane string drawn on the row holding the selection.
        // Row 3 is bases 180..240, where cdsB, cdsA and the selection all reach.
        let row_top = g.top + 3.0 * g.row_h;
        let mut ys: Vec<i32> = texts(&out)
            .iter()
            .filter(|(pos, t)| {
                pos.y >= row_top - 0.5
                    && pos.y < row_top + g.row_h
                    && t.len() == 60
                    && t.contains(' ')
                    && t.chars().any(|c| c.is_ascii_uppercase())
            })
            .map(|(pos, _)| (pos.y * 100.0).round() as i32)
            .collect();
        assert!(
            ys.len() >= 3,
            "the premise: three readings reach this row, not {}",
            ys.len()
        );
        let before = ys.len();
        ys.sort_unstable();
        ys.dedup();
        assert_eq!(
            ys.len(),
            before,
            "two residue strings were painted at the same y: {before} strings, {} lanes",
            ys.len()
        );
    }

    /// PROVEN TO FAIL before the fix: `reverse` came from the caret ordering
    /// alone, and for a wrapping selection the caret ordering is INVERTED —
    /// travelling forward across the origin ends at a caret below the one it
    /// started from, which is exactly the state the drag handler builds. So
    /// every left-to-right wrap-drag was read as reverse. Reverting to
    /// `s.head < s.anchor` turns this red.
    ///
    /// The arc here spells `ATGAAACGCGGTTGCTAA` on the top strand — MKRGC* —
    /// and its reverse complement is LATAFH, which is what the app drew.
    #[test]
    fn a_forward_drag_through_the_origin_translates_the_strand_it_ran_along() {
        let ctx = test_ctx();
        let mut app = {
            // 120 bp circle whose last 5 and first 13 bases are the reading.
            let mut seq = vec![b'C'; 120];
            let arc = b"ATGAAACGCGGTTGCTAA";
            seq[115..120].copy_from_slice(&arc[..5]);
            seq[0..13].copy_from_slice(&arc[5..]);
            let fa = format!(">c\n{}\n", String::from_utf8(seq).unwrap());
            let mut d = Document::from_bytes(fa.as_bytes(), "c.fa".into(), None).unwrap();
            d.apply(pl_core::OpKind::SetTopology(pl_core::Topology::Circular))
                .unwrap();
            let mut app = App::blank();
            app.adopt(d);
            app.tab = Tab::Sequence;
            app
        };
        app.layout.aa_track = aa::TrackMode::Selection;
        app.layout.complement = Some(true);
        // The state a forward wrap-drag leaves: anchor HIGH, head LOW, wrapped.
        app.edit.sel = Some(seqedit::Selection {
            anchor: 115,
            head: 13,
            through_origin: true,
        });
        app.edit.caret = 13;

        let out = paint_out(&mut app, &ctx, window());
        let g = app.seq_grid.expect("painted");
        let ts = texts(&out);

        // Row 0's residues. Read forward, the arc is MKRGC*: M and K sit over
        // bases 116 and 119 on row 1, and row 0 carries R, G, C and the stop,
        // with their middle bases at 2, 5, 8 and 11. Read BACKWARDS — which is
        // what the caret ordering alone said — the same arc is LATAFH and row 0
        // would carry `A T A L` instead, in the lane below the complement.
        let row0 = ts
            .iter()
            .find(|(pos, t)| {
                pos.y > g.top
                    && pos.y < g.top + g.row_h
                    && t.len() == 60
                    && t.contains(' ')
                    && t.chars().any(|c| c.is_ascii_uppercase() || c == '*')
            })
            .expect("row 0's residues");
        let letters: String = row0.1.chars().filter(|c| *c != ' ').collect();
        assert_eq!(
            letters, "RGC",
            "the forward reading, not its reverse complement LATAFH"
        );
        // The terminal stop is painted as its own glyph — a mark is not a plain
        // residue and does not go in the shared string — so it is checked
        // separately, on the same line.
        assert!(
            ts.iter()
                .any(|(pos, t)| t == "*" && (pos.y - row0.0.y).abs() < 0.5),
            "the reading's terminal stop, on the same lane"
        );
        // And ABOVE the top strand, which is where a forward reading goes. Both
        // strands keep a lane RESERVED while `+ selection` is on — the row pitch
        // must not change when a drag reverses direction mid-gesture — so the
        // reservation cannot answer this question and the drawing has to.
        let top = ts
            .iter()
            .find(|(pos, t)| {
                pos.y > g.top && pos.y < g.top + g.row_h && t.len() == 60 && !t.contains(' ')
            })
            .expect("row 0's top strand");
        assert!(
            row0.0.y < top.0.y,
            "the residues are at {} and the strand they read at {}",
            row0.0.y,
            top.0.y
        );
    }

    /// PROVEN TO FAIL against cc36cf7: no complement strand exists there at
    /// all — `seqedit.rs` has no bottom-strand rendering, and the UX review of
    /// 2026-07-31 corrected the survey that said it did.
    #[test]
    fn the_bottom_strand_is_the_complement_of_the_top_in_the_same_columns() {
        let ctx = test_ctx();
        let mut app = aa_app(false, aa::TrackMode::File);
        let out = paint_out(&mut app, &ctx, window());
        let g = app.seq_grid.expect("painted");
        let ts = texts(&out);

        let mol = app.document().unwrap().molecule();
        let top: String = String::from_utf8(mol.seq[0..60].to_vec()).unwrap();
        let bottom: String = mol.seq[0..60]
            .iter()
            .map(|&b| pl_core::iupac::complement(b) as char)
            .collect();
        let t = ts
            .iter()
            .find(|(_, t)| *t == top)
            .expect("row 0's top strand");
        let b = ts
            .iter()
            .find(|(_, t)| *t == bottom)
            .expect("row 0's bottom strand");
        // NOT reversed: column c of the bottom row is the Watson-Crick partner
        // of column c above it, which is the physical duplex and is what keeps
        // one coordinate space for both strands.
        assert_eq!(b.0.x, t.0.x, "same columns");
        assert!(b.0.y > t.0.y, "under it");
        assert!((b.0.y - t.0.y - g.strips.text_h).abs() < 0.01);
    }

    /// PROVEN TO FAIL against cc36cf7: compile-only (no `RowStrips`). It pins
    /// the gate that makes the misleading case impossible by construction —
    /// turning the complement strand off takes the reverse residue lanes with
    /// it, so a reverse translation is never drawn under a top-strand-only
    /// view, where it would read C-terminus to N-terminus left to right.
    #[test]
    fn a_reverse_translation_is_never_drawn_without_the_strand_it_reads() {
        let ctx = test_ctx();
        let mut app = aa_app(true, aa::TrackMode::File);
        paint(&mut app, &ctx, window());
        let g = app.seq_grid.expect("painted");
        assert_eq!(g.strips.aa_rev, 1, "the premise: a reverse translation");
        assert_eq!(g.strips.aa_fwd, 0);

        app.layout.complement = Some(false);
        paint(&mut app, &ctx, window());
        let g = app.seq_grid.expect("painted");
        assert_eq!(g.strips.aa_rev, 0, "the lane went with the strand");
        assert!(!g.strips.complement);
    }

    /// PROVEN TO FAIL against the code as it stood before this fix, on the
    /// running application and not only in principle: with the ORF strip on, the
    /// 4.6 Mb genome read "ORFs: scanning…" at 15 s, 20 s and 60 s, pinned a
    /// core at 110% focused or not, climbed 232 -> 470 MB, and produced its
    /// answer the moment the user left the Sequence tab.
    ///
    /// It exists because the two ORF tests that shipped with the feature CANNOT
    /// catch that, and the reason is worth stating: both call `refresh_orfs()`
    /// exactly ONCE and then poll to completion, which is the one calling
    /// pattern the application never uses. `sequence_tab` calls it at the top of
    /// EVERY frame, and while a scan is running `update` asks for a frame every
    /// 80 ms — so the question is not "does a worker finish" but "does painting
    /// let it". 1,389 green tests said nothing about it.
    ///
    /// So this drives the real loop: paint frames in the production order and
    /// count the workers. `orf_spawns` and not merely `Done`, because on a fast
    /// molecule the old code could still converge by luck between two frames,
    /// and a test that passes by luck is the thing being fixed.
    #[test]
    fn painting_the_tab_every_frame_asks_for_one_orf_scan_and_lets_it_finish() {
        let ctx = test_ctx();
        // Big enough that the scan outlives a frame by a wide margin — the
        // regime the defect lives in. Below about 0.87 Mb at rest the old code
        // converged anyway, which is exactly why an 8 kb fixture proved nothing.
        let mut app = perf_app(400_000, 3_000);
        app.layout.orf_track = true;
        app.doc_code = pl_core::translate::TABLE11;

        // Frame 1 is what asks the question.
        paint(&mut app, &ctx, window());
        assert!(
            app.document().unwrap().orfs.is_running(),
            "the premise: one frame does not finish this scan"
        );
        assert_eq!(app.document().unwrap().orf_spawns, 1);

        // Every frame after it, in the production order, until the answer lands.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut frames = 0usize;
        while app.document().unwrap().orfs.is_running() {
            assert!(
                std::time::Instant::now() < deadline,
                "the scan never finished under repeated painting — {} worker(s) spawned",
                app.document().unwrap().orf_spawns
            );
            paint(&mut app, &ctx, window());
            frames += 1;
        }
        assert!(
            frames >= 2,
            "the premise: more than one further frame was painted while it ran"
        );
        let d = app.document().unwrap();
        assert_eq!(
            d.orf_spawns, 1,
            "one question, asked once, across {frames} frames"
        );
        let got = d.orfs.done().expect("the scan finished");
        assert_eq!(got.code, 11);
        assert!(!got.orfs.is_empty(), "and it is a real answer");
        // And the strip the row height is reserved from now exists, which is
        // what the user was waiting for.
        assert!(app.orf_strip);
    }

    /// Changing the table really does re-ask, so the idempotence above is
    /// idempotence and not a scan that can never be replaced.
    #[test]
    fn changing_the_table_asks_the_orf_scan_again() {
        let ctx = test_ctx();
        let mut app = seq_app();
        app.layout.orf_track = true;
        app.doc_code = pl_core::translate::TABLE11;
        paint(&mut app, &ctx, window());
        assert_eq!(app.document().unwrap().orf_spawns, 1);
        // The same question again changes nothing, however many frames.
        for _ in 0..8 {
            paint(&mut app, &ctx, window());
        }
        assert_eq!(app.document().unwrap().orf_spawns, 1);
        // A different one is a different question.
        app.doc_code = pl_core::translate::table(1).expect("table 1");
        paint(&mut app, &ctx, window());
        assert_eq!(
            app.document().unwrap().orf_spawns,
            2,
            "table 1 is not the answer table 11 gave"
        );
    }

    /// PROVEN TO FAIL against cc36cf7: `Document::start_orfs` does not exist
    /// there, so this is compile-only at that commit.
    ///
    /// What it asserts is that the GUI asks `pl_core::orf` the same question
    /// `pl orfs` asks it. The CLI's defaults are table 11 — chosen in
    /// `bins/pl/src/main.rs` because "this is a plasmid tool, and its molecules
    /// are read in bacteria" — and `Params::default()`, which is `min_aa: 30`,
    /// `require_start`, `include_incomplete`, `nested: false`. A GUI that
    /// silently turned on `nested` would report 3.5x as many ORFs as the CLI
    /// over the same molecule and both numbers would look plausible.
    #[test]
    fn the_orf_strip_finds_what_pl_orfs_finds_on_the_same_molecule_and_table() {
        let mut app = seq_app();
        app.layout.orf_track = true;
        app.doc_code = pl_core::translate::TABLE11;
        app.refresh_orfs();
        // The worker is a thread; wait for the ANSWER rather than for the
        // transition, which anything else that polls can consume first.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            app.bench.get_mut().unwrap().poll_orfs();
            if app.document().unwrap().orfs.done().is_some() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "the ORF worker hung");
            std::thread::yield_now();
        }
        let d = app.document().unwrap();
        let got = d.orfs.done().expect("a finished scan");
        let want = pl_core::orf::find_orfs(
            &d.molecule().seq,
            pl_core::translate::TABLE11,
            d.molecule().topology.is_circular(),
            &pl_core::orf::Params::default(),
        );
        assert!(!want.is_empty(), "the premise: this molecule has ORFs");
        assert_eq!(got.orfs, want, "the same call the CLI makes");
        assert_eq!(got.code, 11);
        assert_eq!(got.min_aa, 30);
        // And every one that can be drawn is in the index, in its own frame's
        // lane. `laps > 0` cannot be an interval and is counted instead.
        let mut in_index = Vec::new();
        got.index.query(0, d.molecule().len(), &mut in_index);
        let drawable = want.iter().filter(|o| o.laps == 0).count();
        let distinct: std::collections::BTreeSet<u32> = in_index.iter().map(|iv| iv.feat).collect();
        assert_eq!(distinct.len(), drawable);
        assert_eq!(got.lapping, want.len() - drawable);
        for iv in &in_index {
            let o = &want[iv.feat as usize];
            let want_lane = if o.strand.is_reverse() {
                3 + o.frame
            } else {
                o.frame
            };
            assert_eq!(iv.lane, want_lane, "frame {} on its own line", o.frame);
        }
    }

    /// PROVEN TO FAIL against cc36cf7: compile-only (`aa::Translations` does not
    /// exist). This is the one that gives `Segment::translated` its meaning —
    /// the bit has been parsed by the `.dna` reader, offered by the feature
    /// editor and read by NOTHING in this program until now, and the checkbox's
    /// own hover text said so.
    #[test]
    fn the_translated_flag_turns_a_misc_feature_into_a_track() {
        let mut app = seq_app();
        let mut f = pl_core::Feature::new("decR his", "misc_feature");
        f.strand = Strand::Forward;
        let mut s = pl_core::Segment::new(1, 300);
        s.translated = true;
        f.segments.push(s);
        assert!(app.edit(pl_core::OpKind::SetFeature {
            index: None,
            feature: Box::new(f),
        }));
        app.refresh_annotations();
        assert_eq!(app.tr.paths().len(), 1, "the flag alone produced a reading");
        assert!(app.tr.paths()[0].from_flag);
        assert_eq!(app.tr.fwd_lanes, 1);

        // And clearing it takes the track away again, through the same
        // `OpKind::SetFeature` every other feature edit goes through — so it
        // undoes, which `oplog.rs` already hashes `translated` for.
        let mut f = app.document().unwrap().molecule().features[0].clone();
        f.segments[0].translated = false;
        assert!(app.edit(pl_core::OpKind::SetFeature {
            index: Some(0),
            feature: Box::new(f),
        }));
        app.refresh_annotations();
        assert!(app.tr.is_empty(), "a misc_feature is not a CDS");
        app.do_undo();
        app.refresh_annotations();
        assert_eq!(app.tr.paths().len(), 1, "and the undo brings it back");
    }

    /// PROVEN TO FAIL against cc36cf7: compile-only. A track is a VIEW, and
    /// this is the check that it stays one — switching every track on and
    /// painting must leave the log exactly where it was.
    #[test]
    fn showing_a_translation_does_not_edit_the_document() {
        let ctx = test_ctx();
        let mut app = aa_app(false, aa::TrackMode::Selection);
        let before = app.document().unwrap().log.cursor();
        let saved = app.document().unwrap().unsaved();
        app.layout.orf_track = true;
        for _ in 0..3 {
            paint(&mut app, &ctx, window());
        }
        let d = app.document().unwrap();
        assert_eq!(d.log.cursor(), before, "no operation was recorded");
        assert_eq!(d.unsaved(), saved, "and the document is no dirtier");
    }

    /// PROVEN TO FAIL at bd96e5b, behaviourally, on the very first assertion:
    /// `.exact_size(380.0)` sets `outer_size_range = Rangef::point(380)`, so
    /// `fit_per_row` measures 40 bases and no drag can move it. Both halves
    /// fail there — the resting width and the drag.
    #[test]
    fn the_split_moves_and_the_row_width_follows_it() {
        let ctx = test_ctx();
        let mut app = seq_app();
        paint(&mut app, &ctx, window());

        assert_eq!(
            app.edit.per_row(),
            60,
            "the default reaches the GenBank sixty; it was 40 at 380 pt"
        );
        let at_rest = app.layout.panel_w.expect("the panel reported its width");
        assert!((at_rest - App::DEF_PANEL).abs() < 1.0, "{at_rest}");

        // Grab the separator and drag it right, giving the map the room.
        let sep = egui::pos2(1280.0 - at_rest, 400.0);
        paint(&mut app, &ctx, pointer_to(sep));
        paint(&mut app, &ctx, pointer_button(sep, true));
        for x in [sep.x + 100.0, sep.x + 200.0, sep.x + 320.0] {
            paint(&mut app, &ctx, pointer_to(egui::pos2(x, 400.0)));
        }
        paint(
            &mut app,
            &ctx,
            pointer_button(egui::pos2(sep.x + 320.0, 400.0), false),
        );
        paint(&mut app, &ctx, window());

        let narrow = app.layout.panel_w.expect("still reporting");
        assert!(
            (narrow - App::MIN_PANEL).abs() < 1.0,
            "the drag ran into the 300 pt stop rather than past it: {narrow}"
        );
        assert_eq!(app.edit.per_row(), 30, "and the row followed it down");

        // And back, so the gesture is not one-way.
        let sep = egui::pos2(1280.0 - narrow, 400.0);
        paint(&mut app, &ctx, pointer_to(sep));
        paint(&mut app, &ctx, pointer_button(sep, true));
        for x in [sep.x - 100.0, sep.x - 200.0, sep.x - 300.0] {
            paint(&mut app, &ctx, pointer_to(egui::pos2(x, 400.0)));
        }
        paint(
            &mut app,
            &ctx,
            pointer_button(egui::pos2(sep.x - 300.0, 400.0), false),
        );
        paint(&mut app, &ctx, window());
        assert_eq!(app.edit.per_row(), 60, "back to sixty");
        assert!(app.layout.panel_w.unwrap() > 560.0);
    }

    /// The panel may eat the window's width but never all of it.
    ///
    /// PROVEN TO FAIL at bd96e5b for the trivial reason that nothing moves
    /// there at all. What it is really guarding is OURS, not egui's: egui only
    /// caps the panel at the window width, so with no maximum of our own the
    /// map pane goes to zero, `map::show` gets a zero-width `max_rect`, and the
    /// map silently vanishes with nothing on screen explaining why.
    #[test]
    fn dragging_the_split_all_the_way_leaves_the_map_a_pane_to_live_in() {
        let ctx = test_ctx();
        let mut app = seq_app();
        paint(&mut app, &ctx, window());
        let sep = egui::pos2(1280.0 - app.layout.panel_w.unwrap(), 400.0);
        paint(&mut app, &ctx, pointer_to(sep));
        paint(&mut app, &ctx, pointer_button(sep, true));
        // Past the left edge of the window, and then some.
        for x in [400.0f32, 100.0, -500.0] {
            paint(&mut app, &ctx, pointer_to(egui::pos2(x, 400.0)));
        }
        paint(
            &mut app,
            &ctx,
            pointer_button(egui::pos2(-500.0, 400.0), false),
        );
        paint(&mut app, &ctx, window());

        let w = app.layout.panel_w.unwrap();
        assert!(
            w <= 1280.0 - App::MIN_MAP + 1.0,
            "the map was left {} pt: {w}",
            1280.0 - w
        );
        assert!(w >= App::MIN_PANEL, "{w}");
        assert_eq!(app.edit.per_row(), 60, "still a full row at the stop");
    }

    /// COMPILE-ONLY FAILURE at bd96e5b, and said plainly: `App::seq_grid` does
    /// not exist there, so this does not build rather than not passing. The
    /// click arithmetic it exercises is *already correct* at bd96e5b, because
    /// the grouping in this change is painted rather than spaced and no gap was
    /// ever introduced — that is the point, not an omission.
    ///
    /// What it is really proof against is the mutation, which was run:
    /// replacing the column mapping with one that adds a separator cell every
    /// ten columns, matching a painter changed the same way, makes this fail at
    /// column 59 while a test at column 0 goes on passing. Column 0 is correct
    /// under every wrong formula.
    ///
    /// It is also the only place that shows the LAST column of a 60-base row is
    /// reachable at all: at bd96e5b the row is 40 wide, so column 59 does not
    /// exist and this click lands on base 120 instead of 180.
    #[test]
    fn a_click_on_the_last_column_of_a_row_lands_on_that_base() {
        let ctx = test_ctx();
        let mut app = seq_app();
        paint(&mut app, &ctx, window());
        assert_eq!(app.edit.per_row(), 60, "the premise: a full-width row");
        let g = app.seq_grid.expect("the grid was painted");
        assert_eq!(g.first_row, 0);

        // Row 2, its LAST cell, a fifth of the way in — so the nearest gap is
        // the one BEFORE base 180, not the one after it.
        let row = 2u64;
        let col = 59u64;
        let at = egui::pos2(
            g.x0 + (col as f32 + 0.2) * g.advance,
            g.top + (row - g.first_row) as f32 * g.row_h + g.row_h * 0.5,
        );
        paint(&mut app, &ctx, pointer_to(at));
        paint(&mut app, &ctx, pointer_button(at, true));
        paint(&mut app, &ctx, pointer_button(at, false));
        assert_eq!(
            app.edit.caret,
            row * 60 + col,
            "a click on column {col} of row {row}"
        );

        // Past the middle of the same cell is the gap AFTER it, and past the
        // end of the row clamps to that same gap rather than running on into
        // the next row's first base.
        for (dx, want) in [(0.7f32, 60u64), (400.0, 60)] {
            let at = egui::pos2(
                g.x0 + (col as f32 + dx) * g.advance,
                g.top + (row - g.first_row) as f32 * g.row_h + g.row_h * 0.5,
            );
            paint(&mut app, &ctx, pointer_to(at));
            paint(&mut app, &ctx, pointer_button(at, true));
            paint(&mut app, &ctx, pointer_button(at, false));
            assert_eq!(app.edit.caret, row * 60 + want, "dx {dx}");
        }
    }

    /// COMPILE-ONLY FAILURE at bd96e5b, same reason and same mutation. A drag
    /// routes through the same `hit`, so this is the check that the *other*
    /// consumers of the column mapping agree with it: the selection it asserts
    /// on is what the highlight rectangle and the caret are drawn from.
    #[test]
    fn a_drag_across_a_row_boundary_selects_exactly_the_bases_dragged_over() {
        let ctx = test_ctx();
        let mut app = seq_app();
        paint(&mut app, &ctx, window());
        let g = app.seq_grid.expect("the grid was painted");
        let at = |row: u64, col: u64| {
            egui::pos2(
                g.x0 + col as f32 * g.advance,
                g.top + (row - g.first_row) as f32 * g.row_h + g.row_h * 0.5,
            )
        };

        // From the gap before base 51 (row 0, column 50) to the gap before
        // base 131 (row 2, column 10) — across two row boundaries.
        let from = at(0, 50);
        let to = at(2, 10);
        paint(&mut app, &ctx, pointer_to_at(from, 0.0));
        paint(&mut app, &ctx, pointer_button_at(from, true, 0.1));
        // Held, not jogged: see `window_at`.
        paint(&mut app, &ctx, pointer_to_at(from, 1.0));
        paint(&mut app, &ctx, pointer_to_at(at(1, 30), 1.1));
        paint(&mut app, &ctx, pointer_to_at(to, 1.2));
        paint(&mut app, &ctx, pointer_button_at(to, false, 1.3));

        let s = app.edit.sel.expect("a drag makes a selection");
        assert_eq!(s.anchor, 50, "the gap the press landed on");
        assert_eq!(s.head, 130, "the gap the release landed on");
        assert!(!s.through_origin, "a forward drag is not a wrap");
        // 80 bases: 10 on row 0, 60 on row 1, 10 on row 2.
        assert_eq!(s.base_count(8_117), 80);
    }

    /// PROVEN TO FAIL at bd96e5b, behaviourally.
    ///
    /// The caret sentence for a circular molecule at caret 0 is 72 characters,
    /// which wraps to two lines; the buttons landed on a third and the notice
    /// on a fourth, against a fixed `readout_h = 30.0`. Everything past the
    /// reservation was drawn below the panel rect and cut by its clip rect.
    ///
    /// Three passes rather than one, and that is not a fudge: a bottom panel
    /// learns its content's height by laying it out, so the first pass sizes it
    /// and the later ones show it. Twenty passes would not help at bd96e5b,
    /// because 30.0 is a constant.
    /// The `sel` half is a CONFIRMED REGRESSION of the feature editor's own
    /// making, and this test could not see it: it ran with no selection at all,
    /// so `take the other arc` — the widest button in the row — was never drawn,
    /// and it asserted only VERTICAL containment. Measured with the new
    /// "New feature…" button beside "Design primers…": 416 px of content against
    /// ~361 px of panel at a 300 pt split, so the arc button rendered as
    /// `her arc (8,090 bp)` and the readout line `4..30 · 27 bp` as `27 bp` —
    /// the coordinates cut off the panel whose entire job is showing them.
    #[test]
    fn the_readout_and_its_button_are_not_cut_off_at_any_split() {
        let selections = [
            None,
            // Bases 4..30 on the circle: enough of a selection that both dialog
            // buttons enable AND `take the other arc (8,090 bp)` is offered.
            Some(seqedit::Selection {
                anchor: 3,
                head: 30,
                through_origin: false,
            }),
        ];
        for width in [App::DEF_PANEL, App::MIN_PANEL] {
            for sel in selections {
                let ctx = test_ctx();
                let mut app = seq_app();
                app.layout.panel_w = Some(width);
                // Caret 0 on a circle: the longest form the sentence takes.
                app.edit.caret = 0;
                app.edit.sel = sel;
                let mut out = paint_out(&mut app, &ctx, window());
                for _ in 0..2 {
                    out = paint_out(&mut app, &ctx, window());
                }

                // The defect itself, measured: nothing in the panel is painted
                // below the clip rect it was handed. THIS assertion compiles and
                // runs at bd96e5b, where it fails by about 40 pt.
                let lost = drawn_below_the_window(&out, 840.0);
                assert!(
                    lost < 1.0,
                    "{lost:.0} pt of the readout is laid out below the window at a \
                     {width} pt split"
                );

                let r = app.seq_readout.expect("the readout was laid out");
                assert!(
                    r.bottom() <= 840.5,
                    "the readout runs {:.0} pt past the window at a {width} pt panel",
                    r.bottom() - 840.0
                );
                assert!(
                    r.height() >= 40.0,
                    "the readout is {:.0} pt tall at a {width} pt panel, which cannot \
                     hold a sentence that wraps plus a row of buttons",
                    r.height()
                );
                // SIDEWAYS, which is what the new button broke. A wrapped row
                // grows downwards, which the assertions above already bound.
                let (cut, what) = text_clipped_horizontally(&out, r.y_range());
                assert!(
                    cut < 1.0,
                    "{cut:.0} pt of {what:?} is cut off the side of a {width} pt panel with \
                     sel={sel:?}"
                );
                // And the grid above it did not get squeezed out of existence.
                let g = app.seq_grid.expect("the grid was painted");
                assert!(
                    g.top + 8.0 * g.row_h < r.top(),
                    "only {:.0} pt of sequence left above the readout at {width}",
                    r.top() - g.top
                );
            }
        }
    }

    /// PROVEN TO FAIL against the MUTATION named in `annot.rs`: dropping
    /// `doc_generation` from the index's version tuple. It is compile-only
    /// against bd96e5b, where there is no index at all.
    ///
    /// Both documents sit at cursor `None` — every freshly opened one does — so
    /// keying on the cursor alone compares them equal, skips the rebuild, and
    /// paints the first file's features onto the second. Every ribbon lands
    /// somewhere plausible and nothing errors.
    #[test]
    fn a_second_document_is_not_drawn_with_the_first_ones_features() {
        let gb = |name: &str, at: u64| {
            format!(
                "LOCUS       x                      400 bp    DNA     linear   SYN 01-JAN-2026\n\
                 FEATURES             Location/Qualifiers\n\
                 \x20    gene            {at}..{}\n\
                 \x20                    /label={name}\n\
                 ORIGIN\n        1 {}\n//\n",
                at + 49,
                "acgt".repeat(100)
            )
        };
        let mut app = App::blank();
        app.adopt(Document::from_bytes(gb("first", 10).as_bytes(), "a.gb".into(), None).unwrap());
        app.refresh_annotations();
        let mut got = Vec::new();
        app.annot.query(0, 400, &mut got);
        assert_eq!(
            got.iter().map(|i| (i.lo, i.hi)).collect::<Vec<_>>(),
            vec![(9, 59)],
            "the premise: A's feature"
        );

        app.adopt(Document::from_bytes(gb("second", 200).as_bytes(), "b.gb".into(), None).unwrap());
        assert_eq!(
            app.document().unwrap().log.cursor(),
            None,
            "the collision this test is about: both documents are at cursor None"
        );
        app.refresh_annotations();
        got.clear();
        app.annot.query(0, 400, &mut got);
        assert_eq!(
            got.iter().map(|i| (i.lo, i.hi)).collect::<Vec<_>>(),
            vec![(199, 249)],
            "B is drawn with B's features"
        );
    }

    /// The house pattern, from `doc.rs`: a ratio, not an absolute time, so it
    /// means the same thing on a slow machine.
    ///
    /// A plasmid with a full-length `backbone` misc_feature is exactly the case
    /// that makes the "binary search then walk backwards" shape degenerate into
    /// a linear scan, which is why the index is an augmented tree and not that.
    #[test]
    fn a_frame_of_the_sequence_tab_does_not_scan_the_features() {
        let n = 4_641_652u64;
        let mut mol = pl_core::Molecule {
            seq: vec![b'a'; 1_000],
            topology: pl_core::Topology::Circular,
            ..Default::default()
        };
        // One genome-length feature first, then 9,000 ordinary ones — the shape
        // MG1655 arrives in, and the shape that defeats a prefix-max walk.
        let mut backbone = pl_core::Feature::new("backbone", "misc_feature");
        backbone.segments.push(pl_core::Segment::new(1, n));
        mol.features.push(backbone);
        let mut s = 0x2545_F491_4F6C_DD1Du64;
        for i in 0..9_000u64 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let a = 1 + s % (n - 3_000);
            let mut f = pl_core::Feature::new(format!("g{i}"), "CDS");
            f.segments.push(pl_core::Segment::new(a, a + 900));
            mol.features.push(f);
        }
        let ix = annot::AnnotIndex::build(&mol, (0, None));

        // Twenty frames of forty visible rows.
        let t = std::time::Instant::now();
        let mut out = Vec::new();
        let mut seen = 0usize;
        for f in 0..20u64 {
            for r in 0..40u64 {
                let lo = (f * 40 + r) * 60;
                out.clear();
                ix.query(lo, lo + 60, &mut out);
                seen += out.len();
            }
        }
        let twenty_frames = t.elapsed();
        std::hint::black_box(seen);

        // ONE frame done the naive way: every feature, every segment, per row.
        let t = std::time::Instant::now();
        let mut seen = 0usize;
        for r in 0..40u64 {
            let (lo, hi) = (r * 60, r * 60 + 60);
            for f in &mol.features {
                for sg in &f.segments {
                    if sg.start.saturating_sub(1) < hi && sg.end > lo {
                        seen += 1;
                    }
                }
            }
        }
        let one_naive_frame = t.elapsed();
        std::hint::black_box(seen);

        assert!(
            twenty_frames * 10 < one_naive_frame,
            "twenty frames of index queries took {twenty_frames:?}; one frame of \
             the naive per-row scan took {one_naive_frame:?}"
        );
    }

    /// The index rides along with an edit, so it has to be small against the
    /// work that edit already does. `Document::apply` performs a defensive
    /// `Molecule::clone`, measured in `seqedit.rs` at 3.85 ms on a 4.6 Mb
    /// molecule; if the build were comparable it would belong on a worker.
    #[test]
    fn the_index_build_costs_less_than_the_clone_it_rides_along_with() {
        let n = 4_641_652usize;
        let mut mol = pl_core::Molecule {
            seq: vec![b'a'; n],
            topology: pl_core::Topology::Circular,
            ..Default::default()
        };
        let mut s = 0x2545_F491_4F6C_DD1Du64;
        for i in 0..9_000u64 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let a = 1 + s % (n as u64 - 3_000);
            let mut f = pl_core::Feature::new(format!("g{i}"), "CDS");
            f.segments.push(pl_core::Segment::new(a, a + 900));
            mol.features.push(f);
        }

        // Best of several runs, not one sample. A single measurement is noisy on a
        // shared CI runner, and asymmetric here: the build runs first on cold
        // caches and a cold allocator while the clone runs second, warm — one cold
        // build against one warm clone tipped this ratio on a macOS runner (7.6 ms
        // vs 2.5 ms) though neither cost had changed. Interleaving and taking the
        // minimum is the least-perturbed estimate of each intrinsic cost.
        let mut build = std::time::Duration::MAX;
        let mut clone = std::time::Duration::MAX;
        for _ in 0..5 {
            let t = std::time::Instant::now();
            let ix = annot::AnnotIndex::build(&mol, (0, None));
            build = build.min(t.elapsed());
            std::hint::black_box(&ix);

            let t = std::time::Instant::now();
            let c = mol.clone();
            clone = clone.min(t.elapsed());
            std::hint::black_box(&c);
        }

        // A tripwire for the build belonging on a worker thread, not a benchmark:
        // it must stay within a small multiple of the clone every edit already
        // pays for.
        //
        // 15x, RAISED FROM 5x ON MEASUREMENT. The 5 was chosen for "the ~2-3x
        // intrinsic ratio seen across CI arches" and that figure was simply
        // wrong. Three consecutive CI runs failed here on commits that changed
        // nothing near this code:
        //
        //     b2d4a44   11.35 ms / 1.66 ms  = 6.8x
        //     e0ff80c   32.14 ms / 5.86 ms  = 5.5x
        //     fe89c5c   24.36 ms / 3.94 ms  = 6.2x
        //
        // The absolute times move by 3x with runner load; the RATIO does not. It
        // sits near 6, so a bound of 5 fails a correct build most of the time —
        // and a tripwire that fires half the time is worse than none, because it
        // trains its reader to stop looking. It did: three red runs went by
        // unnoticed in one session because CI had stopped meaning anything.
        //
        // 15x keeps what the test is actually for. It exists to catch the build
        // becoming an order of magnitude dearer than the clone — the point at
        // which it belongs on a worker — not to pin a constant factor, and it
        // now has better than 2x headroom over the worst honest measurement.
        assert!(
            build < clone * 15,
            "index build {build:?} against the molecule clone {clone:?} that \
             every edit already pays for"
        );
    }

    /// PROVEN TO FAIL against the MUTATION: passing `vertical_scroll_offset`
    /// on every frame instead of only on the frame `per_row` changed pins the
    /// offset and nothing scrolls at all; dropping it entirely leaves the
    /// pixel offset alone and the base at the top of the viewport jumps by the
    /// ratio of the two row widths. Compile-only against bd96e5b, where the
    /// row width cannot change in the first place.
    ///
    /// The measured version of the defect, on the user's own file: scrolled to
    /// base 4,000 at 40 per row is offset 1,330; the same offset at 60 per row
    /// is base 6,000. Two thousand bases forward, while the user is dragging a
    /// splitter — and not reversible, because the content shrinks and an offset
    /// near the bottom is clamped on the way out and not restored on the way
    /// back.
    #[test]
    fn a_reflow_keeps_the_top_of_the_viewport_on_the_same_base() {
        let ctx = test_ctx();
        let mut app = seq_app();
        paint(&mut app, &ctx, window());
        assert_eq!(app.edit.per_row(), 60);

        // Scroll a long way down, with the pointer over the grid.
        let g = app.seq_grid.expect("the grid was painted");
        let over = egui::pos2(g.x0 + 100.0, g.top + 100.0);
        for _ in 0..12 {
            paint(
                &mut app,
                &ctx,
                egui::RawInput {
                    events: vec![
                        egui::Event::PointerMoved(over),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -400.0),
                            phase: egui::TouchPhase::Move,
                            modifiers: egui::Modifiers::default(),
                        },
                    ],
                    ..window()
                },
            );
        }
        paint(&mut app, &ctx, window());
        let before = app.seq_grid.expect("still painted");
        let base_at_top = before.first_row * before.per_row;
        assert!(
            base_at_top > 1_000,
            "the premise: scrolled somewhere worth losing, not base {base_at_top}"
        );

        // Now narrow the panel, which takes the row from 60 bases to 30.
        let sep = egui::pos2(1280.0 - app.layout.panel_w.unwrap(), 400.0);
        paint(&mut app, &ctx, pointer_to(sep));
        paint(&mut app, &ctx, pointer_button(sep, true));
        for x in [sep.x + 100.0, sep.x + 200.0, sep.x + 320.0] {
            paint(&mut app, &ctx, pointer_to(egui::pos2(x, 400.0)));
        }
        paint(
            &mut app,
            &ctx,
            pointer_button(egui::pos2(sep.x + 320.0, 400.0), false),
        );
        paint(&mut app, &ctx, window());
        paint(&mut app, &ctx, window());

        let after = app.seq_grid.expect("still painted");
        assert_eq!(after.per_row, 30, "the premise: the row really reflowed");
        let now = after.first_row * after.per_row;
        // Within one row of where it was. Without the anchor this is out by
        // the ratio — half the file, on an 8 kb plasmid.
        assert!(
            now.abs_diff(base_at_top) <= after.per_row,
            "the view was on base {base_at_top} and the reflow moved it to {now}"
        );
    }

    /// And it keeps the caret where it was ON SCREEN, not merely on screen.
    ///
    /// PROVEN TO FAIL against the MUTATION, which was run: setting the offset to
    /// `row_of(anchor, per_row) * row_h` — the previous expression — drags the
    /// caret's row to the top slot and this fails with "the caret's row moved
    /// from slot 5 to slot 0". Compile-only against bd96e5b, where the row
    /// width cannot change.
    ///
    /// The measured version: on the genome the caret sat on the second visible
    /// row, one keystroke reflowed, and its row was pulled to the first. That
    /// fires on every splitter step and, before the row-height fix above, twice
    /// per keystroke.
    #[test]
    fn a_reflow_keeps_the_caret_at_the_same_height_in_the_viewport() {
        let ctx = test_ctx();
        let mut app = seq_app();
        paint(&mut app, &ctx, window());
        let g = app.seq_grid.expect("painted");
        assert_eq!(g.per_row, 60);

        // Scroll well down, then put the caret on a row a few slots below the
        // top of what is showing.
        let over = egui::pos2(g.x0 + 100.0, g.top + 100.0);
        for _ in 0..12 {
            paint(
                &mut app,
                &ctx,
                egui::RawInput {
                    events: vec![
                        egui::Event::PointerMoved(over),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -400.0),
                            phase: egui::TouchPhase::Move,
                            modifiers: egui::Modifiers::default(),
                        },
                    ],
                    ..window()
                },
            );
        }
        paint(&mut app, &ctx, window());
        let g = app.seq_grid.expect("painted");
        let slot = 5u64;
        let at = egui::pos2(
            g.x0 + 3.5 * g.advance,
            g.top + slot as f32 * g.row_h + g.row_h * 0.5,
        );
        paint(&mut app, &ctx, pointer_to(at));
        paint(&mut app, &ctx, pointer_button(at, true));
        paint(&mut app, &ctx, pointer_button(at, false));
        paint(&mut app, &ctx, window());

        let before = app.seq_grid.expect("painted");
        let caret_row = seqedit::row_of(app.edit.caret, before.per_row);
        assert_eq!(
            caret_row - before.first_row,
            slot,
            "the premise: the caret is well below the top of the viewport"
        );

        // Reflow by dragging the splitter to the narrow stop.
        let sep = egui::pos2(1280.0 - app.layout.panel_w.unwrap(), 400.0);
        paint(&mut app, &ctx, pointer_to(sep));
        paint(&mut app, &ctx, pointer_button(sep, true));
        for x in [sep.x + 100.0, sep.x + 200.0, sep.x + 320.0] {
            paint(&mut app, &ctx, pointer_to(egui::pos2(x, 400.0)));
        }
        paint(
            &mut app,
            &ctx,
            pointer_button(egui::pos2(sep.x + 320.0, 400.0), false),
        );
        paint(&mut app, &ctx, window());
        paint(&mut app, &ctx, window());

        let after = app.seq_grid.expect("painted");
        assert!(after.per_row < before.per_row, "the premise: it reflowed");
        let now = seqedit::row_of(app.edit.caret, after.per_row);
        assert_eq!(
            now - after.first_row,
            slot,
            "the caret's row moved from slot {slot} to slot {}",
            now.saturating_sub(after.first_row)
        );
    }

    /// Everything a row draws is on the grid the LETTERS are on.
    ///
    /// COMPILE-ONLY at bd96e5b (`GridGeom` and `RowLayout` do not exist), but
    /// this is the assertion the rest of the suite did not have, and its absence
    /// was the real finding: four separate mutations of the column mapping were
    /// run against the whole 194-test pl-gui suite and three of them stayed
    /// green. The one that matters is the one the brief names — grouping the
    /// bases in tens by inserting a space every ten characters. It renders as a
    /// nicer GenBank-style view, puts the caret four bases left of the letter it
    /// names by column 47, and nothing anywhere noticed, because every existing
    /// check compared `col_x` with `x_col`: two pure functions over the same
    /// expression.
    ///
    /// So this one reads the PAINTED geometry instead. It pulls the row's own
    /// galley out of the frame and asks where the glyphs actually landed, then
    /// asks the caret and the selection rectangle to agree with them. Both
    /// mutations were run: inserting a separator every ten characters into the
    /// painted string fails the pitch assertion at column 10, and adding a
    /// separator cell to `col_x` alone fails the caret assertion at column 30.
    #[test]
    fn the_caret_and_the_selection_land_on_the_glyphs_that_were_painted() {
        let ctx = test_ctx();
        let mut app = seq_app();
        let mut out = paint_out(&mut app, &ctx, window());
        let per_row = app.edit.per_row();
        assert_eq!(per_row, 60, "the premise: a full-width row");

        // The letters, as the frame really placed them.
        //
        // Compared against the nominal grid, not against a uniform float
        // pitch, and the difference is worth stating because it was measured
        // here rather than assumed.
        //
        // `epaint` snaps every glyph to a whole device pixel
        // (`text_layout.rs`: `glyph.pos.x = round_to_pixel(glyph.pos.x)`), so
        // at this metric — advance 6.9236 at ppp 1 — consecutive steps are 7,
        // 6, 7, 7, 7... and the deviation from `x0 + k * advance` is a sawtooth
        // in (-0.88, +0.08) that resets about every thirteenth glyph. Bounded
        // and NOT cumulative, which is the whole question: an error that
        // accumulated would be a cell and a half out by column 59 and would
        // look perfect in a screenshot of column 0.
        let xs = painted_row_glyphs(&out, per_row as usize);
        assert_eq!(xs.len(), per_row as usize);
        let adv = app.seq_grid.expect("painted").advance;
        // One device pixel. Every wrong column formula is out by at least one
        // whole advance, which is seven times this.
        let tol = 1.01 / ctx.pixels_per_point();
        for k in 1..xs.len() {
            assert!(
                (xs[k] - xs[0] - k as f32 * adv).abs() <= tol,
                "glyph {k} was painted at {:.3}, which is {:.3} off the \
                 {adv:.3} pt grid every other x in this view is computed from",
                xs[k],
                xs[k] - xs[0] - k as f32 * adv
            );
        }

        // The caret, at both ends of the row and in the middle. Column 59 is
        // the one that matters: column 0 is correct under every wrong formula.
        for col in [0u64, 30, 59] {
            app.edit.caret = col;
            app.edit.sel = None;
            out = paint_out(&mut app, &ctx, window());
            let x = painted_caret(&out).unwrap_or_else(|| panic!("no caret at column {col}"));
            let want = xs[col as usize];
            assert!(
                (x - want).abs() <= tol,
                "the caret for column {col} is painted at {x:.2}, and that \
                 column's letter at {want:.2}"
            );
        }
        // And gap 60 — the one with no glyph of its own — is drawn at the
        // START of the next row and not at the right edge of this one, because
        // that gap is where the next row's first base begins. Only at the end
        // of the MOLECULE does it belong to the row's right edge, which is what
        // `on_this_row`'s second clause is for.
        app.edit.caret = per_row;
        app.edit.sel = None;
        out = paint_out(&mut app, &ctx, window());
        let x = painted_caret(&out).expect("a caret at the end-of-row gap");
        assert!(
            (x - xs[0]).abs() <= tol,
            "the end-of-row gap is painted at {x:.2}; the next row starts at {:.2}",
            xs[0]
        );

        // And the selection wash, whose two corners are the other two places an
        // x is turned into a column.
        app.edit.caret = 20;
        app.edit.sel = Some(seqedit::Selection {
            anchor: 10,
            head: 20,
            through_origin: false,
        });
        out = paint_out(&mut app, &ctx, window());
        let r = painted_selection(&out, &ctx).expect("the wash was painted");
        assert!(
            (r.left() - xs[10]).abs() <= tol && (r.right() - xs[20]).abs() <= tol,
            "the wash runs {:.2}..{:.2}; bases 11..20 are drawn at {:.2}..{:.2}",
            r.left(),
            r.right(),
            xs[10],
            xs[20]
        );
    }

    /// Where the first full row's BASES were actually painted.
    ///
    /// Separators are tolerated in the string and skipped in the answer, on
    /// purpose: the mutation this test exists for inserts a space every ten
    /// characters, and it should fail on where the letters landed rather than
    /// on the helper failing to recognise the row at all.
    fn painted_row_glyphs(out: &egui::FullOutput, want: usize) -> Vec<f32> {
        for cs in &out.shapes {
            let egui::Shape::Text(t) = &cs.shape else {
                continue;
            };
            let txt = t.galley.text();
            if !txt.bytes().all(|b| b"ACGT ".contains(&b))
                || txt.bytes().filter(|b| *b != b' ').count() != want
            {
                continue;
            }
            let row = &t.galley.rows[0];
            return row
                .glyphs
                .iter()
                .filter(|g| g.chr != ' ')
                .map(|g| t.pos.x + row.pos.x + g.pos.x)
                .collect();
        }
        panic!("no sequence row was painted");
    }

    /// The caret is the only 1.5 pt line in this view; every other rule — the
    /// tens ticks, the boundary ticks, the selection edges — is 1.0.
    fn painted_caret(out: &egui::FullOutput) -> Option<f32> {
        out.shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::LineSegment { points, stroke }
                if (stroke.width - 1.5).abs() < 0.01 && points[0].x == points[1].x =>
            {
                Some(points[0].x)
            }
            _ => None,
        })
    }

    fn painted_selection(out: &egui::FullOutput, ctx: &egui::Context) -> Option<egui::Rect> {
        let want = Palette::of(ctx.theme() == egui::Theme::Dark).selection();
        out.shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Rect(r) if r.fill == want => Some(r.rect),
            _ => None,
        })
    }

    /// A pointer over a base names THAT base, wherever in the cell it is.
    ///
    /// PROVEN TO FAIL before this run, behaviourally, on the second probe of
    /// the first cell: the hover line read a caret gap index as a base index, so
    /// past the middle of every glyph it named the base after it. Measured in
    /// the running app on pKoV: 20% into base 180's cell said "base 180" and 75%
    /// into the SAME cell said "base 181". At the last column of a row it named
    /// the first base of the next row — sixty cells from the pointer.
    ///
    /// This line is the design's stated non-colour channel for every ribbon
    /// above it, so it is also the thing a user checks a feature edge against.
    #[test]
    fn the_hover_line_names_the_cell_the_pointer_is_in_at_both_ends_of_a_row() {
        let ctx = test_ctx();
        let mut app = seq_app();
        paint(&mut app, &ctx, window());
        let g = app.seq_grid.expect("painted");
        assert_eq!(g.per_row, 60, "the premise: a full-width row");

        let hover = |app: &mut App, ctx: &egui::Context, row: u64, col: f32| -> Option<String> {
            let at = egui::pos2(
                g.x0 + col * g.advance,
                g.top + (row - g.first_row) as f32 * g.row_h + g.row_h * 0.5,
            );
            paint(app, ctx, pointer_to(at));
            app.seq_hover.clone()
        };

        // Column 0, and the last column of the row — the one where being out by
        // a gap names a base on another row entirely.
        for (row, col, want) in [
            (0u64, 0u64, 1u64),
            (0, 9, 10),
            (0, 59, 60),
            (2, 59, 180),
            (2, 0, 121),
        ] {
            for frac in [0.05f32, 0.5, 0.95] {
                let got = hover(&mut app, &ctx, row, col as f32 + frac);
                assert_eq!(
                    got.as_deref()
                        .map(|s| s.split(' ').nth(1).unwrap_or("").to_string()),
                    Some(fmt_int(want)),
                    "row {row} column {col} at {frac} across the cell: {got:?}"
                );
            }
        }

        // And off the cells there is no base to name, rather than the nearest
        // one. Past the right edge of a full row:
        let off = egui::pos2(g.x0 + 60.5 * g.advance, g.top + g.row_h * 0.5);
        paint(&mut app, &ctx, pointer_to(off));
        assert_eq!(app.seq_hover, None, "past the last cell of the row");
        // And in the gutter, left of column 0:
        let gutter = egui::pos2(g.x0 - 4.0, g.top + g.row_h * 0.5);
        paint(&mut app, &ctx, pointer_to(gutter));
        assert_eq!(app.seq_hover, None, "in the coordinate gutter");
    }

    /// The hover line clears when the pointer leaves the grid.
    ///
    /// PROVEN TO FAIL before this run: the assignment was guarded by
    /// `if hover_out.is_some()`, so the readout went on naming base 3,930 with
    /// the pointer deep inside the map pane.
    #[test]
    fn the_hover_line_stops_naming_a_base_once_the_pointer_leaves() {
        let ctx = test_ctx();
        let mut app = seq_app();
        paint(&mut app, &ctx, window());
        let g = app.seq_grid.expect("painted");
        let on = egui::pos2(g.x0 + 10.0 * g.advance, g.top + g.row_h * 1.5);
        paint(&mut app, &ctx, pointer_to(on));
        assert!(app.seq_hover.is_some(), "the premise: it named a base");
        // Into the map pane, which is everything left of the panel.
        paint(&mut app, &ctx, pointer_to(egui::pos2(200.0, 400.0)));
        assert_eq!(app.seq_hover, None);
    }

    /// A plasmid whose digest has actually finished.
    fn digested(app: &mut App) {
        let t = std::time::Instant::now();
        loop {
            let d = app.bench.get_mut().expect("a document");
            if matches!(d.digest, DigestState::Done(_)) {
                return;
            }
            d.digest.poll();
            assert!(
                t.elapsed() < std::time::Duration::from_secs(30),
                "the digest never finished"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// The gel is built when something changes, and NOT once a frame.
    ///
    /// PROVEN TO FAIL against the code this replaces, which called
    /// `self.gel.build(...)` unconditionally from `central` on every repaint,
    /// defended by a comment saying "there is nothing to cache and nothing to
    /// go stale". On a 4.6 Mb genome that frame ran `Gel::run` over ~14,000
    /// fragments across seven lanes and formatted kilobytes of prose, and
    /// twelve mouse-moves over the pane cost 4.8x the same twelve over the map.
    ///
    /// Identity, not equality: two equal pictures built twice is exactly the
    /// waste being measured, so this asks whether the SAME `Vec` came back.
    #[test]
    fn the_gel_is_rebuilt_only_when_something_it_depends_on_changes() {
        let mut app = App::blank();
        app.load(PathBuf::from("../../prototype/demo-construct.gb"));
        digested(&mut app);

        app.gel_ready().expect("a finished digest builds");
        let first = app
            .gel_cache
            .as_ref()
            .expect("cached")
            .1
            .scene
            .items
            .as_ptr();
        for _ in 0..5 {
            app.gel_ready().expect("still fine");
            assert_eq!(
                app.gel_cache
                    .as_ref()
                    .expect("cached")
                    .1
                    .scene
                    .items
                    .as_ptr(),
                first,
                "the gel was rebuilt with nothing changed"
            );
        }

        // ...and every control that CAN change the picture does rebuild it.
        // A key that missed one of these would serve a stale gel, which is the
        // failure a memo trades for the one above.
        type Change = (&'static str, fn(&mut App));
        let changes: Vec<Change> = vec![
            ("agarose", |a| a.gel.conditions.agarose_percent = 2.0),
            ("band width", |a| a.gel.conditions.band_mm = 4.0),
            ("run length", |a| a.gel.conditions.run_mm = 120.0),
            ("ladder", |a| a.gel.ladder = "100bp"),
            ("dark field", |a| a.gel.inverted = !a.gel.inverted),
            ("arrangement", |a| {
                a.gel.arrangement = gel::Arrangement::Together
            }),
            ("a tick", |a| {
                a.gel.picked.insert("EcoRI".into());
            }),
            ("the enzyme filter", |a| {
                a.enzyme_set = pl_enzymes::EnzymeSet::Unique
            }),
        ];
        for (what, change) in changes {
            let before = app
                .gel_cache
                .as_ref()
                .expect("cached")
                .1
                .scene
                .items
                .as_ptr();
            change(&mut app);
            app.gel_ready().expect("still fine");
            let after = app
                .gel_cache
                .as_ref()
                .expect("cached")
                .1
                .scene
                .items
                .as_ptr();
            assert_ne!(before, after, "changing the {what} did not rebuild the gel");
        }

        // And an EDIT, which is the one that matters: a gel of a sequence that
        // has since changed is the stale-answer defect `Document::apply`'s
        // unconditional re-digest exists to prevent.
        let before = app
            .gel_cache
            .as_ref()
            .expect("cached")
            .1
            .scene
            .items
            .as_ptr();
        app.bench
            .get_mut()
            .expect("a document")
            .apply(pl_core::OpKind::InsertAt {
                at: 1,
                seq: "GAATTCGAATTC".into(),
            })
            .expect("an insert applies");
        digested(&mut app);
        app.gel_ready().expect("still fine");
        assert_ne!(
            before,
            app.gel_cache
                .as_ref()
                .expect("cached")
                .1
                .scene
                .items
                .as_ptr(),
            "an edit left the old gel on screen"
        );
    }

    /// The row pitch is a property of the DOCUMENT, not of a background
    /// worker's phase.
    ///
    /// PROVEN TO FAIL against the MUTATION, which was run: taking the
    /// reservation from `self.annot.cut_count() > 0` — the previous expression
    /// — fails the last assertion with a row height of 26.41 against 38.41.
    /// Compile-only against bd96e5b, which draws no strip at all.
    ///
    /// Measured on the 4.6 Mb genome, where the digest is 1,634 ms: one
    /// keystroke took the pitch 43.41 -> 31.41 and back, so the whole view
    /// reflowed and re-anchored twice, over a second apart, per typing burst.
    /// On an 8 kb plasmid the scan is milliseconds and none of it is visible,
    /// which is why it was not seen.
    #[test]
    fn one_keystroke_does_not_change_the_row_height_while_the_digest_reruns() {
        let ctx = test_ctx();
        let mut app = seq_app();
        digested(&mut app);
        paint(&mut app, &ctx, window());
        let settled = app.seq_row_h;
        assert!(
            app.annot.cut_count() > 0 && app.enz_strip,
            "the premise: this molecule has admitted cuts, so a strip is reserved"
        );

        // Any edit restarts the digest, and nothing polls it here.
        app.bench
            .get_mut()
            .expect("a document")
            .apply(pl_core::OpKind::InsertAt {
                at: 101,
                seq: "acgt".into(),
            })
            .expect("a legal insert");
        assert!(
            app.document().unwrap().digest.is_running(),
            "the premise: the scan restarted"
        );
        paint(&mut app, &ctx, window());
        assert_eq!(
            app.annot.cut_count(),
            0,
            "the premise: the new scan has not landed, so nothing is drawn"
        );
        assert_eq!(
            app.seq_row_h, settled,
            "the row pitch followed the worker: {} while scanning against {settled} settled",
            app.seq_row_h
        );
    }

    /// The default split reaches the GenBank sixty and is not one point wider
    /// than it has to be.
    ///
    /// Both halves matter and they pull against each other. Below the threshold
    /// the row is not the row GenBank prints, which is the whole point of the
    /// layout work. Above it, every extra point comes out of the map pane, and a
    /// narrower pane is a smaller ring, a shorter `ring::label_room` and more
    /// enzyme names shortened. It used to be worse than "shortened": with a
    /// *fixed* 132 pt reserve, a pane narrower than it was tall rendered
    /// "EcoRI 7,530" as "coRI 7,530" — a truncation that reads as a different
    /// enzyme rather than as damage — and at 560 that was seven labels on the
    /// user's own window.
    ///
    /// COMPILE-ONLY at bd96e5b, where the panel is `exact_size(380.0)` and
    /// neither number exists.
    #[test]
    fn the_default_split_reaches_sixty_and_takes_no_more_than_it_needs() {
        for (w, want, why) in [
            (
                App::DEF_PANEL,
                60u64,
                "the default reaches the GenBank sixty",
            ),
            (
                App::DEF_PANEL - 12.0,
                60,
                "with metric headroom: a wider scrollbar or a different \
                 monospace face still gets sixty",
            ),
            (
                App::DEF_PANEL - 40.0,
                50,
                "and it is not padded — 40 pt below the default the row is \
                 already short, so the default is near the threshold and not \
                 sitting 60 pt above it taking width off the map",
            ),
        ] {
            let ctx = test_ctx();
            let mut app = seq_app();
            app.layout.panel_w = Some(w);
            paint(&mut app, &ctx, window());
            assert_eq!(app.edit.per_row(), want, "at a {w} pt panel: {why}");
        }
    }

    /// A window narrower than both floors together keeps the PANEL usable and
    /// lets the map take the loss, and nothing panics on the way.
    ///
    /// Not a defect but a stated trade, and this is where it is stated. It is
    /// only reachable by ignoring `min_inner_size` (880 x 560 against a 660 pt
    /// combined floor), which `SetWindowPos` does.
    #[test]
    fn a_window_too_narrow_for_both_floors_keeps_the_panel_and_shrinks_the_map() {
        for (w, h) in [(660.0f32, 400.0f32), (584.0, 341.0), (404.0, 131.0)] {
            let ctx = test_ctx();
            let mut app = seq_app();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(w, h),
                )),
                ..Default::default()
            };
            for _ in 0..3 {
                paint(&mut app, &ctx, input.clone());
            }
            let p = app.layout.panel_w.expect("the panel reported its width");
            assert!(
                p >= App::MIN_PANEL - 1.0 || p >= w - 1.0,
                "the panel was squeezed to {p} in a {w} x {h} client"
            );
            assert!(
                app.edit.per_row() >= 10,
                "and the row still holds bases: {}",
                app.edit.per_row()
            );
        }
    }

    // -----------------------------------------------------------------------
    // The plasmid map: the label ring, the ruler's band, and the caption
    //
    // These paint real frames and assert on the shapes that came back, because
    // every complaint being answered here is about where something ended up on
    // screen — and `map.rs` can be self-consistent about its numbers while the
    // picture is wrong. That is exactly what shipped: `LABEL_RESERVE = 132.0`
    // was a perfectly consistent constant that cut the front off `EcoRI 7,530`.
    // -----------------------------------------------------------------------

    /// The user's own plasmid: 8,117 bp circular, with the feature table the
    /// Features tab lists for it.
    ///
    /// Built here rather than read from `pKoV with His decR.dna`, because a test
    /// that needs a file in one person's Downloads folder fails on every other
    /// machine. The coordinates are that file's, taken off the Features tab.
    fn pkov() -> pl_core::Molecule {
        let mut s = 0x1234_5678_9abc_def1u64;
        let seq: Vec<u8> = (0..8_117)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                b"ACGT"[(s >> 33) as usize & 3]
            })
            .collect();
        let mut mol = pl_core::Molecule {
            seq,
            topology: pl_core::Topology::Circular,
            ..Default::default()
        };
        for &(name, start, end, strand) in &[
            ("cat promoter", 7_748u64, 7_850u64, Strand::Reverse),
            ("CmR", 7_088, 7_747, Strand::Reverse),
            ("sacB promoter", 3_398, 3_843, Strand::Reverse),
            // The reverse feature that matters twice: it is the magenta arc that
            // painted over the "3,247" ruler tick, and 3,247 falls inside it.
            ("SacB", 1_976, 3_397, Strand::Reverse),
            ("f1 ori", 3_945, 4_399, Strand::Reverse),
            ("pSC101 ori", 363, 585, Strand::Unoriented),
            ("Rep101(Ts)", 633, 1_583, Strand::Forward),
            ("decR", 5_423, 5_878, Strand::Unoriented),
            ("decR his", 5_423, 5_905, Strand::Unoriented),
        ] {
            let mut f = pl_core::Feature::new(name, "misc_feature");
            f.strand = strand;
            f.segments = vec![pl_core::Segment::new(start, end)];
            mol.features.push(f);
        }
        mol
    }

    /// pKoV's 22 unique cutters, at the coordinates the map printed for them.
    ///
    /// A `Digest` by hand rather than `digest_all` on the synthetic sequence
    /// above: the *positions* are what the layout is being tested against, and
    /// three of these pairs — SalI/XbaI 6 bp apart, SphI/NsiI and XmaI/SmaI 2 bp
    /// apart — are the co-located cases that decide whether merging is right.
    fn pkov_cutters() -> Vec<pl_enzymes::Digest> {
        pkov_cutter_names()
            .iter()
            .map(|&(name, pos)| pl_enzymes::Digest {
                enzyme: pl_enzymes::by_name(name)
                    .unwrap_or_else(|| panic!("{name} is not in the shipped enzyme table")),
                positions: vec![pos],
            })
            .collect()
    }

    fn pkov_cutter_names() -> Vec<(&'static str, u64)> {
        vec![
            ("AflII", 271),
            ("SpeI", 562),
            ("NdeI", 1_682),
            ("HindIII", 2_059),
            ("SnaBI", 2_648),
            ("BsrGI", 2_711),
            ("SalI", 4_413),
            ("XbaI", 4_419),
            ("SphI", 4_758),
            ("NsiI", 4_760),
            ("BglII", 4_886),
            ("SacI", 5_171),
            ("PmeI", 5_345),
            ("PstI", 5_464),
            ("BamHI", 5_588),
            ("MluI", 5_932),
            ("BclI", 6_561),
            ("XmaI", 6_917),
            ("SmaI", 6_919),
            ("ScaI", 7_117),
            ("EcoRI", 7_530),
            ("BbsI", 7_963),
        ]
    }

    /// One frame of nothing but the map, filling a pane of the given size.
    ///
    /// `Frame::NONE`, so the pane the map is handed *is* the rect returned and
    /// "did a label run off the pane" is a question about numbers this test
    /// knows. Two passes because egui's first frame has no galley cache and the
    /// map measures its own labels to decide the radius.
    fn paint_map(
        mol: &pl_core::Molecule,
        caption: &str,
        digest: &[pl_enzymes::Digest],
        w: f32,
        h: f32,
    ) -> (Vec<egui::Shape>, egui::Rect) {
        paint_map_sel(mol, caption, digest, w, h, None)
    }

    /// The same, with a sequence selection on it.
    fn paint_map_sel(
        mol: &pl_core::Molecule,
        caption: &str,
        digest: &[pl_enzymes::Digest],
        w: f32,
        h: f32,
        sel: Option<pl_core::Segment>,
    ) -> (Vec<egui::Shape>, egui::Rect) {
        paint_map_with(mol, caption, digest, w, h, sel, pl_enzymes::EnzymeSet::All)
    }

    /// The same again, under a stated enzyme filter.
    ///
    /// Every map test used to pass `EnzymeSet::All` — the one value for which a
    /// filter bug cannot manifest — so `debug_assert!(told.closes())` was
    /// satisfied by construction and a disclosure that misdescribed three of the
    /// five settings passed 1,336 tests. A check that cannot fail proves nothing.
    fn paint_map_with(
        mol: &pl_core::Molecule,
        caption: &str,
        digest: &[pl_enzymes::Digest],
        w: f32,
        h: f32,
        sel: Option<pl_core::Segment>,
        set: pl_enzymes::EnzymeSet,
    ) -> (Vec<egui::Shape>, egui::Rect) {
        let ctx = test_ctx();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
        let input = egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        };
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let out = ctx.run_ui(input.clone(), |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        map::show(
                            ui,
                            mol,
                            caption,
                            digest,
                            None,
                            None,
                            sel.clone(),
                            None,
                            set,
                            None,
                        );
                    });
            });
            shapes = flat_shapes(&out.shapes);
        }
        (shapes, rect)
    }

    /// Every painted shape, with `Shape::Vec` expanded.
    fn flat_shapes(clipped: &[egui::epaint::ClippedShape]) -> Vec<egui::Shape> {
        fn walk(s: &egui::Shape, out: &mut Vec<egui::Shape>) {
            match s {
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                other => out.push(other.clone()),
            }
        }
        let mut out = Vec::new();
        for cs in clipped {
            walk(&cs.shape, &mut out);
        }
        out
    }

    /// Every text drawn in one font, as `(text, rect)`.
    ///
    /// Family as well as size: the enzyme labels are monospace 10 and the line
    /// saying what the map is not showing is proportional 10, and a test that
    /// mixed them would assert about the wrong thing.
    fn texts_in(
        shapes: &[egui::Shape],
        size: f32,
        family: egui::FontFamily,
    ) -> Vec<(String, egui::Rect)> {
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Text(t) => {
                    let f = &t.galley.job.sections.first()?.format.font_id;
                    ((f.size - size).abs() < 0.01 && f.family == family).then(|| {
                        (
                            t.galley.text().to_string(),
                            egui::Rect::from_min_size(t.pos, t.galley.size()),
                        )
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// Every hairline polyline: leaders, ruler ticks, join hairs.
    fn hairlines(shapes: &[egui::Shape]) -> Vec<Vec<egui::Pos2>> {
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::LineSegment { points, stroke } if stroke.width <= 1.6 => {
                    Some(points.to_vec())
                }
                egui::Shape::Path(p) if p.stroke.width <= 1.6 && p.points.len() >= 2 => {
                    Some(p.points.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// Every feature band, as `(polyline, half the stroke width)`.
    ///
    /// Picked out by weight: a band is 9 pt, the backbone is 1.5 and every
    /// leader is 1 or less, so there is nothing to confuse it with.
    fn feature_bands(shapes: &[egui::Shape]) -> Vec<(Vec<egui::Pos2>, f32)> {
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Path(p) if p.stroke.width >= 6.0 && p.points.len() >= 2 => {
                    Some((p.points.clone(), p.stroke.width * 0.5))
                }
                _ => None,
            })
            .collect()
    }

    /// Every vertex of every painted shape, so "did any ink leave the pane" is a
    /// question about numbers.
    fn all_vertices(shapes: &[egui::Shape]) -> Vec<egui::Pos2> {
        let mut out = Vec::new();
        for s in shapes {
            match s {
                egui::Shape::Path(p) => out.extend(p.points.iter().copied()),
                egui::Shape::LineSegment { points, .. } => out.extend(points.iter().copied()),
                egui::Shape::Circle(c) => out.extend([
                    egui::Pos2::new(c.center.x - c.radius, c.center.y - c.radius),
                    egui::Pos2::new(c.center.x + c.radius, c.center.y + c.radius),
                ]),
                egui::Shape::Text(t) => {
                    out.extend([t.pos, t.pos + t.galley.size()]);
                }
                _ => {}
            }
        }
        out
    }

    /// PROVEN TO FAIL at e087e27 and in the working tree before this pass.
    ///
    /// pET28a's `rep_origin` is `2464` — a single-base GenBank location, and a
    /// real one: the file's own note is "base 2464 represents the first base of
    /// the newly synthesized single strand". `arc_points` floored the sweep it
    /// counted STEPS with and interpolated with the raw `a1 - a0`, so the arc came
    /// back as three coincident points, and `Shape::line` over a zero-length path
    /// with a 9 pt stroke tessellated into a translucent wedge across half the map
    /// pane. `draw_arrowhead` did the same with `sweep == 0`, giving `head == 0`
    /// and three vertices on one ray. It showed up on four of nine real files as
    /// feature-coloured lines drawn through the backbone, the caption and the
    /// disclosure line, and it is radius-dependent — absent at a maximised window
    /// and present at the default — so a change that alters every radius on the map
    /// cannot leave it alone.
    #[test]
    fn a_feature_a_few_bases_long_does_not_tessellate_across_the_pane() {
        let mut mol = pkov();
        // One base, three bases, nine bases: the shapes seen in the field, on a
        // 1 bp, a 3 bp CDS and a 9 bp `-10` box respectively.
        for (name, start, end, strand) in [
            ("rep_origin", 2_464u64, 2_464u64, Strand::Unoriented),
            ("phoE", 6_163, 6_165, Strand::Forward),
            ("-35", 46, 51, Strand::Reverse),
            ("-10", 191, 199, Strand::Forward),
        ] {
            let mut f = pl_core::Feature::new(name, "misc_feature");
            f.strand = strand;
            f.segments = vec![pl_core::Segment::new(start, end)];
            mol.features.push(f);
        }
        for (w, h) in [(706.0f32, 756.0f32), (880.0, 620.0), (1400.0, 950.0)] {
            // Asserted on the TESSELLATED mesh, not on the shapes.
            //
            // The shapes were always in the pane: `arc_points` returned three
            // points on the ring and they were coincident, not distant. The wedge
            // is manufactured one stage later, where the tessellator computes a
            // segment normal from a zero-length segment and gets infinities. So a
            // test that walks `Shape` vertices passes with the defect present —
            // this one was written that way first and did.
            let ctx = test_ctx();
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
            let input = egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            };
            let mut clipped = Vec::new();
            for _ in 0..2 {
                let out = ctx.run_ui(input.clone(), |ui| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ui, |ui| {
                            map::show(
                                ui,
                                &mol,
                                "pET28a",
                                &pkov_cutters(),
                                None,
                                None,
                                None,
                                None,
                                pl_enzymes::EnzymeSet::All,
                                None,
                            );
                        });
                });
                clipped = out.shapes;
            }
            let grown = rect.expand(2.0);
            let mut verts = 0usize;
            for prim in ctx.tessellate(clipped, 1.0) {
                if let egui::epaint::Primitive::Mesh(m) = prim.primitive {
                    for v in &m.vertices {
                        verts += 1;
                        assert!(
                            v.pos.x.is_finite() && v.pos.y.is_finite(),
                            "{w}x{h}: a mesh vertex is at {:?}",
                            v.pos
                        );
                        assert!(
                            grown.contains(v.pos),
                            "{w}x{h}: a mesh vertex at {:?} is outside the {rect:?} pane",
                            v.pos
                        );
                    }
                }
            }
            assert!(
                verts > 500,
                "{w}x{h}: only {verts} mesh vertices — nothing drawn"
            );

            // And the short features are still drawn, as marks. Dropping them
            // would satisfy everything above and lose four features.
            let shapes = flat_shapes(
                &ctx.run_ui(input.clone(), |ui| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ui, |ui| {
                            map::show(
                                ui,
                                &mol,
                                "pET28a",
                                &pkov_cutters(),
                                None,
                                None,
                                None,
                                None,
                                pl_enzymes::EnzymeSet::All,
                                None,
                            );
                        });
                })
                .shapes,
            );
            let (centre, r) = backbone(&shapes);
            let marks = shapes
                .iter()
                .filter(|s| match s {
                    egui::Shape::LineSegment { points, stroke } => {
                        (stroke.width - 1.75).abs() < 0.01
                            && (points[0] - centre).length() > r * 0.5
                    }
                    _ => false,
                })
                .count();
            assert!(marks >= 4, "{w}x{h}: {marks} radial marks, not 4");
        }
    }

    /// PROVEN TO FAIL against the working tree as handed over: `map::show` took
    /// `ui.max_rect()`, the whole CentralPanel, while `recovery_banner` and the
    /// notice strip are laid out inside the same panel above the map.
    ///
    /// At e087e27 that was harmless, because `label_slots` only ever produced two
    /// side columns and the top of the pane was empty. `ring::place_ring` puts a
    /// row there, and `EcoRI  7,530   BbsI  7,963   AflII  271   SpeI  562` was
    /// painted across the banner's own text with SpeI's leader drawn down through
    /// the `Discard` button. The banner is the recover-or-discard decision for an
    /// unsaved draft, so map ink over its buttons is worse than cosmetic.
    #[test]
    fn the_map_is_drawn_below_whatever_shares_its_panel() {
        let ctx = test_ctx();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(760.0, 800.0));
        let input = egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        };
        let mol = pkov();
        let cutters = pkov_cutters();
        // A strip standing in for the banner: the same shape, laid out first in
        // the same panel, and tall enough to reach where the twelve-o'clock row
        // goes.
        // 130 pt, which is what the real banner occupies inside the panel once
        // its path line and its two buttons are laid out. A shorter strip is not a
        // test: at 96 the twelve-o clock row lands at y = 99 on this pane and
        // clears it by three points, so the assertion passes with the defect
        // present. Measured, then chosen.
        let banner_h = 130.0;
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let out = ctx.run_ui(input.clone(), |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        ui.allocate_space(egui::vec2(ui.available_width(), banner_h));
                        map::show(
                            ui,
                            &mol,
                            "pKoV with His decR",
                            &cutters,
                            None,
                            None,
                            None,
                            None,
                            pl_enzymes::EnzymeSet::All,
                            None,
                        );
                    });
            });
            shapes = flat_shapes(&out.shapes);
        }
        let labels = texts_in(&shapes, 10.0, egui::FontFamily::Monospace);
        assert!(labels.len() >= 15, "only {} labels", labels.len());
        for (text, r) in &labels {
            assert!(
                r.top() >= banner_h - 0.5,
                "{text:?} is drawn at {r:?}, inside the {banner_h} pt strip above the map"
            );
        }
        for v in all_vertices(&shapes) {
            assert!(
                v.y >= banner_h - 0.5 || !v.is_finite(),
                "map ink at {v:?} is inside the {banner_h} pt strip above the map"
            );
        }
    }

    /// The backbone: the widest circle on the map.
    fn backbone(shapes: &[egui::Shape]) -> (egui::Pos2, f32) {
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Circle(c) => Some((c.center, c.radius)),
                _ => None,
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .expect("the backbone was drawn")
    }

    /// PROVEN TO FAIL at e087e27, on the "inside the pane" assertion.
    ///
    /// There, `room` works out to `132 - 63 = 69` pt whatever the window size —
    /// `lx` is pinned to `cx ± (tick_r + 22)` and `tick_r` grows with the same
    /// `w/2` the radius shrinks by, so the two cancel — which is 11 characters
    /// at this font's 6 pt advance. `HindIII  2,059` is 14 and ran 19 pt past the
    /// right edge; `BamHI  5,588` and eight others ran off the left. A cut
    /// coordinate is a *wrong* coordinate, and the label the reader is left with
    /// still looks like an enzyme name: `EcoRI  7,530` reads as `coRI  7,530`.
    ///
    /// The other three assertions pass at e087e27 and are here to stop the fix
    /// buying room by breaking something that already worked.
    #[test]
    fn every_enzyme_label_is_whole_inside_the_pane_and_points_at_its_own_tick() {
        let mol = pkov();
        let cutters = pkov_cutters();
        // Four pane sizes, widened from three when the font swap made this the
        // acceptance test for a new face.
        //
        // 706x756 is the map pane at the shipped default split on the user's own
        // window and is the size the original 23/23-clipped measurement was taken
        // at. 880x620 and 560x900 are the extremes of the splitter. 400x420 is
        // added: `MIN_PANEL` is 300, the radius has a 40 pt floor, and the
        // regimes in `ring::label_room` that are face-INDEPENDENT — the 30 % cap
        // and the row term — only bind on a small pane. A face change moves
        // `widest` and not the cap, so a small pane is where the two can come
        // apart, and none was tested.
        //
        // A FIFTH PANE, 340x900, WAS TRIED AND TAKEN BACK OUT, and the reason is
        // worth keeping because the obvious reading of it is wrong. It goes red
        // on assertion 1 with `"Hin..."`. That is not this swap: measured at
        // 0aa0f88 on the same molecule the same pane gives `"Hi..."` — the
        // ellipsis is `map::shortened_to`'s designed last resort at a width where
        // the reserve simply cannot hold a name, and IBM Plex Mono keeps one more
        // character there than Hack did. Nor can the rest of this test mean
        // anything at that width: all 22 labels come out as bare enzyme names, so
        // "points at its own tick" has no coordinate to point with. The pane is
        // outside the regime this test is about, so it is recorded here rather
        // than asserted about, and `a_shortened_label_never_shows_half_a_coordinate`
        // remains what covers the shortening itself.
        for (w, h) in [
            (706.0f32, 756.0f32),
            (880.0, 620.0),
            (560.0, 900.0),
            (400.0, 420.0),
        ] {
            let (shapes, pane) = paint_map(&mol, "pKoV with His decR", &cutters, w, h);
            let (centre, r) = backbone(&shapes);
            let all = texts_in(&shapes, 10.0, egui::FontFamily::Monospace);
            // A feature label is a feature NAME, whole or with an ellipsis; a
            // site label carries a coordinate. Since feature names joined this
            // ring, assertions 1 and 2 below cover them for free — which is the
            // point of extending this test rather than writing a second harness
            // — but 3 and 4 are about enzymes and must not be handed a name.
            let feature_names: Vec<&str> = mol.features.iter().map(|f| f.name.as_str()).collect();
            let is_feature = |t: &str| {
                let stem = t.strip_suffix("...").unwrap_or(t);
                feature_names
                    .iter()
                    .any(|n| *n == t || (t.ends_with("...") && n.starts_with(stem)))
            };
            let labels: Vec<(String, egui::Rect)> = all
                .iter()
                .filter(|(t, _)| !is_feature(t))
                .cloned()
                .collect();
            assert!(
                labels.len() >= 15,
                "{w}x{h}: only {} labels on a plasmid with 22 unique cutters",
                labels.len()
            );

            // 0. Every feature is named. `pl export` has written all nine of
            //    these into the SVG since the exporter was built, while the
            //    screen drew none of them.
            for f in &mol.features {
                assert!(
                    all.iter().any(|(t, _)| {
                        let stem = t.strip_suffix("...").unwrap_or(t);
                        *t == f.name || (t.ends_with("...") && f.name.starts_with(stem))
                    }),
                    "{w}x{h}: the feature {:?} is nowhere on the map",
                    f.name
                );
            }

            // 1. Whole, and inside the pane. This is the one that fails at
            //    e087e27.
            //
            //    "Whole" is asked of SITE labels only: a shortened cut
            //    coordinate is unrecoverable on the page, and a shortened
            //    feature name is one hover and one list row away, which is the
            //    asymmetry `FEATURE_NAME_CAP_CHARS` is built on. Staying inside
            //    the pane is asked of everything.
            for (text, rect) in &all {
                assert!(
                    is_feature(text) || !text.ends_with("..."),
                    "{w}x{h}: {text:?} did not fit the room reserved for it"
                );
                assert!(
                    rect.left() >= pane.left() - 0.5
                        && rect.right() <= pane.right() + 0.5
                        && rect.top() >= pane.top() - 0.5
                        && rect.bottom() <= pane.bottom() + 0.5,
                    "{w}x{h}: {text:?} is drawn at {rect:?}, outside the {pane:?} pane"
                );
            }

            // 2. No two overlap — feature names included, which is what says
            //    the new entries went through the same packer.
            for i in 0..all.len() {
                for j in i + 1..all.len() {
                    let hit = all[i].1.intersects(all[j].1);
                    assert!(
                        !hit,
                        "{w}x{h}: {:?} at {:?} overlaps {:?} at {:?}",
                        all[i].0, all[i].1, all[j].0, all[j].1
                    );
                }
            }

            // 3. Every enzyme that cuts once is still named somewhere. Merging
            //    two co-located sites into one tick must not lose either name,
            //    and dropping a label must not be how the map fits.
            //
            //    A folded label is `A  1,234 / B  1,236` — every name carrying
            //    its own coordinate — so the parse splits on " / " first. It used
            //    to be `A/B  1,234-1,236`, a range, which is the form this pass
            //    removed: five names against two numbers on pET28a's polylinker,
            //    with three of the five cut positions printed nowhere.
            let mut named: Vec<&str> = Vec::new();
            for (text, _) in &labels {
                for part in text.split(" / ") {
                    let head = part.split("  ").next().unwrap_or_default();
                    named.extend(head.split('/'));
                }
            }
            for (want, _) in pkov_cutter_names() {
                assert!(
                    named.contains(&want),
                    "{w}x{h}: {want} cuts once and is nowhere on the map: {named:?}"
                );
            }

            // 4. Every leader ends at its own tick and nobody else's.
            //
            // Only for labels that still carry a coordinate. On a pane small
            // enough that `ring::label_room`'s 30 % cap binds, shortening drops the
            // WHOLE coordinate rather than cutting it — which is correct, and is
            // what `a_shortened_label_never_shows_half_a_coordinate` exists to
            // require — leaving `"AflII"` with nothing to match a tick against.
            // Discovered by adding the 400x420 pane above: this block panicked
            // with `no coordinate in "AflII"`, which was this assertion meeting a
            // regime it was never written for, not a defect in the map.
            //
            // The skip is guarded so it cannot quietly swallow the cases that
            // matter. At the three panes wide enough to print coordinates it must
            // cover EVERY label; at 400x420 a label may drop its coordinate, but
            // only WHOLE — a label with no parseable coordinate must carry no
            // digit at all, because a digit left behind is a partial coordinate
            // and a partial coordinate is a wrong one. Without that second half
            // "no coordinate" would become a way for this assertion to stop
            // asserting.
            //
            // Do NOT restore a blanket `checked > 0` here. It held at 400x420 only
            // by a single label — `SpeI  562`, which fits in IBM Plex Mono's
            // 0.600 em and did not in Hack's 0.602051, so at 0aa0f88 the count was
            // zero — and an assertion that survives on one label's worth of
            // rounding is an assertion about the face, not about the map.
            let lines = hairlines(&shapes);
            let mut checked = 0usize;
            for (text, rect) in &labels {
                // The FIRST coordinate in the label, which is the tick's own
                // base: `Site::anchor` is `positions.first()`, and a folded label
                // lists its members in coordinate order.
                let Some(coord) = text
                    .split(" / ")
                    .next()
                    .and_then(|first| first.rsplit("  ").next())
                    .map(|c| c.replace(',', ""))
                    .and_then(|c| c.parse::<u64>().ok())
                else {
                    assert!(
                        !text.chars().any(|c| c.is_ascii_digit()),
                        "{w}x{h}: {text:?} has no coordinate this can read but does carry a \
                         digit, so the coordinate was cut rather than dropped -- which is a \
                         wrong coordinate on a plasmid map"
                    );
                    continue;
                };
                checked += 1;
                // The leader is the hairline ending nearest this label.
                let anchor = rect.center();
                let leader = lines
                    .iter()
                    .filter(|l| l.len() >= 2)
                    .min_by(|a, b| {
                        let d = |l: &Vec<egui::Pos2>| {
                            (*l.last().unwrap() - anchor)
                                .length()
                                .min((l[0] - anchor).length())
                        };
                        d(a).partial_cmp(&d(b)).unwrap()
                    })
                    .expect("some hairline was drawn");
                let far = if (leader[0] - anchor).length()
                    > (*leader.last().unwrap() - anchor).length()
                {
                    leader[0]
                } else {
                    *leader.last().unwrap()
                };
                // Where the tick for THIS coordinate is: on the ray at its own
                // angle, outside the backbone.
                let a = -std::f32::consts::FRAC_PI_2
                    + (coord.saturating_sub(1)) as f32 / 8_117.0 * std::f32::consts::TAU;
                let v = far - centre;
                assert!(
                    v.length() > r,
                    "{w}x{h}: {text:?}'s leader starts inside the ring"
                );
                let off = (v.y * a.cos() - v.x * a.sin()).abs() / v.length().max(1.0);
                assert!(
                    off < 0.02,
                    "{w}x{h}: {text:?}'s leader starts at {far:?}, which is not on the ray \
                     to base {coord} from {centre:?}"
                );
            }
            // The wide panes must print a coordinate on EVERY label. If a label
            // ever loses one here, the `continue` above turns from a narrow-pane
            // allowance into a hole, and this is what notices.
            if w >= 560.0 {
                assert_eq!(
                    checked,
                    labels.len(),
                    "{w}x{h}: {} of {} labels lost their coordinate on a pane wide \
                     enough to print one",
                    labels.len() - checked,
                    labels.len()
                );
            }
        }
    }

    /// PROVEN TO FAIL at e087e27: the "3,247" tick is painted over by SacB.
    ///
    /// The ruler and the reverse-strand lanes shared radii there — the number
    /// sat at `r - 16` in a 9 pt face, spanning `r-21.5 .. r-11.5`, and reverse
    /// lane 0 spans `r-13.5 .. r-4.5` — and the features are painted second, so
    /// the features always won. Exactly one of the five labelled ticks broke on
    /// this file, which is not luck: it is the one tick that happens to fall
    /// inside a reverse feature.
    #[test]
    fn a_ruler_number_is_clear_of_every_feature_band() {
        let mol = pkov();
        let (shapes, _) = paint_map(&mol, "pKoV with His decR", &pkov_cutters(), 706.0, 756.0);
        let numbers = texts_in(&shapes, 9.0, egui::FontFamily::Monospace);
        assert_eq!(
            numbers.len(),
            5,
            "every other tick of ten is labelled: {numbers:?}"
        );
        assert!(
            numbers.iter().any(|(t, _)| t == "3,247"),
            "the tick the user could not read is drawn at all: {numbers:?}"
        );

        let bands = feature_bands(&shapes);
        assert!(!bands.is_empty(), "the features were drawn");
        for (text, rect) in &numbers {
            for (points, half) in &bands {
                // The band is a thick polyline; test its centre line and the
                // midpoint of each step against the number's box grown by half
                // the band's width. `arc_points` samples about every 1.5
                // degrees, which is a few points across a box this size.
                let grown = rect.expand(*half);
                for w in points.windows(2) {
                    let mid = egui::pos2((w[0].x + w[1].x) * 0.5, (w[0].y + w[1].y) * 0.5);
                    for p in [w[0], mid, w[1]] {
                        assert!(
                            !grown.contains(p),
                            "the ruler number {text:?} at {rect:?} is painted over by a \
                             feature band passing through {p:?}"
                        );
                    }
                }
            }
        }
    }

    /// The four other plasmids the clipping sweep is run against.
    ///
    /// **They are not in the repo and never were** — the 0ebaa41 measurement
    /// that found 23/23 labels clipped on pET28a was taken with a standalone
    /// program against files on the author's machine. So these are built from
    /// `pkov()`'s backbone with each plasmid's REAL feature names and lengths
    /// substituted, which is what the layout is actually a function of: the
    /// packer never sees a base, only a name, an angle and a width. Written
    /// down rather than glossed, because "the five-plasmid check" reads as a
    /// claim about five files.
    ///
    /// pET28a's 137-character MCS `/label` is the case that matters. It is the
    /// name `SITE_WEIGHT` was introduced for and the one that drives
    /// `FEATURE_NAME_CAP_CHARS`.
    fn plasmid(name: &str) -> pl_core::Molecule {
        let mut mol = pkov();
        let names: Vec<&str> = match name {
            // The same molecule as `pkov()` read through GenBank: the nine
            // SnapGene primers arrive as `primer_bind` features, and the
            // longest of those names is 16 characters.
            "pkov.gb" => vec![
                "cat promoter",
                "CmR",
                "sacB promoter",
                "SacB",
                "f1 ori",
                "pSC101 ori",
                "Rep101(Ts)",
                "decR",
                "decR his",
                "F_his colony PCR",
                "R_his colony PCR",
                "F1ori-F",
                "F1ori-R",
                "CAT-R",
                "decR-F",
                "decR-R",
                "sacB-F",
                "sacB-R",
            ],
            "pET28a" => vec![
                "f1 ori",
                "KanR",
                "ori",
                "lacI",
                "T7 promoter",
                "lac operator",
                "RBS",
                "6xHis",
                "T7 tag",
                "thrombin site",
                "T7 terminator",
                "rop",
                "Multiple Cloning Site (MCS); contains unique sites for NcoI, NdeI, NheI, BamHI, \
                 EcoRI, SacI, SalI, HindIII, NotI, EagI and XhoI",
            ],
            "pACYC184" => vec!["CmR", "TcR", "p15A ori", "CmR promoter"],
            "pUC19" => vec![
                "CAP binding site",
                "lac promoter",
                "lac operator",
                "lacZ-alpha",
                "MCS",
                "AmpR",
                "AmpR promoter",
                "ori",
                "M13 rev",
                "M13 fwd",
            ],
            other => panic!("no fixture for {other}"),
        };
        mol.features.clear();
        let span = mol.seq.len() as u64;
        for (i, n) in names.iter().enumerate() {
            let mut f = pl_core::Feature::new(*n, "misc_feature");
            // Spread them right round the ring, so every one of the four runs
            // and both side columns are exercised.
            let start = 1 + (span - 400) * i as u64 / names.len().max(1) as u64;
            f.segments = vec![pl_core::Segment::new(start, start + 300)];
            f.strand = if i % 2 == 0 {
                Strand::Forward
            } else {
                Strand::Reverse
            };
            mol.features.push(f);
        }
        mol
    }

    /// PROVEN TO FAIL at 528dcd9 on assertion 0: the map's label list was built
    /// from cut sites alone, so on every one of these the count of feature names
    /// on screen was ZERO while `pl export` wrote all of them into the SVG.
    ///
    /// This is the sweep 0ebaa41 is remembered for, re-run because adding
    /// feature names changes what "the widest label" means and the reserve is
    /// computed from it. 0ebaa41 found 23/23 labels clipped on pET28a before its
    /// fix; nothing here may put one back.
    #[test]
    fn no_label_is_clipped_on_any_of_the_five_plasmids() {
        let cutters = pkov_cutters();
        for file in ["pKoV .dna", "pkov.gb", "pET28a", "pACYC184", "pUC19"] {
            let mol = if file == "pKoV .dna" {
                pkov()
            } else {
                plasmid(file)
            };
            for (w, h) in [(706.0f32, 756.0f32), (880.0, 620.0), (560.0, 900.0)] {
                let (shapes, pane) = paint_map(&mol, file, &cutters, w, h);
                let all = texts_in(&shapes, 10.0, egui::FontFamily::Monospace);
                let names: Vec<&str> = mol.features.iter().map(|f| f.name.as_str()).collect();
                let is_feature = |t: &str| {
                    let stem = t.strip_suffix("...").unwrap_or(t);
                    names
                        .iter()
                        .any(|n| *n == t || (t.ends_with("...") && n.starts_with(stem)))
                };
                // 0. Some feature name reached the ring at all.
                assert!(
                    all.iter().any(|(t, _)| is_feature(t)),
                    "{file} {w}x{h}: not one feature name on the map"
                );
                for (text, rect) in &all {
                    // 1. Inside the pane, whole glyphs, nothing over the edge.
                    assert!(
                        rect.left() >= pane.left() - 0.5
                            && rect.right() <= pane.right() + 0.5
                            && rect.top() >= pane.top() - 0.5
                            && rect.bottom() <= pane.bottom() + 0.5,
                        "{file} {w}x{h}: {text:?} at {rect:?} is outside the {pane:?} pane"
                    );
                    // 2. A CUT COORDINATE is never shortened. This is the
                    //    0ebaa41 defect itself.
                    assert!(
                        is_feature(text) || !text.ends_with("..."),
                        "{file} {w}x{h}: the site label {text:?} was clipped"
                    );
                }
                // 3. And nothing overlaps anything.
                for i in 0..all.len() {
                    for j in i + 1..all.len() {
                        assert!(
                            !all[i].1.intersects(all[j].1),
                            "{file} {w}x{h}: {:?} overlaps {:?}",
                            all[i].0,
                            all[j].0
                        );
                    }
                }
            }
        }
    }

    /// PROVEN TO FAIL against the naive implementation — feature names charged
    /// to the reserve at full width, which is what `pl_draw` does — and that is
    /// the only kind of test worth writing here.
    ///
    /// Measured with `target/release/pl.exe` at HEAD: pKoV exports at r = 224.6,
    /// and the same molecule with a 30-character name on `Rep101(Ts)` (mid base
    /// 1108 = 49 degrees, the RIGHT column) exports at r = 135 — 40% of the ring
    /// gone. The same name on `pSC101 ori` (21 degrees, the TOP row) changes
    /// nothing, so the rule is angle-dependent and therefore ROTATION-dependent:
    /// "Set origin at selected feature" would resize the user's map by 40% as a
    /// side effect of renumbering, which nothing the user did asked for.
    ///
    /// Three things degrade silently when r falls, which is why this is pinned
    /// rather than left to judgement: `bases_per_arc` moves, so the map's claim
    /// about which cuts share a tick changes; `centre_room` shrinks, so the
    /// disclosure drops from `long()` to `short()` and stops naming what was
    /// hidden; and `inside_of`'s `lanes_kept` falls, so inward feature bands
    /// overprint each other — a feature name silently hiding feature bands being
    /// the worst possible outcome of a change whose purpose is to name features.
    #[test]
    fn a_long_feature_name_does_not_shrink_the_ring() {
        let cutters = pkov_cutters();
        let base = pkov();
        let mut long = pkov();
        // 33 characters, and a real pET/pGEM feature name.
        long.features[6].name = "SP6 transcription initiation site".into();
        assert_eq!(long.features[6].name.len(), 33);
        let mut huge = pkov();
        huge.features[6].name = "M".repeat(137);
        for (w, h) in [
            (706.0f32, 756.0f32),
            (880.0, 620.0),
            (560.0, 900.0),
            (400.0, 420.0),
        ] {
            let r_of = |m: &pl_core::Molecule| backbone(&paint_map(m, "pKoV", &cutters, w, h).0).1;
            let (ra, rb, rc) = (r_of(&base), r_of(&long), r_of(&huge));
            // PAST THE CAP, MORE CHARACTERS ARE FREE. This is the assertion the
            // naive implementation fails: charged at full width, 137 characters
            // takes the ring to the 30% floor while 33 does not, so `rb != rc`.
            assert!(
                (rb - rc).abs() < 0.5,
                "{w}x{h}: 137 characters cost more than 33 ({rb} against {rc}) — the cap is                  not binding"
            );
            // AND THE CAP IS SMALL. The whole feature contribution is bounded by
            // `FEATURE_NAME_CAP_CHARS` plus the swatch, so a 33-character name
            // can never do what it does to the exporter: measured with
            // `target/release/pl.exe`, the same substitution on `Rep101(Ts)`
            // takes pKoV's exported ring from 224.6 to 135, which is 40% of it.
            assert!(
                ra - rb < 14.0 && rb > ra * 0.9,
                "{w}x{h}: a 33-character feature name took the ring from {ra} to {rb}"
            );
        }
    }

    /// PROVEN TO FAIL at 528dcd9: `map.rs` had zero references to a selection,
    /// so the commonest gesture a cloner makes showed nowhere on the picture
    /// they make decisions from.
    ///
    /// The origin-crossing case is the one worth asserting. Interpolating from
    /// `angle_of(start)` to `angle_of(end)` when `start > end` paints the
    /// COMPLEMENT — the defect `Band::segs` records, a 2,499 bp band drawn for a
    /// 187 bp feature — so 7,900..200 must cover 418 bases of arc and not 7,699.
    #[test]
    fn a_sequence_selection_is_drawn_on_the_map_including_across_the_origin() {
        let mol = pkov();
        let cutters = pkov_cutters();
        let plain = paint_map(&mol, "pKoV", &cutters, 706.0, 756.0).0;
        let n_plain = plain.len();

        // A small ordinary selection: something more is drawn.
        let (some, _) = paint_map_sel(
            &mol,
            "pKoV",
            &cutters,
            706.0,
            756.0,
            Some(pl_core::Segment::new(1_000, 2_000)),
        );
        assert!(some.len() > n_plain, "nothing was drawn for the selection");

        // 7,900..200 on an 8,117 bp circle is 418 bases: 8,117 - 7,900 + 1 + 200.
        let (cross, _) = paint_map_sel(
            &mol,
            "pKoV",
            &cutters,
            706.0,
            756.0,
            Some(pl_core::Segment::new(7_900, 200)),
        );
        let (centre, r) = backbone(&cross);
        // The arc is the widest stroke sitting on the backbone radius.
        let arc_len = |shapes: &[egui::Shape]| -> f32 {
            shapes
                .iter()
                .filter_map(|sh| match sh {
                    egui::Shape::Path(p) if (p.stroke.width - 3.0).abs() < 0.01 => Some(
                        p.points
                            .windows(2)
                            .map(|w| (w[1] - w[0]).length())
                            .sum::<f32>(),
                    ),
                    _ => None,
                })
                .sum()
        };
        let drawn = arc_len(&cross);
        let want = 418.0 / 8_117.0 * std::f32::consts::TAU * r;
        assert!(
            (drawn - want).abs() < want * 0.15,
            "an origin-crossing selection of 418 bases drew {drawn:.1} pt of arc where {want:.1} \
             was wanted (the complement would be {:.1})",
            (8_117.0 - 418.0) / 8_117.0 * std::f32::consts::TAU * r
        );
        assert!(centre.x > 0.0, "the backbone was found");
    }

    /// PROVEN TO FAIL at 528dcd9: the "Nothing open." guard sat above the tab
    /// dispatch and swallowed `Tab::Library` with the other five, while the tab
    /// strip is drawn above the guard — so the user could select the one surface
    /// that needs no molecule and be told there was nothing open.
    ///
    /// The Library is the only cross-file search in the app and there is no
    /// in-document search at all, so it is the only sequence search that exists.
    #[test]
    fn the_library_tab_opens_with_no_document() {
        let ctx = test_ctx();
        let mut app = App::blank();
        app.tab = Tab::Library;
        assert!(app.document().is_none(), "the premise");
        let mut said = Vec::new();
        for _ in 0..2 {
            let out = ctx.run_ui(window(), |ui| {
                app.side_panel(ui);
            });
            said = flat_shapes(&out.shapes)
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect();
        }
        assert!(
            !said.iter().any(|t| t.contains("Nothing open")),
            "the Library tab must not be gated on a document: {said:?}"
        );
        assert!(
            said.iter().any(|t| t.contains("folder")),
            "and it must show its own empty state: {said:?}"
        );
    }

    /// A label too wide for its column loses its coordinate whole.
    ///
    /// COMPILE-AND-RUN at e087e27 and it PASSES there, for the wrong reason:
    /// nothing was ever shortened, it was silently cropped by the clip rect
    /// instead, so the galley always held the full text. This is a guard on the
    /// shortening introduced here — `EcoRI  7,5...` reads as a cut position and
    /// is not one, which is the same class of wrongness as the clipping it
    /// replaces.
    #[test]
    fn a_shortened_label_never_shows_half_a_coordinate() {
        let mol = pkov();
        let cutters = pkov_cutters();
        let full: Vec<String> = cutters
            .iter()
            .map(|d| format!("{}  {}", d.enzyme.name, doc::fmt_int(d.positions[0])))
            .collect();
        let mut ever_shortened = false;
        for (w, h) in [(350.0f32, 480.0f32), (420.0, 520.0), (500.0, 600.0)] {
            let (shapes, _) = paint_map(&mol, "pKoV with His decR", &cutters, w, h);
            for (text, _) in texts_in(&shapes, 10.0, egui::FontFamily::Monospace) {
                if full.contains(&text) {
                    continue;
                }
                ever_shortened = true;
                let body = text.strip_suffix("...").unwrap_or(&text);
                // Whatever is drawn must be a prefix of some label's NAME —
                // never of the coordinate, and never a merged group cut in half.
                //
                // A FEATURE name is also a name, and a shortened one is fine
                // here for the reason `FEATURE_NAME_CAP_CHARS` gives: the whole
                // string is one hover and one list row away, while a shortened
                // coordinate is unrecoverable on the page. `c...` in this
                // assertion's own error message was `cat promoter`, not half of
                // a number.
                let names = full
                    .iter()
                    .map(|f| f.rsplit_once("  ").map_or(f.as_str(), |(n, _)| n))
                    .chain(mol.features.iter().map(|f| f.name.as_str()));
                let ok = names.into_iter().any(|n| n.starts_with(body)) && !body.is_empty();
                assert!(
                    ok,
                    "{w}x{h}: {text:?} is not a name, it is part of a number"
                );
            }
        }
        assert!(
            ever_shortened,
            "a check that cannot fail proves nothing: no pane here was narrow enough"
        );
    }

    /// A guard on a defect this phase INTRODUCED, and it passes at e087e27.
    ///
    /// Giving the ruler a band of its own means flooring it clear of whatever is
    /// written in the middle, and at the app's own 880 x 560 minimum the map pane
    /// is about 350 pt wide — narrow enough that the line saying what the map is
    /// not showing was wider than the ring. The floor then pushed "6,494" and
    /// "3,247" *outside* the backbone and in among the enzyme names, which is
    /// worse than the collision it was avoiding. Dodging a centre line the ring
    /// cannot hold is not possible, so the line is shortened or dropped and the
    /// floor is bounded.
    #[test]
    fn nothing_written_in_the_middle_leaves_the_ring() {
        let mol = pkov();
        let cutters = pkov_cutters();
        for (w, h) in [(350.0f32, 480.0f32), (300.0, 300.0), (706.0, 756.0)] {
            let (shapes, _) = paint_map(&mol, "pKoV with His decR", &cutters, w, h);
            let (centre, r) = backbone(&shapes);
            for (text, rect) in texts_in(&shapes, 9.0, egui::FontFamily::Monospace) {
                let far = [
                    rect.left_top(),
                    rect.right_top(),
                    rect.left_bottom(),
                    rect.right_bottom(),
                ]
                .iter()
                .map(|c| (*c - centre).length())
                .fold(0.0f32, f32::max);
                assert!(
                    far <= r,
                    "{w}x{h}: the ruler number {text:?} reaches {far:.1} pt, outside the                      {r:.1} pt ring"
                );
            }
            // And nothing written in the middle is crossed by a ruler number,
            // which is what the ruler's own band exists for.
            let mut middle: Vec<(String, egui::Rect)> =
                texts_in(&shapes, 10.0, egui::FontFamily::Proportional);
            middle.extend(texts_in(&shapes, 15.0, egui::FontFamily::Proportional));
            middle.extend(texts_in(&shapes, 11.0, egui::FontFamily::Monospace));
            // The NOTE is the line this code chooses, so it is the one held to
            // the ring's width. The caption is the plasmid's name at a fixed 15
            // pt and can overhang a token ring: at a 300 pt pane it crosses the
            // 1.5 pt backbone hairline by about 7 pt at each end, which is an
            // overhang and not a legibility failure — the note running across
            // the coloured feature bands was.
            for (text, rect) in texts_in(&shapes, 10.0, egui::FontFamily::Proportional) {
                assert!(
                    rect.width() <= 2.0 * r,
                    "{w}x{h}: the centre note {text:?} is {:.1} pt wide across a {:.1} pt ring",
                    rect.width(),
                    2.0 * r
                );
            }
            for (num, nr) in texts_in(&shapes, 9.0, egui::FontFamily::Monospace) {
                for (text, rect) in &middle {
                    assert!(
                        !nr.intersects(*rect),
                        "{w}x{h}: the ruler number {num:?} at {nr:?} crosses {text:?} at {rect:?}"
                    );
                }
            }
        }
    }

    /// PROVEN TO FAIL at e087e27: the caption reads "pKoV with His decR.dna".
    ///
    /// The fallback to the filename is right and stays — the `.dna` container
    /// carries no molecule name at all, so there is nothing else to print. What
    /// was wrong is printing the container's extension as part of the plasmid's
    /// name. The second half of this test is the control, and it PASSES at
    /// e087e27: a real molecule name beats the filename, and the fix must not
    /// disturb that.
    #[test]
    fn the_caption_drops_the_extension_and_still_prefers_a_real_name() {
        let mol = pkov();

        // A .dna, which carries no name field of any kind.
        let dna = pl_fileio::snapgene::from_molecule(&mol);
        let d = Document::from_bytes(&dna, "pKoV with His decR.dna".into(), None)
            .expect("the synthesised .dna reads back");
        assert!(
            d.molecule().name.is_empty(),
            "the premise: SnapGene has no name field"
        );
        assert_eq!(
            d.title, "pKoV with His decR.dna",
            "and the document keeps the whole filename, for the toolbar and the hover"
        );
        assert_eq!(caption_of_map(d), "pKoV with His decR");

        // A GenBank file, which does. Its LOCUS name must win over the filename.
        let gb = pl_fileio::genbank::write(&mol, "ignored.gb", today());
        let d = Document::from_bytes(gb.as_bytes(), "some other name.gb".into(), None)
            .expect("the GenBank reads back");
        let locus = d.molecule().name.clone();
        assert!(!locus.is_empty(), "the premise: GenBank names its molecule");
        assert_eq!(
            caption_of_map(d),
            locus,
            "a LOCUS name is a real name and a filename is a guess"
        );
    }

    /// What the map actually paints in the middle of the ring, for one document.
    ///
    /// Read off a painted frame rather than by calling the expression in
    /// `central`, because the defect was at the call site: `map.rs`'s own comment
    /// about the fallback was correct and `d.title` was the wrong thing to hand
    /// it.
    fn caption_of_map(d: Document) -> String {
        let ctx = test_ctx();
        let mut app = App::blank();
        app.adopt(d);
        let mut shown = String::new();
        for _ in 0..2 {
            let out = ctx.run_ui(window(), |ui| app.central(ui));
            let texts = texts_in(
                &flat_shapes(&out.shapes),
                15.0,
                egui::FontFamily::Proportional,
            );
            shown = texts.first().map(|(t, _)| t.clone()).unwrap_or_default();
        }
        shown
    }

    /// The layout is shared, so the ordering must not depend on the font.
    ///
    /// COMPILE-ONLY at e087e27: `pl_draw::ring` does not exist there, and neither
    /// does any shared placement to test — that is the divergence this phase
    /// closed. What it asserts is the property that makes one layout able to
    /// serve two painters: the GUI measures its labels with egui's monospace
    /// galleys and the exporters measure theirs with Helvetica's advances, so the
    /// two disagree about every width, and the run a label lands in and its order
    /// within that run must still come out identical.
    #[test]
    fn the_screen_and_the_figure_order_the_ring_identically() {
        let sites = pkov_cutter_names();
        let texts: Vec<String> = sites
            .iter()
            .map(|&(name, pos)| format!("{name}  {pos}"))
            .collect();

        // The screen's metric, taken from inside a frame because that is the
        // only place egui will lay out a galley.
        let ctx = test_ctx();
        let mut screen_w: Vec<f64> = Vec::new();
        let _ = ctx.run_ui(window(), |ui| {
            screen_w = texts
                .iter()
                .map(|t| {
                    ui.painter()
                        .layout_no_wrap(t.clone(), egui::FontId::monospace(10.0), pal(ui).ink2)
                        .size()
                        .x as f64
                })
                .collect();
        });
        assert_eq!(screen_w.len(), texts.len());
        let order_with = |widths: &[f64]| -> Vec<(pl_draw::ring::Side, usize)> {
            let labels: Vec<pl_draw::ring::RingLabel> = sites
                .iter()
                .zip(widths)
                .map(|(&(_, pos), &width)| pl_draw::ring::RingLabel {
                    angle: (pos.saturating_sub(1)) as f64 / 8_117.0 * std::f64::consts::TAU,
                    width,
                    height: 13.0,
                    weight: 1.0,
                })
                .collect();
            let g = pl_draw::ring::RingGeom {
                cx: 353.0,
                cy: 378.0,
                tick_r: 242.0,
                gap: 26.0,
                row_half: 30f64.to_radians(),
                row_gap: 10.0,
                left: 6.0,
                right: 700.0,
                top: 19.0,
                bottom: 750.0,
            };
            let ring = pl_draw::ring::place_ring(&labels, &g);
            let mut out: Vec<(pl_draw::ring::Side, usize, f64)> = ring
                .placed
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    p.map(|p| {
                        let key = match p.side {
                            pl_draw::ring::Side::Left | pl_draw::ring::Side::Right => p.at.1,
                            _ => p.at.0,
                        };
                        (p.side, i, key)
                    })
                })
                .collect();
            out.sort_by(|a, b| {
                format!("{:?}", a.0)
                    .cmp(&format!("{:?}", b.0))
                    .then(a.2.partial_cmp(&b.2).unwrap())
            });
            out.into_iter().map(|(s, i, _)| (s, i)).collect()
        };

        let on_screen = order_with(&screen_w);
        // The figure's metric: Helvetica's real advances at the figure's own
        // type size, which is what the PDF and EPS back ends crop against.
        let figure_w: Vec<f64> = texts
            .iter()
            .map(|t| pl_draw::pdf::text_width_in(t, 12.0, false))
            .collect();
        assert!(
            screen_w
                .iter()
                .zip(&figure_w)
                .any(|(a, b)| (a - b).abs() > 1.0),
            "the premise: the two metrics disagree about the widths"
        );
        let in_figure = order_with(&figure_w);

        assert!(!on_screen.is_empty());
        assert_eq!(
            on_screen, in_figure,
            "the two painters put the same label in a different place"
        );
    }

    /// The app's figure and `pl export`'s figure are the SAME figure.
    ///
    /// Both now build a `ring::Disclosure` for themselves — the app from
    /// `Document::digest`, `bins/pl` from `pl_enzymes::digest_all` — and if those
    /// two disagree by one enzyme the two figures differ by a line of text, which
    /// is a figure you cannot cite. Checked here rather than by hashing one
    /// exported file, because a hash tells you that today's two agreed and this
    /// tells you why they must.
    ///
    /// `pl export`'s own filter is reproduced from the same table, so the
    /// comparison is between the two callers' arithmetic and not between two
    /// copies of one call.
    #[test]
    fn the_app_and_pl_export_ask_the_same_question_of_the_same_molecule() {
        let mut app = App::blank();
        // A real file, not the synthetic fixture: the counts are the point.
        let gb = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/export-fixture/hostile-names.gb"
        ))
        .expect("the export fixture is in the tree");
        let d = Document::from_bytes(&gb, "hostile-names.gb".into(), None).unwrap();
        app.adopt(d);
        // `figure_options` reads `d.digest`, which a worker fills. Wait for the
        // real one rather than faking it: the whole question is whether what the
        // app has agrees with what the table says, so substituting the table here
        // would be asking the table twice.
        let doc = app.bench.get_mut().unwrap();
        for _ in 0..2_000 {
            if doc.digest.poll() && !doc.digest.is_running() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let d = app.document().unwrap();
        assert!(
            !d.digest.results().is_empty(),
            "the digest worker did not finish"
        );

        let opts = App::figure_options(d, pl_enzymes::EnzymeSet::All);
        let told = opts.note.expect("the figure carries the disclosure");

        // What `bins/pl` computes, from the shipped table rather than from the
        // document's cached digest.
        let mut cli = pl_draw::ring::Disclosure::default();
        for r in pl_enzymes::digest_all(d.molecule()) {
            let n = r.count();
            if n == 0 {
                continue;
            }
            cli.cutters += 1;
            if n == 2 {
                cli.dual += 1;
            } else if n > 2 {
                cli.multi += 1;
            }
        }
        assert_eq!(told.cutters, cli.cutters, "a different number of cutters");
        assert_eq!(told.dual, cli.dual);
        assert_eq!(told.multi, cli.multi);
        assert!(told.closes(), "{told:?} does not account for every cutter");
        // And the counts are ENZYMES.
        //
        // What stood here was a TAUTOLOGY carrying exactly that claim:
        // `told.labelled >= opts.sites.len().saturating_sub(told.hidden)`, where
        // `figure_options` sets `told.hidden` from `sites_dropped` and
        // `sites_dropped` was `opts.sites.len() - sites_named` — so the right-hand
        // side WAS `told.labelled` and the assertion reduced to `x >= x`. It could
        // not fail for any molecule, any renderer or any bug, and a check that
        // cannot fail carrying a comment about the property it does not test is
        // worse than no check: it is why nobody looked again while the counting
        // defect it named was live.
        //
        // Restated as a conservation law over the enzymes asked for, which CAN
        // fail — but not here, and the reason is worth stating exactly, because
        // the obvious one is wrong. It is not that `hostile-names.gb` happens to
        // hold three unique cutters. It is that `figure_options` builds `sites`
        // from `filter(is_unique_cutter)` and `positions[0]`, so it can only ever
        // hand `scene` ONE pair per enzyme: a mention, a label and an enzyme are
        // the same integer for every molecule this call site will ever see, and
        // swapping the fixture would not change that. Measured: this assertion
        // passes at 0ebaa41, where `sites_named` counted mentions.
        //
        // So it is a contract, not a detector. It says what `figure_options` owes
        // its reader and it will fail the day the filter widens — which is
        // precisely the change `pl export --sites dual` already represents one
        // layer over, and got wrong. The detector for the counting defect itself
        // is `pl_draw`'s
        // `the_disclosure_closes_on_every_sites_filter_not_only_the_one_with_no_folds`
        // and `bins/pl`'s `the_disclosure_closes_on_a_multi_cutter_in_every_sites_mode`,
        // both of which fail at 0ebaa41.
        let enzymes: std::collections::BTreeSet<&str> =
            opts.sites.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            told.labelled + told.hidden,
            enzymes.len(),
            "{told:?} against {} distinct enzymes asked for in {} sites",
            enzymes.len(),
            opts.sites.len()
        );
    }
}
