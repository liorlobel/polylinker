//! Polylinker — a plasmid viewer that runs offline and asks nothing of anyone.
//!
//! Everything it decides about a molecule it asks `pl-core`, `pl-fileio` and
//! `pl-enzymes`, the same crates behind the `pl` command line and the browser
//! build. This binary is presentation.

// A console window alongside the app on Windows is noise for a GUI, but keep it
// in debug builds so panics and eprintln stay visible while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod annot;
mod design;
mod doc;
mod library;
mod map;
mod recover;
mod seqedit;
mod settings;
mod theme;

use std::path::PathBuf;

use eframe::egui::{self, Align, Layout, RichText, Sense, Ui};
use pl_core::Strand;

use doc::{describe, fmt_int, DigestState, Document};
use theme::Palette;

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

/// The path a user has to walk to set the origin, as prose points at it.
pub fn set_origin_path() -> String {
    format!("{MOLECULE_MENU} > {SET_ORIGIN_ITEM}")
}

/// Theme-resolved colours for whatever `ui` is currently drawing into.
fn pal(ui: &Ui) -> Palette {
    Palette::of(ui.visuals().dark_mode)
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

#[derive(PartialEq, Eq, Clone, Copy)]
enum Tab {
    Features,
    Library,
    Enzymes,
    Sequence,
    History,
    File,
}

struct App {
    document: Option<Document>,
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
    /// Feature under the pointer, from either the map or the list.
    hot: Option<usize>,
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

    /// Documents left by a session that did not close cleanly.
    ///
    /// Listed, with age and edit count, and never restored automatically.
    /// Silently reopening a draft over whatever the user meant to open is the
    /// failure mode; choosing between two drafts is something they can do and
    /// this program cannot.
    fn recovery_banner(&mut self, ui: &mut Ui) {
        if self.stale.is_empty() {
            return;
        }
        let mut restore: Option<usize> = None;
        let mut discard: Option<usize> = None;
        egui::Frame::NONE
            .fill(pal(ui).selection())
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("A previous session did not close cleanly")
                        .color(pal(ui).ink)
                        .strong(),
                );
                for (i, (path, snap)) in self.stale.iter().enumerate() {
                    ui.horizontal(|ui| {
                        match snap {
                            Ok(s) => {
                                let from = s
                                    .original
                                    .as_ref()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_else(|| "never saved to a file".into());
                                ui.label(
                                    RichText::new(format!(
                                        "{} — {} edit(s), from {from}",
                                        if s.title.is_empty() {
                                            "untitled"
                                        } else {
                                            &s.title
                                        },
                                        s.ops
                                    ))
                                    .color(pal(ui).ink2),
                                );
                            }
                            // Damaged, and still offered: the body of the file
                            // is plain GenBank, so the sequence is very likely
                            // recoverable even when the header is not.
                            Err(e) => {
                                ui.label(
                                    RichText::new(format!(
                                        "{} — damaged ({e}), the sequence may still be readable",
                                        path.display()
                                    ))
                                    .color(pal(ui).warn),
                                );
                            }
                        }
                        if ui.button("Open").clicked() {
                            restore = Some(i);
                        }
                        if ui.button("Discard").clicked() {
                            discard = Some(i);
                        }
                    });
                }
                ui.label(
                    RichText::new("These are copies. Your original files were not modified.")
                        .color(pal(ui).muted)
                        .size(11.0),
                );
            });

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
                Ok(d) => {
                    self.adopt(d);
                    self.status = format!("recovered from {}", path.display());
                    // The path is deliberately dropped: a recovered document is
                    // unsaved, so Save has to ask where, and cannot overwrite
                    // the original with a draft the user has not looked at.
                    let _ = self.stale.remove(i);
                }
                Err(e) => self.error = Some(format!("{}: {e}", path.display())),
            }
        } else if let Some(i) = discard {
            let (path, _) = self.stale.remove(i);
            recover::clear(&path);
            self.status = format!("discarded {}", path.display());
        }
    }

    /// Write the current document to the recovery file, if it is time.
    ///
    /// Never writes to the file the user opened. An editor that quietly
    /// rewrites the original every few minutes has turned "close without
    /// saving" into a lie.
    fn autosave(&mut self) {
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
            if now.duration_since(last) < Self::AUTOSAVE_EVERY {
                return;
            }
        }
        if self.document.is_none() || self.recovery.is_none() {
            return;
        }
        // Design B's rule 6, and the one whose absence loses data: an autosave
        // that wrote `log.current()` while a typing run was open would write a
        // recovery file missing the user's last forty keystrokes.
        self.settle();
        let Some(doc) = &self.document else { return };
        let here = Autosaved {
            original: doc.path.clone(),
            title: doc.title.clone(),
            cursor: doc.log.cursor(),
        };
        // Already on disk, byte for byte. See [`Autosaved`] for why this is a
        // cursor and not the op count it used to be.
        if self.autosaved.as_ref() == Some(&here) {
            return;
        }
        // An unedited document has nothing to protect: the user's own file
        // already holds it. Writing one anyway would also let merely *opening*
        // a second file discard the first one's unsaved draft, which is the
        // opposite of this function's job.
        //
        // Undoing back to the base of the document already in the recovery file
        // is a different case: that really is the state on screen, so it is
        // written, and the file stops offering a branch the user has stepped
        // off.
        let same_document = self
            .autosaved
            .as_ref()
            .is_some_and(|a| a.same_document(&here));
        if here.cursor.is_none() && !same_document {
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
            document: None,
            error: None,
            notice: None,
            edit: seqedit::SeqEdit::new(),
            tab: Tab::Features,
            selected: None,
            hot: None,
            filter: String::new(),
            status: String::new(),
            enzyme_set: pl_enzymes::EnzymeSet::All,
            scan: None,
            lib_mode: library::Mode::Name,
            lib_query: String::new(),
            lib_absent: false,
            recovery: None,
            stale: Vec::new(),
            autosaved: None,
            last_autosave: None,
            dna_owner: None,
            design: None,
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
            enz_strip: false,
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
        // Opening a file named on the command line makes the app usable as a
        // file association and from a terminal.
        if let Some(arg) = std::env::args_os().nth(1) {
            app.load(PathBuf::from(arg));
        }
        app
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
        self.document = Some(d);
        // The annotation index's other half of its identity. Two documents can
        // sit at the same cursor — every one of them starts at `None` — so
        // without this the second file opened is drawn with the first file's
        // features, plausibly and silently.
        self.doc_generation = self.doc_generation.wrapping_add(1);
        // A different molecule is a different question about whether the strip
        // is needed, so the answer held across the previous file's re-digests
        // does not carry over.
        self.enz_strip = false;
        self.error = None;
        self.notice = None;
        self.edit = seqedit::SeqEdit::new();
        self.selected = None;
        self.hot = None;
        // The design panel belongs to the molecule it was opened on. It used to
        // survive a document swap holding the previous file's title, length,
        // topology, target and report while being redrawn against the new
        // molecule's bases — and "Add to document" then wrote file A's primer
        // coordinates, under file A's name, into file B. Nothing in the panel
        // says which file it came from once the title bar has changed, so it is
        // closed rather than relabelled.
        self.close_design("the design panel was closed: it was designed against the previous file");
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

    fn load(&mut self, path: PathBuf) {
        match Document::open(&path) {
            Ok(d) => {
                self.status = describe(d.molecule(), d.format);
                // Say so when the file held more than we are showing. A viewer
                // that stays silent is indistinguishable from a file with
                // fewer records in it — which is how 1,879 features went
                // missing from a 124-record file without anyone noticing.
                if d.records_in_file > 1 {
                    self.status = format!(
                        "{}  —  showing record 1 of {} in this file",
                        self.status, d.records_in_file
                    );
                }
                if !d.unrepresentable_locations.is_empty() {
                    self.status = format!(
                        "{}  —  {} location(s) skipped as unrepresentable",
                        self.status,
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
                    self.status = format!(
                        "{}  —  {} coordinate problem{} in this file: {}",
                        self.status,
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
                self.adopt(d);
            }
            Err(e) => {
                self.error = Some(e);
                self.document = None;
                self.status.clear();
                // Deliberate, and announced. `design_panel` took the panel out
                // of `self` before it checked for a document, so a failed load
                // dropped the panel, its constraints and its report on the
                // floor in the same frame with nothing said. Putting the panel
                // *back* would be worse — it would then reattach to whatever
                // opens next.
                self.close_design(
                    "the design panel was closed: the document it described is no longer open",
                );
            }
        }
    }

    fn pick_file(&mut self) {
        let picked = rfd::FileDialog::new()
            .add_filter(
                "Sequence files",
                &["dna", "gb", "gbk", "genbank", "fa", "fasta", "seq", "ape"],
            )
            .add_filter("SnapGene", &["dna"])
            .add_filter("GenBank", &["gb", "gbk", "genbank"])
            .add_filter("FASTA", &["fa", "fasta", "fna"])
            .pick_file();
        if let Some(p) = picked {
            self.load(p);
        }
    }

    fn export(&mut self, as_fasta: bool) {
        self.settle();
        let Some(d) = &self.document else { return };
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
            Ok(()) => self.wrote(&path, &note),
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
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
    fn figure_options(d: &doc::Document) -> pl_draw::Options {
        let mut opts = pl_draw::Options {
            title: Some(pl_fileio::caption_of(&d.title).to_string()),
            sites: d
                .digest
                .results()
                .iter()
                .filter(|x| x.is_unique_cutter())
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
        let results = d.digest.results();
        let cutting = |f: &dyn Fn(usize) -> bool| results.iter().filter(|x| f(x.count())).count();
        let mut told = pl_draw::ring::Disclosure {
            cutters: cutting(&|n| n > 0),
            dual: cutting(&|n| n == 2),
            multi: cutting(&|n| n > 2),
            // Zero because the filter above is `is_unique_cutter`, which never
            // turns a single cutter away. `pl`'s `Sites::of` needs the term
            // because `--sites none` turns away all of them; if this filter ever
            // widens, `closes()` below fails rather than misdescribing a class.
            single: 0,
            ..Default::default()
        };
        let (_, first) = pl_draw::scene(d.molecule(), opts.clone());
        told.labelled = first.sites_named;
        told.hidden = first.sites_dropped;
        told.shortened = first.sites_shortened;
        debug_assert!(told.closes(), "{told:?} does not account for every cutter");
        opts.note = (told.cutters > 0).then_some(told);
        opts
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
        let Some(d) = &self.document else { return };
        let stem = pl_fileio::genbank::locus_name(&d.title);
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.pdf"))
            .add_filter("PDF", &["pdf"])
            .save_file()
        else {
            return;
        };
        let (bytes, drawn, font) = pl_draw::circular_pdf(d.molecule(), Self::figure_options(d));

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
        let Some(d) = &self.document else { return };
        let stem = pl_fileio::genbank::locus_name(&d.title);
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.svg"))
            .add_filter("SVG", &["svg"])
            .save_file()
        else {
            return;
        };
        let (svg, drawn) = pl_draw::circular_svg(d.molecule(), Self::figure_options(d));

        match std::fs::write(&path, svg) {
            Ok(()) => self.wrote(&path, &Self::figure_note(&drawn).join("  —  ")),
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
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
        if let Some(p) = &self.recovery {
            recover::clear(p);
        }
        // Once, here, and not on drag-release or per frame — that would be a
        // synchronous file write inside a paint loop. If the app crashes the
        // layout is lost, and that is the right trade: a window layout is not
        // the user's data.
        settings::save(self.layout);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

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

        self.autosave();

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
                        self.adopt(d);
                        self.status = what;
                    }
                    Err(e) => self.error = Some(e),
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
        if keys.save && self.document.is_some() {
            self.export(false);
        }

        // The digest worker cannot wake the UI, so poll it and keep repainting
        // while it runs.
        let mut running = false;
        if let Some(d) = &mut self.document {
            if d.digest.poll() {
                ctx.request_repaint();
            }
            running = d.digest.is_running();
        }
        // The folder scan is the same shape and the same contract.
        if let Some(s) = &mut self.scan {
            if s.poll() {
                ctx.request_repaint();
            }
            running |= s.is_running();
        }
        if running {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
        }

        self.hot = None;
        self.top_bar(ui);
        self.side_panel(ui);
        self.central(ui);
        self.paste_dialog(&ctx);
        self.design_panel(&ctx);
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
        let asking = self.edit.pending_paste.is_some();
        let typing = ctx.memory(|m| m.focused()).is_some();
        let designing = self.design.is_some();
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
                // No disclosure caret either, and it was tried and photographed:
                // egui's `menu_button` paints a plain button, so a triangle has
                // to be a glyph, and the default faces have none. U+25BE came out
                // as an empty box on all three menus — the same trap
                // `strand_word` already documents for U+2190, which rendered as a
                // box in the proportional face. So a menu is told from a button
                // by what it says rather than by its shape, which is why all
                // three are nouns for what they write out and why "Edit" — a
                // verb, and the platform's name for a menu holding Undo and
                // Redo — could not stay.
                if ui.button("Open…").on_hover_text("Ctrl+O").clicked() {
                    self.pick_file();
                }
                let has = self.document.is_some();
                ui.add_enabled_ui(has, |ui| {
                    ui.menu_button("Save", |ui| {
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
                    .on_hover_text("Ctrl+S — save the molecule");
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
                    ui.menu_button("Export map", |ui| {
                        if ui
                            .button("SVG…")
                            .on_hover_text("Vector map, for a figure")
                            .clicked()
                        {
                            self.export_svg();
                            ui.close();
                        }
                        if ui
                            .button("PDF…")
                            .on_hover_text("The same map, for a manuscript")
                            .clicked()
                        {
                            self.export_pdf();
                            ui.close();
                        }
                    })
                    .response
                    .on_hover_text("the plasmid map as a picture");
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
                    if let Some(d) = &self.document {
                        if d.digest.is_running() {
                            ui.add(egui::Spinner::new().size(13.0));
                            ui.label(RichText::new("digesting").color(pal(ui).muted).size(12.0));
                        }
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        if let Some(d) = &self.document {
                            // A dot rather than the usual asterisk-in-the-title:
                            // the point is that edits exist and are undoable,
                            // not that a file is dirty — nothing here writes
                            // over the original.
                            let marker = if d.edited() { " •" } else { "" };
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
        let (can_undo, can_redo) = match &self.document {
            Some(d) => (d.log.can_undo(), d.log.can_redo()),
            None => (false, false),
        };

        ui.add_enabled_ui(can_undo, |ui| {
            if ui.button("Undo").on_hover_text("Ctrl+Z").clicked() {
                self.do_undo();
            }
        });
        ui.add_enabled_ui(can_redo, |ui| {
            // Ctrl+Shift+Z has been wired since the shortcut block was written
            // and was advertised nowhere.
            if ui
                .button("Redo")
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

        let has = self.document.is_some();
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
            ui.menu_button(MOLECULE_MENU, |ui| {
                let circular = self
                    .document
                    .as_ref()
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
                        if let (Some(d), Some(i)) = (&self.document, sel) {
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
                ui.add_enabled_ui(sel.is_some(), |ui| {
                    if ui.button("Remove selected feature").clicked() {
                        if let Some(i) = sel {
                            self.edit(pl_core::OpKind::RemoveFeature { index: i });
                            self.selected = None;
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
        let Some(d) = &mut self.document else { return };
        if self.edit.run().is_none() {
            return;
        }
        // Only a genuine commit failure is promoted to the strip above the map.
        // The Sequence tab's own transient line — "'Z' is not a nucleotide" —
        // belongs under the sequence and must survive the next click.
        let held = self.edit.notice.take();
        self.edit.commit(d);
        match self.edit.notice.clone() {
            Some(failed) => self.notice = Some(failed),
            None => self.edit.notice = held,
        }
    }

    /// Run an edit and report a refusal instead of dropping it.
    ///
    /// Returns whether it went in. Most callers issue one operation and the
    /// `notice` is the whole answer, but a gesture that is two operations has
    /// to know which of them landed: "Ctrl+Z twice to undo both" after only one
    /// took undoes the user's previous, unrelated edit as well.
    fn edit(&mut self, kind: pl_core::OpKind) -> bool {
        self.settle();
        let Some(d) = &mut self.document else {
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
                self.edit.sel = None;
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
            .document
            .as_ref()
            .and_then(|d| self.edit.undo_over_pair(d.log.cursor()));
        if let Some(d) = &mut self.document {
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
        if let Some(d) = &mut self.document {
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
                        (Tab::History, "History"),
                        (Tab::File, "File"),
                    ] {
                        if ui.selectable_label(self.tab == tab, label).clicked() {
                            self.tab = tab;
                        }
                    }
                });
                ui.separator();

                if self.document.is_none() {
                    ui.add_space(20.0);
                    ui.label(RichText::new("Nothing open.").color(pal(ui).muted));
                    return;
                }

                match self.tab {
                    Tab::Features => self.features_tab(ui),
                    Tab::Library => self.library_tab(ui),
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
        ui.horizontal(|ui| {
            ui.label("filter");
            ui.text_edit_singleline(&mut self.filter);
        });
        let needle = self.filter.to_lowercase();
        let selected = self.selected;

        let mut hot = None;
        let mut clicked = None;
        let doc = self.document.as_ref().expect("checked by caller");
        // Read once: `extent` needs the molecule the feature belongs to, and
        // borrowing it inside the row closure would fight the iterator.
        let span = doc.molecule().span();
        let circular = doc.molecule().topology.is_circular();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, f) in doc.molecule().features.iter().enumerate() {
                if !needle.is_empty()
                    && !f.name.to_lowercase().contains(&needle)
                    && !f.kind.to_lowercase().contains(&needle)
                {
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
                            ui.label(RichText::new(&f.name).strong().size(12.5));
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
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.add_space(17.0);
                            ui.label(RichText::new(&f.kind).color(pal(ui).muted).size(11.0));
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
    }

    fn enzymes_tab(&mut self, ui: &mut Ui) {
        let d = self.document.as_ref().expect("checked by caller");
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
                if ui.selectable_label(set == s, s.label()).clicked() {
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
                    enzyme_row(
                        ui,
                        e.enzyme.name,
                        e.enzyme.site,
                        &e.positions,
                        true,
                        verdict(*i),
                        poor_single_site_note(e.enzyme.name, e.count()),
                    );
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
                    enzyme_row(
                        ui,
                        e.enzyme.name,
                        e.enzyme.site,
                        &e.positions,
                        false,
                        verdict(*i),
                        poor_single_site_note(e.enzyme.name, e.count()),
                    );
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
        let Some(d) = &self.document else { return };
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
        let want = match &self.document {
            Some(d) => (self.doc_generation, d.log.cursor()),
            None => return,
        };
        if self.annot.version != want {
            let ix = {
                let d = self.document.as_ref().expect("checked above");
                annot::AnnotIndex::build(d.molecule(), want)
            };
            self.annot = ix;
            self.cuts_for = None;
        }

        let done = matches!(
            self.document.as_ref().expect("checked above").digest,
            DigestState::Done(_)
        );
        let key = (want, self.enzyme_set, done);
        if self.cuts_for == Some(key) {
            return;
        }
        let cuts = {
            let d = self.document.as_ref().expect("checked above");
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
        let gate = seqedit::Editability::of(
            self.document
                .as_ref()
                .expect("checked by caller")
                .molecule(),
        );

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

        let n = self
            .edit
            .effective_len(self.document.as_ref().unwrap().molecule());

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
        // reads as the knife edge it is not: the default has 13.5 pt of slack
        // over the threshold, not 12.
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
        let has_cuts = self.enz_strip;
        let enz_h = if has_cuts { ENZ_H } else { 0.0 };
        let lanes = self.annot.lanes;
        let row_h = enz_h + TICK_H + text_h + lanes as f32 * LANE_PITCH;

        // A pending run is not in the log, so the digest describes the
        // COMMITTED sequence. A typed base can create or destroy a site, so a
        // mark translated into effective coordinates is not merely displaced —
        // it can be a site that no longer exists, drawn confidently.
        let typing = self.edit.run().is_some();
        let show_cuts = !typing && self.annot.cut_count() > 0;

        self.sequence_header(ui, n, rows, has_cuts, typing);
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
                let d = self.document.as_ref().expect("checked by caller");
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

                    for r in range.clone() {
                        let r = r as u64;
                        let start = r * per_row;
                        let end = (start + per_row).min(n);
                        let y = rect.top() + (r - first) as f32 * row_h;
                        let y_tick = y + enz_h;
                        let y_text = y_tick + TICK_H;
                        let y_lane = y_text + text_h;

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
                        if resp.drag_started() || resp.clicked() {
                            click = Some((hit(pos), ui.input(|i| i.modifiers.shift)));
                        } else if resp.dragged() {
                            drag_to = Some(hit(pos));
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
        if let Some((to, shift)) = click {
            let d = self.document.as_mut().expect("checked by caller");
            self.edit.place(d, to, shift);
            if !shift {
                self.edit.dragging = true;
            }
        }
        if let Some(to) = drag_to {
            let d = self.document.as_mut().expect("checked by caller");
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
            let d = self.document.as_ref().expect("checked by caller");
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
        let can_design = self.design.is_none();
        let mut flip = false;
        let mut design = false;

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
                    let row_h = ui.spacing().interact_size.y.max(22.0);
                    let w = ui.available_width();
                    ui.allocate_ui_with_layout(
                        egui::vec2(w, row_h),
                        Layout::right_to_left(Align::Center),
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
                    ui.label(
                        RichText::new(mol_line)
                            .monospace()
                            .size(11.5)
                            .color(pal(ui).ink2),
                    );
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
    }

    fn sequence_header(&mut self, ui: &mut Ui, n: u64, rows: usize, has_cuts: bool, typing: bool) {
        let d = self.document.as_ref().expect("checked by caller");
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
        let d = self.document.as_ref()?;
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
        let d = self.document.as_mut()?;
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
        let Some(d) = &self.document else { return };
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
            let d = self.document.as_mut().expect("checked at the top");
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
        if self.edit.pending_paste.is_some() {
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
        let events = ui.input(|i| i.events.clone());
        // The same number the renderer and the hit-test use, measured last
        // frame. Two different row widths in one frame is how Up/Down and a
        // click end up disagreeing about which base is under the pointer.
        let per_row = self.edit.per_row();
        let per_page = self.edit.visible_rows.max(1) * per_row;

        for ev in events {
            let Some(d) = self.document.as_mut() else {
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
        let d = self.document.as_ref().expect("checked by caller");
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

    fn central(&mut self, ui: &mut Ui) {
        let error = self.error.clone();
        let selected = self.selected;
        let hot = self.hot;
        let mut hovered_out = None;
        let mut clicked_out = None;

        // Asked here, outside the paint closure, and answered from a memo.
        // Which application owns the extension is *read*, never changed —
        // claiming .dna at install time is how two plasmid editors end up
        // fighting over double-click — but reading it live inside the closure
        // spawned a `cmd /C assoc .dna` child process on every repaint and
        // blocked the UI thread on it until cmd.exe exited.
        let association = if self.error.is_none() && self.document.is_none() {
            association_note(self.dna_owner())
        } else {
            String::new()
        };

        let notice = self.notice.clone();
        let mut dismiss = false;

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
                        ui.horizontal(|ui| {
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
                            if ui.button("Dismiss").clicked() {
                                dismiss = true;
                            }
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
            let Some(d) = &self.document else {
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
            let r = map::show(ui, d.molecule(), caption, d.digest.results(), selected, hot);
            hovered_out = r.hovered;
            clicked_out = r.clicked;
        });

        if dismiss {
            self.notice = None;
        }
        if hovered_out.is_some() {
            self.hot = hovered_out;
        }
        if let Some(i) = clicked_out {
            self.selected = if self.selected == Some(i) {
                None
            } else {
                Some(i)
            };
            self.tab = Tab::Features;
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
        let Some(d) = self.document.as_ref() else {
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
            Ok(p) => self.design = Some(p),
            Err(e) => self.notice = Some(e),
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
        if self.document.is_none() {
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
            let Some(d) = self.document.as_ref() else {
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

    fn paste_dialog(&mut self, ctx: &egui::Context) {
        // The selection the question was asked about, not wherever the caret
        // has since got to. It was stored and then thrown away: with the
        // document still live behind a non-modal window, one click was enough
        // to make the paste land somewhere else while the dialog's own text
        // described the old target.
        let Some((report, target)) = self.edit.pending_paste.clone() else {
            return;
        };
        let n = self.document.as_ref().map_or(0, |d| d.molecule().len());
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
            if let Some(d) = &mut self.document {
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
/// egui's default proportional face has no U+2190, so a `←` set in it comes out
/// as a tofu box; the features list gets away with the arrow only because it
/// asks for `.monospace()`. The hover readout is the sequence view's non-colour
/// channel — the one place a reverse feature's direction is stated in the
/// sequence at all — and an empty box states nothing. Found by looking at the
/// running app: `strand_glyphs_cover_every_variant` asserts the strings are
/// non-empty, and a tofu box is a non-empty string.
fn strand_word(s: Strand) -> &'static str {
    match s {
        Strand::Forward => "forward",
        Strand::Reverse => "reverse",
        Strand::Both => "both strands",
        Strand::Unoriented => "no strand",
    }
}

/// One enzyme, with whatever qualifies the answer.
///
/// `blocked` is the methylation verdict: `docs/PLAN.md` §7.1 requires such
/// sites be "struck through, not hidden". A site that will not cut is still a
/// site — it exists in the sequence, appears on everyone else's map, and cuts
/// the moment the plasmid goes through a dam- strain. Hiding it produces a map
/// that disagrees with every other tool for reasons the user cannot see.
fn enzyme_row(
    ui: &mut Ui,
    name: &str,
    site: &str,
    positions: &[u64],
    unique: bool,
    blocked: Option<pl_enzymes::methylation::SiteEffect>,
    poor_single_site: Option<&'static str>,
) {
    ui.horizontal(|ui| {
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

    #[test]
    fn strand_glyphs_cover_every_variant() {
        for s in [
            Strand::Forward,
            Strand::Reverse,
            Strand::Both,
            Strand::Unoriented,
        ] {
            assert!(!strand_glyph(s).is_empty());
            // In words as well, and ASCII, because the hover readout is set in
            // egui's proportional face and that face has no U+2190: the arrow
            // rendered there as an empty box. Non-empty was not enough to ask.
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
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
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
        // gate cannot be inferred from the proportional one: U+2192 is in Hack and
        // in nothing else `default_fonts` embeds, and Hack is not in the
        // proportional chain.
        assert!(!renders_as_tofu(
            &ctx,
            egui::FontFamily::Monospace,
            11.0,
            '\u{2192}'
        ));
        assert!(renders_as_tofu(
            &ctx,
            egui::FontFamily::Proportional,
            11.0,
            '\u{2192}'
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
    /// What the band buys, concretely: IBM Plex Mono and JetBrains Mono are
    /// 0.600 em and Cascadia Mono is 0.585938 (measured with fontTools), so all
    /// three drop in with NO constant moved. Fira Code is 0.615385 and breaks
    /// `DEF_PANEL - 12`; Iosevka is 0.500 and breaks `DEF_PANEL - 40` in the other
    /// direction. Whoever picks a face outside the band must move `DEF_PANEL` --
    /// not edit the expectation, because the "and it is not padded" half of that
    /// test is what stops the details panel quietly eating the map pane.
    #[test]
    fn the_advance_band_that_keeps_every_per_row_expectation() {
        const LEN: u64 = 8_117; // `seq_app`'s molecule, so a 5-character gutter
        let per_row = |p: f32, ratio: f32, c: f32| -> u64 {
            let g = seqedit::gutter_w(LEN, 11.0 * ratio);
            seqedit::fit_per_row(p - c - g, 11.5 * ratio)
        };

        // --- measure C, and the face we have, from the real painter -----------
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        let hack = ctx.fonts_mut(|f| f.glyph_width(&egui::FontId::monospace(11.5), 'A')) / 11.5;
        // Hack-Regular 3.003 is 1233/2048; if this moves, the numbers in the doc
        // above are about a different font than the one in the binary.
        assert!(
            (hack - 0.602_051).abs() < 1e-4,
            "the incumbent monospace advance ratio is {hack}, not Hack's 0.602051"
        );

        // The smallest panel that reaches sixty, from the painter itself.
        // Bisected rather than stepped: `fit_per_row` is monotonic in the width,
        // and a 0.5 pt linear sweep cost this test 27 seconds of a suite that
        // people have to be willing to run.
        let reaches = |p: f32| -> bool {
            let ctx = egui::Context::default();
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
        let c = sixty - 60.0 * (11.5 * hack) - seqedit::gutter_w(LEN, 11.0 * hack);
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
                per_row(panel, hack, c),
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
             face in the binary = {hack:.6} em"
        );
        // Measured with fontTools on this machine, except where marked.
        for (name, ratio, inside) in [
            ("Hack 3.003 (the incumbent)", 0.602_051f32, true),
            ("IBM Plex Mono", 0.600, true),
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

        let ctx = egui::Context::default();
        // One frame, so the font set exists.
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
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
    ///   U+2192 `→`  Hack yes · Ubuntu-Light MISSING · NotoEmoji MISSING · emoji-icon MISSING
    ///   U+2190 `←`  Hack yes · all three others MISSING
    ///   U+2194 `↔`  Hack yes · NotoEmoji yes
    ///   U+25B6 `▶`  Hack yes · both emoji fonts yes · Ubuntu-Light MISSING
    ///
    /// The forward and reverse arrows have exactly ONE supplier in the binary and it
    /// is the face being swapped. They are `strand_glyph`'s two commonest values,
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
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
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
        // The sizes the app really asks for: 9.0 and 9.5 are the map's ruler and the
        // sequence view's, 11.0 and 11.5 everything else.
        for size in [9.0f32, 9.5, 11.0, 11.5] {
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
    #[test]
    fn a_sequence_row_shapes_to_one_glyph_per_base_on_one_pitch() {
        // Ligature-prone pairs that survive `is_ascii_graphic`, padded with bases.
        let mol = pl_core::Molecule {
            seq: b"ACGT--ACGT**ACGT..ACGT->ACGT=>ACGT<=ACGT!=ACGT::ACGT//ACGT<>++".to_vec(),
            topology: pl_core::Topology::Circular,
            ..Default::default()
        };
        let e = seqedit::SeqEdit::new();
        let mut row = String::new();
        e.row_text(&mol, 0, 60, &mut row);
        assert_eq!(row.chars().count(), 60, "sixty cells: {row:?}");
        for pair in ["--", "**", "..", "->", "=>", "<=", "!=", "::", "//"] {
            assert!(
                row.contains(pair),
                "{pair:?} did not survive row_text, so this proves nothing about \
                 ligatures: {row:?}"
            );
        }

        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        // One device pixel. `epaint` snaps every glyph's x to a whole device pixel
        // (`text_layout.rs`: `glyph.pos.x = round_to_pixel(glyph.pos.x)`), so a
        // correct face still shows a bounded sawtooth of about 0.87 pt at this
        // metric. A collapsed cluster is out by a whole advance, seven times this.
        let tol = 1.01 / ctx.pixels_per_point();

        // All four properties as one function returning WHICH one broke, so the same
        // code can be pointed at a row that must pass and a row that must fail.
        let check = |text: &str, size: f32| -> Result<(), String> {
            let chars: Vec<char> = text.chars().collect();
            let advance = ctx.fonts_mut(|f| f.glyph_width(&egui::FontId::monospace(size), 'A'));
            let job = egui::text::LayoutJob::simple_singleline(
                text.to_string(),
                egui::FontId::monospace(size),
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

        for size in [9.5f32, 11.0, 11.5] {
            check(&row, size).unwrap_or_else(|e| {
                panic!("at {size} pt the sequence row does not shape to a grid: {e}")
            });
        }

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
        let err = check(collapsing, 11.5)
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
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
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
        app.document = Some(edited_doc("x.fa", SEQ));
        app.autosave();
        assert_eq!(
            autosaved(&path).0.topology,
            pl_core::Topology::Circular,
            "the premise: the first edit was written"
        );

        let d = app.document.as_mut().unwrap();
        d.undo().unwrap();
        d.apply(pl_core::OpKind::ReverseComplement).unwrap();
        assert_eq!(d.log.path().len(), 1, "the collision this test is about");
        // The thirty-second throttle is a separate question. Clear it so this
        // is about identity and nothing else.
        app.last_autosave = None;
        app.autosave();

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
        app.document = Some(edited_doc("a.fa", "AAAACCCCGGGGTTTTAAGG"));
        app.autosave();
        assert_eq!(autosaved(&path).1, "a.fa", "the premise");

        app.document = Some(edited_doc("b.fa", "GGGGGGGGTTTTTTTTAACC"));
        app.last_autosave = None;
        app.autosave();

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
        app.document = Some(edited_doc("x.fa", "AAAACCCCGGGGTTTTAAGG"));
        app.autosave();
        assert!(path.exists());
        std::fs::remove_file(&path).unwrap();
        app.last_autosave = None;
        for _ in 0..100 {
            app.autosave();
        }
        assert!(!path.exists(), "an unchanged document was rewritten");
    }

    #[test]
    fn merely_opening_another_file_does_not_discard_an_unsaved_draft() {
        // The other half of "an unedited document has nothing to protect". A
        // file that has only been *looked at* must not overwrite somebody's
        // unsaved edits, however stale the identity check thinks they are.
        let (mut app, path) = app_with_recovery("browse");
        app.document = Some(edited_doc("a.fa", "AAAACCCCGGGGTTTTAAGG"));
        app.autosave();

        app.document = Some(Document::from_bytes(b">b\nTTTTTTTT\n", "b.fa".into(), None).unwrap());
        app.last_autosave = None;
        for _ in 0..10 {
            app.autosave();
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
        app.document = Some(edited_doc("x.fa", SEQ));
        app.autosave();
        assert_eq!(autosaved(&path).0.topology, pl_core::Topology::Circular);

        app.document.as_mut().unwrap().undo().unwrap();
        app.last_autosave = None;
        app.autosave();
        assert_eq!(autosaved(&path).0.topology, pl_core::Topology::Linear);
    }

    #[test]
    fn an_unedited_document_is_not_autosaved_at_all() {
        // The other control. The user's own file already holds this, and a
        // recovery file that exists is this program's only record of an
        // unclean exit.
        let (mut app, path) = app_with_recovery("unedited");
        app.document =
            Some(Document::from_bytes(b">x\nAAAACCCCGGGG\n", "x.fa".into(), None).unwrap());
        for _ in 0..10 {
            app.autosave();
        }
        assert!(!path.exists(), "nothing had been edited");
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
        app.autosave();
        if let (Some(t), Some(d)) = (typed, app.document.as_mut()) {
            app.edit.type_text(d, t, now);
        }
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
        let d = app.document.as_ref().unwrap();
        assert_eq!(
            d.log.path().len(),
            0,
            "forty keystrokes inside one second are still one open run"
        );
        assert_eq!(app.edit.run().unwrap().inserted.len(), 40);

        // And when the typing stops, the run closes on its own and becomes
        // exactly one operation — one Ctrl+Z for the lot.
        frame(&mut app, None, t + seqedit::Run::IDLE_SECONDS);
        let d = app.document.as_ref().unwrap();
        assert_eq!(d.log.path().len(), 1);
        assert_eq!(d.molecule().len(), 16 + 40);
        app.do_undo();
        assert_eq!(app.document.as_ref().unwrap().molecule().len(), 16);
    }

    #[test]
    fn an_autosave_that_writes_never_leaves_out_the_open_run() {
        // The other half, and the reason the settle is there at all: a recovery
        // file written from `log.current()` mid-run is missing the user's last
        // keystrokes. Moving the throttle above the settle must not cost this.
        let (mut app, path) = app_with_recovery("midrun");
        app.adopt(Document::from_bytes(b">x\nAAAACCCCGGGGTTTT\n", "x.fa".into(), None).unwrap());
        let d = app.document.as_mut().unwrap();
        app.edit.caret = 16;
        app.edit.type_text(d, "gggg", 500.0);
        assert!(app.edit.run().is_some(), "the premise: a run is open");

        // The autosave falls due.
        app.last_autosave = None;
        app.autosave();

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
        app.document = Some(circle_with("ABCDEFGHIJKL", "inner", 5, 8));
        app.edit.sel = Some(seqedit::Selection {
            anchor: 10,
            head: 2,
            through_origin: true,
        });

        let clip = app.do_cut(500.0).expect("four bases on the clipboard");
        assert_eq!(clip, "KLAB", "read across the origin, in reading order");
        assert_eq!(
            app.document.as_ref().unwrap().molecule().seq,
            b"CDEFGHIJ".to_vec()
        );
        let said = app.edit.notice.clone().unwrap_or_default();
        assert!(said.contains("cut 4 bases"), "said {said:?}");
        assert!(said.contains("renumbered"), "said {said:?}");
    }

    #[test]
    fn cutting_a_whole_feature_away_still_names_it() {
        let mut app = App::blank();
        app.document = Some(circle_with("AAAACCCCGGGGTTTTAAGG", "AmpR", 5, 8));
        app.edit.sel = Some(seqedit::Selection {
            anchor: 4,
            head: 8,
            through_origin: false,
        });

        app.do_cut(500.0).expect("four bases");
        assert!(app
            .document
            .as_ref()
            .unwrap()
            .molecule()
            .features
            .is_empty());
        let said = app.edit.notice.clone().unwrap_or_default();
        assert!(said.contains("cut 4 bases"), "said {said:?}");
        assert!(said.contains("AmpR"), "said {said:?}");
    }

    #[test]
    fn a_cut_with_nothing_selected_removes_nothing_and_says_so() {
        let mut app = App::blank();
        app.document = Some(circle_with("AAAACCCCGGGGTTTTAAGG", "f", 1, 4));
        app.edit.caret = 5;
        assert_eq!(app.do_cut(500.0), None);
        assert_eq!(app.document.as_ref().unwrap().molecule().len(), 20);
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
        app.document = Some(circle_with("ABCDEFGHIJKL", "inner", 5, 8));
        app.edit.sel = Some(seqedit::Selection {
            anchor: 10,
            head: 2,
            through_origin: true,
        });
        app.do_cut(500.0).unwrap();
        let d = app.document.as_ref().unwrap();
        assert_eq!(d.log.path().len(), 2, "the premise: two operations");
        assert_eq!(d.molecule().seq, b"CDEFGHIJ".to_vec());

        app.do_undo();
        let d = app.document.as_ref().unwrap();
        assert_eq!(
            d.molecule().seq,
            b"ABCDEFGHIJKL".to_vec(),
            "one press, and the numbering is the one the file had"
        );
        assert_eq!(d.molecule().features[0].start(), 5, "and so is the feature");
        assert!(app.status.contains("origin"), "status {:?}", app.status);

        // And forward again, both halves together.
        app.do_redo();
        let d = app.document.as_ref().unwrap();
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
        app.document = Some(Document::of_molecule(mol));
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
        app.document =
            Some(Document::from_bytes(b">a\nAAAACCCCGGGGTTTT\n", "a.fa".into(), None).unwrap());
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
        let ctx = egui::Context::default();
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
                let ctx = egui::Context::default();
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

    /// What `global_shortcuts` decides, with `focused` optionally holding focus.
    fn shortcuts_with(app: &App, key: egui::Key, focused: Option<&str>) -> Shortcuts {
        let ctx = egui::Context::default();
        ctx.begin_pass(ctrl(key));
        if let Some(name) = focused {
            ctx.memory_mut(|m| m.request_focus(egui::Id::new(name)));
        }
        let out = app.global_shortcuts(&ctx);
        let _ = ctx.end_pass();
        out
    }

    /// PROVEN TO FAIL at dfd6ac9 (see the report): with the block as it stood,
    /// `undo` is true while the Features filter holds focus, so Ctrl+Z after a
    /// typo in a search box undoes the *molecule* as well as the typo.
    #[test]
    fn a_shortcut_typed_into_a_focused_text_box_does_not_reach_the_document() {
        let mut app = App::blank();
        app.document =
            Some(Document::from_bytes(b">a\nAAAACCCCGGGGTTTT\n", "a.fa".into(), None).unwrap());

        // The control: nothing focused, so the shortcuts are the app's.
        assert!(shortcuts_with(&app, egui::Key::Z, None).undo);
        assert!(shortcuts_with(&app, egui::Key::Y, None).redo);
        assert!(shortcuts_with(&app, egui::Key::O, None).open);

        assert!(shortcuts_with(&app, egui::Key::S, None).save);

        // The Features tab's filter box, the Library query and the design
        // panel's Spacer field are all plain `TextEdit`s, and egui gives Ctrl+Z
        // to the focused one without consuming it.
        for who in ["features filter", "library query", "design spacer"] {
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

    /// A 900 bp document with a selection, and the design panel open on it.
    fn app_designing() -> App {
        let seq: String = (0..900u32)
            .scan(12_345u64, |s, _| {
                *s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                Some(b"ACGT"[((*s >> 24) & 3) as usize] as char)
            })
            .collect();
        let mut app = App::blank();
        app.document = Some(
            Document::from_bytes(format!(">a\n{seq}\n").as_bytes(), "a.fa".into(), None).unwrap(),
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
        let ctx = egui::Context::default();
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
        let seq = app.document.as_ref().unwrap().molecule().seq.clone();
        app.design.as_mut().unwrap().run(&seq);
        assert!(
            matches!(app.design.as_ref().unwrap().result, Some(Ok(_))),
            "the premise: a report to add"
        );

        // The toolbar stayed live behind a non-modal window.
        app.edit(pl_core::OpKind::ReverseComplement);
        assert!(app
            .document
            .as_ref()
            .unwrap()
            .molecule()
            .features
            .is_empty());

        app.design.as_mut().unwrap().add_request = Some(0);
        design_frame(&mut app);
        assert!(
            app.document
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
        let seq = app.document.as_ref().unwrap().molecule().seq.clone();
        app.design.as_mut().unwrap().run(&seq);
        app.design.as_mut().unwrap().add_request = Some(0);
        design_frame(&mut app);
        assert_eq!(
            app.document.as_ref().unwrap().molecule().features.len(),
            2,
            "and a current report still adds two features"
        );
        assert!(app.status.contains("Ctrl+Z twice"), "{}", app.status);
    }

    /// PROVEN TO FAIL at dfd6ac9: `load`'s error arm left `self.design` alone,
    /// and `design_panel` then dropped the panel — constraints, report and all
    /// — on its own early return, in the same frame, with nothing said.
    #[test]
    fn a_failed_load_closes_the_design_panel_and_says_so() {
        let mut app = app_designing();
        let seq = app.document.as_ref().unwrap().molecule().seq.clone();
        design_frame(&mut app);
        app.design.as_mut().unwrap().run(&seq);

        let bad = std::env::temp_dir().join(format!("pl-gui-notafile-{}.dna", std::process::id()));
        std::fs::write(&bad, b"this is not a sequence file at all").unwrap();
        app.load(bad.clone());
        let _ = std::fs::remove_file(&bad);

        assert!(app.error.is_some(), "the premise: the load failed");
        assert!(app.design.is_none(), "the panel is gone");
        assert!(
            app.notice
                .as_deref()
                .unwrap_or_default()
                .contains("design panel"),
            "and it was said: {:?}",
            app.notice
        );
    }

    /// PROVEN TO FAIL at dfd6ac9: `adopt` reset the caret, the selection and the
    /// highlight but not `self.design`, so the panel survived a document swap
    /// holding the old file's title, length and report — and "Add to document"
    /// wrote file A's primer coordinates, under file A's name, into file B.
    #[test]
    fn opening_another_file_closes_the_design_panel_rather_than_reusing_it() {
        let mut app = app_designing();
        let seq = app.document.as_ref().unwrap().molecule().seq.clone();
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
            app.document.is_some(),
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

    /// PROVEN TO FAIL at bd96e5b, behaviourally, on the very first assertion:
    /// `.exact_size(380.0)` sets `outer_size_range = Rangef::point(380)`, so
    /// `fit_per_row` measures 40 bases and no drag can move it. Both halves
    /// fail there — the resting width and the drag.
    #[test]
    fn the_split_moves_and_the_row_width_follows_it() {
        let ctx = egui::Context::default();
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
        let ctx = egui::Context::default();
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
        let ctx = egui::Context::default();
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
        let ctx = egui::Context::default();
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
    #[test]
    fn the_readout_and_its_button_are_not_cut_off_at_any_split() {
        for width in [App::DEF_PANEL, App::MIN_PANEL] {
            let ctx = egui::Context::default();
            let mut app = seq_app();
            app.layout.panel_w = Some(width);
            // Caret 0 on a circle: the longest form the sentence takes.
            app.edit.caret = 0;
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
                "{lost:.0} pt of the readout is laid out below the window at a                  {width} pt split"
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
            // And the grid above it did not get squeezed out of existence.
            let g = app.seq_grid.expect("the grid was painted");
            assert!(
                g.top + 8.0 * g.row_h < r.top(),
                "only {:.0} pt of sequence left above the readout at {width}",
                r.top() - g.top
            );
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
            app.document.as_ref().unwrap().log.cursor(),
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

        let t = std::time::Instant::now();
        let ix = annot::AnnotIndex::build(&mol, (0, None));
        let build = t.elapsed();
        std::hint::black_box(&ix);

        let t = std::time::Instant::now();
        let c = mol.clone();
        let clone = t.elapsed();
        std::hint::black_box(&c);

        assert!(
            build * 4 < clone * 10,
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
        let ctx = egui::Context::default();
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
        let ctx = egui::Context::default();
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
        let ctx = egui::Context::default();
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
        let ctx = egui::Context::default();
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
        let ctx = egui::Context::default();
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
            let d = app.document.as_mut().expect("a document");
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
        let ctx = egui::Context::default();
        let mut app = seq_app();
        digested(&mut app);
        paint(&mut app, &ctx, window());
        let settled = app.seq_row_h;
        assert!(
            app.annot.cut_count() > 0 && app.enz_strip,
            "the premise: this molecule has admitted cuts, so a strip is reserved"
        );

        // Any edit restarts the digest, and nothing polls it here.
        app.document
            .as_mut()
            .expect("a document")
            .apply(pl_core::OpKind::InsertAt {
                at: 101,
                seq: "acgt".into(),
            })
            .expect("a legal insert");
        assert!(
            app.document.as_ref().unwrap().digest.is_running(),
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
            let ctx = egui::Context::default();
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
            let ctx = egui::Context::default();
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
        let ctx = egui::Context::default();
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
                        map::show(ui, mol, caption, digest, None, None);
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
            let ctx = egui::Context::default();
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
                            map::show(ui, &mol, "pET28a", &pkov_cutters(), None, None);
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
                            map::show(ui, &mol, "pET28a", &pkov_cutters(), None, None);
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
        let ctx = egui::Context::default();
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
                        map::show(ui, &mol, "pKoV with His decR", &cutters, None, None);
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
        for (w, h) in [(706.0f32, 756.0f32), (880.0, 620.0), (560.0, 900.0)] {
            let (shapes, pane) = paint_map(&mol, "pKoV with His decR", &cutters, w, h);
            let (centre, r) = backbone(&shapes);
            let labels = texts_in(&shapes, 10.0, egui::FontFamily::Monospace);
            assert!(
                labels.len() >= 15,
                "{w}x{h}: only {} labels on a plasmid with 22 unique cutters",
                labels.len()
            );

            // 1. Whole, and inside the pane. This is the one that fails at
            //    e087e27.
            for (text, rect) in &labels {
                assert!(
                    !text.ends_with("..."),
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

            // 2. No two overlap.
            for i in 0..labels.len() {
                for j in i + 1..labels.len() {
                    let hit = labels[i].1.intersects(labels[j].1);
                    assert!(
                        !hit,
                        "{w}x{h}: {:?} at {:?} overlaps {:?} at {:?}",
                        labels[i].0, labels[i].1, labels[j].0, labels[j].1
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
            let lines = hairlines(&shapes);
            for (text, rect) in &labels {
                // The FIRST coordinate in the label, which is the tick's own
                // base: `Site::anchor` is `positions.first()`, and a folded label
                // lists its members in coordinate order.
                let coord: u64 = text
                    .split(" / ")
                    .next()
                    .and_then(|first| first.rsplit("  ").next())
                    .map(|c| c.replace(',', ""))
                    .and_then(|c| c.parse().ok())
                    .unwrap_or_else(|| panic!("no coordinate in {text:?}"));
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
                let ok = full.iter().any(|f| {
                    let name = f.rsplit_once("  ").map_or(f.as_str(), |(n, _)| n);
                    name.starts_with(body) && !body.is_empty()
                });
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
        let ctx = egui::Context::default();
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
        let ctx = egui::Context::default();
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
        let doc = app.document.as_mut().unwrap();
        for _ in 0..2_000 {
            if doc.digest.poll() && !doc.digest.is_running() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let d = app.document.as_ref().unwrap();
        assert!(
            !d.digest.results().is_empty(),
            "the digest worker did not finish"
        );

        let opts = App::figure_options(d);
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
