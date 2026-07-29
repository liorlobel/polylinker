//! The Design primers panel.
//!
//! # What it is handed, and the one line that could be quietly wrong
//!
//! "A defined sequence selected by the user" is [`crate::seqedit::Selection`],
//! and the conversion to a [`pl_design::Region`] is the most dangerous line in
//! the feature:
//!
//! ```text
//! let sel = sel.canonical(n, circular);   // NOT clamped(), NOT (lo, hi)
//! ```
//!
//! `Selection` carries `through_origin`, and `seqedit.rs` says exactly why: a
//! pair of carets on a circle names two arcs, not one — `(40, 4961)` on a
//! 5,386 bp plasmid is either the 4,921 bases between them or the 465 across
//! the origin, and no *ordering* of the pair distinguishes them. Reading
//! `(lo, hi)` off a through-origin selection designs for the **complement
//! arc**: a 4,921 bp amplicon where 465 bp was selected. Every number in that
//! report would look reasonable.
//!
//! So the target is derived with the same expression `SeqEdit::readout` prints
//! — the panel's numbers are literally the numbers already on screen, not
//! re-derived — and `through_origin` is *read*, never inferred from the
//! ordering. `canonical()`'s own warning applies: the result is consumed here
//! and never stored back.
//!
//! # It does not track the selection afterwards
//!
//! The target is snapshotted when the panel opens. Precedent and reason are
//! the paste dialog's: `egui::Window` is not modal, the document stays live
//! underneath, and a question asked about one selection must not be answered
//! about another.
//!
//! # Adding to the document
//!
//! Two `OpKind::SetFeature` operations through `App::edit`, so the result lands
//! in the append-only log, inside the WouldCorrupt gate, and inside undo.
//! Nothing writes `Molecule::features` directly.
//!
//! Deliberately **not** `Molecule::primers`, which is the obvious-looking home:
//! no `OpKind` writes that field, so it would bypass the log entirely, and
//! `genbank::write` silently drops a `primers[].sites` entry whose `end <
//! start` — the origin-crossing case recorded in `docs/AUDIT-2026-07-28.md` —
//! while a `Feature` segment at identical coordinates is written correctly as a
//! `join(...)`. Designing a primer across the origin of a plasmid is ordinary.
//! Losing it on export is not.

use eframe::egui::{self, RichText, Ui};
use pl_design::{Constraints, Mode, Region, Report};

use crate::seqedit::Selection;

/// The largest template the panel will scan synchronously.
///
/// A refusal rather than a freeze. The off-target scan is O(candidates ×
/// template) and this window has no worker thread; a GUI that stops responding
/// for a minute is worse than one that says no and names the alternative.
pub const GUI_TEMPLATE_LIMIT: u64 = 200_000;

#[derive(Debug)]
pub struct Panel {
    pub title: String,
    pub bp: u64,
    pub circular: bool,
    /// 1-based inclusive, `end < start` when it wraps. Snapshotted.
    pub target: Region,
    pub target_bp: u64,
    pub c: Constraints,
    pub rt: bool,
    /// Index into `pl_enzymes::ENZYMES`, or `None`.
    pub tail5: Option<usize>,
    pub tail3: Option<usize>,
    pub spacer: String,
    pub result: Option<Result<Report, String>>,
    pub expanded: Option<usize>,
    /// Which reported pairs have already been written to the document.
    pub added: Vec<usize>,
    /// A pair the user asked to add, picked up by `App` after the frame.
    pub add_request: Option<usize>,
    pub close: bool,
}

impl Panel {
    /// Snapshot a selection, or say why there is nothing to design against.
    pub fn open(title: String, bp: u64, circular: bool, sel: Selection) -> Result<Panel, String> {
        let sel = sel.canonical(bp, circular);
        let target_bp = sel.base_count(bp);
        if target_bp == 0 {
            return Err("Select the region to amplify first.".into());
        }
        // The same expression `SeqEdit::readout` uses, so the panel's numbers
        // are the ones already on screen.
        let (a, b) = if sel.through_origin {
            (sel.hi() + 1, sel.lo())
        } else {
            (sel.lo() + 1, sel.hi())
        };
        Ok(Panel {
            title,
            bp,
            circular,
            target: Region::new(a, b),
            target_bp,
            c: Constraints::default(),
            rt: false,
            tail5: None,
            tail3: None,
            spacer: String::new(),
            result: None,
            expanded: None,
            added: Vec::new(),
            add_request: None,
            close: false,
        })
    }

    /// The constraints as the controls currently read them.
    fn constraints(&self) -> Result<Constraints, String> {
        let mut c = if self.rt {
            Constraints::default().rt_pcr()
        } else {
            Constraints::default()
        };
        // Carry across the fields the panel edits directly. Rebuilding from
        // the preset each time is what makes ticking and unticking RT-PCR
        // behave: a preset that only ever loosened would leave the qPCR product
        // window behind after the box was cleared.
        c.mode = self.c.mode;
        c.flank = self.c.flank;
        c.len_min = self.c.len_min;
        c.len_max = self.c.len_max;
        c.tm_min = self.c.tm_min;
        c.tm_max = self.c.tm_max;
        c.tm_opt = (c.tm_min + c.tm_max) / 2.0;
        c.tm_diff_max = self.c.tm_diff_max;
        c.product_min = self.c.product_min;
        c.product_max = self.c.product_max;
        c.max_pairs = self.c.max_pairs;
        c.specificity = self.c.specificity;
        if self.rt {
            c.mode = Mode::Within;
        }

        let spacer = self.spacer.trim().to_ascii_uppercase().into_bytes();
        if let Some(b) = spacer
            .iter()
            .find(|b| !matches!(b, b'A' | b'C' | b'G' | b'T'))
        {
            return Err(format!(
                "The spacer contains '{}', which is not a DNA base. A tail is real DNA \
                 that has to be ordered.",
                *b as char
            ));
        }
        c.tail_five = self.tail5.map(|i| pl_design::params::Tailspec {
            enzyme: &pl_enzymes::ENZYMES[i],
            spacer: spacer.clone(),
        });
        c.tail_three = self.tail3.map(|i| pl_design::params::Tailspec {
            enzyme: &pl_enzymes::ENZYMES[i],
            spacer,
        });
        Ok(c)
    }

    pub fn run(&mut self, seq: &[u8]) {
        if self.bp > GUI_TEMPLATE_LIMIT {
            self.result = Some(Err(format!(
                // The measured cost and the knobs, rather than "run it
                // elsewhere". `pl design` prints the same figures before it
                // starts scanning, so both surfaces say one thing.
                "This template is {} bp. The off-target scan is O(candidates x template) \
                 and this window has no worker thread, so it would stop responding -- \
                 measured on random sequence, about 3 s at 500 kb, 17 s at 1 Mb, 65 s at \
                 2 Mb. Run `pl design` on the command line, where the same estimate is \
                 printed before the scan starts; --no-specificity skips it and says so in \
                 the report, and a smaller flank or a narrower region enumerates fewer \
                 candidates.",
                crate::fmt_int(self.bp)
            )));
            return;
        }
        match self.constraints() {
            Err(e) => self.result = Some(Err(e)),
            Ok(c) => {
                self.c = c.clone();
                self.result = Some(
                    pl_design::design(seq, self.circular, self.target, &c)
                        .map_err(|e| e.to_string()),
                );
                self.expanded = Some(0);
                self.added.clear();
            }
        }
    }

    /// The `pl design` line that reproduces this panel.
    ///
    /// A reproducibility claim you cannot re-run is not one.
    pub fn command(&self) -> String {
        let mut s = format!(
            "pl design {} --region {}..{} --mode {} --flank {} --len {}..{} \
             --tm {:.0}..{:.0} --tm-diff {:.0} --product {}..{} --max {}",
            self.title,
            self.target.start,
            self.target.end,
            self.c.mode.as_str(),
            self.c.flank,
            self.c.len_min,
            self.c.len_max,
            self.c.tm_min,
            self.c.tm_max,
            self.c.tm_diff_max,
            self.c.product_min,
            self.c.product_max,
            self.c.max_pairs
        );
        if self.rt {
            s.push_str(" --rt");
        }
        if !self.c.specificity {
            s.push_str(" --no-specificity");
        }
        if let Some(i) = self.tail5 {
            s.push_str(&format!(" --add-5 {}", pl_enzymes::ENZYMES[i].name));
        }
        if let Some(i) = self.tail3 {
            s.push_str(&format!(" --add-3 {}", pl_enzymes::ENZYMES[i].name));
        }
        if !self.spacer.trim().is_empty() {
            s.push_str(&format!(" --spacer {}", self.spacer.trim()));
        }
        s
    }
}

/// The two features a chosen pair becomes.
///
/// One segment each, over the **footprint only**. The tail has no coordinates
/// on this molecule; a segment covering `start - tail_len` would annotate bases
/// the primer does not match, and every later edit would remap it as though it
/// did. That is the Tm conflation committed to the file.
///
/// A footprint crossing the origin is stored `end < start`, which `Molecule`
/// declares valid and which `format_location` expands to `join(...)` on export.
pub fn features(pair: &pl_design::Pair, stem: &str, rank: usize) -> Vec<pl_core::Feature> {
    let mut out = Vec::new();
    for p in [&pair.forward, &pair.reverse] {
        let mut f = pl_core::Feature::new(p.name(stem), "primer_bind");
        f.strand = match p.side {
            pl_design::Side::Fwd => pl_core::Strand::Forward,
            pl_design::Side::Rev => pl_core::Strand::Reverse,
        };
        f.segments.push(pl_core::Segment::new(p.start, p.end));
        f.set_qualifier(
            "note",
            format!(
                "Polylinker {} design, rank {rank}, penalty {:.2}",
                env!("CARGO_PKG_VERSION"),
                pair.penalty
            ),
        );
        f.set_qualifier(
            "note",
            format!(
                "Tm {:.1} C over the {} nt annealed footprint only; {}",
                p.tm,
                p.footprint.len(),
                Constraints::default().tm_method.describe()
            ),
        );
        f.set_qualifier("note", format!("GC {:.1}%", p.gc));
        if let Some(t) = &p.tail {
            let oligo = p.oligo();
            f.set_qualifier(
                "note",
                format!(
                    "5' tail {} adds {}; it does not pair with the template and is not in \
                     the Tm above. Whole oligo: {}, {} nt{}",
                    String::from_utf8_lossy(&t.bases).to_lowercase(),
                    t.enzyme.name,
                    String::from_utf8_lossy(&oligo),
                    oligo.len(),
                    match p.tm_full {
                        Some(x) => format!(", Tm {x:.1} C from cycle 3"),
                        None => String::new(),
                    }
                ),
            );
        }
        f.set_qualifier("standard_name", p.name(stem));
        out.push(f);
    }
    out
}

/// Draw the panel. Returns false when it should close.
pub fn show(ctx: &egui::Context, panel: &mut Panel, seq: &[u8], dark: bool) -> bool {
    let mut open = true;
    egui::Window::new("Design primers")
        .collapsible(false)
        .resizable(true)
        .default_width(720.0)
        .open(&mut open)
        .show(ctx, |ui| body(ui, panel, seq, dark));
    open && !panel.close
}

fn body(ui: &mut Ui, panel: &mut Panel, seq: &[u8], dark: bool) {
    let pal = crate::theme::Palette::of(dark);
    ui.label(
        RichText::new(format!(
            "{} - {} bp {} - target {}..{}, {} bp{}",
            panel.title,
            crate::fmt_int(panel.bp),
            if panel.circular { "circular" } else { "linear" },
            panel.target.start,
            panel.target.end,
            crate::fmt_int(panel.target_bp),
            if panel.target.wraps() {
                " - crosses the origin"
            } else {
                ""
            }
        ))
        .monospace()
        .size(11.5)
        .color(pal.ink2),
    );
    ui.add_space(6.0);

    egui::Grid::new("pl-design-constraints")
        .num_columns(4)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            ui.label("Mode");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut panel.c.mode, Mode::Contain, "contain")
                    .on_hover_text(
                        "The product contains the whole selection; a primer may begin \
                         outside it. What cloning an ORF wants.",
                    );
                ui.selectable_value(&mut panel.c.mode, Mode::Within, "within")
                    .on_hover_text("Both primers lie inside the selection. qPCR, screening.");
            });
            ui.label("Flank");
            ui.add(egui::DragValue::new(&mut panel.c.flank).range(0..=5_000))
                .on_hover_text(
                    "How far outside the selection a primer's OUTER end may sit. 0 pins \
                     both outer ends exactly to the selection.",
                );
            ui.end_row();

            ui.label("Length");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut panel.c.len_min).range(8..=60));
                ui.label("to");
                ui.add(egui::DragValue::new(&mut panel.c.len_max).range(8..=60));
            });
            ui.label("Product bp");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut panel.c.product_min).range(20..=100_000));
                ui.label("to");
                ui.add(egui::DragValue::new(&mut panel.c.product_max).range(20..=100_000));
            });
            ui.end_row();

            ui.label("Tm")
                .on_hover_text(Constraints::default().tm_method.describe());
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut panel.c.tm_min).range(0.0..=110.0));
                ui.label("to");
                ui.add(egui::DragValue::new(&mut panel.c.tm_max).range(0.0..=110.0));
            });
            ui.label("Max dTm");
            ui.add(egui::DragValue::new(&mut panel.c.tm_diff_max).range(0.0..=30.0));
            ui.end_row();

            ui.label("Pairs");
            ui.add(egui::DragValue::new(&mut panel.c.max_pairs).range(1..=50));
            ui.label("Off-target scan");
            ui.checkbox(&mut panel.c.specificity, "against this molecule")
                .on_hover_text(
                    "Rejects any primer that also anneals somewhere else on this \
                     molecule. It says nothing about a host genome.",
                );
            ui.end_row();

            ui.label("5' tails");
            ui.horizontal(|ui| {
                enzyme_picker(ui, "pl-design-t5", &mut panel.tail5, "forward");
                enzyme_picker(ui, "pl-design-t3", &mut panel.tail3, "reverse");
            });
            ui.label("Spacer");
            ui.add(egui::TextEdit::singleline(&mut panel.spacer).desired_width(120.0))
                .on_hover_text(
                    "Bases 5' of the added site. None by default; many enzymes cut a site \
                     at the very end of a fragment poorly.",
                );
            ui.end_row();
        });

    ui.add_space(4.0);
    ui.checkbox(&mut panel.rt, "RT-PCR / qPCR preset");
    if panel.rt {
        // Persistent and non-dismissible, and still on screen when the user
        // clicks Add. Not a tooltip and not a toast: this is the one thing
        // about the feature that cannot be softened.
        ui.label(
            RichText::new(pl_design::report::RT_PCR_CAVEAT)
                .color(pal.warn)
                .size(11.0),
        );
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("Design").clicked() {
            panel.run(seq);
        }
        if panel.result.is_some() && ui.button("Copy the command").clicked() {
            ui.ctx().copy_text(panel.command());
        }
    });

    ui.add_space(6.0);
    // Cloned rather than borrowed: `results` needs `&mut panel` to record an
    // expansion or an add request, and the report is the thing being drawn.
    match panel.result.clone() {
        None => {
            ui.label(RichText::new("Press Design.").color(pal.muted).size(11.0));
        }
        Some(Err(e)) => {
            ui.label(RichText::new(e).color(pal.warn).size(11.5));
        }
        Some(Ok(r)) => results(ui, panel, &r, &pal),
    }
}

fn enzyme_picker(ui: &mut Ui, id: &str, slot: &mut Option<usize>, which: &str) {
    let label = match slot {
        Some(i) => format!(
            "{} {}",
            pl_enzymes::ENZYMES[*i].name,
            pl_enzymes::ENZYMES[*i].site
        ),
        None => format!("{which}: none"),
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(label)
        .width(190.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(slot, None, format!("{which}: none"));
            for (i, e) in pl_enzymes::ENZYMES.iter().enumerate() {
                ui.selectable_value(slot, Some(i), format!("{} {}", e.name, e.site));
            }
        });
}

fn results(ui: &mut Ui, panel: &mut Panel, r: &Report, pal: &crate::theme::Palette) {
    ui.label(
        RichText::new(format!(
            "{} candidates, {} forward and {} reverse survived, {} pairs built, {} shown",
            crate::fmt_int(r.enumerated as u64),
            r.survivors_forward,
            r.survivors_reverse,
            crate::fmt_int(r.pairs_built as u64),
            r.pairs.len()
        ))
        .color(pal.muted)
        .size(11.0),
    );
    ui.add_space(4.0);

    egui::ScrollArea::vertical()
        .max_height(320.0)
        .show(ui, |ui| {
            for (i, p) in r.pairs.iter().enumerate() {
                let open = panel.expanded == Some(i);
                let head = format!(
                    "{:>2}   penalty {:.2}   {} bp   F {:.1}C   R {:.1}C   dTm {:.1}",
                    i + 1,
                    p.penalty,
                    crate::fmt_int(p.product_bp),
                    p.forward.tm,
                    p.reverse.tm,
                    p.delta_tm
                );
                if ui
                    .selectable_label(open, RichText::new(head).monospace().size(11.5))
                    .clicked()
                {
                    panel.expanded = if open { None } else { Some(i) };
                }
                if !open {
                    continue;
                }
                ui.indent(("pl-design-pair", i), |ui| {
                    for pr in [&p.forward, &p.reverse] {
                        ui.horizontal_wrapped(|ui| {
                            // Two channels, not one: case AND colour. Colour
                            // alone does not survive a greyscale screenshot,
                            // and case alone does not survive a glance.
                            if let Some(t) = &pr.tail {
                                ui.label(
                                    RichText::new(String::from_utf8_lossy(&t.bases).to_lowercase())
                                        .monospace()
                                        .size(11.5)
                                        .color(pal.muted),
                                );
                            }
                            ui.label(
                                RichText::new(
                                    String::from_utf8_lossy(&pr.footprint).to_uppercase(),
                                )
                                .monospace()
                                .size(11.5)
                                .color(pal.ink),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{}..{} ({})  {:.1}C  {:.1}% GC",
                                    pr.start,
                                    pr.end,
                                    pr.side.as_str(),
                                    pr.tm,
                                    pr.gc
                                ))
                                .size(11.0)
                                .color(pal.ink2),
                            );
                        });
                    }
                    for w in &p.warnings {
                        ui.label(RichText::new(w).color(pal.warn).size(10.5));
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .button("Copy both")
                            .on_hover_text("Two lines, name<TAB>oligo, the whole oligo")
                            .clicked()
                        {
                            let stem = stem_of(&panel.title);
                            let text = format!(
                                "{}\t{}\n{}\t{}\n",
                                p.forward.name(&stem),
                                String::from_utf8_lossy(&p.forward.oligo()),
                                p.reverse.name(&stem),
                                String::from_utf8_lossy(&p.reverse.oligo())
                            );
                            ui.ctx().copy_text(text);
                        }
                        let already = panel.added.contains(&i);
                        let b = ui.add_enabled(!already, egui::Button::new("Add to document"));
                        if already {
                            b.on_hover_text("already added");
                        } else if b.clicked() {
                            panel.add_request = Some(i);
                        }
                    });
                });
            }
        });

    ui.add_space(6.0);
    for w in &r.warnings {
        ui.label(RichText::new(w).color(pal.warn).size(10.5));
    }
}

/// A file name reduced to something that can go on a tube.
pub fn stem_of(title: &str) -> String {
    let base = title.rsplit(['/', '\\']).next().unwrap_or(title);
    let base = base.split('.').next().unwrap_or(base);
    let s: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if s.is_empty() {
        "primer".into()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seqedit::Selection;

    /// PROVEN TO FAIL: with the conversion reduced to `(sel.lo() + 1,
    /// sel.hi())` - dropping `through_origin`, which is the whole trap - the
    /// 465 bp selection comes back as 4,921 bp, the complement arc.
    #[test]
    fn a_through_origin_selection_becomes_the_arc_the_user_highlighted() {
        // The trap `seqedit.rs` documents, at the one boundary where it bites.
        // Carets at 40 and 4,961 on a 5,386 bp plasmid name two arcs: the 4,921
        // bases between them, or the 465 across the origin. Reading (lo, hi)
        // gives the first whichever the user meant.
        let n = 5_386;
        let across = Selection {
            anchor: 4_961,
            head: 40,
            through_origin: true,
        };
        let p = Panel::open("pUC19".into(), n, true, across).unwrap();
        assert_eq!(p.target_bp, 465, "the arc the user highlighted");
        assert_eq!(p.target, Region::new(4_962, 40));
        assert!(p.target.wraps());
        // The round trip the conversion has to preserve.
        assert_eq!(p.target.len(n), across.canonical(n, true).base_count(n));

        // The same carets, the other arc.
        let between = Selection {
            anchor: 4_961,
            head: 40,
            through_origin: false,
        };
        let q = Panel::open("pUC19".into(), n, true, between).unwrap();
        assert_eq!(q.target_bp, 4_921);
        assert_eq!(q.target, Region::new(41, 4_961));
        assert!(!q.target.wraps());
        assert_ne!(p.target, q.target, "the bit has to change the answer");
    }

    #[test]
    fn an_empty_selection_is_refused_before_anything_is_designed() {
        let e = Panel::open("x".into(), 100, false, Selection::point(7)).unwrap_err();
        assert!(e.contains("Select the region"), "{e}");
    }

    /// PROVEN TO FAIL: with the segment extended back over the tail - the
    /// natural-looking `start - tail_len`, since the oligo really is that long -
    /// the segment length no longer equals the footprint's, and the file would
    /// then claim 14 bases of the template that the primer does not match.
    #[test]
    fn a_feature_covers_the_footprint_and_never_the_tail() {
        // The coordinate half of the footprint/tail split. A segment covering
        // the tail would annotate bases the primer does not match, and every
        // later edit would remap it as though it did.
        let template: Vec<u8> = (0..3_000u32)
            .scan(9u64, |s, _| {
                *s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                Some(b"ACGT"[((*s >> 24) & 3) as usize])
            })
            .collect();
        let c = Constraints {
            tail_five: Some(pl_design::params::Tailspec {
                enzyme: pl_enzymes::by_name("EcoRI").unwrap(),
                spacer: b"GCGGCCGC".to_vec(),
            }),
            ..Default::default()
        };
        let r = pl_design::design(&template, false, Region::new(1_001, 1_400), &c).unwrap();
        let p = &r.pairs[0];
        let fs = features(p, "demo", 1);
        assert_eq!(fs.len(), 2);
        assert_eq!(fs[0].kind, "primer_bind");
        assert_eq!(fs[0].segments.len(), 1);
        assert_eq!(fs[0].segments[0].start, p.forward.start);
        assert_eq!(fs[0].segments[0].end, p.forward.end);
        assert_eq!(
            fs[0].segments[0].len() as usize,
            p.forward.footprint.len(),
            "the segment is the footprint, not the oligo"
        );
        assert!(p.forward.oligo().len() > p.forward.footprint.len());

        // The tail is recoverable from the file, as text, with no coordinate
        // claiming it exists on the template.
        let notes: Vec<&str> = fs[0]
            .qualifiers
            .iter()
            .filter(|(k, _)| k == "note")
            .filter_map(|(_, v)| v.as_deref())
            .collect();
        let joined = notes.join(" | ");
        assert!(joined.contains("gcggccgcgaattc"), "{joined}");
        assert!(joined.contains("Whole oligo"), "{joined}");
        assert!(joined.contains("not in the Tm above"), "{joined}");
        assert!(joined.contains("footprint only"), "{joined}");
        assert_eq!(fs[1].strand, pl_core::Strand::Reverse);
    }

    #[test]
    fn the_command_reproduces_the_panel() {
        let p = Panel::open(
            "pUC19.gb".into(),
            5_386,
            true,
            Selection {
                anchor: 100,
                head: 600,
                through_origin: false,
            },
        )
        .unwrap();
        let cmd = p.command();
        assert!(
            cmd.starts_with("pl design pUC19.gb --region 101..600"),
            "{cmd}"
        );
        assert!(cmd.contains("--mode contain"), "{cmd}");
    }

    #[test]
    fn a_name_that_goes_on_a_tube() {
        assert_eq!(stem_of("C:/lab/pUC19-myGene.gb"), "pUC19_myGene");
        assert_eq!(stem_of("plain"), "plain");
        assert_eq!(stem_of(".gb"), "primer");
    }
}
