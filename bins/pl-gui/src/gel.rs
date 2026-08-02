//! The virtual gel: what runs in which lane, and what the picture must admit.
//!
//! `pl-gel` has shipped and been tested since the gel work landed and had no
//! GUI entry point at all — `pl gel` could run a diagnostic digest and the
//! application could not. This is that surface, and it adds no simulator: every
//! band position comes from `pl_gel::Gel::run`, every picture from
//! `pl_gel::render::to_scene`, and the drawing from [`crate::scene`].
//!
//! # Lanes are derived, never hand-managed
//!
//! There is no add/remove/reorder UI. A lane list maintained by hand is a
//! second copy of the enzyme choice and drifts from the Enzymes tab; the app
//! has already had to fix that class of split-brain once, at the map's
//! "ONE control, one answer" comment. Here the rule is:
//!
//! > `App::enzyme_set` governs which enzymes can be TICKED. Ticking governs
//! > which LANE an enzyme is in. The tick lives on the enzyme's own row in the
//! > Enzymes tab, and the gel view has no enzyme control at all.
//!
//! That is also why the gel is in the central pane and not a seventh tab: the
//! picker and the picture have to be visible together, or choosing the enzymes
//! and seeing the result are mutually exclusive.
//!
//! # A lane with no cuts on a circle draws NO band
//!
//! The rule and its reasoning live in [`pl_gel::uncut_circle`], not here. It
//! started life in this module, which meant `pl gel demo.gb --lane PacI` drew a
//! band at the contour length for an enzyme that does not cut that plasmid
//! while the application drew nothing — one engine, two pictures, and nothing
//! telling the user which to believe. It is a property of the simulation, so it
//! belongs in the crate every surface calls.
//!
//! # What the Enzymes tab says about methylation, this says too
//!
//! The tab strikes `BclI` through and prints "Dam blocked". The tick on that
//! same row put it in the gel, where its two fragments were drawn as fact —
//! one row giving two opposite answers, which is the split-brain the "ONE
//! control, one answer" rule exists to stop. So [`View::seed`] never picks a
//! blocked enzyme for the DEFAULT gel, and a blocked enzyme the user ticks
//! deliberately is still drawn — hiding it would be the map's old silent filter
//! — with the methylase named in the disclosure strip AND in the picture's own
//! note, so the exported figure carries it too.

use std::collections::BTreeSet;

use pl_core::Topology;
use pl_enzymes::methylation::{Effect, SiteEffect};

/// How the ticked enzymes become lanes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arrangement {
    /// n enzymes, n lanes, one enzyme each.
    Separate,
    /// n enzymes, one lane, all in one tube. `pl gel --lane A+B`.
    Together,
    /// The classic A / B / A+B triple.
    Both,
}

impl Arrangement {
    pub const ALL: [Arrangement; 3] = [
        Arrangement::Separate,
        Arrangement::Together,
        Arrangement::Both,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Arrangement::Separate => "Separate",
            Arrangement::Together => "Together",
            Arrangement::Both => "Both",
        }
    }
    pub fn hover(self) -> &'static str {
        match self {
            Arrangement::Separate => "One lane per enzyme: the single digests, side by side.",
            Arrangement::Together => {
                "One lane, all the enzymes in one tube. A double digest makes fragments \
                 shorter than either single digest does, which is usually the whole reason \
                 for doing it — so this is a different experiment from the lanes beside it."
            }
            Arrangement::Both => "The singles and the combined digest together.",
        }
    }
}

/// Everything the gel view remembers, none of which is about the molecule.
///
/// A gel is a VIEW. Nothing in this module calls `Document::apply`, enters the
/// op log or makes a document dirty.
pub struct View {
    /// Enzyme names ticked into the gel, by name so the set survives a re-scan.
    pub picked: BTreeSet<String>,
    pub arrangement: Arrangement,
    pub ladder: &'static str,
    pub conditions: pl_gel::Conditions,
    /// Dark field with light bands, as `pl gel` writes it.
    pub inverted: bool,
    /// Whether the default lane set has been seeded for this document.
    pub seeded: bool,
    /// What [`View::seed`] chose, when it chose fewer than it could have.
    ///
    /// A derived decision the user did not make, and the one thing the strip
    /// used to stay quiet about: the Enzymes tab prints "12 cut more than once"
    /// three inches away and the gel showed six of them with nothing saying so.
    /// Cleared the moment a tick is touched, because from then on the lane set
    /// is the user's.
    pub seed_note: Option<String>,
}

impl Default for View {
    fn default() -> Self {
        View {
            picked: BTreeSet::new(),
            arrangement: Arrangement::Separate,
            ladder: "1kb",
            conditions: pl_gel::Conditions::default(),
            inverted: true,
            seeded: false,
            seed_note: None,
        }
    }
}

/// One lane before it is run: what is in the tube, and where it cuts.
///
/// The label IS the enzyme list — `EcoRI` or `EcoRI+BamHI`, exactly as
/// `pl gel --lane` spells it — so there is no second copy of the names to fall
/// out of step with it. `names` is that same list unjoined, because the
/// methylation verdict is per enzyme and splitting the label back apart on `+`
/// would be a second parse of a string we already had in pieces.
pub struct Spec {
    pub label: String,
    pub names: Vec<String>,
    /// Distinct cut positions, deduplicated across the enzymes in the tube.
    pub cuts: Vec<u64>,
}

/// The picture, and everything that has to be said beside it.
pub struct Built {
    pub scene: pl_draw::Scene,
    /// The caveat, the merges, the unplaced fragments, the uncut lanes and the
    /// filter — see [`View::build`].
    pub disclosure: Vec<String>,
    /// Enzymes ticked into the gel that the current filter excludes.
    pub suspended: Vec<String>,
    /// How many fragments hide inside a merged band, and in how many bands.
    /// The LADDER counts too: it is the one lane a reader sizes everything else
    /// against, and it was the one lane never measured.
    pub hidden: (usize, usize),
    pub unplaced: usize,
    /// No sample lane at all — a ladder beside nothing.
    pub empty: bool,
}

impl View {
    /// The ladder this gel is running, or the shipped default.
    pub fn ladder(&self) -> pl_gel::Ladder {
        pl_gel::ladder(self.ladder)
            .or_else(|| pl_gel::ladder("1kb"))
            .expect("1kb is a shipped ladder")
    }

    /// The best shipped ladder for the fragments actually in these lanes.
    ///
    /// A 1 kb ladder spans 500–10,000 and is a poor ruler for a digest whose
    /// fragments are all under 1,500. Chosen ONCE, when the gel is first opened
    /// for a document, and the user's afterwards: a ladder that jumps when you
    /// tick another enzyme is disorienting, and the choice is always visible
    /// because it is printed as lane 0's label.
    pub fn best_ladder(fragments: &[u64]) -> &'static str {
        let placed: Vec<u64> = fragments.iter().copied().filter(|f| *f > 0).collect();
        if placed.is_empty() {
            return "1kb";
        }
        let mut best = ("1kb", usize::MAX);
        for l in pl_gel::LADDERS {
            let (lo, hi) = (
                *l.sizes.first().expect("a ladder has sizes"),
                *l.sizes.last().expect("a ladder has sizes"),
            );
            let missed = placed.iter().filter(|f| **f < lo || **f > hi).count();
            // Ties to the earlier entry, which is `1kb` — the CLI's default.
            if missed < best.1 {
                best = (l.name, missed);
            }
        }
        best.0
    }

    /// The lanes this gel runs, in a STATED order.
    ///
    /// Ladder first (matching `cmd_gel`, which always puts it in lane 0), then
    /// single lanes by enzyme name ascending, then the combined lane last.
    /// `pl-gel` has two tests saying it is deterministic, and iterating a
    /// `HashMap` of ticked enzymes here would destroy that from the outside: a
    /// gel that reorders itself between frames is the fastest way to lose a
    /// user's trust in it.
    pub fn specs(&self, results: &[pl_enzymes::Digest], set: pl_enzymes::EnzymeSet) -> Vec<Spec> {
        // A NON-CUTTER IS ABSENT, NOT HIDDEN, so it is not what the filter
        // excludes and its lane is still drawn — with no band and a caption
        // saying why. `EnzymeSet::admits` returns false for `count() == 0` on
        // every set including `All`, so keying the lane on `admits` alone would
        // route an enzyme that stopped cutting after an edit into the
        // "suspended by the filter" sentence, which is a true-looking
        // explanation of the wrong thing.
        let cuts_of = |name: &str| -> Option<Vec<u64>> {
            results
                .iter()
                .find(|d| d.enzyme.name == name && (set.admits(d) || d.count() == 0))
                .map(|d| d.positions.clone())
        };
        let live: Vec<(String, Vec<u64>)> = self
            .picked
            .iter()
            .filter_map(|n| cuts_of(n).map(|c| (n.clone(), c)))
            .collect();
        let mut out = Vec::new();
        if matches!(self.arrangement, Arrangement::Separate | Arrangement::Both) {
            for (name, cuts) in &live {
                out.push(Spec {
                    label: name.clone(),
                    names: vec![name.clone()],
                    cuts: dedup(cuts.clone()),
                });
            }
        }
        let combined = matches!(self.arrangement, Arrangement::Together | Arrangement::Both)
            // One enzyme "together" is the same lane as one enzyme "separate",
            // so `Both` would draw it twice.
            && (live.len() > 1 || self.arrangement == Arrangement::Together);
        if combined && !live.is_empty() {
            let names: Vec<String> = live.iter().map(|(n, _)| n.clone()).collect();
            let mut cuts: Vec<u64> = live.iter().flat_map(|(_, c)| c.clone()).collect();
            cuts = dedup(cuts);
            out.push(Spec {
                label: names.join("+"),
                names,
                cuts,
            });
        }
        out
    }

    /// The methylation verdict for a named enzyme, from the worker's table.
    ///
    /// `verdicts` is parallel to `results`, exactly as `DigestState::verdict`
    /// serves the Enzymes tab, so the gel and the tab read the SAME answer and
    /// cannot disagree about a row. Like the tab's, it is the verdict at that
    /// enzyme's FIRST site: every rule is a property of the (enzyme, methylase)
    /// pair plus local context, so a per-site answer is what the model gives,
    /// and claiming "2 of 2 sites are blocked" would be claiming more than was
    /// computed.
    fn verdict_for(
        results: &[pl_enzymes::Digest],
        verdicts: &[Option<SiteEffect>],
        name: &str,
    ) -> Option<SiteEffect> {
        let i = results.iter().position(|d| d.enzyme.name == name)?;
        verdicts.get(i).copied().flatten()
    }

    /// What has to be said about a lane whose enzymes are methylation-sensitive.
    ///
    /// Named methylase, named enzyme, and what the bands therefore assume. The
    /// tab's own hover says the site "would cut in an unmethylated preparation";
    /// these sentences are the gel's half of that same fact.
    fn methylation_notes(
        spec: &Spec,
        results: &[pl_enzymes::Digest],
        verdicts: &[Option<SiteEffect>],
    ) -> Vec<String> {
        let mut out = Vec::new();
        for name in &spec.names {
            let Some(v) = View::verdict_for(results, verdicts, name) else {
                continue;
            };
            let m = v.methylase.name();
            out.push(match v.effect {
                Effect::Blocked => format!(
                    "{name} is blocked by {m} methylation in this preparation, so these \
                     bands are what an UNMETHYLATED template would give. A plasmid grown \
                     in an ordinary {m}+ strain will not give them."
                ),
                Effect::Impaired => format!(
                    "{name} cleaves poorly when the site is {m}-methylated, and this lane \
                     assumes the digest went to completion. Expect partials."
                ),
                Effect::Unknown => format!(
                    "sources disagree about whether {m} methylation affects {name} here; \
                     this lane assumes it cuts."
                ),
            });
        }
        out
    }

    /// Build the picture and everything that must be said beside it.
    pub fn build(
        &self,
        mol: &pl_core::Molecule,
        results: &[pl_enzymes::Digest],
        verdicts: &[Option<SiteEffect>],
        set: pl_enzymes::EnzymeSet,
        title: &str,
    ) -> Built {
        let ladder = self.ladder();
        let gel = pl_gel::Gel::modelled(self.conditions);
        let specs = self.specs(results, set);
        let circular = mol.topology.is_circular();

        let mut lanes = vec![pl_gel::render::Lane {
            label: format!("{} ladder", ladder.name),
            sim: gel.run(ladder.sizes),
            is_ladder: true,
        }];
        let mut disclosure = Vec::new();
        let mut hidden = (0usize, 0usize);
        let mut unplaced = 0usize;
        // Sentences that must reach the exported figure, not only the strip:
        // methylation and uncut circles both change what the picture MEANS.
        let mut qualifiers: Vec<String> = Vec::new();
        let mut said_methylation: BTreeSet<String> = BTreeSet::new();

        // THE LADDER IS MEASURED TOO, and it was the one lane that never was.
        // `built.unplaced` said 5 on the demo construct while the picture
        // actually omitted 10, because the ladder was pushed in front of this
        // loop rather than through it — and at a 4 mm band width the 1kb-plus
        // ladder's 5000/6000 and 850/1000 co-migrate, so the ruler a reader
        // sizes every other band against silently becomes two rungs shorter
        // than the seventeen they are counting.
        {
            let sim = &lanes[0].sim;
            let merged = sim.merged();
            if !merged.is_empty() {
                let n: usize = merged.iter().map(|g| g.sizes.len()).sum();
                hidden.0 += n;
                hidden.1 += merged.len();
                disclosure.push(format!(
                    "the {} ladder is not {} rungs on this gel: {} — that rung is more than \
                     one band.",
                    ladder.name,
                    ladder.sizes.len(),
                    merged
                        .iter()
                        .map(|g| g.label())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            let big = sim.too_large();
            let small = sim.too_small();
            unplaced += big.len() + small.len();
            if !big.is_empty() || !small.is_empty() {
                let mut parts = Vec::new();
                if !big.is_empty() {
                    parts.push(format!("{} sits at the well", join(&big)));
                }
                if !small.is_empty() {
                    parts.push(format!("{} runs with the dye front", join(&small)));
                }
                let drawn: usize = sim.groups.iter().map(|g| g.sizes.len()).sum();
                disclosure.push(format!(
                    "the {} ladder: {}. {} of its {} bands are drawn.",
                    ladder.name,
                    parts.join(", and "),
                    drawn,
                    ladder.sizes.len()
                ));
            }
        }

        for spec in &specs {
            // The rule and the sentence are `pl_gel`'s, so `pl gel` and every
            // other caller draw the same tube. See its docs for why an uncut
            // circle has no band at the contour length.
            let intact = pl_gel::uncut_circle(spec.cuts.len(), circular);
            let frags = if intact {
                Vec::new()
            } else {
                pl_enzymes::fragments_from_cuts(&spec.cuts, mol.len(), mol.topology)
            };
            let sim = gel.run(&frags);
            if intact {
                let note = pl_gel::uncut_circle_note(&spec.label);
                disclosure.push(note.clone());
                qualifiers.push(note);
            }
            // ONCE PER ENZYME, not once per lane. The sentence is about the
            // enzyme, and `Both` puts a blocked enzyme in its own lane AND in
            // the combined one, so it would otherwise be printed twice in the
            // strip and twice in the exported figure's note.
            for note in View::methylation_notes(spec, results, verdicts) {
                if said_methylation.insert(note.clone()) {
                    disclosure.push(note.clone());
                    qualifiers.push(note);
                }
            }
            let merged = sim.merged();
            if !merged.is_empty() {
                let n: usize = merged.iter().map(|g| g.sizes.len()).sum();
                hidden.0 += n;
                hidden.1 += merged.len();
                // NAMED, not merely visible. `Item::Text::bold` marks these in
                // the scene and there is no bold face installed, so words are
                // the channel that survives to the screen. `Group::label` is
                // the SAME naming the band carries, capped the same way — see
                // `pl_gel::MAX_LISTED` for what an uncapped list of 1,769
                // co-migrating fragments did to the picture and to this line.
                disclosure.push(format!(
                    "{}: {n} fragments hide in {} band{} — {}.",
                    spec.label,
                    merged.len(),
                    if merged.len() == 1 { "" } else { "s" },
                    merged
                        .iter()
                        .map(|g| g.label())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            let big = sim.too_large();
            let small = sim.too_small();
            unplaced += big.len() + small.len();
            if !big.is_empty() || !small.is_empty() {
                let mut parts = Vec::new();
                if !big.is_empty() {
                    parts.push(format!("{} too large to place", join(&big)));
                }
                if !small.is_empty() {
                    parts.push(format!("{} too small to place", join(&small)));
                }
                disclosure.push(format!(
                    "{}: {}. This gel resolves {}–{} bp.",
                    spec.label,
                    parts.join("; "),
                    sim.range.0,
                    sim.range.1
                ));
            }
            lanes.push(pl_gel::render::Lane {
                label: spec.label.clone(),
                sim,
                is_ladder: false,
            });
        }

        // Ticked but excluded by the filter: SUSPENDED, never silently deleted.
        // Silently dropping a lane is the map's old silent-filter defect
        // wearing a different hat.
        let suspended: Vec<String> = self
            .picked
            .iter()
            .filter(|n| {
                results
                    .iter()
                    .any(|d| d.enzyme.name == n.as_str() && d.count() > 0 && !set.admits(d))
            })
            .cloned()
            .collect();
        if !suspended.is_empty() {
            disclosure.push(format!(
                "{} {} in your gel but not in “{}”; {} lane{} not drawn.",
                suspended.join(", "),
                if suspended.len() == 1 { "is" } else { "are" },
                set.label(),
                if suspended.len() == 1 { "its" } else { "their" },
                if suspended.len() == 1 { " is" } else { "s are" },
            ));
        }

        // The calibration statement, VERBATIM and never re-worded here. It is a
        // method on `Simulation` rather than a UI string precisely so that a UI
        // cannot forget it, and it is shown for a measured calibration too —
        // its absence must never be readable as "nothing to know".
        let caveat = lanes
            .last()
            .map(|l| l.sim.caveat())
            .unwrap_or_else(|| gel.run(&[]).caveat());
        disclosure.insert(0, caveat.clone());
        // A DERIVED LANE SET SAYS SO. `seed` keeps six of however many cut more
        // than once, and the Enzymes tab prints the true count three inches
        // away; the strip stayed silent about the one decision the user did not
        // make. Dropped as soon as a tick is touched.
        if let Some(note) = &self.seed_note {
            disclosure.push(note.clone());
        }
        disclosure.push(
            "The ladders are standard size sets, not any supplier's product: a ladder with a \
             doublet or a bright reference band will not look like this."
                .into(),
        );
        // Counter-intuitive and worth saying out loud, because every other
        // simulation a biologist meets has it the other way round.
        disclosure.push(
            "The fragment sizes are exact — they come from the sequence and the cut \
             positions. Where they sit on the gel is modelled."
                .into(),
        );

        // The picture's own note is the caveat FOLLOWED BY whatever else
        // changes what it means. A figure that leaves this machine saying
        // "modelled from 1% agarose" and nothing about the BclI lane being a
        // dam- prediction is a figure somebody will put in a thesis.
        let mut note = caveat;
        for q in &qualifiers {
            note.push(' ');
            note.push_str(q);
        }
        let scene = pl_gel::render::to_scene(
            &lanes,
            &pl_gel::render::Options {
                inverted: self.inverted,
                note: Some(note),
                ..Default::default()
            },
            title,
        );
        Built {
            scene,
            disclosure,
            suspended,
            hidden,
            unplaced,
            empty: specs.is_empty(),
        }
    }

    /// Seed a lane set for a document that has none, and choose its ladder.
    ///
    /// Tiered, and each tier is a defensible answer to "what would this user
    /// run". An empty gel is a dead end; a ladder-only gel that says why is
    /// not.
    pub fn seed(
        &mut self,
        mol: &pl_core::Molecule,
        results: &[pl_enzymes::Digest],
        verdicts: &[Option<SiteEffect>],
        set: pl_enzymes::EnzymeSet,
    ) {
        self.seeded = true;
        self.picked.clear();
        self.seed_note = None;
        // A BLOCKED ENZYME IS NOT A DEFAULT. `BclI` is the one unconditional
        // Dam block in the table and it cuts a plasmid twice, so it sorted
        // straight into tier 1 and the gel opened on a lane the Enzymes tab was
        // striking through three inches away. Ticked by hand it still draws —
        // see `methylation_notes` — but nothing should CHOOSE it.
        let admitted: Vec<&pl_enzymes::Digest> = results
            .iter()
            .filter(|d| {
                set.admits(d)
                    && View::verdict_for(results, verdicts, d.enzyme.name)
                        .is_none_or(|v| v.effect != Effect::Blocked)
            })
            .collect();

        // Tier 1: everything that cuts more than once. A circular plasmid cut
        // ONCE linearises to a single band and is diagnostically useless — on
        // the demo construct that is 25 of the 27 cutters — so opening on those
        // would be opening on a wall of identical single bands.
        let mut multi: Vec<&pl_enzymes::Digest> =
            admitted.iter().copied().filter(|d| d.count() > 1).collect();
        multi.sort_by(|a, b| {
            b.count()
                .cmp(&a.count())
                .then(a.enzyme.name.cmp(b.enzyme.name))
        });
        if !multi.is_empty() {
            for d in multi.iter().take(6) {
                self.picked.insert(d.enzyme.name.to_string());
            }
            if multi.len() > 6 {
                self.seed_note = Some(format!(
                    "This gel is a suggestion: {} of the {} enzymes that cut more than once, \
                     chosen by cut count. Tick the others on the Enzymes tab.",
                    self.picked.len(),
                    multi.len()
                ));
            }
            self.arrangement = Arrangement::Separate;
        } else {
            // Tier 2: no multi-cutter — which is exactly what `Unique` and
            // `Unique 6+ base` guarantee — so seed one COMBINED lane from the
            // pair of unique cutters whose double digest this gel actually
            // resolves. That is the question `Simulation::resolves` exists to
            // answer, and 27 cutters is 351 simulations, which is microseconds.
            let cutters: Vec<&pl_enzymes::Digest> =
                admitted.iter().copied().filter(|d| d.count() > 0).collect();
            let gel = pl_gel::Gel::modelled(self.conditions);
            let mut chosen: Option<(&str, &str)> = None;
            'outer: for (i, a) in cutters.iter().enumerate() {
                for b in cutters.iter().skip(i + 1) {
                    let mut cuts = a.positions.clone();
                    cuts.extend(b.positions.iter().copied());
                    let frags =
                        pl_enzymes::fragments_from_cuts(&dedup(cuts), mol.len(), mol.topology);
                    let sim = gel.run(&frags);
                    if frags.len() >= 2 && sim.resolves(frags[0], frags[1]) {
                        chosen = Some((a.enzyme.name, b.enzyme.name));
                        break 'outer;
                    }
                }
            }
            // No pair resolves: take the first two by name and let the
            // disclosure say the pair will not separate. That is a true and
            // useful answer, not a failure.
            let pair = chosen.or(match cutters.as_slice() {
                [a, b, ..] => Some((a.enzyme.name, b.enzyme.name)),
                _ => None,
            });
            if let Some((a, b)) = pair {
                self.picked.insert(a.to_string());
                self.picked.insert(b.to_string());
                self.arrangement = Arrangement::Together;
            }
        }

        // And the ruler that best covers what is actually in the lanes.
        let mut all: Vec<u64> = Vec::new();
        for spec in self.specs(results, set) {
            if spec.cuts.is_empty() && mol.topology == Topology::Circular {
                continue;
            }
            all.extend(pl_enzymes::fragments_from_cuts(
                &spec.cuts,
                mol.len(),
                mol.topology,
            ));
        }
        self.ladder = View::best_ladder(&all);
    }
}

fn dedup(mut v: Vec<u64>) -> Vec<u64> {
    v.sort_unstable();
    v.dedup();
    v
}

/// The SAME naming the picture's captions use — capped the same way, because a
/// strip line listing 1,764 unplaceable fragments is no more readable than the
/// caption was. See `pl_gel::MAX_LISTED`.
fn join(v: &[u64]) -> String {
    pl_gel::name_sizes(v, ", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::Molecule;

    fn digest(mol: &Molecule) -> Vec<pl_enzymes::Digest> {
        pl_enzymes::digest_all(mol)
    }

    /// The verdict table `doc::DigestState` hands the Enzymes tab, computed the
    /// same way its worker does — at each enzyme's FIRST site, because a site
    /// that wraps the origin does not map back from a cut position.
    fn verdicts(mol: &Molecule) -> Vec<Option<SiteEffect>> {
        pl_enzymes::ENZYMES
            .iter()
            .map(|e| {
                pl_enzymes::cut_sites(&mol.seq, mol.topology, e)
                    .first()
                    .and_then(|s| {
                        pl_enzymes::methylation::site_effect(
                            e,
                            &mol.seq,
                            (s.site_start - 1) as usize,
                            mol.topology,
                            &mol.methylation,
                        )
                    })
            })
            .collect()
    }

    /// No methylation at all — the shape every test that is not about
    /// methylation wants.
    fn clean(results: &[pl_enzymes::Digest]) -> Vec<Option<SiteEffect>> {
        vec![None; results.len()]
    }

    fn demo() -> Molecule {
        let data = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../prototype/demo-construct.gb"
        ))
        .expect("the demo construct");
        pl_fileio::load(&data).expect("it parses").0
    }

    fn view(names: &[&str]) -> View {
        View {
            picked: names.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    /// PROVEN TO FAIL at 78a46f2: there is no gel in the GUI there at all.
    ///
    /// The oracle is `pl gel prototype/demo-construct.gb --cut NdeI`, whose
    /// table is
    ///
    /// ```text
    ///   NdeI
    ///     42.5 mm  2144
    ///     58.0 mm  1036
    /// ```
    ///
    /// Both numbers are asserted, not just their order: a picture whose bands
    /// are in the right sequence at the wrong distances is exactly the failure
    /// `Placement` exists to prevent.
    #[test]
    fn the_bands_are_where_pl_gel_puts_them() {
        let mol = demo();
        let results = digest(&mol);
        let v = view(&["NdeI"]);
        let built = v.build(
            &mol,
            &results,
            &clean(&results),
            pl_enzymes::EnzymeSet::All,
            "t",
        );
        let spec = &v.specs(&results, pl_enzymes::EnzymeSet::All)[0];
        let frags = pl_enzymes::fragments_from_cuts(&spec.cuts, mol.len(), mol.topology);
        let mut sorted = frags.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![1_036, 2_144], "the digest itself");

        let sim = pl_gel::Gel::modelled(pl_gel::Conditions::default()).run(&frags);
        let mm: Vec<String> = sim.groups.iter().map(|g| format!("{:.1}", g.mm)).collect();
        assert_eq!(mm, vec!["42.5", "58.0"], "the mm `pl gel` prints");

        // And the picture puts them there. `to_scene` draws a band at
        // `TOP + mm * scale` with TOP = 34 and scale = 4, so the two label
        // baselines are those two distances and nothing else.
        let ys = band_ys(&built.scene);
        assert_eq!(ys.len(), 2, "two bands in the sample lane");
        for (y, mm) in ys.iter().zip([42.5, 58.0]) {
            assert!(
                ((y - 34.0) / 4.0 - mm).abs() < 0.05,
                "a band at scene y {y} is {} mm, not {mm}",
                (y - 34.0) / 4.0
            );
        }
    }

    /// Every piece of text the picture itself carries.
    fn texts(sc: &pl_draw::Scene) -> Vec<String> {
        sc.items
            .iter()
            .filter_map(|i| match i {
                pl_draw::Item::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The y of each sample-lane band, in scene units.
    fn band_ys(sc: &pl_draw::Scene) -> Vec<f64> {
        // Sample band labels are the only `Anchor::Start` text at 9 pt.
        let mut ys: Vec<f64> = sc
            .items
            .iter()
            .filter_map(|i| match i {
                pl_draw::Item::Text {
                    y,
                    size,
                    anchor: pl_draw::scene::Anchor::Start,
                    ..
                } if (*size - 9.0).abs() < 1e-9 => Some(*y),
                _ => None,
            })
            .collect();
        ys.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        ys
    }

    /// PROVEN TO FAIL at 78a46f2 (no GUI gel), and against the obvious wrong
    /// implementation: dropping the fragments the gel cannot place.
    ///
    /// The measured `EcoRI+BamHI` double digest of the demo construct is
    /// 3,159 + **21**, and on a 1% gel the 21 has nowhere to go. It is in the
    /// picture's caption at 8.5 pt and it must be in the strip at readable
    /// size, because this is the case a real digest hits constantly.
    #[test]
    fn a_fragment_the_gel_cannot_place_is_disclosed_and_not_dropped() {
        let mol = demo();
        let results = digest(&mol);
        let mut v = view(&["EcoRI", "BamHI"]);
        v.arrangement = Arrangement::Together;
        let built = v.build(
            &mol,
            &results,
            &clean(&results),
            pl_enzymes::EnzymeSet::All,
            "t",
        );

        let spec = &v.specs(&results, pl_enzymes::EnzymeSet::All)[0];
        let frags = pl_enzymes::fragments_from_cuts(&spec.cuts, mol.len(), mol.topology);
        let mut sorted = frags.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![21, 3_159], "the digest itself");

        assert_eq!(built.unplaced, 1);
        let said = built.disclosure.join(" | ");
        assert!(said.contains("21 too small to place"), "{said}");
        assert!(said.contains("500–10000 bp"), "{said}");

        // THE INVARIANT: every fragment the digest makes is either in a drawn
        // group or named as unplaceable. Exactly once, and never nowhere.
        let sim = pl_gel::Gel::modelled(v.conditions).run(&frags);
        let drawn: usize = sim.groups.iter().map(|g| g.sizes.len()).sum();
        assert_eq!(
            sim.bands.len(),
            drawn + sim.too_large().len() + sim.too_small().len(),
            "a fragment went missing between the digest and the picture"
        );
    }

    /// PROVEN TO FAIL at 78a46f2 (no GUI gel).
    ///
    /// Two fragments 100 bp apart at 2 kb are ONE band on a 1% gel. The scene
    /// labels it `2000/2100` and marks it `bold: true` — and there is no bold
    /// face installed in this application, so if the strip did not say it in
    /// words the merge would reach the screen with no channel at all.
    #[test]
    fn co_migrating_fragments_are_merged_and_said_to_be() {
        let mol = Molecule {
            seq: b"A".repeat(4_100),
            topology: Topology::Linear,
            ..Default::default()
        };
        let v = View::default();
        let gel = pl_gel::Gel::modelled(v.conditions);
        let sim = gel.run(&[2_000, 2_100, 6_000]);
        assert_eq!(sim.merged().len(), 1);

        // Through the view, on a molecule whose digest really does this: one
        // cut before base 2,001 of a 4,100 bp linear molecule gives 2,000 and
        // 2,100.
        let results = vec![pl_enzymes::Digest {
            enzyme: pl_enzymes::by_name("EcoRI").expect("shipped"),
            positions: vec![2_001],
        }];
        let v = view(&["EcoRI"]);
        let built = v.build(
            &mol,
            &results,
            &clean(&results),
            pl_enzymes::EnzymeSet::All,
            "t",
        );
        assert_eq!(built.hidden, (2, 1), "{:?}", built.disclosure);
        let said = built.disclosure.join(" | ");
        assert!(said.contains("2 fragments hide in 1 band"), "{said}");
        // NAMED, not merely counted. `to_scene` marks the merge with
        // `bold: true` and there is no bold face installed in this
        // application, so the words are the channel that survives.
        assert!(said.contains("2000/2100"), "{said}");
        // And the picture draws ONE band where the digest made two fragments.
        assert_eq!(band_ys(&built.scene).len(), 1);
    }

    /// PROVEN TO FAIL against `pl gel prototype/demo-construct.gb --lane PacI`,
    /// which prints `34.1 mm 3180` and draws a band for an enzyme that does not
    /// cut this plasmid at all.
    ///
    /// The tube holds supercoiled circle. It does not run at the contour
    /// length; it runs well ahead of it, and `pl-gel` has no term for topology.
    #[test]
    fn a_lane_with_no_cuts_on_a_circle_draws_no_band() {
        let mol = demo();
        assert_eq!(mol.topology, Topology::Circular);
        let results = digest(&mol);
        let v = view(&["PacI"]);
        assert_eq!(
            results
                .iter()
                .find(|d| d.enzyme.name == "PacI")
                .expect("PacI is shipped")
                .count(),
            0,
            "the fixture depends on PacI not cutting this plasmid"
        );
        let built = v.build(
            &mol,
            &results,
            &clean(&results),
            pl_enzymes::EnzymeSet::All,
            "t",
        );
        // The ladder's bands are drawn; the sample lane's are not.
        assert!(band_ys(&built.scene).is_empty(), "a band was drawn");
        let said = built.disclosure.join(" | ");
        assert!(said.contains("PacI does not cut this molecule"), "{said}");
        assert!(said.contains("supercoiling is not modelled"), "{said}");

        // A LINEAR molecule with no cuts is a different answer and does get a
        // band: that really is the uncut input at its own length.
        let linear = Molecule {
            seq: b"A".repeat(3_000),
            topology: Topology::Linear,
            ..Default::default()
        };
        let results = vec![pl_enzymes::Digest {
            enzyme: pl_enzymes::by_name("PacI").expect("shipped"),
            positions: vec![],
        }];
        let built = v.build(
            &linear,
            &results,
            &clean(&results),
            pl_enzymes::EnzymeSet::All,
            "t",
        );
        assert_eq!(band_ys(&built.scene).len(), 1, "the uncut linear input");
    }

    /// A `Dam+` copy of the demo construct, as `.dna` files from a real
    /// SnapGene install carry — which is this user's primary input format.
    fn dam_positive() -> Molecule {
        let mut mol = demo();
        mol.methylation.dam = true;
        mol.methylation.dcm = true;
        mol
    }

    /// PROVEN TO FAIL before this change: the Enzymes tab struck `BclI`
    /// through and printed "Dam blocked", the tick on that same row put it on
    /// the gel, and the gel drew its two fragments as fact. One row, two
    /// opposite answers — the split-brain "ONE control, one answer" exists to
    /// stop — and `grep -i methyl bins/pl-gui/src/gel.rs` came back empty, so
    /// no lane, caption or disclosure line could have mentioned it.
    ///
    /// The tick was not even the user's: `seed` sorts multi-cutters by cut
    /// count and BclI cuts this plasmid twice, so a Dam+ file OPENED on that
    /// lane.
    #[test]
    fn a_methylation_blocked_enzyme_is_never_seeded_and_never_drawn_in_silence() {
        let mol = dam_positive();
        let results = digest(&mol);
        let v = verdicts(&mol);
        // The fixture depends on this: BclI is the one unconditional Dam block
        // in the table, at TGATCA.
        let bcli = View::verdict_for(&results, &v, "BclI").expect("BclI is Dam-blocked");
        assert_eq!(bcli.effect, Effect::Blocked);
        assert_eq!(bcli.methylase, pl_enzymes::methylation::Methylase::Dam);
        assert!(
            results
                .iter()
                .find(|d| d.enzyme.name == "BclI")
                .expect("shipped")
                .count()
                > 1,
            "and it is a multi-cutter, which is why `seed` used to reach for it"
        );

        // (a) NOT SEEDED. The default gel of this file must not open on a lane
        // the Enzymes tab is striking through.
        let mut seeded = View::default();
        seeded.seed(&mol, &results, &v, pl_enzymes::EnzymeSet::All);
        assert!(
            !seeded.picked.contains("BclI"),
            "seeded with a blocked enzyme: {:?}",
            seeded.picked
        );

        // (b) TICKED BY HAND, IT STILL DRAWS — hiding it would be the map's old
        // silent filter — but the methylase is named, in the strip AND in the
        // picture's own note, so an exported figure carries it too.
        let hand = view(&["BclI"]);
        let built = hand.build(&mol, &results, &v, pl_enzymes::EnzymeSet::All, "t");
        assert_eq!(
            band_ys(&built.scene).len(),
            2,
            "the two fragments are drawn"
        );
        let said = built.disclosure.join(" | ");
        assert!(
            said.contains("BclI is blocked by Dam methylation"),
            "{said}"
        );
        assert!(said.contains("UNMETHYLATED template"), "{said}");
        let in_picture = texts(&built.scene).join(" ");
        for word in ["blocked", "Dam", "UNMETHYLATED"] {
            assert!(
                in_picture.contains(word),
                "{word:?} never reached the picture: {in_picture}"
            );
        }

        // ONCE, however many lanes the enzyme is in. `Both` puts it in its own
        // lane AND in the combined one, and the sentence is about the enzyme.
        let mut both = view(&["BclI", "AgeI"]);
        both.arrangement = Arrangement::Both;
        let b = both.build(&mol, &results, &v, pl_enzymes::EnzymeSet::All, "t");
        assert_eq!(
            both.specs(&results, pl_enzymes::EnzymeSet::All).len(),
            3,
            "the fixture needs BclI in two lanes"
        );
        assert_eq!(
            b.disclosure
                .iter()
                .filter(|l| l.contains("blocked by Dam"))
                .count(),
            1,
            "{:?}",
            b.disclosure
        );

        // (c) THE CONTROL. The same enzyme on an unmethylated template says
        // nothing about methylation, so the sentence cannot be boilerplate.
        let plain = demo();
        let plain_results = digest(&plain);
        let plain_v = verdicts(&plain);
        assert!(View::verdict_for(&plain_results, &plain_v, "BclI").is_none());
        let built = hand.build(
            &plain,
            &plain_results,
            &plain_v,
            pl_enzymes::EnzymeSet::All,
            "t",
        );
        assert!(
            !built.disclosure.join(" ").contains("methylation"),
            "{:?}",
            built.disclosure
        );
        let mut seeded = View::default();
        seeded.seed(&plain, &plain_results, &plain_v, pl_enzymes::EnzymeSet::All);
        assert!(seeded.picked.contains("BclI"), "{:?}", seeded.picked);
    }

    /// PROVEN TO FAIL before this change: the ladder was pushed in as `lanes[0]`
    /// BEFORE the loop that measures every lane, so the one lane a reader sizes
    /// everything else against was the one lane never audited.
    ///
    /// `built.unplaced` said 5 on the demo construct while the picture actually
    /// omitted 10 — the 1kb-plus ladder's own 12000, 100, 200, 300 and 400 —
    /// and at a band width the GUI offers, two of its rungs are doublets with
    /// `built.hidden` reporting `(0, 0)`.
    #[test]
    fn the_ladder_is_audited_like_every_other_lane() {
        let mol = demo();
        let results = digest(&mol);
        let mut v = view(&["NdeI"]);
        v.ladder = "1kb-plus";
        let built = v.build(
            &mol,
            &results,
            &clean(&results),
            pl_enzymes::EnzymeSet::All,
            "t",
        );
        // 12000 is past the top of a 1% gel and 100-400 run with the dye front:
        // five of the ladder's seventeen sizes.
        assert_eq!(built.unplaced, 5, "{:?}", built.disclosure);
        let said = built.disclosure.join(" | ");
        assert!(said.contains("the 1kb-plus ladder"), "{said}");
        assert!(said.contains("12 of its 17 bands are drawn"), "{said}");

        // AND ITS DOUBLETS. At 4 mm bands — inside the range the GUI's own
        // control offers — 5000/6000 and 850/1000 co-migrate, so a reader
        // counting seventeen rungs is counting two that are not there.
        let mut wide = v;
        wide.conditions.band_mm = 4.0;
        let built = wide.build(
            &mol,
            &results,
            &clean(&results),
            pl_enzymes::EnzymeSet::All,
            "t",
        );
        assert!(built.hidden.0 >= 4, "{:?}", built.hidden);
        let said = built.disclosure.join(" | ");
        assert!(said.contains("is not 17 rungs on this gel"), "{said}");
        assert!(said.contains("5000/6000"), "{said}");
    }

    /// A lane set the app chose says it chose. `seed` keeps six of however many
    /// cut more than once and the Enzymes tab prints the true count three
    /// inches away; the strip used to say nothing at all.
    #[test]
    fn a_seeded_gel_admits_it_is_showing_a_subset() {
        let mol = demo();
        let results = digest(&mol);
        let v = verdicts(&mol);
        let multi = results
            .iter()
            .filter(|d| d.count() > 1 && pl_enzymes::EnzymeSet::All.admits(d))
            .count();
        assert!(multi > 6, "the fixture needs more than six multi-cutters");
        let mut seeded = View::default();
        seeded.seed(&mol, &results, &v, pl_enzymes::EnzymeSet::All);
        let built = seeded.build(&mol, &results, &v, pl_enzymes::EnzymeSet::All, "t");
        let said = built.disclosure.join(" | ");
        assert!(said.contains("This gel is a suggestion"), "{said}");
        assert!(said.contains(&format!("of the {multi} enzymes")), "{said}");

        // And it goes away once the lane set is the user's: `seed` is what sets
        // the note, and a hand-built view never had one.
        let hand = view(&["NdeI"]);
        let built = hand.build(&mol, &results, &v, pl_enzymes::EnzymeSet::All, "t");
        assert!(
            !built.disclosure.join(" ").contains("suggestion"),
            "{:?}",
            built.disclosure
        );
    }

    /// A gel with nothing in it says so, rather than reading as "this molecule
    /// has no sites".
    #[test]
    fn a_gel_with_no_enzymes_ticked_knows_that_it_is_empty() {
        let mol = demo();
        let results = digest(&mol);
        let built = view(&[]).build(
            &mol,
            &results,
            &clean(&results),
            pl_enzymes::EnzymeSet::All,
            "t",
        );
        assert!(built.empty);
        assert!(band_ys(&built.scene).is_empty());
        // And a gel with a lane is not empty, so the flag means one thing.
        let built = view(&["NdeI"]).build(
            &mol,
            &results,
            &clean(&results),
            pl_enzymes::EnzymeSet::All,
            "t",
        );
        assert!(!built.empty);
    }

    /// A merged band's name in the strip is the SAME name the band carries, and
    /// both are capped. See `pl_gel::MAX_LISTED`.
    ///
    /// PROVEN TO FAIL before that cap, both ways: the scene came out tens of
    /// thousands of points wide and one disclosure line was 8,768 characters.
    #[test]
    fn a_band_of_a_thousand_fragments_is_counted_rather_than_listed() {
        // 1,769 fragments of 600-650 bp: every one is far closer to the next
        // than a band width, so single linkage makes them ONE band — which is
        // exactly what a genome digest by a 6-cutter does.
        let cuts: Vec<u64> = (0..1_768).map(|i| 600 + i * 650).collect();
        let n = cuts.last().copied().expect("non-empty") + 600;
        let mol = Molecule {
            seq: b"A".repeat(n as usize),
            topology: Topology::Linear,
            ..Default::default()
        };
        let results = vec![pl_enzymes::Digest {
            enzyme: pl_enzymes::by_name("EcoRI").expect("shipped"),
            positions: cuts,
        }];
        let built = view(&["EcoRI"]).build(
            &mol,
            &results,
            &clean(&results),
            pl_enzymes::EnzymeSet::All,
            "t",
        );
        // Two bands: the 1,767 internal 650s, and the two 600-ish ends, which
        // this gel does resolve from them.
        assert_eq!(built.hidden, (1_769, 2), "{:?}", built.disclosure);
        let said = built.disclosure.join(" | ");
        assert!(said.contains("1769 fragments hide in 2 bands"), "{said}");
        assert!(
            said.contains("650 (1767 fragments)"),
            "the band's own name: {said}"
        );
        let longest = said.split(" | ").map(str::len).max().unwrap_or(0);
        assert!(longest < 400, "a disclosure line is {longest} characters");
        // The picture stays a picture: an uncapped label made this 280,947 pt.
        assert!(built.scene.width < 900.0, "{} pt", built.scene.width);
    }

    /// Narrowing the filter must not silently delete a lane.
    #[test]
    fn an_enzyme_the_filter_excludes_is_suspended_by_name_and_not_dropped() {
        let mol = demo();
        let results = digest(&mol);
        // SmaI cuts twice, so `Unique` excludes it.
        let v = view(&["SmaI"]);
        let built = v.build(
            &mol,
            &results,
            &clean(&results),
            pl_enzymes::EnzymeSet::Unique,
            "t",
        );
        assert_eq!(built.suspended, vec!["SmaI".to_string()]);
        let said = built.disclosure.join(" | ");
        assert!(said.contains("SmaI is in your gel but not in"), "{said}");
        // And it is genuinely not drawn, rather than drawn and complained about.
        assert!(band_ys(&built.scene).is_empty());
    }

    /// The caveat is the first thing said, always, and for a measured
    /// calibration too — a disclosure that is sometimes absent teaches the user
    /// that its absence means "nothing to know".
    #[test]
    fn the_calibration_statement_is_always_the_first_line() {
        let mol = demo();
        let results = digest(&mol);
        for names in [vec!["NdeI"], vec!["PacI"], vec![]] {
            let v = view(&names);
            let built = v.build(
                &mol,
                &results,
                &clean(&results),
                pl_enzymes::EnzymeSet::All,
                "t",
            );
            assert!(
                built.disclosure[0].contains("not good enough to size an unknown band"),
                "{:?}",
                built.disclosure[0]
            );
        }
    }

    #[test]
    fn the_lane_order_is_stated_and_does_not_wander() {
        let mol = demo();
        let results = digest(&mol);
        let mut v = view(&["SmaI", "NdeI", "XmaI"]);
        v.arrangement = Arrangement::Both;
        let a: Vec<String> = v
            .specs(&results, pl_enzymes::EnzymeSet::All)
            .iter()
            .map(|s| s.label.clone())
            .collect();
        assert_eq!(a, vec!["NdeI", "SmaI", "XmaI", "NdeI+SmaI+XmaI"]);
        for _ in 0..8 {
            let b: Vec<String> = v
                .specs(&results, pl_enzymes::EnzymeSet::All)
                .iter()
                .map(|s| s.label.clone())
                .collect();
            assert_eq!(a, b);
        }
    }

    #[test]
    fn the_seeded_gel_opens_on_the_enzymes_worth_looking_at() {
        let mol = demo();
        let results = digest(&mol);
        let mut v = View::default();
        v.seed(&mol, &results, &clean(&results), pl_enzymes::EnzymeSet::All);
        // Never more than six, and every one of them worth a lane: a circular
        // plasmid cut ONCE linearises to a single band, so a gel of unique
        // cutters is a wall of identical bands and tells nobody anything.
        assert!(
            !v.picked.is_empty() && v.picked.len() <= 6,
            "{:?}",
            v.picked
        );
        for name in &v.picked {
            let d = results
                .iter()
                .find(|d| d.enzyme.name == name.as_str())
                .expect("a real enzyme");
            assert!(d.count() > 1, "{name} cuts {} times", d.count());
        }

        // A set with no multi-cutter falls back to a resolving PAIR rather than
        // to nothing, because a single cut on a circle is one band.
        let mut v = View::default();
        v.seed(
            &mol,
            &results,
            &clean(&results),
            pl_enzymes::EnzymeSet::Unique,
        );
        assert_eq!(v.arrangement, Arrangement::Together);
        assert_eq!(v.picked.len(), 2, "{:?}", v.picked);
    }

    #[test]
    fn the_ladder_is_the_one_that_covers_the_fragments() {
        // All under 1,500: the 1 kb ladder starts at 500 and would be a poor
        // ruler.
        assert_eq!(View::best_ladder(&[120, 300, 900]), "100bp");
        assert_eq!(View::best_ladder(&[2_000, 6_000]), "1kb");
        // Nothing to go on ties to the CLI's default.
        assert_eq!(View::best_ladder(&[]), "1kb");
    }
}
