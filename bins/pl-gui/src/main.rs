//! Polylinker — a plasmid viewer that runs offline and asks nothing of anyone.
//!
//! Everything it decides about a molecule it asks `pl-core`, `pl-fileio` and
//! `pl-enzymes`, the same crates behind the `pl` command line and the browser
//! build. This binary is presentation.

// A console window alongside the app on Windows is noise for a GUI, but keep it
// in debug builds so panics and eprintln stay visible while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod doc;
mod map;
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
    Enzymes,
    Sequence,
    History,
    File,
}

struct App {
    document: Option<Document>,
    error: Option<String>,
    tab: Tab,
    selected: Option<usize>,
    /// Feature under the pointer, from either the map or the list.
    hot: Option<usize>,
    filter: String,
    status: String,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Styles are per-theme in egui 0.35, so adjust both rather than
        // stamping one over the user's light/dark preference.
        cc.egui_ctx.all_styles_mut(|style| {
            theme::apply(&mut style.visuals);
            style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        });

        let mut app = App {
            document: None,
            error: None,
            tab: Tab::Features,
            selected: None,
            hot: None,
            filter: String::new(),
            status: String::new(),
        };
        // Opening a file named on the command line makes the app usable as a
        // file association and from a terminal.
        if let Some(arg) = std::env::args_os().nth(1) {
            app.load(PathBuf::from(arg));
        }
        app
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
                self.document = Some(d);
                self.error = None;
                self.selected = None;
                self.hot = None;
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
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if debug_geometry() {
            eprintln!(
                "geometry: root={:?} clip={:?} ppp={}",
                ui.max_rect(),
                ui.clip_rect(),
                ctx.pixels_per_point()
            );
        }

        // Files dropped anywhere on the window.
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(f) = dropped.first() {
            if let Some(path) = &f.path {
                self.load(path.clone());
            } else if let Some(bytes) = &f.bytes {
                match Document::from_bytes(bytes, f.name.clone(), None) {
                    Ok(d) => {
                        self.status = describe(d.molecule(), d.format);
                        self.document = Some(d);
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }

        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
            self.pick_file();
        }
        // Ctrl+Z / Ctrl+Y, plus Ctrl+Shift+Z for the mac-shaped habit.
        let (undo, redo) = ctx.input(|i| {
            let cmd = i.modifiers.command;
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
        if running {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
        }

        self.hot = None;
        self.top_bar(ui);
        self.side_panel(ui);
        self.central(ui);
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

    /// Run an edit and report a refusal instead of dropping it.
    fn edit(&mut self, kind: pl_core::OpKind) {
        let Some(d) = &mut self.document else { return };
        let what = kind.describe();
        match d.apply(kind) {
            Ok(()) => {
                self.status = format!("{what} — Ctrl+Z to undo");
                self.error = None;
            }
            // The log refuses an edit that would leave the annotations
            // describing something the sequence does not contain. Saying which
            // edit and why is the whole point of refusing rather than
            // corrupting.
            Err(e) => self.error = Some(format!("cannot {what}: {e}")),
        }
    }

    fn do_undo(&mut self) {
        if let Some(d) = &mut self.document {
            match d.undo() {
                Ok(()) => {
                    self.status = "undone".into();
                    self.error = None;
                }
                Err(e) => self.error = Some(e.to_string()),
            }
            self.selected = None;
        }
    }

    fn do_redo(&mut self) {
        if let Some(d) = &mut self.document {
            match d.redo() {
                Ok(()) => {
                    self.status = "redone".into();
                    self.error = None;
                }
                Err(e) => self.error = Some(e.to_string()),
            }
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
                    Tab::Enzymes => self.enzymes_tab(ui),
                    Tab::Sequence => self.sequence_tab(ui),
                    Tab::History => self.history_tab(ui),
                    Tab::File => self.file_tab(ui),
                }
            });
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
            DigestState::Running(_) => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(RichText::new("scanning…").color(pal(ui).muted));
                });
                return;
            }
            DigestState::Done(_) => {}
        }

        let uniq: Vec<_> = d.unique_cutters().collect();
        let multi: Vec<_> = d.cutters().filter(|x| !x.is_unique_cutter()).collect();
        let total = d.digest.results().len();
        let non = total - uniq.len() - multi.len();

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(RichText::new(format!("{} unique cutters", uniq.len())).strong());
            ui.add_space(2.0);
            for e in &uniq {
                enzyme_row(ui, e.enzyme.name, e.enzyme.site, &e.positions, true);
            }
            ui.add_space(10.0);
            ui.label(
                RichText::new(format!("{} cut more than once", multi.len())).color(pal(ui).muted),
            );
            ui.add_space(2.0);
            for e in &multi {
                enzyme_row(ui, e.enzyme.name, e.enzyme.site, &e.positions, false);
            }
            ui.add_space(10.0);
            ui.label(
                RichText::new(format!("{non} of {total} do not cut"))
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

    fn sequence_tab(&mut self, ui: &mut Ui) {
        let d = self.document.as_ref().expect("checked by caller");
        let seq = &d.molecule().seq;
        if seq.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new("This file carries no bases.").color(pal(ui).muted));
            return;
        }

        const PER_ROW: usize = 60;
        let rows = seq.len().div_ceil(PER_ROW);
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace);

        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "{} bp · {} rows · case preserved",
                fmt_int(seq.len() as u64),
                fmt_int(rows as u64)
            ))
            .color(pal(ui).muted)
            .size(11.0),
        );
        ui.add_space(2.0);

        // Only the visible rows are laid out: a 4.6 Mb genome is 77,000 rows and
        // building them all would stall for seconds.
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_h, rows, |ui, range| {
                for r in range {
                    let start = r * PER_ROW;
                    let end = (start + PER_ROW).min(seq.len());
                    let line = String::from_utf8_lossy(&seq[start..end]).into_owned();
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{:>9}", fmt_int(start as u64 + 1)))
                                .monospace()
                                .size(11.0)
                                .color(pal(ui).muted),
                        );
                        ui.label(RichText::new(line).monospace().size(11.5));
                    });
                }
            });
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
                            for (k, v) in &m.notes {
                                ui.label(RichText::new(k).color(pal(ui).muted).size(11.0));
                                ui.label(RichText::new(v).size(11.0));
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

        egui::CentralPanel::default().show(ui, |ui| {
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
                        RichText::new(
                            "Drop a .dna, GenBank or FASTA file here\n\n\
                             Nothing leaves this machine.",
                        )
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
}

fn strand_glyph(s: Strand) -> &'static str {
    match s {
        Strand::Forward => "→",
        Strand::Reverse => "←",
        Strand::Both => "↔",
        Strand::Unoriented => "·",
    }
}

fn enzyme_row(ui: &mut Ui, name: &str, site: &str, positions: &[u64], unique: bool) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{name:<9}"))
                .monospace()
                .size(11.5)
                .color(if unique { pal(ui).ink } else { pal(ui).ink2 }),
        );
        ui.label(
            RichText::new(site)
                .monospace()
                .size(11.0)
                .color(pal(ui).muted),
        );
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
}
