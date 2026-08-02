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
}

/// The whole answer for one set of enzymes.
pub struct Plan {
    pub frags: Vec<Frag>,
    pub prods: Vec<Prod>,
    /// Why there is nothing to show, when there is nothing to show.
    pub note: Option<String>,
}

/// The panel's own state. Outlives a frame; does not outlive its document.
pub struct Panel {
    pub enzymes: BTreeSet<String>,
    pub blunt: bool,
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
            enzymes: picked.clone(),
            blunt: false,
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
        p.plan = Some(plan(mol, donor_mol, &p.enzymes, p.blunt));
        p.stale = false;
    }
    let mut open = true;
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

            // Enzymes: only the ones that cut, so the list is short and every
            // entry does something.
            ui.label(egui::RichText::new("Cut with").strong());
            ui.horizontal_wrapped(|ui| {
                for e in pl_enzymes::ENZYMES {
                    // EITHER molecule, once there are two. An enzyme that cuts
                    // only the donor is exactly the one a user reaches for when
                    // the insert has the site and the vector's is somewhere
                    // else — and it was not on this list at all.
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
            if ui.checkbox(&mut p.blunt, "join blunt ends too").changed() {
                p.stale = true;
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
                });
            }
        });
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
    enzymes: &BTreeSet<String>,
    blunt: bool,
) -> Plan {
    if enzymes.is_empty() {
        return Plan {
            frags: Vec::new(),
            prods: Vec::new(),
            note: Some("Tick an enzyme to cut with.".into()),
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
        if pool.len() == 1 && pool[0].circular {
            return Plan {
                frags: Vec::new(),
                prods: Vec::new(),
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

    let opts = pl_clone::ligate::Options {
        blunt,
        ..Default::default()
    };
    // Both arms produce the same thing: a molecule and the fragments laid down
    // in order, in the flattened index space above.
    let laid: Vec<(pl_clone::Dseq, Vec<(usize, bool)>)> = if donor.is_none() {
        match pl_clone::ligate::ligate(&pools[0], &opts) {
            Ok(ps) => ps.into_iter().map(|p| (p.seq, p.order)).collect(),
            Err(e) => {
                return Plan {
                    frags: described,
                    prods: Vec::new(),
                    note: Some(e.to_string()),
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
                    (c.product.seq, order)
                })
                .collect(),
            Err(e) => {
                return Plan {
                    frags: described,
                    prods: Vec::new(),
                    note: Some(e.to_string()),
                }
            }
        }
    };

    let prods: Vec<Prod> = laid
        .iter()
        .map(|(seq, order)| build(&sources, &frags, &described, seq, order))
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
        let p = plan(&mol("ACGTACGTACGT", true), None, &BTreeSet::new(), false);
        assert!(p.frags.is_empty() && p.prods.is_empty());
        assert!(p.note.unwrap().contains("Tick an enzyme"));
    }

    #[test]
    fn an_enzyme_that_does_not_cut_says_so() {
        let p = plan(
            &mol("ACGTACGTACGTACGT", true),
            None,
            &ticked(&["BamHI"]),
            false,
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

        let p = plan(&m, None, &ticked(&["BamHI"]), false);
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

        let p = plan(&vector, Some(&giver), &ticked(&["EcoRI", "BamHI"]), false);
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
        let p = plan(&v, Some(&d), &ticked(&["BamHI"]), false);
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

        let p = plan(&m, None, &ticked(&["BamHI"]), false);
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
        let off = plan(&m, None, &ticked(&["EcoRV"]), false);
        assert!(off.prods.is_empty());
        assert!(off.note.unwrap().contains("blunt"));
        let on = plan(&m, None, &ticked(&["EcoRV"]), true);
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
