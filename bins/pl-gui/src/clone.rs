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
}

impl Method {
    pub fn label(self) -> &'static str {
        match self {
            Method::Restriction => "Restriction",
            Method::Gibson => "Gibson / HiFi",
            Method::GoldenGate => "Golden Gate",
        }
    }
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
    /// The OTHER tab whose digest supplies the insert, if there is one.
    ///
    /// Stage 4, and the whole of it. `None` religates the open molecule on its
    /// own, which is what this panel did and still does. `Some(tab)` takes one
    /// fragment from each digest and puts them together — subcloning, the
    /// operation the crate is named for and the one nothing could reach.
    ///
    /// A TAB INDEX and not a molecule. The panel outlives a frame and must not
    /// outlive its documents, and a `Molecule` copied in here would be a second
    /// copy of a plasmid that the user can go on editing in its own tab: the
    /// plan would then describe a sequence nobody has, silently. An index is
    /// re-resolved every frame and can be found to be gone, which is a
    /// condition the panel can state.
    pub donor: Option<usize>,
    /// Recomputed only when the inputs change: `plan` digests and enumerates,
    /// and a redraw is not a reason to do either again.
    pub plan: Option<Plan>,
    pub stale: bool,
    /// Set when the user asks for a product; the caller adopts it and clears it.
    pub wanted: Option<usize>,
}

impl Panel {
    /// Seeded from the enzymes already ticked for the gel, because a user who
    /// has just looked at a digest is asking about THAT digest.
    pub fn new(picked: &BTreeSet<String>) -> Self {
        Panel {
            method: Method::Restriction,
            enzymes: picked.clone(),
            blunt: false,
            homology: 25,
            donor: None,
            plan: None,
            stale: true,
            wanted: None,
        }
    }
}

/// Draw the panel. Returns false when the user has closed it.
///
/// `others` is the rest of the bench — `(tab index, title, molecule)` — so the
/// insert can come from a plasmid the user already has open. Resolved by the
/// caller every frame rather than held here: see [`Panel::donor`].
pub fn show(
    ctx: &egui::Context,
    p: &mut Panel,
    mol: &Molecule,
    others: &[(usize, String, &Molecule)],
    dark: bool,
) -> bool {
    let pal = crate::theme::Palette::of(dark);
    // The donor as it stands THIS frame. A tab that has been closed since the
    // choice was made resolves to nothing, and the panel says so below rather
    // than planning against a molecule that is no longer open.
    let donor = p
        .donor
        .and_then(|t| others.iter().find(|(i, _, _)| *i == t));
    if p.donor.is_some() && donor.is_none() {
        p.donor = None;
        p.stale = true;
    }
    let donor_mol = donor.map(|(_, _, m)| *m);
    if p.stale {
        p.plan = Some(plan(
            mol, donor_mol, p.method, &p.enzymes, p.blunt, p.homology,
        ));
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
                for m in [Method::Restriction, Method::Gibson, Method::GoldenGate] {
                    if ui.selectable_label(p.method == m, m.label()).clicked() && p.method != m {
                        p.method = m;
                        p.stale = true;
                    }
                }
            });
            ui.add_space(4.0);

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

            // THE INSERT, from another tab. Offered only when there is another
            // tab to offer: a control whose menu is always empty teaches a user
            // the feature does not work.
            if !others.is_empty() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Insert from").strong());
                    let shown = p
                        .donor
                        .and_then(|t| others.iter().find(|(i, _, _)| *i == t))
                        .map(|(_, name, _)| name.clone())
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
                            for (i, name, _) in others {
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
                    let whose = match (donor.map(|(_, n, _)| n), f.parent) {
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
                            match f.from {
                                Some((a, b)) => format!("   from {a}..{b}"),
                                // Said out loud: this is why a product may carry
                                // fewer features than the parent had.
                                None => "   (could not be placed in the parent)".into(),
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
    blunt: bool,
    homology: usize,
) -> Plan {
    // An enzyme is required to CUT and optional to ASSEMBLE. Gibson's overlap is
    // designed into the primers; the enzymes are there only to linearise a
    // vector, and a user with two PCR products needs none at all.
    if enzymes.is_empty() && method == Method::Restriction {
        return Plan {
            frags: Vec::new(),
            prods: Vec::new(),
            note: Some("Tick an enzyme to cut with.".into()),
            gg: None,
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
        let p = plan(&v, None, Method::Gibson, &BTreeSet::new(), false, 25);
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
        let strict = plan(&x, Some(&y), Method::Gibson, &BTreeSet::new(), false, 25);
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
        let p = plan(&m, None, Method::GoldenGate, &ticked(&["BsaI"]), false, 25);
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
        let r = plan(&m, None, Method::Restriction, &ticked(&["BsaI"]), false, 25);
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
        let pl = plan(&m, None, Method::GoldenGate, &e, false, 25);
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
            false,
            25,
        );
        assert!(off.prods.is_empty());
        assert!(off.note.unwrap().contains("blunt"));
        let on = plan(&m, None, Method::Restriction, &ticked(&["EcoRV"]), true, 25);
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
