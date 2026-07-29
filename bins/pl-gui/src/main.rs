//! Polylinker — a plasmid viewer that runs offline and asks nothing of anyone.
//!
//! Everything it decides about a molecule it asks `pl-core`, `pl-fileio` and
//! `pl-enzymes`, the same crates behind the `pl` command line and the browser
//! build. This binary is presentation.

// A console window alongside the app on Windows is noise for a GUI, but keep it
// in debug builds so panics and eprintln stay visible while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod design;
mod doc;
mod library;
mod map;
mod recover;
mod seqedit;
mod theme;

use std::path::PathBuf;

use eframe::egui::{self, Align, Layout, RichText, Sense, Ui};
use pl_core::Strand;

use doc::{describe, fmt_int, DigestState, Document};
use theme::Palette;

/// Theme-resolved colours for whatever `ui` is currently drawing into.
fn pal(ui: &Ui) -> Palette {
    Palette::of(ui.visuals().dark_mode)
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
        // Anything left in the recovery directory by another process is an
        // unclean exit. Listed, never auto-restored: which of two drafts is the
        // wanted one is something the user knows and this program does not.
        match recover::recovery_dir() {
            Ok(dir) => {
                app.stale = recover::stale(&dir);
                app.recovery = Some(recover::recovery_path(&dir, 0));
                if !app.stale.is_empty() {
                    app.status = format!(
                        "{} document(s) from a session that did not close cleanly — see Recover",
                        app.stale.len()
                    );
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
        self.error = None;
        self.notice = None;
        self.edit = seqedit::SeqEdit::new();
        self.selected = None;
        self.hot = None;
        self.last_autosave = Some(std::time::Instant::now());
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
        let lossy = if as_fasta {
            Vec::new()
        } else {
            d.molecule().features_without_expressible_orientation()
        };
        let note = if lossy.is_empty() {
            String::new()
        } else {
            // GenBank has no way to say "unoriented", so those features are
            // written as forward. For about half of them that is a directional
            // claim the source never made.
            format!(
                "  —  {} feature(s) written as forward; GenBank cannot express their strand",
                lossy.len()
            )
        };
        match std::fs::write(&path, text) {
            Ok(()) => self.status = format!("wrote {}{note}", path.display()),
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
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
        let Some(d) = &self.document else { return };
        let stem = pl_fileio::genbank::locus_name(&d.title);
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.pdf"))
            .add_filter("PDF", &["pdf"])
            .save_file()
        else {
            return;
        };
        let (bytes, drawn, font) = pl_draw::circular_pdf(d.molecule(), pl_draw::Options::default());

        let mut note = String::new();
        if !drawn.labels_hidden.is_empty() {
            note.push_str(&format!(
                "  —  {} label(s) did not fit: {}",
                drawn.labels_hidden.len(),
                drawn.labels_hidden.join(", ")
            ));
        }
        if !drawn.malformed.is_empty() {
            note.push_str(&format!(
                "  —  {} feature(s) lie outside the molecule and are not drawn: {}",
                drawn.malformed.len(),
                drawn.malformed.join(", ")
            ));
        }
        if !font.unencodable.is_empty() {
            note.push_str(&format!(
                "  —  {} name(s) hold characters Helvetica cannot show and were written with '?': {}. Export SVG to keep them",
                font.unencodable.len(),
                font.unencodable.join(", ")
            ));
        }
        match std::fs::write(&path, bytes) {
            Ok(()) => self.status = format!("wrote {}{note}", path.display()),
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }

    /// Write the map as SVG.
    ///
    /// Deliberately the default `pl_draw::Options`, the same ones `pl export`
    /// uses, so the app and the command line produce byte-identical files for
    /// the same molecule. A figure that changes depending on which of the two
    /// you reached for is a figure you cannot cite.
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
        let (svg, drawn) = pl_draw::circular_svg(d.molecule(), pl_draw::Options::default());

        // A map missing three labels looks exactly like a plasmid with three
        // fewer features, so the count goes in the status line rather than
        // nowhere.
        let mut note = String::new();
        if !drawn.labels_hidden.is_empty() {
            note.push_str(&format!(
                "  —  {} label(s) did not fit: {}",
                drawn.labels_hidden.len(),
                drawn.labels_hidden.join(", ")
            ));
        }
        if !drawn.malformed.is_empty() {
            note.push_str(&format!(
                "  —  {} feature(s) lie outside the molecule and are not drawn: {}",
                drawn.malformed.len(),
                drawn.malformed.join(", ")
            ));
        }
        match std::fs::write(&path, svg) {
            Ok(()) => self.status = format!("wrote {}{note}", path.display()),
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

        // A paste is waiting on an answer, and these shortcuts are read
        // straight off the context rather than from a widget, so no amount of
        // modality in the dialog would stop them. Undoing while the question is
        // on screen changes the document the question is about.
        let asking = self.edit.pending_paste.is_some();
        if !asking && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
            self.pick_file();
        }
        // Ctrl+Z / Ctrl+Y, plus Ctrl+Shift+Z for the mac-shaped habit.
        let (undo, redo) = ctx.input(|i| {
            let cmd = i.modifiers.command && !asking;
            (
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                cmd && (i.key_pressed(egui::Key::Y)
                    || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
            )
        });
        if undo {
            self.do_undo();
        }
        if redo {
            self.do_redo();
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

impl App {
    fn top_bar(&mut self, ui: &mut Ui) {
        egui::Panel::top(egui::Id::new("toolbar")).show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Open…").on_hover_text("Ctrl+O").clicked() {
                    self.pick_file();
                }
                let has = self.document.is_some();
                ui.add_enabled_ui(has, |ui| {
                    if ui.button("Save GenBank…").clicked() {
                        self.export(false);
                    }
                    if ui.button("FASTA…").clicked() {
                        self.export(true);
                    }
                    if ui
                        .button("Map SVG…")
                        .on_hover_text("Vector map, for a figure")
                        .clicked()
                    {
                        self.export_svg();
                    }
                    if ui
                        .button("Map PDF…")
                        .on_hover_text("The same map, for a manuscript")
                        .clicked()
                    {
                        self.export_pdf();
                    }
                });

                ui.separator();
                self.edit_group(ui);

                ui.separator();
                if let Some(d) = &self.document {
                    // A dot rather than the usual asterisk-in-the-title: the
                    // point is that edits exist and are undoable, not that a
                    // file is dirty — nothing here writes over the original.
                    let shown = if d.edited() {
                        format!("{} •", d.title)
                    } else {
                        d.title.clone()
                    };
                    let title = ui.label(RichText::new(shown).strong());
                    if let Some(p) = &d.path {
                        title.on_hover_text(p.display().to_string());
                    }
                    ui.label(RichText::new(&self.status).color(pal(ui).muted).size(12.0));
                } else {
                    ui.label(
                        RichText::new("Open a .dna, GenBank or FASTA file, or drop one here")
                            .color(pal(ui).muted),
                    );
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::global_theme_preference_switch(ui);
                    if let Some(d) = &self.document {
                        if d.digest.is_running() {
                            ui.add(egui::Spinner::new().size(13.0));
                            ui.label(RichText::new("digesting").color(pal(ui).muted).size(12.0));
                        }
                    }
                });
            });
            ui.add_space(4.0);
        });
    }

    /// Undo, redo, and the edits that need no selection.
    ///
    /// Every one of these goes through the operation log, so every one is
    /// undoable and every one shows up in the History tab. There is no other
    /// path that mutates a molecule.
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
            if ui.button("Redo").on_hover_text("Ctrl+Y").clicked() {
                self.do_redo();
            }
        });

        let has = self.document.is_some();
        ui.add_enabled_ui(has, |ui| {
            ui.menu_button("Edit", |ui| {
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
                        .button("Set origin at selected feature")
                        .on_hover_text("renumber the plasmid to start at this feature")
                        .clicked()
                    {
                        if let (Some(d), Some(i)) = (&self.document, sel) {
                            if let Some(f) = d.molecule().features.get(i) {
                                let origin = f.start();
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
    fn edit(&mut self, kind: pl_core::OpKind) {
        self.settle();
        let Some(d) = &mut self.document else { return };
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
            }
            // The log refuses an edit that would leave the annotations
            // describing something the sequence does not contain. Saying which
            // edit and why is the whole point of refusing rather than
            // corrupting — and it goes to `notice`, with the document still on
            // screen, not to the "could not read that file" takeover.
            Err(e) => self.notice = Some(format!("Cannot {what}: {e}.\nNothing was changed.")),
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

    fn side_panel(&mut self, ui: &mut Ui) {
        egui::Panel::right(egui::Id::new("details"))
            .exact_size(380.0)
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
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
                                ui.label(
                                    RichText::new(format!(
                                        "{}..{}",
                                        fmt_int(f.start()),
                                        fmt_int(f.end())
                                    ))
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

        // The methylation verdict for each shown enzyme. Computed at the first
        // site: every rule here is a property of the (enzyme, methylase) pair
        // plus local context, so a per-site answer is what the model gives —
        // this shows the first, which is exact for the unique cutters that
        // matter most and indicative for the rest.
        let mol = d.molecule();
        let verdict = |dg: &pl_enzymes::Digest| -> Option<pl_enzymes::methylation::SiteEffect> {
            // Ask the digester where the site was rather than deriving it back
            // from the cut. `cut_sites` carries `site_start` alongside the
            // position because it already knows both, so there is exactly one
            // mapping from a match to a cut in the tree. Recomputing it here
            // was a second one, and the two disagreed on any site wrapping the
            // origin.
            let site = pl_enzymes::cut_sites(&mol.seq, mol.topology, dg.enzyme)
                .into_iter()
                .next()?;
            pl_enzymes::methylation::site_effect(
                dg.enzyme,
                &mol.seq,
                (site.site_start - 1) as usize,
                mol.topology,
                &mol.methylation,
            )
        };

        let shown: Vec<_> = results.iter().filter(|x| set.admits(x)).collect();
        let uniq: Vec<_> = shown.iter().filter(|x| x.is_unique_cutter()).collect();
        let multi: Vec<_> = shown.iter().filter(|x| !x.is_unique_cutter()).collect();

        egui::ScrollArea::vertical().show(ui, |ui| {
            if !uniq.is_empty() {
                ui.label(RichText::new(format!("{} unique cutters", uniq.len())).strong());
                ui.add_space(2.0);
                for e in &uniq {
                    enzyme_row(
                        ui,
                        e.enzyme.name,
                        e.enzyme.site,
                        &e.positions,
                        true,
                        verdict(e),
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
                for e in &multi {
                    enzyme_row(
                        ui,
                        e.enzyme.name,
                        e.enzyme.site,
                        &e.positions,
                        false,
                        verdict(e),
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
                        RichText::new(if here { "▶" } else { " " })
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
    fn sequence_tab(&mut self, ui: &mut Ui) {
        use seqedit::{Editability, Selection};

        let now = ui.input(|i| i.time);
        let gate = Editability::of(
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

        // ONE pitch, derived from the font the row is actually painted in, and
        // used by `show_rows`, by the y -> row hit-test and by the painter.
        //
        // The old row was a `ui.horizontal` of two labels, which advanced
        // 21.0 px against the 18.125 px `show_rows` was told to assume — the
        // `interact_size` floor. Read-only that drift is cosmetic; the moment a
        // click has to name a base it is 8 rows of overshoot at the bottom of a
        // 60-row viewport, i.e. the user clicks one base and the caret lands
        // 480 bases away.
        let font = egui::FontId::monospace(11.5);
        let gutter_font = egui::FontId::monospace(11.0);
        let (row_h, advance) = ui.ctx().fonts_mut(|f| {
            (
                f.row_height(&font).max(1.0),
                f.glyph_width(&font, 'A').max(1.0),
            )
        });
        // The coordinate gutter, and then as many bases as are left over. The
        // row width is measured, not assumed: sixty cells plus the gutter is
        // wider than this panel, and a base that is off the edge cannot be
        // clicked.
        let gutter_w = 62.0;
        let scrollbar = ui.spacing().scroll.bar_width + 4.0;
        let per_row = seqedit::fit_per_row(ui.available_width() - gutter_w - scrollbar, advance);
        self.edit.set_per_row(per_row);

        let n = self
            .edit
            .effective_len(self.document.as_ref().unwrap().molecule());
        let rows = (n.div_ceil(per_row).max(1)) as usize;

        self.sequence_header(ui, n, rows);
        self.sequence_keys(ui, now);

        // `show_rows` computes its pitch as `row_height + item_spacing.y` from
        // the *enclosing* ui, so zeroing the spacing here makes pitch == row_h
        // and leaves one number for the renderer, the hit-test and the caret.
        // Set after the header, which wants its ordinary spacing.
        ui.spacing_mut().item_spacing.y = 0.0;

        // Reserve the readout: it is the number a biologist reads out loud when
        // they order a primer, so it does not scroll away.
        let readout_h = 30.0;
        let grid_h = (ui.available_height() - readout_h).max(60.0);

        let mut click: Option<(u64, bool)> = None;
        let mut drag_to: Option<u64> = None;
        let mut released = false;
        let mut double: Option<u64> = None;
        let mut visible = 1u64;

        {
            let d = self.document.as_ref().expect("checked by caller");
            let mol = d.molecule();
            let edit = &self.edit;
            let p = pal(ui);
            let sel = edit
                .sel
                .map(|s| s.canonical(mol.len(), mol.topology.is_circular()));
            let caret = edit.caret.min(n);
            let mut line = String::with_capacity(per_row as usize);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(grid_h)
                .show_rows(ui, row_h, rows, |ui, range| {
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
                    let x0 = rect.left() + gutter_w;
                    let painter = ui.painter_at(rect);
                    if debug_geometry() {
                        // The grid's own numbers, because a screenshot cannot
                        // settle whether a base sits inside the panel: this
                        // helper process is not per-monitor DPI aware, so
                        // Windows tells anything measuring the window
                        // *virtualised* coordinates.
                        eprintln!(
                            "seqgrid: rect={:?} clip={:?} x0={x0:.1} advance={advance:.2} \
                             row_h={row_h:.2} per_row={per_row} right_edge={:.1}",
                            rect,
                            ui.clip_rect(),
                            x0 + per_row as f32 * advance
                        );
                    }

                    let hit = |pos: egui::Pos2| -> u64 {
                        let r = (((pos.y - rect.top()) / row_h).floor() as i64)
                            .clamp(0, (range.end - range.start) as i64 - 1)
                            as u64
                            + first;
                        let col = (((pos.x - x0) / advance).round() as i64).clamp(0, per_row as i64)
                            as u64;
                        (r * per_row + col).min(n)
                    };

                    for r in range.clone() {
                        let r = r as u64;
                        let start = r * per_row;
                        let end = (start + per_row).min(n);
                        let y = rect.top() + (r - first) as f32 * row_h;

                        // Selection: one rectangle per row per contiguous
                        // range, clipped against this row. Nothing iterates the
                        // selection itself.
                        if let Some(s) = sel {
                            let spans: [(u64, u64); 2] = if s.through_origin {
                                [(0, s.lo()), (s.hi(), n)]
                            } else {
                                [(s.lo(), s.hi()), (0, 0)]
                            };
                            for (a, b) in spans {
                                let a = a.max(start);
                                let b = b.min(end);
                                if b > a {
                                    painter.rect_filled(
                                        egui::Rect::from_min_max(
                                            egui::pos2(x0 + (a - start) as f32 * advance, y),
                                            egui::pos2(
                                                x0 + (b - start) as f32 * advance,
                                                y + row_h,
                                            ),
                                        ),
                                        0.0,
                                        p.selection(),
                                    );
                                }
                            }
                        }

                        painter.text(
                            egui::pos2(rect.left() + gutter_w - 6.0, y),
                            egui::Align2::RIGHT_TOP,
                            fmt_int(start + 1),
                            gutter_font.clone(),
                            p.muted,
                        );
                        edit.row_text(mol, start, end, &mut line);
                        painter.text(
                            egui::pos2(x0, y),
                            egui::Align2::LEFT_TOP,
                            &line,
                            font.clone(),
                            p.ink,
                        );

                        // The caret sits on the row that contains the gap. At
                        // the very end of the molecule that is the last row's
                        // right edge, not the first column of a row that does
                        // not exist.
                        let on_this_row = (start..end).contains(&caret)
                            || (caret == end && (end == n || end == start));
                        if on_this_row && sel.is_none_or(|s| s.is_empty(mol.len())) {
                            let x = x0 + (caret - start) as f32 * advance;
                            painter.vline(x, y..=(y + row_h), egui::Stroke::new(1.5, p.accent));
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
                });
        }

        self.edit.visible_rows = visible;

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

        // -- the readout ---------------------------------------------------
        ui.add_space(3.0);
        let (mol_line, other_arc) = {
            let d = self.document.as_ref().expect("checked by caller");
            let mol = d.molecule();
            let n_c = mol.len();
            // Offered only when there really are two arcs to choose between.
            let two_arcs = mol.topology.is_circular()
                && self
                    .edit
                    .sel
                    .map(|s| s.canonical(n_c, true))
                    .is_some_and(|s| !s.is_empty(n_c) && s.base_count(n_c) < n_c);
            let alt = two_arcs.then(|| {
                let s = self.edit.sel.unwrap().canonical(n_c, true);
                n_c - s.base_count(n_c)
            });
            (self.edit.readout(mol), alt)
        };
        let (n_c, mol_circular) = {
            let mol = self
                .document
                .as_ref()
                .expect("checked by caller")
                .molecule();
            (mol.len(), mol.topology.is_circular())
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(mol_line)
                    .monospace()
                    .size(11.5)
                    .color(pal(ui).ink2),
            );
            // The explicit way to flip a bit that Shift+click cannot infer,
            // because a click has no direction of travel. Both lengths are on
            // screen so the choice is visible, and there is no shortest-arc
            // heuristic: a tool that silently picks the 60 bp arc because it is
            // shorter will one day silently pick the 4,921 bp one.
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
                    // Flipped and stored raw. Canonical form is what the op
                    // derivation and the painter ask for, and it is lossy about
                    // the direction of travel this button exists to state.
                    if let Some(s) = &mut self.edit.sel {
                        s.through_origin = !s.through_origin;
                    }
                }
            }

            // Design lives beside the readout because that is where the
            // selection is already described: the panel's target line repeats
            // these numbers rather than deriving its own.
            let has_sel = self
                .edit
                .sel
                .is_some_and(|s| !s.canonical(n_c, mol_circular).is_empty(n_c));
            let b = ui.add_enabled(
                has_sel && self.design.is_none(),
                egui::Button::new(RichText::new("Design primers…").size(11.0)),
            );
            if !has_sel {
                b.on_hover_text("Select the region to amplify first.");
            } else if b.clicked() {
                self.open_design();
            }
        });
        if let Some(msg) = self.edit.notice.clone() {
            ui.label(RichText::new(msg).color(pal(ui).warn).size(11.0));
        }
    }

    fn sequence_header(&mut self, ui: &mut Ui, n: u64, rows: usize) {
        let d = self.document.as_ref().expect("checked by caller");
        let pending = self.edit.run().is_some();
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
            if pending {
                // Saying so is not decoration: between keystrokes the enzyme
                // list and the map describe the document *before* this run.
                ui.label(RichText::new("· typing").color(pal(ui).accent).size(11.0));
            }
            if d.molecule().topology.is_circular() {
                ui.label(
                    RichText::new("· circular: row 1 and the last row meet at the origin")
                        .color(pal(ui).muted)
                        .size(11.0),
                );
            }
        });
        ui.add_space(2.0);
    }

    /// Ctrl+C. Returns what belongs on the clipboard.
    ///
    /// Split out of the event arm so it can be tested: the whole of what these
    /// two do wrong is in what they say afterwards, and a body inside a `match`
    /// over `egui::Event` can only be exercised by driving a window.
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

            let caption = if d.molecule().name.is_empty() {
                d.title.as_str()
            } else {
                d.molecule().name.as_str()
            };
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
        let Some(mut panel) = self.design.take() else {
            return;
        };
        let dark = ctx.options(|o| o.theme_preference) != egui::ThemePreference::Light;
        let (seq, dark) = {
            let Some(d) = self.document.as_ref() else {
                return;
            };
            (d.molecule().seq.clone(), dark)
        };
        let keep = design::show(ctx, &mut panel, &seq, dark);

        // Applied after the frame, because `App::edit` needs `&mut self` and
        // the panel is holding a borrow of it during the closure.
        if let Some(i) = panel.add_request.take() {
            if let Some(Ok(r)) = &panel.result {
                if let Some(p) = r.pairs.get(i) {
                    let stem = design::stem_of(&panel.title);
                    let fs = design::features(p, &stem, i + 1);
                    let names: Vec<String> = fs.iter().map(|f| f.name.clone()).collect();
                    for f in fs {
                        self.edit(pl_core::OpKind::SetFeature {
                            index: None,
                            feature: Box::new(f),
                        });
                    }
                    panel.added.push(i);
                    // Two operations, so one Ctrl+Z leaves half a pair in the
                    // document. There is no OpKind that adds two features and
                    // inventing one is out of scope, so this gets a sentence
                    // rather than a shrug: a silent half-state is what ADR-2
                    // exists to prevent.
                    self.status = format!(
                        "added 2 primer_bind features, {} and {} - Ctrl+Z twice to undo both",
                        names[0], names[1]
                    );
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

fn strand_glyph(s: Strand) -> &'static str {
    match s {
        Strand::Forward => "→",
        Strand::Reverse => "←",
        Strand::Both => "↔",
        Strand::Unoriented => "·",
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
            let more = if positions.len() > 4 { "…" } else { "" };
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
}
