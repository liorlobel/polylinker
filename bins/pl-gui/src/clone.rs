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
//! THAT INTERVAL IS MODULAR when the parent is a circle. A plasmid cut once
//! gives a fragment that begins at the cut, runs to the end of the file and
//! carries on from base 1 — so "inside the fragment" is a statement about an
//! ARC and not about `fs <= x <= fe`, and [`locate`] duly reports an end past
//! the parent's length to say so. Reading that interval as a plain one left
//! every feature upstream of the cut out of a religation that is otherwise the
//! plasmid you started with; [`place`] is where the arithmetic now lives.
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
    /// Where this fragment sits in its parent, when it could be placed.
    ///
    /// 1-based, inclusive, and **not always an interval**: `(fs, fe)` with `fe`
    /// past the parent's length is a fragment that runs off the end of the file
    /// and continues at base 1, which is what a fragment of a circle does unless
    /// a cut happens to land on the origin. A plasmid cut once with a unique
    /// enzyme is the everyday case — its single fragment comes back as
    /// `(cut + 1, cut + n)`.
    ///
    /// So neither number may be shown to a user as it stands, and neither may be
    /// compared with `<=` to decide whether a feature is inside: [`span`]
    /// renders it and [`place`] tests it, both modulo the parent's length. Read
    /// as a plain interval, `fe` names a base the parent does not have — "from
    /// 6..49" for a 44 bp plasmid — and every feature in the wrapped tail is
    /// declared outside a fragment that physically contains it.
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
    ///
    /// `dropped` is counted ONCE PER FEATURE, over the union of the parents this
    /// construct is made of — every feature of a contributing molecule either
    /// travelled or did not, so `carried + dropped` is exactly that union's
    /// size. It used to be a running tally taken inside the walk over fragments,
    /// which counted a parent's whole feature list once for every piece of it
    /// this product used: a two-fragment religation that loses nothing reported
    /// "10 carried, 10 dropped", and the methods paragraph said the same in
    /// words a user pastes into a notebook.
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
    // What methylation does to each enzyme in `pl_enzymes::ENZYMES`, by name.
    //
    // THE FIFTH SURFACE, and it was the only one silent about this. The Enzymes
    // tab, the map's tooltip, the sequence view and the gel all strike a blocked
    // enzyme through, chip it with the methylase and say so on hover; this panel
    // drew a bare checkbox. It is also the panel whose output is a CONSTRUCT, so
    // the cost of not saying it is a user planning a digest the enzyme will not
    // perform and finding out at the bench.
    //
    // By name rather than by index into `results()`: this panel iterates
    // `pl_enzymes::ENZYMES` and the digest is indexed by whatever the Enzymes
    // tab is showing, so an index would silently pair the wrong verdict with the
    // wrong enzyme the moment a filter is applied.
    methylation: &std::collections::HashMap<&'static str, crate::doc::Methylated>,
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
                    // The same three channels the other four surfaces use:
                    // strikethrough, a chip naming the methylase and the count,
                    // and a sentence on hover. Colour is never the only one.
                    let v = methylation.get(e.name);
                    let dead = v.is_some_and(|m| m.all_blocked());
                    let label = if dead {
                        egui::RichText::new(e.name).strikethrough().color(pal.warn)
                    } else {
                        egui::RichText::new(e.name)
                    };
                    let mut resp = ui.checkbox(&mut on, label);
                    if let Some(m) = v {
                        resp = resp.on_hover_text(format!(
                            "{}. This preparation blocks {}.{}",
                            m.chip(),
                            m.of_sites(m.blocked),
                            if m.all_blocked() {
                                " Selecting it will not cut this molecule."
                            } else {
                                ""
                            }
                        ));
                    }
                    if resp.changed() {
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
                                // THROUGH `span`, because `from` is not an
                                // interval on a circle: printing its second
                                // number raw gave "from 6..49" for a 44 bp
                                // plasmid, a coordinate the molecule does not
                                // have, in the list a user reads to work out
                                // which band is their vector.
                                (Some(from), _) => format!(
                                    "   from {}",
                                    span(from, parent_len(f, mol, donor_mol))
                                ),
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
                    // THE REASONS, all of them. This sentence used to name two —
                    // cut in half, or from an ambiguous fragment — while the
                    // count also included every feature of a parent whose other
                    // fragments this construct does not use, which is most of a
                    // vector in any subcloning. A hover that lists two causes
                    // for a number produced by four sends a user hunting for
                    // damage that is not there.
                    ui.label(carried).on_hover_text(
                        "A feature travels only when its whole span sits inside one fragment \
                         this construct uses, and that fragment could be placed in the parent. \
                         Anything cut in half, anything on a piece this construct leaves out, \
                         and anything from a fragment whose origin is ambiguous stays behind \
                         rather than being put at a coordinate that merely looks right.",
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
///
/// THE END MAY EXCEED THE PARENT'S LENGTH, and deliberately: the match is taken
/// in the doubled string and returned as `(s + 1, s + len)` with no `% n`, so a
/// fragment that crosses the origin says "I start at 6 and run 44 bases" as
/// `(6, 49)` on a 44 bp plasmid rather than as the empty-looking `(6, 5)`. The
/// two callers that show a number to a user fold it back through [`span`]; the
/// one that decides whether a feature is inside works modulo `n` in [`place`].
/// Nobody may treat the pair as a plain interval — see [`Frag::from`].
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

/// A fragment's parent span, written in coordinates the parent actually has.
///
/// [`locate`] reports an origin-crossing fragment with its end past the parent's
/// length, because that is the only way to say "44 bases starting at 6" in one
/// pair of numbers. Shown raw, that is a lie about the molecule: the fragment
/// list printed "from 6..49" for a 44 bp plasmid, and the methods paragraph
/// printed "assembled from 1201..5400" for a 5,000 bp one — text written to be
/// pasted into a manuscript, naming a base nobody can go and look at.
///
/// So the end is folded back into `1..=n` and the wrap is SAID rather than left
/// for the reader to infer, because `6..5` on its own reads like an off-by-one
/// or an empty range instead of what it is: a fragment that runs off the end of
/// the file and continues at base 1.
fn span((fs, fe): (u64, u64), n: u64) -> String {
    // A parent with no bases cannot have placed a fragment at all, so this arm
    // is unreachable through `plan`; it exists so the `%` below can never be a
    // division by zero on a hand-built `Frag`.
    if n == 0 || fe == 0 {
        return format!("{fs}..{fe}");
    }
    let end = (fe - 1) % n + 1;
    if fe > n {
        format!("{fs}..{end} through the origin")
    } else {
        format!("{fs}..{end}")
    }
}

/// How long the molecule this fragment came out of is.
///
/// Measured the way [`locate`] measured it — through `from_utf8_lossy` — rather
/// than as `seq.len()`, so the modulus used to fold a coordinate is the same
/// number the coordinate was produced against. For an ASCII molecule, which is
/// every molecule that reaches here, the two are equal; for one carrying a stray
/// byte they are not, and a modulus that is one base out puts the fold one base
/// out with it.
fn parent_len(f: &Frag, mol: &Molecule, donor: Option<&Molecule>) -> u64 {
    let m = match f.parent {
        0 => mol,
        // A fragment attributed to a donor that is no longer there cannot
        // happen — `plan` builds both lists in one pass — and falling back to
        // the open molecule keeps that impossibility from becoming a panic.
        _ => donor.unwrap_or(mol),
    };
    String::from_utf8_lossy(&m.seq).len() as u64
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
            // THROUGH `span`, for the reason that function gives: this sentence
            // is written to be pasted into a methods section, and "assembled
            // from 1201..5400" on a 5,000 bp plasmid states as fact a position
            // that molecule does not have.
            let at = match f.from {
                Some(from) => span(from, parent_len(f, mol, donor)),
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
        // THE REASON, corrected twice over. The clause used to say only "did not
        // sit whole inside one placeable fragment", which left out the other
        // half of what the number counts — a feature on a piece this construct
        // does not use, which in a subcloning is most of the donor. And it
        // carried a run of fourteen spaces mid-sentence, a heredoc artifact that
        // reached the clipboard exactly as written.
        out.push_str(&format!(
            "; {} were not, because their span did not sit whole inside one placeable \
             fragment this construct used",
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
///
/// The per-feature arithmetic is [`place`]'s, and the reason it is a function
/// rather than four lines inside the walk below is that it has to know the
/// fragment's GEOMETRY — both strand lengths and `ovhg` — and not merely where
/// the fragment sits. A mirror written from the watson length alone put every
/// feature of an inverted fragment one overhang out of place.
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
    // WHICH features travelled, not how many times a fragment failed to take
    // one. `dropped` used to be incremented inside this loop, which runs once
    // per FRAGMENT while `parent.features` is the whole parent's list — so every
    // feature was counted as dropped once for each fragment it is not in, and a
    // two-piece religation that loses nothing printed "10 carried, 10 dropped".
    // `carried + dropped` exceeding the parent's feature count is
    // self-contradictory on its face, and it was on screen and in the methods
    // paragraph. The count is taken once, after the walk, from this set.
    let mut travelled: BTreeSet<(usize, usize)> = BTreeSet::new();
    for (idx, flipped) in order {
        let parent = sources[described[*idx].parent].mol;
        // THE LAYOUT'S BASIS IS WATSON'S LENGTH, which is what `locate` measured
        // the fragment against and what `join` concatenates for an unflipped
        // piece. `Dseq::len()` spans the single-stranded ends too, so using it
        // here would drift the layout by an overhang per junction.
        //
        // It is NOT the basis for the mirror below — see [`place`], which takes
        // the crick length and `ovhg` for that — and it MUST stay watson for
        // `place`'s interval test, because a feature's offset arrives in the
        // parent's coordinates and those are watson's.
        //
        // **THE CURSOR IS A SEPARATE QUESTION, AND IT IS WHERE THIS WAS WRONG.**
        // A flipped fragment contributes its CRICK length to the product. The
        // two agree whenever a fragment's ends leave overhangs of equal width —
        // every fragment of a single-enzyme digest, and every fragment of a
        // digest by enzymes sharing a width — and otherwise differ by exactly
        // the difference in those widths, so every feature laid down after the
        // flipped fragment shifts by that much.
        //
        // This advanced by watson in both cases, behind a comment claiming that
        // "no digest is known that both produces such a fragment and seals it
        // flipped, so it is guarded rather than corrected here". That sentence
        // shipped in v0.10.1 and was false when it was written. Ten common
        // enzymes swept pairwise over a four-fragment circle: 256 of 848
        // flipped placements are of a fragment whose ends differ in width, and
        // 43 of 840 carried features landed on bases that are not their own —
        // shifted by two for NdeI against a four-base cutter, by four for blunt
        // SmaI. `place`'s bounds check does NOT catch it, because the shifted
        // coordinate is still inside the product: the feature is placed,
        // silently, on the wrong bases, in a construct somebody orders primers
        // against. A double digest is the most ordinary thing this panel does.
        //
        // See `a_flipped_fragment_does_not_shift_the_features_behind_it`.
        let flen = frags[*idx].watson.len() as u64;
        if let Some((fs, _)) = described[*idx].from {
            let slot = Slot {
                fs,
                // The parent's own length, as `locate` measured it, so the
                // interval below can be modular. An unplaceable fragment needs
                // none of this: nothing from it travels, and the features it
                // would have carried are counted as dropped by not being in
                // `travelled`.
                n: sources[described[*idx].parent].seq.len() as u64,
                watson: flen,
                crick: frags[*idx].crick.len() as i64,
                ovhg: frags[*idx].ovhg,
                flipped: *flipped,
                at,
                product: full.len() as u64,
            };
            for (fi, f) in parent.features.iter().enumerate() {
                let Some(segments) = place(f, &slot) else {
                    continue;
                };
                let mut moved = f.clone();
                moved.segments = segments;
                if *flipped {
                    moved.strand = match f.strand {
                        Strand::Forward => Strand::Reverse,
                        Strand::Reverse => Strand::Forward,
                        other => other,
                    };
                }
                mol.features.push(moved);
                travelled.insert((described[*idx].parent, fi));
                carried += 1;
            }
        }
        at += if *flipped {
            frags[*idx].crick.len() as u64
        } else {
            flen
        };
    }

    // COUNTED ONCE, over the union of the parents this construct is made of.
    // Every feature of a contributing parent either travelled or did not, and
    // the ones that did not are exactly what "dropped" has always claimed to
    // mean. `saturating_sub` cannot fire — a fragment carries a feature at most
    // once and `travelled` is keyed by (parent, feature), so it can never hold
    // more entries than the parents have features — and it is written this way
    // rather than as `-` so that a future third caller of `build` cannot turn a
    // miscount into a panic.
    let dropped = used_parents
        .iter()
        .map(|p| sources[*p].mol.features.len())
        .sum::<usize>()
        .saturating_sub(travelled.len());

    Prod {
        mol,
        circular: seq.circular,
        order: order.to_vec(),
        carried,
        dropped,
        junctions,
    }
}

/// One fragment as it sits in the finished product: everything [`place`] needs
/// to turn a parent coordinate into a product coordinate, and nothing else.
///
/// A struct rather than eight arguments because the geometry travels together
/// and half of it is easy to leave behind — which is precisely what happened:
/// the mirror for a flipped fragment was written from `watson` alone, and
/// `crick` and `ovhg` were never consulted anywhere in the walk.
struct Slot {
    /// 1-based position in the parent of the fragment's first watson base.
    fs: u64,
    /// The parent's length, so the fragment's interval can be modular.
    n: u64,
    /// The fragment's watson length: how far the fragment reaches along the
    /// parent, and how far the product's cursor advances past it.
    watson: u64,
    /// The fragment's crick length. `i64` because the mirror subtracts from it.
    crick: i64,
    /// The fragment's `ovhg`: where crick's 3' end sits relative to watson's 5'
    /// start, negative when watson protrudes on the left.
    ovhg: i64,
    /// Laid in end-for-end, so the product takes `crick` where the parent's
    /// coordinates were measured along `watson`.
    flipped: bool,
    /// 0-based offset of this slot's first base in the product.
    at: u64,
    /// The product's length, for the bounds check at the end.
    product: u64,
}

/// Where a parent's feature lands in the product, if it lands at all.
///
/// # Whole or not at all
///
/// Half a promoter carried into a construct is a claim the parent never made,
/// and its coordinates would be right while the biology was wrong — so the
/// segments are collected into an `Option` and one that cannot be placed refuses
/// the whole feature. A feature with no segments is not a span to carry either;
/// that guard came from `within`, which this function replaces.
///
/// # The interval is modular
///
/// `slot.fs` and the fragment's watson length describe an ARC of the parent, not
/// an interval: a plasmid cut once gives a fragment that starts at the cut, runs
/// off the end of the file and continues at base 1. Testing `s.start >= fs` left
/// every feature in that wrapped tail out of a religation that is otherwise the
/// plasmid you started with — on a real vector cut in its MCS, that is every
/// feature upstream of the MCS — and told the user they had been "cut in half".
/// `(s.start + n - fs) % n` is the offset that answers the question the plain
/// comparison was asking.
///
/// # The mirror is crick's, not watson's
///
/// A flipped fragment contributes `flip(f).watson`, which IS `f.crick`, and
/// crick is the reverse complement of a window offset from watson's by the
/// overhang: `crick[j] = comp(full[b1 - 1 - j])` with `b1 = t0 - ovhg + c`. With
/// `a = p - t0` that gives `j = c - 1 - ovhg - a`, and the `w - 1 - a` this code
/// used before agrees with it only when `ovhg == c - w`. For the ordinary
/// restriction fragment — the same 5' overhang at both ends, so `c == w` and
/// `ovhg == -k` — it was `k` too small, and k is 4 for EcoRI, BamHI, BglII,
/// HindIII, XhoI, SalI, NheI, SpeI, XbaI and every other four-base 5' cutter.
/// Every feature on an inverted fragment was four bases to the 5' side of its
/// own sequence: on a CDS, a frameshift in what the map draws and what the
/// exported GenBank claims, in a construct the user opens and orders primers
/// against.
///
/// # Off the end of the product
///
/// `j` can fall outside `0..watson`, and legitimately so: the first `k` bases of
/// a flipped fragment's watson are its 5' overhang, which is single-stranded, so
/// their partners in the product come from the NEXT fragment's overhang — the
/// two annealed, which is what the junction is. `at + j` lands there and is
/// right. It is right, that is, when there IS a next fragment: for the last
/// fragment of a circle the same arithmetic runs past the end of the molecule,
/// where the bases are real but their coordinate is on the other side of the
/// origin. Rather than emit a position the product does not have, or invent the
/// two-segment wrapped feature this module refuses to carry in the other
/// direction, such a feature does not travel and is counted as dropped.
///
/// # The mirror reverses the LIST, not only each span
///
/// Mapping `a < b` to `m - b < m - a` puts each individual span on the bases it
/// names and STILL leaves a multi-exon feature wrong, because the mirror also
/// swaps which exon comes first: the parent's leading exon is the product's
/// trailing one. Collected in the parent's order the list therefore comes back
/// DESCENDING, and nothing downstream repairs it. `genbank::write` emits the
/// parts in stored order (`format_location`), `aa.rs` reverses them to READ a
/// Reverse feature rather than to sort them, and `pl convert --to genbank
/// --stdout` returns either spelling byte for byte — measured, both ways round.
///
/// That order is not free-floating. `Feature::segments` is stored in JOIN order
/// and a Reverse feature is read back to front — `crates/pl-fileio/src/genbank.rs`
/// states the convention where it reverses the parts of a
/// `join(complement(a),complement(b))`, and `aa.rs` applies it, checked there
/// against pKoV SacB's stored `/translation`. So a descending list is written as
/// `complement(join(hi,lo))`, and everything that splices that — this program's
/// own amino-acid track, and Biopython 1.87, measured — reads rc(lo) then
/// rc(hi): **THE EXONS COME OUT SWAPPED.** A two-exon CDS (a real intron, or an
/// origin-crossing feature, which GenBank stores as a two-part join) travelling
/// on an inverted fragment therefore exported a spliced product that is not the
/// gene, at coordinates sitting comfortably inside the construct with nothing on
/// screen saying so: on the fixture in
/// `an_inverted_multi_exon_feature_keeps_its_exons_in_splice_order`, `CCATCTTG`
/// where the parent splices `CTTGCCAT`. The defect is symmetric — a Reverse
/// parent feature flipped to Forward was stored wrong the same way.
///
/// The reversal is conditioned on `flipped` and must stay so. An unflipped
/// fragment's mapping is order-preserving (`a`, `b` both increase with
/// `s.start`), so reversing there would break the ordinary case the same way.
///
/// NOT A REGRESSION, though `place` is new code: the inline `.map(...).collect()`
/// it replaced, at `f0e4a6f:1512-1526`, collected in the parent's order too. The
/// behaviour was carried forward, not introduced.
fn place(f: &Feature, slot: &Slot) -> Option<Vec<Segment>> {
    if f.segments.is_empty() {
        return None;
    }
    let mut out = f
        .segments
        .iter()
        .map(|s| {
            // A coordinate the parent does not have is not a coordinate.
            // `s.start >= 1` and `s.end <= n` were implied by the old interval
            // test (`s.start >= fs >= 1`, `s.end <= fe`) and have to be said out
            // loud now that the test is modular, which would otherwise fold a
            // nonsense position into a real one. `s.start <= s.end` refuses a
            // feature that itself crosses the origin: it cannot be expressed as
            // one span in the product either.
            if slot.n == 0 || s.start == 0 || s.start > s.end || s.end > slot.n {
                return None;
            }
            let a = (s.start + slot.n - slot.fs) % slot.n;
            let b = a + (s.end - s.start);
            // Whole span inside the fragment, measured along the arc from its
            // start rather than between two endpoints.
            if b >= slot.watson {
                return None;
            }
            let (lo, hi) = if slot.flipped {
                let m = slot.crick - 1 - slot.ovhg;
                // `b` maps low and `a` maps high: the mirror reverses the order
                // WITHIN a span, and — see below — between them as well.
                (m - b as i64, m - a as i64)
            } else {
                (a as i64, b as i64)
            };
            let (lo, hi) = (slot.at as i64 + lo + 1, slot.at as i64 + hi + 1);
            if lo < 1 || hi > slot.product as i64 {
                return None;
            }
            Some(Segment::new(lo as u64, hi as u64))
        })
        .collect::<Option<Vec<Segment>>>()?;
    // Back into join order. Each span is already on the right bases; this is
    // the exon ORDER, which the mirror inverted along with everything else.
    if slot.flipped {
        out.reverse();
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    /// The clone panel says the same thing about a blocked enzyme that the other
    /// four surfaces say.
    ///
    /// **THE PANEL WHOSE OUTPUT IS A CONSTRUCT WAS THE ONE THAT SAID NOTHING.**
    /// The Enzymes tab, the map tooltip, the sequence view and the gel all
    /// strike a blocked enzyme through and name the methylase; `clone::show`
    /// drew a bare checkbox gated only on `cut_positions`, so a user could plan
    /// a digest the enzyme will not perform and find out at the bench.
    ///
    /// Asserted on `doc::Methylated`'s own accessors rather than on the widget,
    /// because those are what the panel now renders and what the other four
    /// surfaces already rendered — the point is that one type answers for all
    /// five. A test that re-implemented the phrasing would let the five drift
    /// while staying green.
    ///
    /// PROVEN TO FAIL by reverting `all_blocked()` to `blocked > 0`: an enzyme
    /// with one dead site out of four is then struck through as though it does
    /// not cut, which is the mirror defect `doc::Methylated` was introduced to
    /// remove and which this panel would otherwise have reintroduced.
    #[test]
    fn a_blocked_enzyme_reads_the_same_here_as_in_the_enzymes_tab() {
        use pl_enzymes::methylation::{Effect, Methylase, SiteEffect};

        let site = |effect| SiteEffect {
            methylase: Methylase::Dam,
            effect,
        };

        // Every site dead: struck through, and the hover says so.
        let all_dead = crate::doc::Methylated {
            worst: site(Effect::Blocked),
            total: 2,
            blocked: 2,
            affected: 2,
        };
        assert!(
            all_dead.all_blocked(),
            "two of two blocked is a dead enzyme"
        );
        assert_eq!(all_dead.live(), 0);
        assert!(
            all_dead.chip().contains("all 2 sites"),
            "the chip must carry the count: {}",
            all_dead.chip()
        );

        // One of four: methylation is worth saying, and the enzyme still cuts.
        // This is the case the strikethrough must NOT claim.
        let partial = crate::doc::Methylated {
            worst: site(Effect::Blocked),
            total: 4,
            blocked: 1,
            affected: 1,
        };
        assert!(
            !partial.all_blocked(),
            "an enzyme with three live sites still cuts and must not be drawn as dead"
        );
        assert_eq!(partial.live(), 3);
        assert!(
            partial.chip().contains("1 of 4 sites"),
            "the chip must say how many of how many: {}",
            partial.chip()
        );
    }

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

    /// The bases a molecule really has over a 1-based inclusive span.
    ///
    /// THE ORACLE FOR EVERY FEATURE ASSERTION BELOW, and it is deliberately not
    /// an assertion about coordinates. The whole failure mode of a feature remap
    /// is a span that sits comfortably inside the construct and names the wrong
    /// bases; a number compared against a number cannot see that, and every
    /// placement defect this file's tests exist to hold down — an inverted
    /// fragment mirrored on the wrong strand length, a fragment interval read as
    /// though a circle had no origin, and a product cursor advanced by watson
    /// past a fragment that contributes crick — produces coordinates that look
    /// entirely reasonable.
    ///
    /// NECESSARY AND NOT SUFFICIENT, which the list above once implied it was.
    /// A multi-exon feature can have every one of its spans on exactly the right
    /// bases and still be the wrong gene, because the EXONS can be in the wrong
    /// order; reading each span says nothing about that. That one is caught by
    /// splicing the spans rather than reading them —
    /// `an_inverted_multi_exon_feature_keeps_its_exons_in_splice_order` composes
    /// this with `rc` to do it.
    fn bases(m: &Molecule, a: u64, b: u64) -> String {
        let seq = String::from_utf8_lossy(&m.seq).to_string();
        seq[(a - 1) as usize..b as usize].to_ascii_uppercase()
    }

    /// The carried feature of that name, or a failure that says which is missing.
    fn named<'a>(m: &'a Molecule, name: &str) -> &'a Feature {
        m.features
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("{name} did not travel into the product"))
    }

    fn rc(s: &str) -> String {
        String::from_utf8(pl_core::reverse_complement(s.as_bytes())).expect("ASCII in, ASCII out")
    }

    /// A flipped fragment must not move the features laid down behind it.
    ///
    /// PROVEN TO FAIL at d8c218b (v0.10.1), where this reported `misplaced=43`
    /// of 840 carried features: `build` advanced its product cursor by the
    /// fragment's WATSON length in every case, while a flipped fragment
    /// contributes its CRICK length. The two differ by the difference in the
    /// two ends' overhang widths, so on a digest by enzymes of unequal width
    /// every feature laid down after a flipped fragment shifted by that much --
    /// NdeI's two against a four-base cutter, blunt SmaI's zero against four.
    ///
    /// TO RE-BREAK IT: in `build`, replace the whole `at +=` expression with
    /// `at += flen;`.
    ///
    /// **A TWO-FRAGMENT FIXTURE CANNOT SEE THIS, AND THE FIRST ATTEMPT USED
    /// ONE.** Two fragments of a double digest carry one end of each enzyme, so
    /// neither can be sealed either way round and nothing ever flips: that run
    /// reported `flip=0` and passed against the broken code. Four fragments --
    /// two sites of each enzyme -- is the smallest fixture that both flips and
    /// mismatches. The `flip` and `mismatch` counters are ASSERTED, not merely
    /// printed, so an edit that stops reaching the case fails here instead of
    /// going quietly green.
    #[test]
    fn a_flipped_fragment_does_not_shift_the_features_behind_it() {
        let names = [
            "EcoRI", "BamHI", "NdeI", "SmaI", "PstI", "SacI", "KpnI", "XhoI", "HindIII", "SalI",
        ];
        let mut checked = 0usize;
        let mut flip = 0usize;
        let mut mismatch = 0usize;
        let mut bad: Vec<String> = Vec::new();
        for a in 0..names.len() {
            for b in 0..names.len() {
                if a == b {
                    continue;
                }
                let (ea, eb) = (names[a], names[b]);
                let sa = match pl_enzymes::ENZYMES.iter().find(|e| e.name == ea) {
                    Some(e) => e.site.to_string(),
                    None => continue,
                };
                let sb = match pl_enzymes::ENZYMES.iter().find(|e| e.name == eb) {
                    Some(e) => e.site.to_string(),
                    None => continue,
                };
                if !sa.bytes().all(|c| b"ACGT".contains(&c))
                    || !sb.bytes().all(|c| b"ACGT".contains(&c))
                {
                    continue;
                }
                // Two sites of each enzyme, so a fragment can carry the same
                // enzyme at both ends and therefore be sealed either way round.
                let seq = format!(
                    "{}CCCCTTTAAAGGG{}CCCAAATTTCCC{}GGGTTTAAACCC{}TTTGGGAAA",
                    sa, sb, sa, sb
                );
                let mut m = mol(&seq, true);
                m.name = "p".into();
                let mut f1 = Feature::new("alpha", "CDS");
                f1.strand = Strand::Forward;
                f1.segments
                    .push(Segment::new((sa.len() + 3) as u64, (sa.len() + 12) as u64));
                m.features.push(f1);
                let mut f2 = Feature::new("beta", "CDS");
                f2.strand = Strand::Forward;
                let off = sa.len() + 16 + sb.len();
                f2.segments
                    .push(Segment::new((off + 3) as u64, (off + 12) as u64));
                m.features.push(f2);

                let p = plan(
                    &m,
                    None,
                    Method::Restriction,
                    &ticked(&[ea, eb]),
                    &Primers::default(),
                    true,
                    25,
                );
                if p.note.is_some() {
                    continue;
                }
                let parent = String::from_utf8_lossy(&m.seq)
                    .to_string()
                    .to_ascii_uppercase();
                for pr in &p.prods {
                    for (i, fl) in pr.order.iter() {
                        if *fl {
                            flip += 1;
                            if p.frags[*i].left.len() != p.frags[*i].right.len() {
                                mismatch += 1;
                            }
                        }
                    }
                    let prod = String::from_utf8_lossy(&pr.mol.seq)
                        .to_string()
                        .to_ascii_uppercase();
                    for f in &pr.mol.features {
                        let s = &f.segments[0];
                        if s.start < 1 || s.end as usize > prod.len() || s.start > s.end {
                            continue;
                        }
                        let got = prod[(s.start - 1) as usize..s.end as usize].to_string();
                        let ws = m
                            .features
                            .iter()
                            .find(|g| g.name == f.name)
                            .expect("the parent feature")
                            .segments[0]
                            .clone();
                        let want_f = parent[(ws.start - 1) as usize..ws.end as usize].to_string();
                        let want_r = String::from_utf8_lossy(&pl_core::reverse_complement(
                            want_f.as_bytes(),
                        ))
                        .to_string();
                        checked += 1;
                        if got != want_f && got != want_r {
                            bad.push(format!(
                                "{ea}+{eb} {} at {}..{} reads {got}, parent says {want_f}",
                                f.name, s.start, s.end
                            ));
                        }
                    }
                }
            }
        }
        eprintln!(
            "checked={checked} flip={flip} mismatch={mismatch} misplaced={}",
            bad.len()
        );
        assert!(
            checked > 500 && flip > 100 && mismatch > 100,
            "the sweep stopped reaching the case it exists for: checked={checked} flip={flip} mismatch={mismatch}"
        );
        assert!(
            bad.is_empty(),
            "{} carried feature(s) point at bases that are not their own; first: {}",
            bad.len(),
            bad.first().map(String::as_str).unwrap_or("")
        );
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
    ///
    /// PROVEN TO FAIL at f0e4a6f for its SECOND feature. `within(f, fs, fe)` was
    /// a plain interval test, and the one fragment of a circle cut once has
    /// `from = (6, 49)` on this 44 bp plasmid — it starts at the cut, runs off
    /// the end of the file and comes back at base 1. Every feature between base
    /// 1 and the cut therefore failed `s.start >= fs`, was counted as dropped,
    /// and was left out of a construct that is otherwise base-for-base the
    /// plasmid you started with. On a real vector cut in its MCS that is every
    /// feature upstream of the MCS.
    ///
    /// TO RE-BREAK IT: in `place`, replace
    /// `let a = (s.start + slot.n - slot.fs) % slot.n;` with
    /// `if s.start < slot.fs { return None; } let a = s.start - slot.fs;`.
    ///
    /// The fixture's first four bases were `AAAA` when this test only had to cut
    /// and re-close; they are `TCGA` because a feature read back off a run of A
    /// says nothing about where it landed.
    #[test]
    fn a_single_cut_religates_to_the_same_length_and_keeps_its_features() {
        let seq = "TCGAGGATCCTTTTGCGCGCATATATCCCGGGAAAATTTTCCCC";
        let mut m = mol(seq, true);
        let mut f = Feature::new("a gene", "CDS");
        f.strand = Strand::Forward;
        f.segments.push(Segment::new(15, 30));
        m.features.push(f);
        // ENTIRELY BEFORE THE CUT, which is the case a plain interval cannot
        // express: the fragment begins at base 6 and comes back round to base 5,
        // so these four bases are inside it and no comparison against `fs` will
        // ever say so.
        let mut before_cut = Feature::new("upstream", "misc_feature");
        before_cut.strand = Strand::Forward;
        before_cut.segments.push(Segment::new(1, 4));
        m.features.push(before_cut);

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
        // THE SHAPE OF THE INTERVAL, pinned. 49 is past the end of a 44 bp
        // molecule on purpose — it is how `locate` says "44 bases starting at
        // 6" — and everything downstream has to read it that way rather than as
        // `6..=49`.
        assert_eq!(
            p.frags[0].from,
            Some((6, 49)),
            "the fragment of a circle cut once wraps the origin"
        );
        assert_eq!(p.frags[0].left, "5' GATC");
        assert_eq!(p.prods.len(), 1, "{:?}", p.note);

        let prod = &p.prods[0];
        assert!(prod.circular);
        assert_eq!(
            prod.mol.seq.len(),
            seq.len(),
            "religation changed the length"
        );
        assert_eq!(prod.carried, 2, "a feature did not travel");
        assert_eq!(prod.dropped, 0);
        // And each still covers the same bases, wherever the circle now starts.
        let gene = &named(&prod.mol, "a gene").segments[0];
        assert_eq!(gene.end - gene.start, 15, "the feature changed length");
        assert_eq!(
            bases(&prod.mol, gene.start, gene.end),
            bases(&m, 15, 30),
            "the feature after the cut points at the wrong bases"
        );
        let up = &named(&prod.mol, "upstream").segments[0];
        assert_eq!(
            bases(&prod.mol, up.start, up.end),
            bases(&m, 1, 4),
            "the feature before the cut points at the wrong bases"
        );
    }

    /// A fragment laid in end-for-end must carry its features to the bases they
    /// name — and until this test, NO test in this file exercised one at all.
    ///
    /// PROVEN TO FAIL at f0e4a6f: the mirror was `(flen - 1 - b, flen - 1 - a)`
    /// with `flen = frags[idx].watson.len()`, and a flipped fragment contributes
    /// its CRICK strand, which is a window offset from watson's by the enzyme's
    /// overhang. Every carried feature therefore landed four bases to the 5'
    /// side of its own sequence — four for EcoRI, BamHI, BglII, HindIII, XhoI,
    /// SalI, NheI, SpeI, XbaI and every other four-base 5' cutter. Measured on
    /// this fixture: `gfp` read back as `ATGGTG` where the parent says `CTTGCA`,
    /// whose reverse complement is `TGCAAG`. On a CDS that is a frameshift in
    /// what the map draws and what the exported GenBank claims.
    ///
    /// TO RE-BREAK IT: in `place`, replace `let m = slot.crick - 1 - slot.ovhg;`
    /// with `let m = slot.watson as i64 - 1;`.
    ///
    /// WHY THIS FIXTURE. Two EcoRI sites, one of them straddling the origin so
    /// that NEITHER fragment wraps — the wrap is a different defect's case and
    /// this test must be able to fail for one reason only. Both fragments then
    /// carry an `AATT` end at each end, and `rc("AATT") == "AATT"`, so each
    /// seals in either orientation: `ligate` enumerates the inverted circle
    /// beside the plain religation and the panel offers both with an Open
    /// button. This is everyday non-directional cloning, not a corner.
    #[test]
    fn an_inverted_fragment_carries_its_features_to_the_bases_they_name() {
        let seq = "AATTCGGCATTACGTACGAATTCTTGCACCATGGAG";
        let mut m = mol(seq, true);
        m.name = "pInv".into();
        // In the first fragment, which `ligate` pins unflipped in every product,
        // so this one holds the arm that was already right.
        let mut marker = Feature::new("bla", "CDS");
        marker.strand = Strand::Forward;
        marker.segments.push(Segment::new(8, 13));
        m.features.push(marker);
        // In the second, off-centre and clear of the four-base overhang at each
        // end: off-centre because a feature in the middle of a fragment is
        // mirrored onto itself and would pass either way.
        let mut gene = Feature::new("gfp", "CDS");
        gene.strand = Strand::Forward;
        gene.segments.push(Segment::new(23, 28));
        m.features.push(gene);

        let p = plan(
            &m,
            None,
            Method::Restriction,
            &ticked(&["EcoRI"]),
            &Primers::default(),
            false,
            25,
        );
        assert!(p.note.is_none(), "{:?}", p.note);
        assert_eq!(p.frags.len(), 2, "two sites in a circle give two fragments");
        // Neither wraps, which is what makes this fixture about the mirror only.
        assert_eq!(p.frags[0].from, Some((1, 18)));
        assert_eq!(p.frags[1].from, Some((19, 36)));

        let inverted = p
            .prods
            .iter()
            .find(|pr| pr.order.iter().any(|(_, flipped)| *flipped))
            .expect("no product laid a fragment in end-for-end");
        assert!(inverted.circular);
        assert_eq!(inverted.carried, 2, "a feature did not travel");

        // READ BACK OFF THE PRODUCT'S OWN BASES. `bla` came off the fragment
        // laid forwards and must read as the parent's bases; `gfp` came off the
        // one laid backwards and must read as their reverse complement.
        let bla = &named(&inverted.mol, "bla").segments[0];
        assert_eq!(
            bases(&inverted.mol, bla.start, bla.end),
            bases(&m, 8, 13),
            "the forward fragment's feature moved"
        );
        let gfp = &named(&inverted.mol, "gfp").segments[0];
        assert_eq!(
            bases(&inverted.mol, gfp.start, gfp.end),
            rc(&bases(&m, 23, 28)),
            "the inverted fragment's feature points at the wrong bases"
        );
        // And it reads on the other strand now, because its bases do.
        assert_eq!(named(&inverted.mol, "gfp").strand, Strand::Reverse);
        assert_eq!(named(&inverted.mol, "bla").strand, Strand::Forward);
    }

    /// A two-exon feature on an inverted fragment must SPLICE to the parent's
    /// gene, which takes reversing the exon LIST and not only each exon's span.
    ///
    /// PROVEN TO FAIL at d8c218b: `place` mirrored each segment and collected
    /// them in the PARENT's order, so a flipped fragment's multi-exon feature
    /// came back DESCENDING — on this fixture `[33..36, 27..30]`. Nothing
    /// downstream repairs that: `genbank::write` emits the parts in stored
    /// order, so `Save ▸ GenBank` wrote `complement(join(33..36,27..30))`, and
    /// by the convention `crates/pl-fileio/src/genbank.rs:611` states and
    /// `bins/pl-gui/src/aa.rs` applies — a Reverse feature is read back to front
    /// — that splices to `CCATCTTG` where this gene is `CTTGCCAT`. THE EXONS
    /// COME OUT SWAPPED: a spliced product, and therefore a translated protein,
    /// that is not the gene that was carried. Nothing on screen shows it — both
    /// spans sit comfortably inside the construct and each names bases that are
    /// really its own, and the map draws the same two arcs whichever order the
    /// list is in — so the first place it can surface is the amino-acid track,
    /// or somebody else's reader after the file has left. Biopython 1.87 splices
    /// that file to `CCATCTTG` too, measured, so it is not a private
    /// disagreement this program could settle for itself.
    ///
    /// EVERY feature this module's tests built was single-segment before this
    /// one — 15 `push(Segment::new(..))` calls, no two of them on the same
    /// feature — so the whole shipped suite was blind to it, and stayed green
    /// through the round-1 repair of the neighbouring arithmetic. It is not a
    /// regression from that repair either: `place` is new code, but the inline
    /// `.map(...).collect()` it replaced at `f0e4a6f:1512-1526` collected in the
    /// parent's order too. Carried forward, not introduced.
    ///
    /// TO RE-BREAK IT: in `place`, delete the line `out.reverse();` at
    /// clone.rs:1873 — the body of the `if slot.flipped` immediately before
    /// `Some(out)`. That leaves `let mut out` with nothing mutating it, so
    /// rustc emits an `unused_mut` WARNING; this crate does not deny warnings,
    /// so the build still succeeds and this test is what fails.
    ///
    /// WHY THIS FIXTURE. Sequence, enzyme and fragment geometry are exactly
    /// `an_inverted_fragment_carries_its_features_to_the_bases_they_name`'s —
    /// two EcoRI sites, one of them straddling the origin so that NEITHER
    /// fragment wraps, both fragments carrying an `AATT` end at each end so that
    /// each seals in either orientation — so that this test can fail for the
    /// exon order and for nothing else. The exons are `CTTG` and `CCAT`:
    /// different from each other, and neither equal to its own reverse
    /// complement, so a swap cannot slip through on a symmetry.
    ///
    /// `backbone` IS A CONTROL AND NOT DECORATION. It rides the fragment laid in
    /// forwards, where the mapping preserves order, and it is asserted in FILE
    /// order rather than reversed. A "fix" that reversed unconditionally would
    /// satisfy every assertion about `split gene` and fail on this one.
    ///
    /// The two round-2 clone fixes MEET HERE, which is the other reason for a
    /// multi-segment feature on a flipped fragment: `slot.at` is the product
    /// cursor — the thing finding #1 corrected to advance by crick — and every
    /// coordinate below is `slot.at` plus the mirror. This fixture is a
    /// single-enzyme digest, where `watson.len() == crick.len()` and the cursor's
    /// two branches agree, so the exon order is the only thing under test;
    /// `a_flipped_fragment_does_not_shift_the_features_behind_it` is where the
    /// branches are made to differ.
    #[test]
    fn an_inverted_multi_exon_feature_keeps_its_exons_in_splice_order() {
        let seq = "AATTCGGCATTACGTACGAATTCTTGCACCATGGAG";
        let mut m = mol(seq, true);
        m.name = "pExon".into();
        // On the SECOND fragment, which is the one that can be laid in either
        // way round: `ligate` pins fragment 0 first and unflipped for every
        // circular product (crates/pl-clone/src/ligate.rs).
        let mut gene = Feature::new("split gene", "CDS");
        gene.strand = Strand::Forward;
        gene.segments.push(Segment::new(23, 26)); // CTTG
        gene.segments.push(Segment::new(29, 32)); // CCAT
        m.features.push(gene);
        // On the first fragment, laid in forwards.
        let mut ctrl = Feature::new("backbone", "CDS");
        ctrl.strand = Strand::Forward;
        ctrl.segments.push(Segment::new(8, 10)); // CAT
        ctrl.segments.push(Segment::new(13, 15)); // CGT
        m.features.push(ctrl);

        let p = plan(
            &m,
            None,
            Method::Restriction,
            &ticked(&["EcoRI"]),
            &Primers::default(),
            false,
            25,
        );
        assert!(p.note.is_none(), "{:?}", p.note);
        assert_eq!(p.frags.len(), 2, "two sites in a circle give two fragments");

        let inverted = p
            .prods
            .iter()
            .find(|pr| pr.order.iter().any(|(_, flipped)| *flipped))
            .expect("no product laid a fragment in end-for-end");
        assert!(inverted.circular);
        assert_eq!(inverted.carried, 2, "a feature did not travel");

        let gene = named(&inverted.mol, "split gene");
        assert_eq!(gene.segments.len(), 2, "an exon was lost");
        assert_eq!(
            gene.strand,
            Strand::Reverse,
            "the fixture stopped exercising the inverted fragment"
        );
        // THE ORACLE IS THE SPLICED BASES, not the coordinates. The whole shape
        // of this defect is two spans that each name the right bases in the
        // wrong sequence, and a number compared against a number cannot see
        // that. Read the way `genbank.rs` and `aa.rs` read a Reverse feature:
        // the parts back to front, each reverse-complemented.
        let spliced: String = gene
            .segments
            .iter()
            .rev()
            .map(|s| rc(&bases(&inverted.mol, s.start, s.end)))
            .collect();
        assert_eq!(
            spliced,
            format!("{}{}", bases(&m, 23, 26), bases(&m, 29, 32)),
            "the inverted fragment's exons splice in the wrong order"
        );
        // And again as coordinates, because that is the form the file carries
        // and the form every other reader trusts: join order is ascending here,
        // and a descending list is written as `complement(join(hi,lo))`.
        let exons: Vec<(u64, u64)> = gene.segments.iter().map(|s| (s.start, s.end)).collect();
        assert!(
            exons[0].0 < exons[1].0,
            "the exon list came back descending: {exons:?}"
        );

        let back = named(&inverted.mol, "backbone");
        assert_eq!(back.segments.len(), 2, "an exon was lost");
        assert_eq!(back.strand, Strand::Forward);
        let plain: String = back
            .segments
            .iter()
            .map(|s| bases(&inverted.mol, s.start, s.end))
            .collect();
        assert_eq!(
            plain,
            format!("{}{}", bases(&m, 8, 10), bases(&m, 13, 15)),
            "the fragment laid in forwards must keep its exons in file order"
        );
    }

    /// A feature reaching into a junction's overhang travels while there is a
    /// neighbour to hold its other strand, and does not when there is not.
    ///
    /// The first `k` bases of a fragment's watson are single-stranded — they are
    /// the 5' overhang — so on an inverted fragment their partners in the
    /// product come from the NEXT fragment's overhang, annealed to them, which
    /// is what the junction is. `place` lands them there and is right to. For
    /// the LAST fragment of a circle the same arithmetic runs past the end of
    /// the molecule, where the bases are real but their coordinate is on the far
    /// side of the origin, so the feature does not travel at all.
    ///
    /// PROVEN TO FAIL at f0e4a6f: the old mirror put this feature at 31..35,
    /// reading `TGCAA` where the parent says `ATTCT` — inside the construct, on
    /// the wrong bases, with nothing saying so.
    ///
    /// TO RE-BREAK IT: in `place`, delete
    /// `if lo < 1 || hi > slot.product as i64 { return None; }`.
    #[test]
    fn a_feature_over_a_junction_overhang_is_dropped_rather_than_placed_off_the_end() {
        let seq = "AATTCGGCATTACGTACGAATTCTTGCACCATGGAG";
        let mut m = mol(seq, true);
        // Starts one base into the second fragment's four-base 5' overhang.
        let mut edge = Feature::new("edge", "misc_feature");
        edge.strand = Strand::Forward;
        edge.segments.push(Segment::new(20, 24));
        m.features.push(edge);

        let p = plan(
            &m,
            None,
            Method::Restriction,
            &ticked(&["EcoRI"]),
            &Primers::default(),
            false,
            25,
        );
        assert_eq!(p.frags.len(), 2);

        // THE CONTROL, and it is what stops this being a test that passes by
        // carrying nothing: laid forwards, the same feature travels and reads
        // back as the parent's bases.
        let plain = p
            .prods
            .iter()
            .find(|pr| pr.order.iter().all(|(_, flipped)| !*flipped))
            .expect("no plain religation");
        assert_eq!(plain.carried, 1);
        let s = &named(&plain.mol, "edge").segments[0];
        assert_eq!(bases(&plain.mol, s.start, s.end), bases(&m, 20, 24));

        let inverted = p
            .prods
            .iter()
            .find(|pr| pr.order.iter().any(|(_, flipped)| *flipped))
            .expect("no product laid a fragment in end-for-end");
        assert_eq!(
            inverted.carried, 0,
            "a feature was placed at a coordinate the product does not have"
        );
        assert!(inverted.mol.features.is_empty());
        assert_eq!(inverted.dropped, 1, "and it must be counted as dropped");
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
    ///
    /// PROVEN TO FAIL at f0e4a6f, but only since the `>= 1` below became `== 1`.
    /// `dropped` was incremented inside a loop over `parent.features` that runs
    /// once per FRAGMENT, so this parent's single feature was counted twice —
    /// once for each of the two pieces it is not inside — so the old
    /// `assert!(dropped >= 1)` passed at 1 and at 2 alike. It could not fail
    /// for the thing its own name claims, which is why the number is now
    /// pinned.
    ///
    /// TO RE-BREAK IT: in `build`, replace the two lines
    /// `used_parents` / `.map(|p| sources[*p].mol.features.len())` of the
    /// `dropped` binding with `order` /
    /// `.map(|(i, _)| sources[described[*i].parent].mol.features.len())`, which
    /// counts a parent's features once per FRAGMENT again.
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
            assert_eq!(
                prod.dropped, 1,
                "one feature was lost, and the parent has one feature"
            );
        }
    }

    /// Two pieces of one plasmid, a feature in each, and nothing lost.
    ///
    /// PROVEN TO FAIL at f0e4a6f: `dropped` was counted per FRAGMENT over the
    /// whole parent's feature list, so a feature carried by fragment A was
    /// counted as dropped while fragment B was being walked, and vice versa.
    /// This construct — every base of the plasmid, both features present — was
    /// reported as `2 feature(s) carried, 2 dropped`, in the panel's warning
    /// colour, under a hover promising that anything dropped had been cut in
    /// half. On a 3 kb plasmid with ten features it reads "10 carried, 10
    /// dropped" and sends the user hunting for damage that is not there. The
    /// same number goes into the Copy-record paragraph, where `carried +
    /// dropped` exceeding the parent's feature count is self-contradictory on
    /// its face.
    ///
    /// TO RE-BREAK IT: in `build`, replace the two lines
    /// `used_parents` / `.map(|p| sources[*p].mol.features.len())` of the
    /// `dropped` binding with `order` /
    /// `.map(|(i, _)| sources[described[*i].parent].mol.features.len())`.
    #[test]
    fn two_fragments_of_one_parent_carry_a_feature_each_and_drop_nothing() {
        let seq = "AAAAGGATCCTTTTGCGCGCATATATGGATCCAAAATTTTCCCC";
        let mut m = mol(seq, true);
        // BamHI cuts after bases 5 and 27, giving fragments at 6..27 and
        // 28..49 — the second wraps the origin. One feature sits whole inside
        // each, so a religation loses nothing at all.
        let mut left = Feature::new("left", "CDS");
        left.strand = Strand::Forward;
        left.segments.push(Segment::new(12, 22));
        m.features.push(left);
        let mut right = Feature::new("right", "CDS");
        right.strand = Strand::Forward;
        right.segments.push(Segment::new(33, 40));
        m.features.push(right);

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
        assert!(!p.prods.is_empty(), "{:?}", p.note);
        for prod in &p.prods {
            assert_eq!(
                prod.carried, 2,
                "{:?}: a feature did not travel",
                prod.order
            );
            assert_eq!(
                prod.dropped, 0,
                "{:?}: a religation that loses nothing reported a loss",
                prod.order
            );
        }

        // And the pieces really are where they say: read back off the plain
        // religation, which is the parent rotated to start at the cut.
        let plain = p
            .prods
            .iter()
            .find(|pr| pr.order.iter().all(|(_, flipped)| !*flipped))
            .expect("no plain religation");
        for (name, a, b) in [("left", 12u64, 22u64), ("right", 33, 40)] {
            let s = &named(&plain.mol, name).segments[0];
            assert_eq!(
                bases(&plain.mol, s.start, s.end),
                bases(&m, a, b),
                "{name} points at the wrong bases"
            );
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
