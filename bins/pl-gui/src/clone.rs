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

/// One fragment of the digest, with where it came from when that is knowable.
pub struct Frag {
    pub len: usize,
    pub left: String,
    pub right: String,
    /// 1-based inclusive span in the parent, when the fragment could be placed.
    pub from: Option<(u64, u64)>,
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
            plan: None,
            stale: true,
            wanted: None,
        }
    }
}

/// Draw the panel. Returns false when the user has closed it.
pub fn show(ctx: &egui::Context, p: &mut Panel, mol: &Molecule, dark: bool) -> bool {
    let pal = crate::theme::Palette::of(dark);
    if p.stale {
        p.plan = Some(plan(mol, &p.enzymes, p.blunt));
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
                    let cuts = !pl_enzymes::cut_positions(&mol.seq, mol.topology, e).is_empty();
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
            ui.separator();

            let Some(pl) = &p.plan else { return };

            if !pl.frags.is_empty() {
                ui.label(egui::RichText::new(format!("{} fragments", pl.frags.len())).strong());
                for (i, f) in pl.frags.iter().enumerate() {
                    ui.label(
                        egui::RichText::new(format!(
                            "  {}.  {} bp   {}  …  {}{}",
                            i + 1,
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

/// Plan a digest and religation of `mol` with the named enzymes.
///
/// Pure: no `Ui`, no document, no worker. That is deliberate — the equivalent
/// logic for the Enzymes tab started life inside a closure in the tab and could
/// not be asserted without standing up a frame.
pub fn plan(mol: &Molecule, enzymes: &BTreeSet<String>, blunt: bool) -> Plan {
    if enzymes.is_empty() {
        return Plan {
            frags: Vec::new(),
            prods: Vec::new(),
            note: Some("Tick an enzyme to cut with.".into()),
        };
    }
    let seq: String = String::from_utf8_lossy(&mol.seq).to_ascii_uppercase();
    let circular = mol.topology.is_circular();
    let d = pl_clone::Dseq::new(&seq, circular);

    // The digest itself lives in `pl-clone` since Stage 4. It was written out
    // here, and `subclone` needs the identical operation on a second molecule —
    // a digest performed one way in the panel and another way in the engine is
    // two answers to "what are the fragments".
    let frags = pl_clone::digest(&d, enzymes.iter().filter_map(|n| pl_enzymes::by_name(n)));

    if frags.len() == 1 && frags[0].circular {
        return Plan {
            frags: Vec::new(),
            prods: Vec::new(),
            note: Some(format!(
                "None of {} cuts this molecule.",
                enzymes.iter().cloned().collect::<Vec<_>>().join(", ")
            )),
        };
    }

    let described: Vec<Frag> = frags
        .iter()
        .map(|f| Frag {
            len: f.len(),
            left: end_label(&f.left_end()),
            right: end_label(&f.right_end()),
            from: locate(&f.watson, &seq, circular),
        })
        .collect();

    let opts = pl_clone::ligate::Options {
        blunt,
        ..Default::default()
    };
    let products = match pl_clone::ligate::ligate(&frags, &opts) {
        Ok(p) => p,
        Err(e) => {
            return Plan {
                frags: described,
                prods: Vec::new(),
                note: Some(e.to_string()),
            }
        }
    };

    let prods: Vec<Prod> = products
        .iter()
        .map(|p| build(mol, &seq, &frags, &described, p))
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
fn build(
    parent: &Molecule,
    seq: &str,
    frags: &[pl_clone::Dseq],
    described: &[Frag],
    p: &pl_clone::ligate::Product,
) -> Prod {
    let full = p.seq.to_string_full();
    let mut mol = Molecule {
        name: format!("{} product", parent.name),
        seq: full.clone().into_bytes(),
        topology: if p.seq.circular {
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
    for (idx, flipped) in &p.order {
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
        circular: p.seq.circular,
        order: p.order.clone(),
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
        let p = plan(&mol("ACGTACGTACGT", true), &BTreeSet::new(), false);
        assert!(p.frags.is_empty() && p.prods.is_empty());
        assert!(p.note.unwrap().contains("Tick an enzyme"));
    }

    #[test]
    fn an_enzyme_that_does_not_cut_says_so() {
        let p = plan(&mol("ACGTACGTACGTACGT", true), &ticked(&["BamHI"]), false);
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

        let p = plan(&m, &ticked(&["BamHI"]), false);
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

    /// A feature spanning a cut site cannot travel whole, so it does not travel.
    #[test]
    fn a_feature_cut_in_half_is_dropped_and_counted() {
        let seq = "AAAAGGATCCTTTTGCGCGCATATATGGATCCAAAATTTTCCCC";
        let mut m = mol(seq, true);
        // Spans the second BamHI site.
        let mut f = Feature::new("straddles", "misc_feature");
        f.segments.push(Segment::new(25, 36));
        m.features.push(f);

        let p = plan(&m, &ticked(&["BamHI"]), false);
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
        let off = plan(&m, &ticked(&["EcoRV"]), false);
        assert!(off.prods.is_empty());
        assert!(off.note.unwrap().contains("blunt"));
        let on = plan(&m, &ticked(&["EcoRV"]), true);
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
