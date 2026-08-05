//! Restriction cloning: cut the open molecule, see what religates, open the
//! product.
//!
//! The entry point `pl-clone` never had. The crate could cut (`try_cut`), could
//! say whether two ends seal (`End::ligates_with`) and, since the ligation
//! engine landed, could build the product — and `bins/pl-gui` did not depend on
//! it, so none of that was reachable by clicking anything. A user could digest a
//! plasmid in the app and could not put it back together.
//!
//! # What this carries across, and what it refuses to guess
//!
//! A product is only useful if it keeps the features. `try_cut` returns
//! `Dseq`s with no provenance — they are sequence and ends, not "bases 400 to
//! 1,400 of pKoV" — so the parent interval each fragment came from is
//! RECOVERED here by matching the fragment against the parent, and a feature
//! travels only when its whole span sits inside one identified fragment.
//!
//! Where a fragment cannot be placed unambiguously, nothing from it is carried
//! and the count says so. A feature put at the wrong coordinate in a construct
//! somebody then orders primers against is worse than a feature that is simply
//! absent, and the absence is visible while the error is not.

use std::collections::BTreeSet;

use eframe::egui;
use pl_core::{Feature, Molecule, Segment, Strand, Topology};

/// One fragment of a digest, with where it came from when that is knowable.
pub struct Frag {
    pub len: usize,
    pub left: String,
    pub right: String,
    /// 1-based inclusive span in the parent, when the fragment could be placed.
    pub from: Option<(u64, u64)>,
    /// Why `from` is `None`, when the reason is knowable.
    ///
    /// For a digest fragment this stays `None` and the panel says what it always
    /// said — the bases matched in more than one place. For an AMPLICON that is
    /// not what happened: `pcr` has already refused any pair whose 12 nt seed
    /// binds twice, so a product it returns occurs at most once. The reason is
    /// that the product is not a stretch of the template at all, and a 5' tail
    /// does exactly that — forward `GAATTC` plus 20 nt of pUC19 amplifies a
    /// product carrying an EcoRI site pUC19 does not have, which is the entire
    /// reason anyone puts a tail on a primer. Saying "could not be placed in the
    /// parent" there tells a user their file is ambiguous when it is not.
    pub unplaced: Option<String>,
    /// Which digest this came out of: 0 the open molecule, 1 the donor.
    ///
    /// Stage 4. With one molecule the answer was always 0 and the field would
    /// have been noise; with two it is the difference between carrying the
    /// vector's features and carrying the insert's, and getting it wrong puts a
    /// resistance marker's name on somebody's gene.
    pub parent: usize,
}

/// One molecule the digest can be religated into.
pub struct Prod {
    pub mol: Molecule,
    pub circular: bool,
    /// Fragment indices in the order used, and whether each was flipped.
    pub order: Vec<(usize, bool)>,
    /// Features carried over, and features that could not be.
    pub carried: usize,
    pub dropped: usize,
    /// What was sealed at each junction, already in words.
    ///
    /// A `5' GATC` for a ligation and a `30 bp homology` for an assembly — two
    /// different kinds of fact that a report has to state in one sentence, so
    /// they are turned into words where the difference is still known rather
    /// than downstream where it is not.
    pub junctions: Vec<String>,
}

/// The whole answer for one set of enzymes.
pub struct Plan {
    pub frags: Vec<Frag>,
    pub prods: Vec<Prod>,
    /// Why there is nothing to show, when there is nothing to show.
    pub note: Option<String>,
    /// The Golden Gate overhang check, for [`Method::GoldenGate`].
    ///
    /// Separate from `note` because it is not a refusal: a set with a fatal
    /// fault still assembles into SOMETHING, and hiding the products would be
    /// less honest than showing them beside the reason they are not what was
    /// intended.
    pub gg: Option<GgReport>,
    /// The oligos this plan amplified with, for [`Method::Pcr`].
    ///
    /// Carried on the plan rather than passed to [`report`], which already takes
    /// seven arguments — and this is the better home anyway: a methods paragraph
    /// naming a product without naming the two oligos that made it cannot be
    /// repeated by anybody.
    pub pcr: Option<Primers>,
}

/// What the Golden Gate overhang check found.
///
/// A view of `pl_clone::goldengate::Report`, flattened to strings, so the panel
/// does not have to match on `Fault` and the crate does not have to know about
/// a `Ui`. `fatal` travels with each line because the two severities are
/// genuinely different and must not be painted the same: a repeat or a
/// palindrome gives you a DIFFERENT construct, a near neighbour gives you mostly
/// the right one with a wrong minor product.
pub struct GgReport {
    pub overhangs: Vec<String>,
    pub faults: Vec<(String, bool)>,
    pub usable: bool,
    pub caveat: &'static str,
}

/// How the pieces are joined.
///
/// The engine has had all of these since long before anything could reach them.
/// `pl-clone` could cut, could ligate by ends, could assemble by homology and
/// could validate a Golden Gate overhang set — and `bins/pl-gui` offered one of
/// the four, so the rest were code nobody could run by clicking anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Cut with restriction enzymes, join by compatible ends.
    Restriction,
    /// Join by sequence HOMOLOGY: Gibson, HiFi, In-Fusion, SLIC.
    ///
    /// The overlap is designed into the primers rather than left by an enzyme,
    /// so the ends play no part and the enzymes above are optional — they are
    /// there only to linearise a vector.
    Gibson,
    /// Cut with a Type IIS enzyme and ligate the released pieces.
    ///
    /// A restriction cloning by mechanism, and a different operation in
    /// practice: the recognition site leaves with the cut, so the junction
    /// carries no scar and cannot be re-cut, and WHICH pieces join is decided
    /// entirely by four bases the designer chose. That is why this is its own
    /// method rather than a checkbox — the answer a user needs is not "what can
    /// be built" but "will these overhangs build the one thing I meant", and
    /// `pl_clone::goldengate` answers it.
    GoldenGate,
    /// Amplify between two primers.
    ///
    /// Not a joining method at all, and it sits here because it is the step
    /// BETWEEN the two this panel already does: the app could design a primer
    /// pair and could assemble fragments, and could not make the amplicon in
    /// the middle. A user had to leave, run `pl pcr`, and come back.
    Pcr,
}

impl Method {
    pub fn label(self) -> &'static str {
        match self {
            Method::Restriction => "Restriction",
            Method::Gibson => "Gibson / HiFi",
            Method::GoldenGate => "Golden Gate",
            Method::Pcr => "PCR",
        }
    }
}

/// The two oligos a PCR amplifies with, 5'->3', **tails included**.
///
/// A NAMED PAIR and not `(&str, &str)`, and the reason is call sites rather than
/// silence. `plan` is called from about twenty places; two adjacent `&str`
/// parameters there are two things a reader cannot tell apart and a writer can
/// transpose without the compiler noticing.
///
/// MEASURED, because the first draft of this comment claimed something better
/// and untrue — that a swapped pair silently amplifies the complement arc of a
/// circle. It does not: an oligo written as a reverse primer is the reverse
/// complement of its binding site, so as a forward primer it has nothing on the
/// plus strand to bind, and `pcr` answers "does not anneal" on a line and on a
/// circle alike. `the_two_oligos_are_not_interchangeable` pins that. The type
/// earns its place at the call site; the engine is what catches the swap.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Primers {
    pub forward: String,
    pub reverse: String,
}

/// The panel's own state. Outlives a frame; does not outlive its document.
pub struct Panel {
    pub method: Method,
    pub enzymes: BTreeSet<String>,
    pub blunt: bool,
    /// Shortest homology that counts as a junction, for [`Method::Gibson`].
    ///
    /// Exposed because it is a real experimental parameter — a 20 bp HiFi
    /// overlap and a 40 bp Gibson one are both ordinary — and because leaving it
    /// at a hidden 25 would silently report "no assembly" for a design that
    /// works. Bounded below at 12 in the UI: under about 20 a chance match in a
    /// plasmid stops being negligible, and a chance match here does not give a
    /// wrong length, it gives a confidently wrong construct.
    pub homology: usize,
    /// The two oligos, for [`Method::Pcr`]. Seeded from the Design panel's
    /// chosen pair when the panel is opened — see `App::open_clone_panel`.
    pub primers: Primers,
    /// The OTHER tab whose digest supplies the insert, if there is one.
    ///
    /// Stage 4, and the whole of it. `None` religates the open molecule on its
    /// own, which is what this panel did and still does. `Some(tab)` takes one
    /// fragment from each digest and puts them together — subcloning, the
    /// operation the crate is named for and the one nothing could reach.
    ///
    /// A TAB REFERENCE and not a molecule. The panel outlives a frame and must
    /// not outlive its documents, and a `Molecule` copied in here would be a
    /// second copy of a plasmid that the user can go on editing in its own tab:
    /// the plan would then describe a sequence nobody has, silently. A reference
    /// is re-resolved every frame and can be found to be gone, which is a
    /// condition the panel can state.
    ///
    /// A [`crate::bench::TabId`] AND NOT A TAB INDEX, which is what it was.
    /// Resolution below is numeric equality against a freshly enumerated bench,
    /// and the staleness check beside it fires only when the reference resolves
    /// to NOTHING — never when it resolves to something DIFFERENT. A position
    /// can do the second: `Bench::close` is a `Vec::remove` and `Bench::reopen`
    /// pushes at the end, so tabs A,B,C,D,E with this panel on B and the donor
    /// on C become A,C,D,E,B after one Ctrl+W and one Ctrl+Shift+T, and
    /// `donor = Some(2)` then names D. The donor row and the Copy-report text
    /// say D while `plan` still holds C's fragments, and Open builds the
    /// construct from C. Closing any tab to the LEFT of the donor does the same
    /// thing in one keystroke, and does it with the panel untouched now that
    /// `App::close_tab` no longer scatters a default view over the tab on
    /// screen. An id is minted once per tab and travels with it through the
    /// reopen stack, so no reordering can move it.
    pub donor: Option<crate::bench::TabId>,
    /// Recomputed only when the inputs change: `plan` digests and enumerates,
    /// and a redraw is not a reason to do either again.
    pub plan: Option<Plan>,
    /// Set by whoever changes an input this panel OWNS: the method, the enzyme
    /// set, the blunt flag, the homology, the primers, the donor.
    pub stale: bool,
    /// Where the DOCUMENT stood when `plan` was built, compared against the
    /// live cursor every frame — the identity `design::Panel` and
    /// `featedit::Panel` already keep against the same hazard, and the one this
    /// panel did not have.
    ///
    /// `stale` alone cannot carry the document, because this panel is not the
    /// only thing that changes the molecule and it is not modal. `egui::Window`
    /// is not modal; the clone panel is absent from the `designing` predicate
    /// that stands Ctrl+Z down (`App::shortcuts`); and `App::sequence_keys`
    /// returns early for the design panel and the feature editor and not for
    /// this one. So Molecule ▸ Reverse complement, Molecule ▸ Make linear and
    /// every typed base are live behind an open panel. Outside this module
    /// `stale` was set in exactly one place — `after_the_cursor_moved`, which is
    /// undo, redo and a seek — so a FORWARD edit left the fragment lengths, the
    /// junction descriptions and the Copy-report text all describing the
    /// molecule as it was, and Open then adopted `plan.prods[i].mol`: a
    /// construct assembled from bases the document no longer has, with nothing
    /// on screen saying so. `adopt` calls that "the worst thing this program can
    /// produce".
    ///
    /// AN ID AND NOT A FLAG, because there is no one place to set a flag. The
    /// forward routes to the log are three: `App::edit` (the Molecule menu, the
    /// feature editor's Save), `App::settle` (a run of typing becoming one
    /// operation) and `SeqEdit::apply_gesture` (typing over a selection,
    /// Backspace, Delete, Paste) — and the last of those is handed a `Document`
    /// and has no `App` to mark. Every one of them moves `log.cursor()`, so one
    /// comparison at the draw site covers all three and anything added later.
    ///
    /// Content-addressed, like `featedit::Panel::stale_reason`: an edit and its
    /// undo land back on the same id and the plan is not rebuilt for nothing.
    pub plan_at: Option<pl_core::oplog::OpId>,
    /// The same thing for [`Panel::donor`], because a plan has TWO parents.
    ///
    /// [`Panel::plan_at`] guards the ACTIVE document, and the donor is by
    /// construction never the active one — `App::clone_donors` filters that tab
    /// out. So every word of `plan_at`'s reasoning applies to the insert and
    /// none of its code reached it: the user Ctrl+Tabs to the donor, deletes the
    /// bases spanning one of its sites, Ctrl+Tabs back, and the vector's cursor
    /// has not moved. `switch_tab` restores this panel through `put_view` byte
    /// for byte, `stale` false and `plan_at` intact, so the guard does not fire.
    /// `show` re-resolves the donor molecule fresh every frame and then reads it
    /// only INSIDE the branch that never runs, which is what made the staleness
    /// invisible: the "Insert from" row goes on naming the right plasmid because
    /// the title is re-resolved too, while `plan` holds the pre-edit fragments
    /// and Open adopts `plan.prods[i].mol` — a construct assembled from bases
    /// the donor no longer has, saved to GenBank under a name composed from both
    /// parents.
    ///
    /// `None` BOTH when there is no donor and when the donor has no operations
    /// yet, and that ambiguity costs nothing: choosing a donor, changing one and
    /// clearing one all set `stale` where the click happens, and a donor that
    /// vanishes sets it too.
    pub donor_at: Option<pl_core::oplog::OpId>,
    /// Set when the user asks for a product; the caller adopts it and clears it.
    pub wanted: Option<usize>,
}

impl Panel {
    /// Seeded from the enzymes already ticked for the gel, because a user who
    /// has just looked at a digest is asking about THAT digest.
    pub fn new(picked: &BTreeSet<String>, primers: Primers) -> Self {
        Panel {
            method: Method::Restriction,
            enzymes: picked.clone(),
            blunt: false,
            homology: 25,
            primers,
            donor: None,
            plan: None,
            stale: true,
            plan_at: None,
            donor_at: None,
            wanted: None,
        }
    }
}

/// Draw the panel. Returns false when the user has closed it.
///
/// `others` is the rest of the bench — `(tab id, title, molecule, where that
/// molecule stands in its own history)` — so the insert can come from a plasmid
/// the user already has open. Resolved by the caller every frame rather than
/// held here: see [`Panel::donor`], which is also why the first element is an id
/// and not a position. The fourth element is the donor's half of the staleness
/// question and exists for the reason [`Panel::donor_at`] gives.
///
/// `at` is where `mol` stands in its own history, likewise resolved every frame.
/// It is what makes the plan on screen a plan of the molecule on screen: see
/// [`Panel::plan_at`].
pub fn show(
    ctx: &egui::Context,
    p: &mut Panel,
    mol: &Molecule,
    at: Option<pl_core::oplog::OpId>,
    others: &[(
        crate::bench::TabId,
        String,
        &Molecule,
        Option<pl_core::oplog::OpId>,
    )],
    dark: bool,
) -> bool {
    let pal = crate::theme::Palette::of(dark);
    // The donor as it stands THIS frame. A tab that has been closed since the
    // choice was made resolves to nothing, and the panel says so below rather
    // than planning against a molecule that is no longer open. It cannot resolve
    // to a DIFFERENT tab, because the thing compared is an id and not a
    // position — see [`Panel::donor`] for what it cost when it was.
    let donor = p.donor.and_then(|t| others.iter().find(|(i, ..)| *i == t));
    if p.donor.is_some() && donor.is_none() {
        p.donor = None;
        p.stale = true;
    }
    let donor_mol = donor.map(|(_, _, m, _)| *m);
    let donor_at = donor.and_then(|(.., a)| *a);
    // Either the panel's own inputs changed, or one of the two DOCUMENTS did
    // underneath it. The second disjunct is the one that was missing and the
    // third is the one the second left behind; [`Panel::plan_at`] and
    // [`Panel::donor_at`] are the whole of why they are here.
    if p.stale || p.plan_at != at || p.donor_at != donor_at {
        p.plan = Some(plan(
            mol, donor_mol, p.method, &p.enzymes, &p.primers, p.blunt, p.homology,
        ));
        p.plan_at = at;
        p.donor_at = donor_at;
        p.stale = false;
    }
    let mut open = true;
    let mut wanted_report: Option<usize> = None;
    egui::Window::new("Cut and religate")
        .open(&mut open)
        .resizable(true)
        .default_width(560.0)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(
                    "Digest the open molecule, then see which fragments can be ligated back \
                     together. Opening a product makes a new unsaved document.",
                )
                .color(pal.muted)
                .size(11.5),
            );
            ui.add_space(6.0);

            // HOW the pieces are joined, first, because it changes what every
            // control below means: the enzymes are a digest under Restriction
            // and a linearisation under Gibson.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Join by").strong());
                for m in [
                    Method::Restriction,
                    Method::Gibson,
                    Method::GoldenGate,
                    Method::Pcr,
                ] {
                    if ui.selectable_label(p.method == m, m.label()).clicked() && p.method != m {
                        p.method = m;
                        p.stale = true;
                    }
                }
            });
            ui.add_space(4.0);

            // PCR CUTS NOTHING, so the whole enzyme block goes. Leaving it
            // visible would be worse than useless: the set is sticky across a
            // method change — it is seeded from the gel — so a leftover BamHI
            // would sit there looking like it takes part in the reaction.
            if p.method == Method::Pcr {
                primer_boxes(ui, p, mol, pal);
            } else {
            // Enzymes: only the ones that cut, so the list is short and every
            // entry does something.
            ui.label(
                egui::RichText::new(match p.method {
                    Method::Restriction => "Cut with",
                    Method::Gibson => "Linearise with (optional)",
                    // Only the Type IIS enzymes are offered below, so the label
                    // says which family rather than leaving a user to wonder
                    // where BamHI went.
                    Method::GoldenGate => "Cut with (Type IIS only)",
                    Method::Pcr => unreachable!("handled above"),
                })
                .strong(),
            );
            ui.horizontal_wrapped(|ui| {
                for e in pl_enzymes::ENZYMES {
                    // EITHER molecule, once there are two. An enzyme that cuts
                    // only the donor is exactly the one a user reaches for when
                    // the insert has the site and the vector's is somewhere
                    // else — and it was not on this list at all.
                    // Golden Gate needs the recognition site to LEAVE with the
                    // cut — that is the whole of why its junctions carry no
                    // scar — so a Type IIP enzyme cannot do it, and offering
                    // fifty that cannot is how a user concludes the method is
                    // broken.
                    if p.method == Method::GoldenGate && !e.cuts_outside_its_site() {
                        continue;
                    }
                    let cuts = !pl_enzymes::cut_positions(&mol.seq, mol.topology, e).is_empty()
                        || donor_mol.is_some_and(|d| {
                            !pl_enzymes::cut_positions(&d.seq, d.topology, e).is_empty()
                        });
                    if !cuts {
                        continue;
                    }
                    let mut on = p.enzymes.contains(e.name);
                    if ui.checkbox(&mut on, e.name).changed() {
                        if on {
                            p.enzymes.insert(e.name.to_string());
                        } else {
                            p.enzymes.remove(e.name);
                        }
                        p.stale = true;
                    }
                }
            });
            ui.add_space(4.0);
            match p.method {
                Method::Restriction => {
                    if ui.checkbox(&mut p.blunt, "join blunt ends too").changed() {
                        p.stale = true;
                    }
                }
                // Golden Gate has neither knob: the blunt policy is meaningless
                // for a method whose whole point is a chosen four-base overhang,
                // and homology is not how it joins.
                Method::GoldenGate => {}
                Method::Pcr => unreachable!("handled above"),
                Method::Gibson => {
                    ui.horizontal(|ui| {
                        ui.label("homology at least");
                        // Floored at 12 rather than 1. Below about 20 a chance
                        // match in a plasmid stops being negligible, and a
                        // chance match does not give a wrong length — it gives a
                        // confidently wrong construct. 12 is low enough for
                        // anyone who really means it and high enough that the
                        // slider cannot be dragged into nonsense.
                        let r = ui.add(
                            egui::DragValue::new(&mut p.homology)
                                .range(12..=120)
                                .suffix(" bp"),
                        );
                        if r.changed() {
                            p.stale = true;
                        }
                    });
                }
            }
            }

            // THE INSERT, from another tab. Offered only when there is another
            // tab to offer: a control whose menu is always empty teaches a user
            // the feature does not work.
            //
            // Hidden under PCR: a PCR amplifies ONE template, the molecule on
            // screen, and `amplify` ignores the donor rather than quietly
            // repurposing it. A selector that changed nothing would be the worse
            // of the two.
            if !others.is_empty() && p.method != Method::Pcr {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Insert from").strong());
                    let shown = p
                        .donor
                        .and_then(|t| others.iter().find(|(i, ..)| *i == t))
                        .map(|(_, name, ..)| name.clone())
                        .unwrap_or_else(|| "— this molecule only —".to_string());
                    egui::ComboBox::from_id_salt("clone-donor")
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(p.donor.is_none(), "— this molecule only —")
                                .clicked()
                                && p.donor.is_some()
                            {
                                p.donor = None;
                                p.stale = true;
                            }
                            for (i, name, ..) in others {
                                if ui.selectable_label(p.donor == Some(*i), name).clicked()
                                    && p.donor != Some(*i)
                                {
                                    p.donor = Some(*i);
                                    p.stale = true;
                                }
                            }
                        });
                });
                if p.donor.is_some() {
                    ui.label(
                        egui::RichText::new(
                            "Both molecules are cut with the enzymes above, and every construct \
                             that takes one fragment from each is listed. Neither file is \
                             changed.",
                        )
                        .color(pal.muted)
                        .size(11.0),
                    );
                }
            }
            ui.separator();

            let Some(pl) = &p.plan else { return };

            if !pl.frags.is_empty() {
                ui.label(egui::RichText::new(format!("{} fragments", pl.frags.len())).strong());
                for (i, f) in pl.frags.iter().enumerate() {
                    // WHICH MOLECULE, when there is more than one. Without it a
                    // list of eight bands is eight numbers, and the user has to
                    // work out from the lengths which half of it is their
                    // vector — which is the one thing they must not get wrong.
                    let whose = match (donor.map(|(_, n, ..)| n), f.parent) {
                        (None, _) => String::new(),
                        (Some(_), 0) => format!("   {}", short(&mol.name)),
                        (Some(name), _) => format!("   {}", short(name)),
                    };
                    ui.label(
                        egui::RichText::new(format!(
                            "  {}.{}  {} bp   {}  …  {}{}",
                            i + 1,
                            whose,
                            f.len,
                            f.left,
                            f.right,
                            match (f.from, &f.unplaced) {
                                (Some((a, b)), _) => format!("   from {a}..{b}"),
                                // An amplicon knows WHY it could not be placed,
                                // and it is not ambiguity — see `Frag::unplaced`.
                                (None, Some(why)) => format!("   {why}"),
                                // Said out loud: this is why a product may carry
                                // fewer features than the parent had.
                                (None, None) => "   (could not be placed in the parent)".into(),
                            }
                        ))
                        .monospace()
                        .size(11.0)
                        .color(pal.ink2),
                    );
                }
                ui.add_space(6.0);
            }

            // THE OVERHANG CHECK, above the products rather than below them.
            // It is the answer to the question a Golden Gate design actually
            // poses, and a user who reads the product list first has already
            // decided the assembly works.
            if let Some(g) = &pl.gg {
                ui.label(
                    egui::RichText::new(if g.usable {
                        "Overhangs: no structural fault found"
                    } else {
                        "Overhangs: this set will not build one construct"
                    })
                    .strong()
                    .color(if g.usable { pal.ink } else { pal.warn }),
                );
                if !g.overhangs.is_empty() {
                    ui.label(
                        egui::RichText::new(format!("  {}", g.overhangs.join("  ")))
                            .monospace()
                            .size(11.0)
                            .color(pal.ink2),
                    );
                }
                for (line, fatal) in &g.faults {
                    // The two severities are painted differently because they
                    // ARE different: a repeat or a palindrome gives you another
                    // construct, a near neighbour gives you mostly the right one
                    // with a wrong minor product.
                    ui.label(
                        egui::RichText::new(format!(
                            "  {} {line}",
                            // WORDS, not a glyph. This project ships its own
                            // faces and has a test that every character in a
                            // refusal exists in them; a decorative cross is one
                            // more thing that can come out as a box, and
                            // "fatal" is clearer than any symbol anyway.
                            if *fatal { "fatal:" } else { "minor:" }
                        ))
                        .size(11.0)
                        .color(if *fatal { pal.warn } else { pal.muted }),
                    );
                }
                // ALWAYS, and the crate insists on it: an empty fault list is
                // "no structural fault found" and not "this will work".
                ui.label(egui::RichText::new(g.caveat).size(10.5).color(pal.muted));
                ui.add_space(6.0);
            }

            if let Some(n) = &pl.note {
                ui.label(egui::RichText::new(n).color(pal.warn));
                return;
            }

            ui.label(egui::RichText::new(format!("{} product(s)", pl.prods.len())).strong());
            for (i, prod) in pl.prods.iter().enumerate() {
                ui.horizontal(|ui| {
                    let order: Vec<String> = prod
                        .order
                        .iter()
                        .map(|(f, flipped)| format!("{}{}", f + 1, if *flipped { "r" } else { "" }))
                        .collect();
                    ui.label(
                        egui::RichText::new(format!(
                            "{} bp {}   {}",
                            prod.mol.seq.len(),
                            if prod.circular { "circular" } else { "linear" },
                            order.join(" + ")
                        ))
                        .monospace()
                        .size(11.0),
                    );
                    let carried = egui::RichText::new(format!(
                        "{} feature(s) carried{}",
                        prod.carried,
                        if prod.dropped > 0 {
                            format!(", {} dropped", prod.dropped)
                        } else {
                            String::new()
                        }
                    ))
                    .size(10.5)
                    .color(if prod.dropped > 0 {
                        pal.warn
                    } else {
                        pal.muted
                    });
                    ui.label(carried).on_hover_text(
                        "A feature travels only when its whole span sits inside one fragment \
                         that could be placed in the parent. Anything cut in half, or from a \
                         fragment whose origin is ambiguous, is left behind rather than put \
                         at a coordinate that merely looks right.",
                    );
                    if ui.button("Open").clicked() {
                        p.wanted = Some(i);
                    }
                    // Beside Open, because the two are the same decision seen
                    // from two ends: the construct you take is the construct you
                    // have to be able to describe six months later.
                    if ui
                        .button("Copy record")
                        .on_hover_text(
                            "The enzymes, the fragments and where each came from, the                              junctions, the features that travelled and the ones that did                              not — followed by what a plan does not establish about a                              reaction.",
                        )
                        .clicked()
                    {
                        wanted_report = Some(i);
                    }
                });
            }
        });
    // AFTER the closure. The report reads `p.plan` and the closure holds `p`
    // mutably; more to the point, a clipboard write inside the paint closure is
    // a side effect buried in a layout pass, which is where this codebase has
    // twice found one it did not expect.
    if let Some(i) = wanted_report {
        if let Some(pl) = &p.plan {
            if let Some(text) = report(mol, donor_mol, p.method, &p.enzymes, p.homology, pl, i) {
                ctx.copy_text(text);
            }
        }
    }
    open
}

/// The two oligo boxes, and a picker over the primers the file already holds.
///
/// Two named boxes rather than one field with a separator, because the two are
/// not interchangeable and a swap does not fail: on a circle, forward and
/// reverse exchanged amplify the COMPLEMENT arc, so a 465 bp product becomes a
/// 4,921 bp one and every number about it looks reasonable.
fn primer_boxes(ui: &mut egui::Ui, p: &mut Panel, mol: &Molecule, pal: crate::theme::Palette) {
    ui.label(egui::RichText::new("Amplify with").strong());
    ui.label(
        egui::RichText::new(
            "Both oligos 5'->3', as you would order them. Tails included — a tail is the \
             reason to have one.",
        )
        .color(pal.muted)
        .size(11.0),
    );
    for (label, field) in [
        ("forward", &mut p.primers.forward),
        ("reverse", &mut p.primers.reverse),
    ] {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).monospace().size(11.0));
            if ui
                .add(
                    egui::TextEdit::singleline(field)
                        .desired_width(360.0)
                        .hint_text("ACGT..."),
                )
                .changed()
            {
                p.stale = true;
            }
        });
    }
    // The primers the FILE already carries. A `.dna` from SnapGene is full of
    // them, and retyping one from the Features list is how a base gets dropped.
    let known: Vec<(String, String)> = mol
        .primers
        .iter()
        .filter(|pr| !pr.seq.is_empty())
        .map(|pr| {
            (
                if pr.name.is_empty() {
                    "unnamed".to_string()
                } else {
                    short(&pr.name)
                },
                pr.seq.to_ascii_uppercase(),
            )
        })
        .collect();
    if !known.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("from this file:")
                    .color(pal.muted)
                    .size(11.0),
            );
            for (name, seq) in &known {
                // Two buttons per primer, because which end it is is the user's
                // knowledge and not ours. A single "use" button would have to
                // guess, and guessing is the swap this whole panel is careful
                // about.
                if ui
                    .small_button(format!("{name} →"))
                    .on_hover_text(format!("Use {seq} as the forward primer"))
                    .clicked()
                {
                    p.primers.forward = seq.clone();
                    p.stale = true;
                }
                if ui
                    .small_button(format!("← {name}"))
                    .on_hover_text(format!("Use {seq} as the reverse primer"))
                    .clicked()
                {
                    p.primers.reverse = seq.clone();
                    p.stale = true;
                }
            }
        });
    }
}

/// A molecule's name, bounded, for a monospace column that must stay a column.
///
/// The same reason the Features list and the tab strip bound theirs: a plasmid
/// saved as a 90-character description is not rare, and one row of it lays out
/// the whole panel.
fn short(name: &str) -> String {
    const CAP: usize = 18;
    if name.chars().count() <= CAP {
        return name.to_string();
    }
    let head: String = name.chars().take(CAP - 1).collect();
    format!("{head}…")
}

/// The end as a short label: `5' GATC`, `3' TGCA`, `blunt`.
fn end_label(e: &pl_clone::End) -> String {
    match e {
        pl_clone::End::Blunt => "blunt".into(),
        pl_clone::End::Overhang { five_prime, bases } => {
            format!("{} {bases}", if *five_prime { "5'" } else { "3'" })
        }
    }
}

/// Where in the parent this fragment came from, by matching its bases.
///
/// `try_cut` hands back sequence and ends with no coordinates, and the product
/// needs coordinates to carry a feature. Recovered rather than assumed: the
/// fragment is searched for in the parent, doubled when the parent is circular
/// so a fragment crossing the origin is found as one run.
///
/// MATCHED ON THE TOP STRAND, not on `to_string_full`. That method fills from
/// crick where watson does not reach, so it spans BOTH single-stranded ends —
/// and a circle cut once has the same four bases at each, making its "full"
/// string four longer than the plasmid it came out of and findable nowhere in
/// it. `watson` is always a contiguous run of the parent's top strand, which is
/// exactly what a coordinate is measured against.
///
/// `None` when it appears zero times or more than once. More than once is the
/// case that matters — a tandem repeat, or a short fragment — and picking the
/// first match there would put somebody's features at a coordinate that merely
/// looks plausible.
fn locate(frag: &str, parent: &str, circular: bool) -> Option<(u64, u64)> {
    if frag.is_empty() || frag.len() > parent.len() {
        return None;
    }
    let hay = if circular {
        format!("{parent}{parent}")
    } else {
        parent.to_string()
    };
    let limit = if circular { parent.len() } else { hay.len() };
    let mut hit = None;
    let hb = hay.as_bytes();
    let fb = frag.as_bytes();
    for start in 0..limit.saturating_sub(0) {
        if start + fb.len() > hb.len() {
            break;
        }
        if &hb[start..start + fb.len()] == fb {
            if hit.is_some() {
                return None; // ambiguous
            }
            hit = Some(start);
        }
    }
    let s = hit?;
    Some((s as u64 + 1, (s + fb.len()) as u64))
}

/// A finished molecule and how it was put together: the fragments in order, and
/// what was sealed at each junction.
///
/// A named type because the three arms that produce it — ligation, subcloning
/// and homology assembly — must produce the SAME thing for `build` to be able to
/// read it, and because clippy is right that the tuple had stopped being
/// legible.
type Laid = (pl_clone::Dseq, Vec<(usize, bool)>, Vec<String>);

/// One molecule going into the plan, with its sequence in the form the digest
/// and `locate` both want.
struct Source<'a> {
    mol: &'a Molecule,
    seq: String,
    circular: bool,
}

/// Plan a digest and religation, optionally taking the insert from `donor`.
///
/// `donor` is Stage 4. `None` religates `mol` on its own, which is what this
/// function has always done and still does bit for bit. `Some` runs
/// [`pl_clone::ligate::subclone`] over the two digests: one fragment from each,
/// which is subcloning.
///
/// Pure: no `Ui`, no document, no worker. That is deliberate — the equivalent
/// logic for the Enzymes tab started life inside a closure in the tab and could
/// not be asserted without standing up a frame.
pub fn plan(
    mol: &Molecule,
    donor: Option<&Molecule>,
    method: Method,
    enzymes: &BTreeSet<String>,
    primers: &Primers,
    blunt: bool,
    homology: usize,
) -> Plan {
    // PCR BRANCHES FIRST, before a single fragment is built, and that ordering
    // is load-bearing rather than tidy. With an empty enzyme set
    // `pl_clone::digest` returns the whole molecule as one fragment, and
    // neither the "Tick an enzyme" guard (Restriction only) nor the "None of X
    // cuts" guard (`!enzymes.is_empty()`) fires — so falling through would show
    // the uncut plasmid as "1 fragment" and hand it to `ligate`.
    if method == Method::Pcr {
        return amplify(mol, primers);
    }
    // An enzyme is required to CUT and optional to ASSEMBLE. Gibson's overlap is
    // designed into the primers; the enzymes are there only to linearise a
    // vector, and a user with two PCR products needs none at all.
    if enzymes.is_empty() && method == Method::Restriction {
        return Plan {
            frags: Vec::new(),
            prods: Vec::new(),
            note: Some("Tick an enzyme to cut with.".into()),
            gg: None,
            pcr: None,
        };
    }
    let mut sources: Vec<Source> = vec![Source {
        mol,
        seq: String::from_utf8_lossy(&mol.seq).to_ascii_uppercase(),
        circular: mol.topology.is_circular(),
    }];
    if let Some(d) = donor {
        sources.push(Source {
            mol: d,
            seq: String::from_utf8_lossy(&d.seq).to_ascii_uppercase(),
            circular: d.topology.is_circular(),
        });
    }

    // The digest itself lives in `pl-clone` since Stage 4. It was written out
    // here, and `subclone` needs the identical operation on a second molecule —
    // a digest performed one way in the panel and another way in the engine is
    // two answers to "what are the fragments".
    let cutters: Vec<&pl_enzymes::Enzyme> = enzymes
        .iter()
        .filter_map(|n| pl_enzymes::by_name(n))
        .collect();
    let pools: Vec<Vec<pl_clone::Dseq>> = sources
        .iter()
        .map(|s| {
            pl_clone::digest(
                &pl_clone::Dseq::new(&s.seq, s.circular),
                cutters.iter().copied(),
            )
        })
        .collect();

    // NAMED PER MOLECULE. "None of BamHI cuts this molecule" is ambiguous the
    // moment there are two, and the one it is silent about is the one the user
    // has to go and look at.
    for (i, pool) in pools.iter().enumerate() {
        if !enzymes.is_empty() && pool.len() == 1 && pool[0].circular {
            return Plan {
                frags: Vec::new(),
                prods: Vec::new(),
                gg: None,
                pcr: None,
                note: Some(format!(
                    "None of {} cuts {}.",
                    enzymes.iter().cloned().collect::<Vec<_>>().join(", "),
                    if sources.len() == 1 {
                        "this molecule".to_string()
                    } else {
                        sources[i].mol.name.clone()
                    }
                )),
            };
        }
    }

    // Flattened, so a fragment has ONE index everywhere: in the list on screen,
    // in a product's order, and in the feature remap. Two index spaces for the
    // same fragments is how the insert's features end up on the vector.
    let mut frags: Vec<pl_clone::Dseq> = Vec::new();
    let mut described: Vec<Frag> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();
    for (pi, pool) in pools.iter().enumerate() {
        offsets.push(frags.len());
        for f in pool {
            described.push(Frag {
                len: f.len(),
                left: end_label(&f.left_end()),
                right: end_label(&f.right_end()),
                from: locate(&f.watson, &sources[pi].seq, sources[pi].circular),
                // A digest fragment's `None` really is ambiguity, which is what
                // the panel has always said. Only an amplicon has a reason.
                unplaced: None,
                parent: pi,
            });
            frags.push(f.clone());
        }
    }

    // GIBSON JOINS BY HOMOLOGY AND NOT BY ENDS, so it does not go through
    // `ligate` at all — the shapes of the ends are irrelevant and the blunt
    // policy has nothing to decide. `assemble` uses every fragment, which is
    // right: a Gibson reaction contains exactly the pieces amplified for it.
    if method == Method::Gibson {
        // A circle has no ends to overlap. Said rather than assembled around,
        // because `assemble` reads a fragment as a plain string and would
        // happily match a circular vector's arbitrary start against something —
        // an answer resting on a junction that does not exist in the tube.
        if let Some(name) = sources
            .iter()
            .enumerate()
            .find(|(i, _)| pools[*i].len() == 1 && pools[*i][0].circular)
            .map(|(_, s)| s.mol.name.clone())
        {
            return Plan {
                frags: described,
                prods: Vec::new(),
                gg: None,
                pcr: None,
                note: Some(format!(
                    "{name} is circular. Gibson joins ends, so linearise it first — tick an \
                     enzyme that cuts it once."
                )),
            };
        }
        let opts = pl_clone::assembly::Options {
            limit: homology,
            ..Default::default()
        };
        let laid = match pl_clone::assembly::assemble(&frags, true, opts) {
            Ok(ps) => ps
                .into_iter()
                .map(|p| {
                    let js: Vec<String> = p
                        .junctions
                        .iter()
                        .map(|n| format!("{n} bp homology"))
                        .collect();
                    (p.seq, p.order, js)
                })
                .collect::<Vec<_>>(),
            Err(e) => {
                return Plan {
                    frags: described,
                    prods: Vec::new(),
                    note: Some(e.to_string()),
                    gg: None,
                    pcr: None,
                }
            }
        };
        let prods: Vec<Prod> = laid
            .iter()
            .map(|(seq, order, js)| build(&sources, &frags, &described, seq, order, js.clone()))
            .collect();
        let note = prods.is_empty().then(|| {
            format!(
                "No two of these share {homology} bp or more at their ends. Gibson needs the \
                 overlap designed into the primers; lower the homology only if you know the \
                 junction really is shorter."
            )
        });
        return Plan {
            frags: described,
            prods,
            note,
            gg: None,
            pcr: None,
        };
    }

    // GOLDEN GATE. The ligation is the same one `ligate` and `subclone` already
    // do — the pieces are joined by their ends, and T4 ligase does not know
    // which enzyme made them. What is different, and what the whole method rests
    // on, is WHICH pieces join: the recognition site leaves with the cut, so the
    // junction carries no scar, and four bases the designer chose decide the
    // order. So the answer a user needs here is not "what can be built" but
    // "will these overhangs build the one thing I meant", and that is a question
    // about the overhang SET rather than about any product.
    let gg = (method == Method::GoldenGate).then(|| {
        let overhangs: Vec<pl_clone::goldengate::Overhang> = frags
            .iter()
            .filter_map(pl_clone::goldengate::left_overhang)
            .collect();
        let r = pl_clone::goldengate::check(&overhangs);
        GgReport {
            overhangs: r.overhangs.clone(),
            faults: r
                .faults
                .iter()
                .map(|f| (f.to_string(), f.is_fatal()))
                .collect(),
            usable: r.is_usable(),
            caveat: r.caveat(),
        }
    });

    let opts = pl_clone::ligate::Options {
        blunt,
        ..Default::default()
    };
    // Both arms produce the same thing: a molecule and the fragments laid down
    // in order, in the flattened index space above.
    let laid: Vec<Laid> = if donor.is_none() {
        match pl_clone::ligate::ligate(&pools[0], &opts) {
            Ok(ps) => ps
                .into_iter()
                .map(|p| {
                    let js: Vec<String> = p.junctions.iter().map(end_label).collect();
                    (p.seq, p.order, js)
                })
                .collect(),
            Err(e) => {
                return Plan {
                    frags: described,
                    prods: Vec::new(),
                    note: Some(e.to_string()),
                    gg,
                    pcr: None,
                }
            }
        }
    } else {
        match pl_clone::ligate::subclone(&pools, &opts) {
            Ok(cs) => cs
                .into_iter()
                .map(|c| {
                    // `order` indexes POOLS in a subcloning — one fragment comes
                    // from each — and `routes[0]` says which fragment that was.
                    let route = c.routes[0].clone();
                    let order = c
                        .product
                        .order
                        .iter()
                        .map(|(pool, flipped)| (offsets[*pool] + route[*pool], *flipped))
                        .collect();
                    let js: Vec<String> = c.product.junctions.iter().map(end_label).collect();
                    (c.product.seq, order, js)
                })
                .collect(),
            Err(e) => {
                return Plan {
                    frags: described,
                    prods: Vec::new(),
                    note: Some(e.to_string()),
                    gg,
                    pcr: None,
                }
            }
        }
    };

    let prods: Vec<Prod> = laid
        .iter()
        .map(|(seq, order, js)| build(&sources, &frags, &described, seq, order, js.clone()))
        .collect();

    let note = if prods.is_empty() {
        Some(if blunt {
            "These fragments have no pair of ends that can be sealed.".into()
        } else {
            "No sticky ends match. Blunt ends are excluded — tick “blunt” to \
             include them."
                .to_string()
        })
    } else {
        None
    };
    Plan {
        frags: described,
        prods,
        note,
        gg,
        pcr: None,
    }
}

/// The cloning that was planned, as prose somebody can paste into a notebook or
/// a methods section.
///
/// EVERY NUMBER COMES FROM THE RUN. `pl_doc::methods` is handed a topic and
/// nothing else, so it can only describe defaults, and its own doc says a caller
/// that knows what a run used should print it from the run. This is that caller.
/// The two halves are joined and not blurred: what happened, then what a plan
/// does not establish about a reaction.
///
/// Past tense and passive, matching the paragraphs it is appended to, so it can
/// be edited rather than rewritten.
pub fn report(
    mol: &Molecule,
    donor: Option<&Molecule>,
    method: Method,
    enzymes: &BTreeSet<String>,
    homology: usize,
    pl: &Plan,
    i: usize,
) -> Option<String> {
    let prod = pl.prods.get(i)?;
    let names: Vec<String> = enzymes.iter().cloned().collect();
    let mut out = String::new();

    let describe = |m: &Molecule| {
        format!(
            "{} ({} bp, {})",
            m.name,
            m.seq.len(),
            if m.topology.is_circular() {
                "circular"
            } else {
                "linear"
            }
        )
    };
    match method {
        Method::Restriction | Method::GoldenGate => {
            out.push_str(&format!(
                "{} was digested with {}",
                describe(mol),
                names.join(" and ")
            ));
            if let Some(d) = donor {
                out.push_str(&format!(", as was {}", describe(d)));
            }
            out.push_str(&format!(", giving {} fragment(s). ", pl.frags.len()));
        }
        Method::Gibson => {
            out.push_str(&describe(mol));
            if let Some(d) = donor {
                out.push_str(&format!(" and {}", describe(d)));
            }
            if !names.is_empty() {
                out.push_str(&format!(" (linearised with {})", names.join(" and ")));
            }
            out.push_str(&format!(
                " were assembled by homology of at least {homology} bp. "
            ));
        }
        // THE TWO OLIGOS BY SEQUENCE, because a methods paragraph naming a
        // product without naming what made it cannot be repeated by anybody.
        // They come off the plan rather than from an argument — `report` is
        // already at seven.
        Method::Pcr => {
            let p = pl.pcr.clone().unwrap_or_default();
            out.push_str(&format!(
                "{} was amplified with {} and {}. ",
                describe(mol),
                p.forward,
                p.reverse
            ));
        }
    }

    // WHICH PIECES, by their place in their own parent. A construct nobody can
    // trace back to a band on a gel cannot be built, so the report says where
    // each piece came from rather than only how long it was.
    let pieces: Vec<String> = prod
        .order
        .iter()
        .map(|(idx, flipped)| {
            let f = &pl.frags[*idx];
            let whose = if donor.is_some() {
                let parent = if f.parent == 0 {
                    mol.name.clone()
                } else {
                    donor.map(|d| d.name.clone()).unwrap_or_default()
                };
                format!("{parent} ")
            } else {
                String::new()
            };
            let at = match f.from {
                Some((a, b)) => format!("{a}..{b}"),
                None => "position not determined".into(),
            };
            format!(
                "{whose}{at} ({} bp{})",
                f.len,
                if *flipped { ", inverted" } else { "" }
            )
        })
        .collect();
    out.push_str(&format!(
        "The construct was assembled from {}",
        pieces.join(" and ")
    ));
    if !prod.junctions.is_empty() {
        out.push_str(&format!(", joined at {}", prod.junctions.join(" and ")));
    }
    out.push_str(&format!(
        ", giving a {} bp {} molecule. ",
        prod.mol.seq.len(),
        if prod.circular { "circular" } else { "linear" }
    ));

    // The features, and what did NOT travel. A count of what was carried
    // without the count of what was not reads as "everything".
    out.push_str(&format!(
        "{} annotated feature(s) were carried into the construct",
        prod.carried
    ));
    if prod.dropped > 0 {
        out.push_str(&format!(
            "; {} were not, because their span did not sit whole inside one placeable              fragment",
            prod.dropped
        ));
    }
    out.push('.');

    // The overhang check travels with the record, faults and caveat alike. A
    // Golden Gate methods paragraph that omits the fidelity caveat is the one
    // sentence in it a reviewer would ask for.
    if let Some(g) = &pl.gg {
        out.push_str(&format!(
            " The Type IIS overhangs were {}.",
            if g.overhangs.is_empty() {
                "not determined".to_string()
            } else {
                g.overhangs.join(", ")
            }
        ));
        if g.faults.is_empty() {
            out.push_str(" No structural fault was found in the set.");
        } else {
            out.push_str(" Faults found: ");
            out.push_str(
                &g.faults
                    .iter()
                    .map(|(l, fatal)| format!("{l}{}", if *fatal { "" } else { " (minor)" }))
                    .collect::<Vec<_>>()
                    .join("; "),
            );
            out.push('.');
        }
        out.push_str(&format!(" The check is {}", g.caveat));
    }

    out.push_str(
        "

",
    );
    out.push_str(&pl_doc::methods(pl_doc::topic("cloning")?));
    Some(out)
}

/// Amplify between two oligos.
///
/// One template — the molecule on screen — because that is what a PCR has. The
/// `donor` a subcloning uses is ignored here rather than quietly repurposed.
///
/// Everything hard is inside `pl_clone::pcr` and stays there: it doubles a
/// circular template so a footprint straddling the origin is found as one run,
/// measures travel as `(r_start + n - f_end) % n` so an amplicon across the
/// origin is ordinary rather than `Inverted`, reads the middle base-by-base with
/// `% n` because slicing wrapped, and judges specificity over BOTH strands at a
/// 12 nt floor — refusing a pair that binds twice rather than picking one.
fn amplify(mol: &Molecule, primers: &Primers) -> Plan {
    let empty = |note: Option<String>| Plan {
        frags: Vec::new(),
        prods: Vec::new(),
        note,
        gg: None,
        pcr: Some(primers.clone()),
    };
    let (f, r) = (primers.forward.trim(), primers.reverse.trim());
    if f.is_empty() || r.is_empty() {
        return empty(Some(
            "Paste both primers, 5'->3', as you would order them — tails included.".into(),
        ));
    }
    let seq: String = String::from_utf8_lossy(&mol.seq).to_ascii_uppercase();
    let circular = mol.topology.is_circular();
    let template = pl_clone::Dseq::new(&seq, circular);
    let product = match pl_clone::pcr(f, r, &template) {
        Ok(p) => p,
        Err(e) => return empty(Some(refusal(&e))),
    };

    // ONE FRAGMENT, laid down once, unflipped — which is what an amplicon is.
    // It goes through `build` like every other product so the feature remap,
    // the naming and the carried/dropped counts are the same code and cannot
    // drift into a second, subtly different answer.
    let from = locate(&product.watson, &seq, circular);
    // `None` here is NOT ambiguity. `pcr` has already refused any pair whose
    // seed binds twice, so the product occurs at most once; what `None` means
    // is that the product is not a stretch of the template — a 5' tail, or an
    // overlapping pair on a circle giving a near-whole-plasmid product.
    let unplaced = from.is_none().then(|| {
        "not a stretch of the template — a 5' tail adds bases the template does not have, \
         so nothing can be carried across"
            .to_string()
    });
    let sources = vec![Source { mol, seq, circular }];
    let described = vec![Frag {
        len: product.len(),
        left: end_label(&product.left_end()),
        right: end_label(&product.right_end()),
        from,
        unplaced,
        parent: 0,
    }];
    let frags = vec![product.clone()];
    // No junctions. A PCR seals nothing, and `report` already omits the
    // "joined at" clause when the list is empty.
    let prods = vec![build(
        &sources,
        &frags,
        &described,
        &product,
        &[(0, false)],
        Vec::new(),
    )];
    Plan {
        frags: described,
        prods,
        note: None,
        gg: None,
        pcr: Some(primers.clone()),
    }
}

/// A PCR refusal, in words that name the character rather than drawing it.
///
/// `PcrError::NotDna` formats the offending character with `{found:?}`, and
/// `char`'s `Debug` keeps any printable non-ASCII one literally. A primer pasted
/// from a vendor's order sheet containing U+4E2D therefore comes back as "the
/// forward primer contains '中'" — and this binary's proportional chain is Plex
/// Sans, Ubuntu, Noto Emoji and emoji-icon-font, none of which has that glyph.
/// `the_tofu_oracle_answers_both_ways_before_anything_relies_on_it` already pins
/// U+4E2D as tofu in both families, so painting the error straight through tells
/// a user their primer contains an empty box — the one shape that cannot say
/// which character to delete.
///
/// So the character is named by CODEPOINT, which is always renderable.
///
/// KNOWN LIMIT, and it belongs to `pl-clone` rather than here: an ASCII
/// character that is not a base — a `?` or a `-` left in a pasted oligo — is not
/// `NotDna` at all. `pcr` checks only `is_ascii`, so such a primer simply fails
/// to anneal and the user is told "the forward primer does not anneal", which
/// sends them to look at their template instead of at the stray character.
/// Widening that check is a correctness-crate change for a GUI reason and wants
/// its own commit and its own argument.
fn refusal(e: &pl_clone::PcrError) -> String {
    match e {
        // `found` IS NON-ASCII BY CONSTRUCTION. `pcr` raises this only under
        // `!s.is_ascii()`, so there is no ASCII case to branch on and a branch
        // for one would be a check that cannot fire. The first draft of this
        // function had exactly that, and the test written for it failed against
        // the branch rather than against the feature.
        pl_clone::PcrError::NotDna { what, found } => format!(
            "the {what} contains U+{:04X}, which is not a DNA base — it may be invisible \
             where you pasted it from",
            *found as u32
        ),
        other => other.to_string(),
    }
}

/// Turn one ligation product into a molecule, carrying what can be carried.
///
/// `sources` is one molecule per digest; `described[i].parent` says which one a
/// fragment came out of. THAT INDIRECTION IS THE WHOLE OF WHAT STAGE 4 ADDED
/// here, and getting it wrong does not fail loudly: the coordinates would still
/// be inside the construct, so a vector's resistance marker would simply appear
/// on the insert's gene, at a span that looks entirely reasonable.
fn build(
    sources: &[Source],
    frags: &[pl_clone::Dseq],
    described: &[Frag],
    seq: &pl_clone::Dseq,
    order: &[(usize, bool)],
    junctions: Vec<String>,
) -> Prod {
    let full = seq.to_string_full();
    // Named after every parent that actually contributed, in order, and not
    // after the open molecule alone: a construct labelled "pUC19 product" when
    // half of it came from pET28a is a file somebody will later mistake for a
    // religation of pUC19.
    let mut used_parents: Vec<usize> = order.iter().map(|(i, _)| described[*i].parent).collect();
    used_parents.dedup();
    used_parents.sort_unstable();
    used_parents.dedup();
    let name = format!(
        "{} product",
        used_parents
            .iter()
            .map(|p| sources[*p].mol.name.clone())
            .collect::<Vec<_>>()
            .join(" + ")
    );
    let mut mol = Molecule {
        name,
        seq: full.clone().into_bytes(),
        topology: if seq.circular {
            Topology::Circular
        } else {
            Topology::Linear
        },
        ..Default::default()
    };

    // Walk the product laying each fragment down in order, so a feature's new
    // coordinate is its offset inside its fragment plus where that fragment
    // starts in the product.
    let mut at: u64 = 0;
    let mut carried = 0usize;
    let mut dropped = 0usize;
    for (idx, flipped) in order {
        let parent = sources[described[*idx].parent].mol;
        let seq = sources[described[*idx].parent].seq.as_str();
        // Watson length, matching what locate measured and what join`n        // concatenates. len() spans the single-stranded ends too, and using it
        // here would drift the layout by an overhang per junction.
        let flen = frags[*idx].watson.len() as u64;
        match described[*idx].from {
            None => {
                // Unplaceable fragment: count what it would have carried so the
                // number on screen is the truth rather than a silent zero.
                dropped += parent
                    .features
                    .iter()
                    .filter(|f| within(f, 1, seq.len() as u64))
                    .count()
                    .min(parent.features.len());
            }
            Some((fs, fe)) => {
                for f in &parent.features {
                    if !within(f, fs, fe) {
                        dropped += 1;
                        continue;
                    }
                    let mut moved = f.clone();
                    moved.segments = f
                        .segments
                        .iter()
                        .map(|s| {
                            // Offset inside the fragment, 0-based.
                            let a = s.start - fs;
                            let b = s.end - fs;
                            let (a, b) = if *flipped {
                                (flen - 1 - b, flen - 1 - a)
                            } else {
                                (a, b)
                            };
                            Segment::new(at + a + 1, at + b + 1)
                        })
                        .collect();
                    if *flipped {
                        moved.strand = match f.strand {
                            Strand::Forward => Strand::Reverse,
                            Strand::Reverse => Strand::Forward,
                            other => other,
                        };
                    }
                    mol.features.push(moved);
                    carried += 1;
                }
            }
        }
        at += flen;
    }

    Prod {
        mol,
        circular: seq.circular,
        order: order.to_vec(),
        carried,
        dropped,
        junctions,
    }
}

/// Is every segment of this feature inside `[lo, hi]`, 1-based inclusive?
///
/// Whole-feature only. Half a promoter carried into a construct is a claim the
/// parent never made, and the coordinates would be right while the biology was
/// wrong.
fn within(f: &Feature, lo: u64, hi: u64) -> bool {
    !f.segments.is_empty()
        && f.segments
            .iter()
            .all(|s| s.start >= lo && s.end <= hi && s.start <= s.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mol(seq: &str, circular: bool) -> Molecule {
        Molecule {
            name: "p".into(),
            seq: seq.as_bytes().to_vec(),
            topology: if circular {
                Topology::Circular
            } else {
                Topology::Linear
            },
            ..Default::default()
        }
    }

    fn ticked(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn nothing_ticked_asks_for_an_enzyme_rather_than_showing_an_empty_list() {
        let p = plan(
            &mol("ACGTACGTACGT", true),
            None,
            Method::Restriction,
            &BTreeSet::new(),
            &Primers::default(),
            false,
            25,
        );
        assert!(p.frags.is_empty() && p.prods.is_empty());
        assert!(p.note.unwrap().contains("Tick an enzyme"));
    }

    #[test]
    fn an_enzyme_that_does_not_cut_says_so() {
        let p = plan(
            &mol("ACGTACGTACGTACGT", true),
            None,
            Method::Restriction,
            &ticked(&["BamHI"]),
            &Primers::default(),
            false,
            25,
        );
        assert!(p.note.unwrap().contains("None of BamHI cuts"));
    }

    /// The whole point: cut and put it back, and get the same molecule.
    #[test]
    fn a_single_cut_religates_to_the_same_length_and_keeps_its_features() {
        let seq = "AAAAGGATCCTTTTGCGCGCATATATCCCGGGAAAATTTTCCCC";
        let mut m = mol(seq, true);
        let mut f = Feature::new("a gene", "CDS");
        f.strand = Strand::Forward;
        f.segments.push(Segment::new(15, 30));
        m.features.push(f);

        let p = plan(
            &m,
            None,
            Method::Restriction,
            &ticked(&["BamHI"]),
            &Primers::default(),
            false,
            25,
        );
        assert_eq!(p.frags.len(), 1, "one site in a circle gives one fragment");
        assert!(p.frags[0].from.is_some(), "the fragment must be placeable");
        assert_eq!(p.frags[0].left, "5' GATC");
        assert_eq!(p.prods.len(), 1, "{:?}", p.note);

        let prod = &p.prods[0];
        assert!(prod.circular);
        assert_eq!(
            prod.mol.seq.len(),
            seq.len(),
            "religation changed the length"
        );
        assert_eq!(prod.carried, 1, "the feature did not travel");
        assert_eq!(prod.dropped, 0);
        // And it still covers the same bases, wherever the circle now starts.
        let s = &prod.mol.features[0].segments[0];
        assert_eq!(s.end - s.start, 15, "the feature changed length");
    }

    /// Stage 4 through the panel: a gene out of one plasmid, into another.
    ///
    /// PROVEN TO FAIL against a7c556a, where `plan` takes one molecule and there
    /// is no way to name a second. The construct here cannot be produced at all
    /// — not with different enzymes, not by ligating everything: `ligate` uses
    /// every fragment, so handing it all four pieces asks for a four-piece
    /// circle and never offers the two-piece one.
    ///
    /// WHAT IT REALLY CHECKS is the feature remap across TWO parents, because
    /// that is the part that fails silently. The coordinates would still land
    /// inside the construct whichever parent they were read from, so a vector's
    /// marker would simply appear on the insert's gene at a span that looks
    /// entirely reasonable, and the file would be wrong in a way nobody sees.
    #[test]
    fn a_gene_from_one_plasmid_goes_into_a_vector_from_another_with_both_names() {
        // One EcoRI and one BamHI site in each, so each digest is two pieces
        // with one AATT end and one GATC end — a directional subcloning.
        let mut vector = mol(
            "GAATTCTTTTTTTTTTTTTTTTTTTTGGATCCAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            true,
        );
        vector.name = "vec".into();
        let mut marker = Feature::new("bla", "CDS");
        marker.strand = Strand::Forward;
        // Wholly inside the long piece, which is the backbone.
        marker.segments.push(Segment::new(36, 55));
        vector.features.push(marker);

        let mut giver = mol(
            "GAATTCCCCCCCCCCCCCCCCCCGGATCCGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
            true,
        );
        giver.name = "donor".into();
        let mut gene = Feature::new("gfp", "CDS");
        gene.strand = Strand::Forward;
        // Wholly inside the short piece, which is the insert.
        gene.segments.push(Segment::new(8, 20));
        giver.features.push(gene);

        let p = plan(
            &vector,
            Some(&giver),
            Method::Restriction,
            &ticked(&["EcoRI", "BamHI"]),
            &Primers::default(),
            false,
            25,
        );
        assert!(p.note.is_none(), "{:?}", p.note);
        assert_eq!(p.frags.len(), 4, "two pieces from each digest");
        assert_eq!(
            p.frags.iter().filter(|f| f.parent == 1).count(),
            2,
            "the donor's fragments are not attributed to the donor"
        );

        // The construct a person wants: the vector's backbone plus the donor's
        // insert, carrying one feature from each.
        let both = p
            .prods
            .iter()
            .find(|pr| pr.carried == 2)
            .expect("no construct carried a feature from each parent");
        assert!(both.circular);
        let names: Vec<&str> = both.mol.features.iter().map(|f| f.name.as_str()).collect();
        assert!(
            names.contains(&"bla") && names.contains(&"gfp"),
            "the construct carries {names:?}"
        );
        // NAMED AFTER BOTH. A file called "vec product" that is half donor is
        // one somebody will later mistake for a religation of the vector.
        assert!(
            both.mol.name.contains("vec") && both.mol.name.contains("donor"),
            "the construct is called {:?}",
            both.mol.name
        );

        // AND THE FEATURES ARE WHERE THEY SAY THEY ARE. Each is read back off
        // the construct's own bases and must still be the parent's — the whole
        // failure mode of a two-parent remap is a span that looks right.
        let seq = String::from_utf8_lossy(&both.mol.seq).to_string();
        for (name, parent, span) in [("bla", &vector, (36u64, 55u64)), ("gfp", &giver, (8, 20))] {
            let f = both
                .mol
                .features
                .iter()
                .find(|f| f.name == name)
                .expect("the feature");
            let s = &f.segments[0];
            let got = &seq[(s.start - 1) as usize..s.end as usize];
            let want = String::from_utf8_lossy(&parent.seq)[(span.0 - 1) as usize..span.1 as usize]
                .to_string();
            assert_eq!(
                got.to_ascii_uppercase(),
                want.to_ascii_uppercase(),
                "{name} points at the wrong bases in the construct"
            );
        }
    }

    /// Stage 5: two pieces with designed overlaps, joined by homology.
    ///
    /// PROVEN TO FAIL against c6fa736: `plan` had no `Method`, so the ONLY join
    /// it could make was by compatible ends. `pl_clone::assembly` has been in
    /// the crate the whole time, fully tested against pydna, and nothing in the
    /// application could reach it — the same gap `ligate` was in before Stage 4.
    ///
    /// The pieces here have NO compatible ends whatsoever: both are blunt, so
    /// the restriction path finds nothing even with blunt joining on. That is
    /// the point of the fixture. It is homology or it is nothing.
    #[test]
    fn two_linear_pieces_with_a_designed_overlap_assemble_by_homology() {
        // 30 bases shared at each junction, which is an ordinary Gibson design.
        let a_tail = "GGCCTTAAGGCCTTAAGGCCTTAAGGCCTT";
        let b_tail = "TTGGCCAATTGGCCAATTGGCCAATTGGCC";
        let a = format!("{b_tail}AAAACCCCGGGGTTTTAAAACCCCGGGGTTTT{a_tail}");
        let b = format!("{a_tail}CCCCAAAAGGGGTTTTCCCCAAAAGGGGTTTT{b_tail}");
        let mut left = mol(&a, false);
        left.name = "piece A".into();
        let mut gene = Feature::new("gfp", "CDS");
        gene.segments.push(Segment::new(35, 55));
        left.features.push(gene);
        let mut right = mol(&b, false);
        right.name = "piece B".into();

        // THE CONTROL, and it is what makes the assertion below mean anything:
        // no ligation reaches this construct. Both pieces are blunt, so there
        // is no sticky end to seal — and blunt joining is left OFF, which is
        // both the default and the honest comparison, since a blunt policy
        // joins any end to any other and would "succeed" at a junction the
        // overlaps were designed to make instead.
        let by_ends = plan(
            &left,
            Some(&right),
            Method::Restriction,
            &ticked(&["BamHI"]),
            &Primers::default(),
            false,
            25,
        );
        assert!(
            by_ends.prods.is_empty(),
            "the fixture is supposed to be unreachable by ligation"
        );

        let p = plan(
            &left,
            Some(&right),
            Method::Gibson,
            &BTreeSet::new(),
            &Primers::default(),
            false,
            25,
        );
        assert!(p.note.is_none(), "{:?}", p.note);
        assert_eq!(p.frags.len(), 2, "two pieces, uncut");
        let prod = p
            .prods
            .iter()
            .find(|pr| pr.circular)
            .expect("no circular assembly");
        // Each 30 bp overlap is written ONCE in the product, so the circle is
        // the two pieces minus the two junctions. A construct at `a + b` would
        // carry both overlaps twice, which is a 60 bp duplication in a plasmid.
        assert_eq!(
            prod.mol.seq.len(),
            a.len() + b.len() - 60,
            "the overlaps were written twice"
        );
        assert_eq!(prod.carried, 1, "the gene did not travel");
        assert!(prod.mol.name.contains("piece A") && prod.mol.name.contains("piece B"));
    }

    /// A circle has no ends to overlap, and saying so beats guessing.
    ///
    /// `assemble` reads a fragment as a plain string, so a circular vector's
    /// arbitrary start would be matched against something and the answer would
    /// rest on a junction that does not exist in the tube.
    #[test]
    fn a_circular_vector_is_asked_to_be_linearised_rather_than_assembled() {
        let mut v = mol("AAAACCCCGGGGTTTTAAAACCCCGGGGTTTTAAAACCCCGGGG", true);
        v.name = "still a circle".into();
        let p = plan(
            &v,
            None,
            Method::Gibson,
            &BTreeSet::new(),
            &Primers::default(),
            false,
            25,
        );
        let note = p.note.expect("a note");
        assert!(note.contains("still a circle"), "{note}");
        assert!(note.contains("linearise"), "{note}");
    }

    /// The homology floor is a real parameter and the message says the number.
    #[test]
    fn homology_shorter_than_the_floor_is_reported_with_the_floor_in_it() {
        // 14 bases shared: found at 12, not at 25.
        let shared = "GGCCTTAAGGCCTT";
        let a = format!("AAAACCCCGGGGTTTTAAAACCCC{shared}");
        let b = format!("{shared}TTTTGGGGCCCCAAAATTTTGGGG");
        let x = mol(&a, false);
        let y = mol(&b, false);
        let strict = plan(
            &x,
            Some(&y),
            Method::Gibson,
            &BTreeSet::new(),
            &Primers::default(),
            false,
            25,
        );
        let note = strict.note.expect("a note at 25");
        assert!(note.contains("25 bp"), "{note}");
        assert!(strict.prods.is_empty());
    }

    /// Stage 5b: the question a Golden Gate design actually poses.
    ///
    /// PROVEN TO FAIL against a9f69e4: `pl_clone::goldengate` has been in the
    /// crate the whole time — repeats, palindromes, cross-pairing and
    /// single-mismatch neighbours, each in both orientations — and the only way
    /// to reach any of it was `pl goldengate` in a terminal. The panel could cut
    /// with BsaI and ligate the pieces, and said nothing whatever about whether
    /// the overhangs would give you one construct or four.
    ///
    /// THE FIXTURE HAS A REAL FAULT IN IT. A cassette whose two BsaI sites leave
    /// the SAME four-base overhang cannot build one thing: the junctions are
    /// interchangeable. A check that only ever ran on a clean set would pass
    /// against a `check` that returned no faults for anything.
    #[test]
    fn a_golden_gate_overhang_set_is_checked_and_its_faults_are_named() {
        // Two inward-facing BsaI sites, both releasing GATC: the same overhang
        // at both junctions.
        let seq = "AAAAGGTCTCAGATCTTTTTTTTTTTTTTTTTTTTGATCTGAGACCAAAACCCCGGGGTTTT";
        let m = mol(seq, true);
        let p = plan(
            &m,
            None,
            Method::GoldenGate,
            &ticked(&["BsaI"]),
            &Primers::default(),
            false,
            25,
        );
        let g = p.gg.expect("Golden Gate must report on the overhangs");
        assert!(
            !g.overhangs.is_empty(),
            "no overhang was read off the digest at all"
        );
        assert!(
            !g.faults.is_empty(),
            "a set with one overhang at two junctions was reported as clean: {:?}",
            g.overhangs
        );
        assert!(
            !g.usable,
            "a repeated overhang is fatal — the junctions can swap"
        );
        assert!(g.faults.iter().any(|(_, fatal)| *fatal), "{:?}", g.faults);
        // The caveat travels with the answer, always. An empty fault list means
        // "no structural fault found", not "this will work", and the crate says
        // so in words the panel must not drop.
        assert!(g.caveat.contains("fidelity"), "{}", g.caveat);

        // AND THE OTHER METHODS DO NOT PRETEND TO ANSWER IT. A restriction
        // religation of the same molecule reports no overhang check, because it
        // has not made one.
        let r = plan(
            &m,
            None,
            Method::Restriction,
            &ticked(&["BsaI"]),
            &Primers::default(),
            false,
            25,
        );
        assert!(
            r.gg.is_none(),
            "a plain religation claimed to have checked Golden Gate overhangs"
        );
    }

    /// Stage 6: the construct, written down.
    ///
    /// PROVEN TO FAIL against 15e3b4a, where `report` does not exist and the
    /// only record of a cloning was the construct itself — a sequence with no
    /// account of where it came from. Six months later that is a plasmid nobody
    /// can describe in a methods section without reconstructing the reasoning
    /// from memory.
    ///
    /// EVERY NUMBER IS ASSERTED AGAINST THE RUN, not merely present. A report
    /// that named the right enzymes and the wrong fragment, or counted the
    /// features it carried and stayed silent about the ones it did not, would
    /// pass a test that only looked for keywords — and would be worse than no
    /// report, because it reads as authoritative.
    #[test]
    fn the_record_states_what_was_done_and_what_it_does_not_establish() {
        let seq = "AAAAGGATCCTTTTGCGCGCATATATGGATCCAAAATTTTCCCC";
        let mut m = mol(seq, true);
        m.name = "pTest".into();
        let mut keeps = Feature::new("kept", "CDS");
        keeps.segments.push(Segment::new(12, 22));
        m.features.push(keeps);
        let mut cut_in_two = Feature::new("straddles", "misc_feature");
        cut_in_two.segments.push(Segment::new(25, 36));
        m.features.push(cut_in_two);

        let pl = plan(
            &m,
            None,
            Method::Restriction,
            &ticked(&["BamHI"]),
            &Primers::default(),
            false,
            25,
        );
        let i = pl
            .prods
            .iter()
            .position(|pr| pr.circular)
            .expect("a circular religation");
        let r = report(
            &m,
            None,
            Method::Restriction,
            &ticked(&["BamHI"]),
            25,
            &pl,
            i,
        )
        .expect("a record");

        // What was done, in the run's own numbers.
        assert!(r.contains("pTest"), "{r}");
        assert!(r.contains("BamHI"), "{r}");
        assert!(
            r.contains(&format!("{} bp", seq.len())),
            "the parent length: {r}"
        );
        assert!(r.contains("5' GATC"), "the junction is not named: {r}");
        assert!(
            r.contains(&format!("{} bp", pl.prods[i].mol.seq.len())),
            "the product length: {r}"
        );
        // BOTH counts. A carried count without a dropped count reads as
        // "everything travelled", which is the one thing it must not say.
        assert!(
            r.contains(&format!("{} annotated", pl.prods[i].carried)),
            "{r}"
        );
        assert!(pl.prods[i].dropped > 0, "the fixture must drop something");
        assert!(
            r.contains("were not, because"),
            "the record is silent about the features that did not travel: {r}"
        );
        // And the limits, from `pl_doc`, so the two halves are joined rather
        // than blurred: what happened, then what it does not establish.
        assert!(
            r.contains("this is a plan and not a result"),
            "the record claims a result: {r}"
        );
        assert!(r.contains("transformation"), "{r}");
    }

    /// A Golden Gate record must carry the fidelity caveat.
    ///
    /// It is the one sentence in such a paragraph a reviewer would ask for, and
    /// the crate that computes the check insists on it: an empty fault list is
    /// "no structural fault found", not "this will work".
    #[test]
    fn a_golden_gate_record_carries_the_caveat_the_check_insists_on() {
        let seq = "AAAAGGTCTCAGATCTTTTTTTTTTTTTTTTTTTTGATCTGAGACCAAAACCCCGGGGTTTT";
        let mut m = mol(seq, true);
        m.name = "pGG".into();
        let e = ticked(&["BsaI"]);
        let pl = plan(
            &m,
            None,
            Method::GoldenGate,
            &e,
            &Primers::default(),
            false,
            25,
        );
        let Some(i) = pl.prods.iter().position(|pr| pr.circular) else {
            // No circular product is a legitimate outcome for this fixture; the
            // caveat then belongs to the panel, which shows it either way.
            assert!(pl.gg.is_some(), "the overhang check must still have run");
            return;
        };
        let r = report(&m, None, Method::GoldenGate, &e, 25, &pl, i).expect("a record");
        assert!(
            r.contains("fidelity"),
            "no caveat in a Golden Gate record: {r}"
        );
        assert!(r.contains("Type IIS overhangs"), "{r}");
    }

    /// The step between the two this panel already did.
    ///
    /// PROVEN TO FAIL against da9fff7: `Method` had three variants and none of
    /// them amplified anything. `pl_clone::pcr` has been in the crate the whole
    /// time, cross-checked against pydna over 29 cases, and `pl-clone` has been
    /// a GUI dependency since Stage 4 — so the app could design a primer pair
    /// and could assemble fragments and could not make the amplicon in between.
    /// A user had to leave, run `pl pcr`, and come back.
    ///
    /// The product is checked BY SEQUENCE against the template's own bases, not
    /// by length: a product of the right length from the wrong arc of a circle
    /// is the exact failure the named `Primers` type exists to prevent, and
    /// length cannot see it.
    #[test]
    fn a_pair_of_primers_amplifies_the_span_between_them() {
        let seq = "AAAAGGATCCTTTTGCGCGCATATATCCCGGGAAAATTTTCCCCGGGGAAAACCCCTTTT";
        let mut m = mol(seq, false);
        m.name = "pTest".into();
        // 18 nt each, comfortably over the 12 nt specificity floor.
        let fwd = &seq[4..22];
        let rev =
            String::from_utf8(pl_core::reverse_complement(&seq.as_bytes()[38..56])).expect("ascii");

        let p = plan(
            &m,
            None,
            Method::Pcr,
            &BTreeSet::new(),
            &Primers {
                forward: fwd.into(),
                reverse: rev.clone(),
            },
            false,
            25,
        );
        assert!(p.note.is_none(), "{:?}", p.note);
        assert_eq!(p.prods.len(), 1, "a PCR has one product");
        let prod = &p.prods[0];
        assert!(!prod.circular, "an amplicon is linear");
        assert_eq!(
            String::from_utf8_lossy(&prod.mol.seq),
            seq[4..56],
            "the amplicon is not the span between the two primers"
        );
        // The record names both oligos, or nobody can repeat it.
        let r = report(&m, None, Method::Pcr, &BTreeSet::new(), 25, &p, 0).expect("a record");
        assert!(
            r.contains(fwd) && r.contains(&rev),
            "the record omits an oligo: {r}"
        );
        assert!(r.contains("amplified with"), "{r}");
    }

    /// A swapped pair is refused, on a line and on a circle alike.
    ///
    /// WRITTEN TO CONFIRM THE OPPOSITE, and it is here because it did not. The
    /// design this feature was built from asserted that a swap silently
    /// amplifies the complement arc of a circle — a plausible and frightening
    /// claim, and the stated reason for making `Primers` a named type. Measured,
    /// it is false: an oligo written as a reverse primer IS the reverse
    /// complement of its site, so as a forward primer it has nothing on the plus
    /// strand to bind, and topology does not change that.
    ///
    /// So the named type is justified by its call sites — about twenty, two
    /// adjacent `&str` among them — and not by a silent wrong answer, and the
    /// doc comment on `Primers` says so. This test is what stops that claim
    /// coming back.
    #[test]
    fn the_two_oligos_are_not_interchangeable() {
        let seq = "AAAAGGATCCTTTTGCGCGCATATATCCCGGGAAAATTTTCCCCGGGGAAAACCCCTTTT";
        let fwd = &seq[4..22];
        let rev =
            String::from_utf8(pl_core::reverse_complement(&seq.as_bytes()[38..56])).expect("ascii");
        let swapped = Primers {
            forward: rev.clone(),
            reverse: fwd.into(),
        };

        // Linear: the swap is refused, and the refusal says why.
        let line = plan(
            &mol(seq, false),
            None,
            Method::Pcr,
            &BTreeSet::new(),
            &swapped,
            false,
            25,
        );
        assert!(line.prods.is_empty());
        let line_note = line.note.clone().unwrap_or_default();
        // "does not anneal" rather than "face away", and the distinction is
        // real: the swapped reverse oligo is a plus-strand sequence, which has
        // nothing to bind as a reverse primer. `Inverted` is what you get from
        // two primers that DO both anneal and point outwards.
        assert!(
            line_note.contains("does not anneal"),
            "the swap on a line was not named: {line_note:?}"
        );

        // Circular too, which is the half that was supposed to be dangerous.
        let circle = plan(
            &mol(seq, true),
            None,
            Method::Pcr,
            &BTreeSet::new(),
            &swapped,
            false,
            25,
        );
        assert!(
            circle.prods.is_empty(),
            "a swapped pair amplified something on a circle — if this ever fires, the              complement-arc hazard is real after all and the panel needs to say so"
        );

        // ...and the fixture is not simply inert: the RIGHT way round works on
        // both topologies, so the refusals above are about the swap and not
        // about the sequences.
        let right = Primers {
            forward: fwd.into(),
            reverse: rev,
        };
        for circular in [false, true] {
            let p = plan(
                &mol(seq, circular),
                None,
                Method::Pcr,
                &BTreeSet::new(),
                &right,
                false,
                25,
            );
            assert_eq!(p.prods.len(), 1, "circular={circular}: {:?}", p.note);
        }
    }

    /// A tailed primer's product is not a stretch of the template, and the
    /// panel must say THAT rather than "could not be placed".
    ///
    /// Adding a site the template lacks is the entire reason anyone puts a tail
    /// on a primer. `pcr` has already refused any pair whose 12 nt seed binds
    /// twice, so an amplicon it returns occurs at most once — the digest's
    /// sentence about ambiguity is simply not what happened here, and telling a
    /// user their file is ambiguous when it is not sends them looking for a
    /// repeat that does not exist.
    #[test]
    fn a_tailed_primer_is_told_apart_from_an_ambiguous_fragment() {
        let seq = "AAAAGGATCCTTTTGCGCGCATATATCCCGGGAAAATTTTCCCCGGGGAAAACCCCTTTT";
        let m = mol(seq, false);
        let fwd = format!("GAATTC{}", &seq[4..22]);
        let rev =
            String::from_utf8(pl_core::reverse_complement(&seq.as_bytes()[38..56])).expect("ascii");
        let p = plan(
            &m,
            None,
            Method::Pcr,
            &BTreeSet::new(),
            &Primers {
                forward: fwd,
                reverse: rev,
            },
            false,
            25,
        );
        assert_eq!(p.prods.len(), 1);
        assert!(
            p.prods[0].mol.seq.starts_with(b"GAATTC"),
            "the tail is not in the product, which is the whole point of a tail"
        );
        let f = &p.frags[0];
        assert!(
            f.from.is_none(),
            "a tailed product is not a stretch of the template"
        );
        let why = f.unplaced.as_deref().unwrap_or("");
        assert!(
            why.contains("tail"),
            "the panel blames ambiguity for a tail: {why:?}"
        );
    }

    /// A refusal must name a character the user can see.
    ///
    /// `PcrError::NotDna` formats the offending character with `{found:?}`, and
    /// `char`'s Debug keeps a printable non-ASCII one literally. This binary's
    /// proportional chain has no CJK, and the tofu oracle already pins U+4E2D as
    /// a box in both families — so the error painted straight through would tell
    /// a user their primer contains an empty rectangle, the one shape that
    /// cannot say which character to delete.
    #[test]
    fn a_primer_with_an_invisible_character_is_named_by_codepoint() {
        let m = mol("AAAAGGATCCTTTTGCGCGCATATATCCCGGG", false);
        let p = plan(
            &m,
            None,
            Method::Pcr,
            &BTreeSet::new(),
            &Primers {
                forward: "AAAAGGATCC\u{4e2d}".into(),
                reverse: "CCCGGG".into(),
            },
            false,
            25,
        );
        let note = p.note.expect("a refusal");
        assert!(
            note.contains("U+4E2D"),
            "the refusal does not name the character in a form that renders: {note}"
        );
        assert!(
            !note.contains('\u{4e2d}'),
            "the refusal paints the character it is complaining about: {note}"
        );
        // AND THE OTHER HALF, which is what stops this being a test about one
        // string: an ordinary absence must not be dressed up as a bad character.
        let p = plan(
            &m,
            None,
            Method::Pcr,
            &BTreeSet::new(),
            &Primers {
                forward: "GGGGGGGGGGGGGGGG".into(),
                reverse: "CCCGGG".into(),
            },
            false,
            25,
        );
        let n2 = p.note.unwrap_or_default();
        assert!(
            n2.contains("does not anneal") && !n2.contains("U+"),
            "a primer that simply is not there was reported as a bad character: {n2}"
        );
    }

    /// Naming the molecule that was not cut, rather than "this molecule".
    ///
    /// With one molecule "None of BamHI cuts this molecule" was unambiguous.
    /// With two it is the least useful sentence available: the one it does not
    /// name is the one the user has to go and look at.
    #[test]
    fn a_donor_that_the_enzymes_do_not_cut_is_named() {
        let mut v = mol("AAAAGGATCCTTTTGCGCGCATATATCCCGGGAAAATTTTCCCC", true);
        v.name = "cuttable".into();
        let mut d = mol("ACGTACGTACGTACGTACGTACGT", true);
        d.name = "uncuttable".into();
        let p = plan(
            &v,
            Some(&d),
            Method::Restriction,
            &ticked(&["BamHI"]),
            &Primers::default(),
            false,
            25,
        );
        let note = p.note.expect("a note");
        assert!(note.contains("uncuttable"), "{note}");
    }

    /// A feature spanning a cut site cannot travel whole, so it does not travel.
    #[test]
    fn a_feature_cut_in_half_is_dropped_and_counted() {
        let seq = "AAAAGGATCCTTTTGCGCGCATATATGGATCCAAAATTTTCCCC";
        let mut m = mol(seq, true);
        // Spans the second BamHI site.
        let mut f = Feature::new("straddles", "misc_feature");
        f.segments.push(Segment::new(25, 36));
        m.features.push(f);

        let p = plan(
            &m,
            None,
            Method::Restriction,
            &ticked(&["BamHI"]),
            &Primers::default(),
            false,
            25,
        );
        assert_eq!(p.frags.len(), 2, "two sites, two fragments");
        for prod in &p.prods {
            assert_eq!(prod.carried, 0, "a straddling feature must not travel");
            assert!(prod.dropped >= 1, "and it must be counted as dropped");
        }
    }

    /// Blunt ends are opt-in here for the same reason they are in the engine.
    #[test]
    fn blunt_products_appear_only_when_asked_for() {
        let m = mol("GATATCAAAAAAAAAAAAAAAAAAAAGATATCTTTTTTTTTT", true);
        let off = plan(
            &m,
            None,
            Method::Restriction,
            &ticked(&["EcoRV"]),
            &Primers::default(),
            false,
            25,
        );
        assert!(off.prods.is_empty());
        assert!(off.note.unwrap().contains("blunt"));
        let on = plan(
            &m,
            None,
            Method::Restriction,
            &ticked(&["EcoRV"]),
            &Primers::default(),
            true,
            25,
        );
        assert!(
            !on.prods.is_empty(),
            "with blunt on there must be a product"
        );
    }

    /// An ambiguous fragment carries nothing rather than guessing.
    #[test]
    fn a_fragment_that_appears_twice_is_not_placed() {
        // The same 12-mer twice over: a fragment matching it has two homes.
        assert_eq!(
            locate("ACGTACGTACGT", "ACGTACGTACGTAAAACGTACGTACGT", false),
            None
        );
        // One home is fine.
        assert_eq!(locate("TTTT", "AAAATTTTGGGG", false), Some((5, 8)));
        // A fragment longer than the parent is not a fragment of it.
        assert_eq!(locate("AAAAAAAA", "AAAA", false), None);
    }
}
