//! Joining fragments by their ENDS, which is what a restriction cloning is.
//!
//! [`crate::assembly`] joins fragments by sequence HOMOLOGY — Gibson, HiFi, and
//! anything else where the overlap is designed into the primers. This module is
//! the other half, and the older one: two fragments meet because a ligase can
//! seal them, which depends on the single-stranded ends and on nothing else.
//! `BamHI` and `BglII` have different recognition sites and the same `GATC`
//! overhang, so a `BamHI` vector takes a `BglII` insert; the sequences either
//! side of the join are irrelevant to whether it works.
//!
//! # Why this exists separately
//!
//! `End::ligates_with` has been here since the beginning and answers "may these
//! two ends be joined". Nothing turned that into a MOLECULE. `try_cut` made
//! fragments and `assemble` joined them by homology, so a user could digest a
//! plasmid and could not religate it — the one operation the whole crate is
//! named for. `docs/PLAN.md` §6 lists "complete digest then religation
//! reconstructs the original" as a validation criterion, and it was a criterion
//! nothing could execute.
//!
//! # The arithmetic
//!
//! `Dseq` is pydna's representation and this follows it: watson runs 5'->3',
//! crick runs 5'->3' antiparallel, and `ovhg` places crick's 3' end relative to
//! watson's 5' start. Joining `a` then `b` concatenates each strand in its own
//! direction — watson forwards, crick backwards — and keeps `a`'s offset:
//!
//! ```text
//! watson = a.watson + b.watson
//! crick  = b.crick  + a.crick
//! ovhg   = a.ovhg
//! ```
//!
//! The shared overhang is not written twice because it was only ever on one
//! strand of each fragment: `a` carried it on one, `b` carries it on the other,
//! and annealing them is what the concatenation expresses. Every field here is
//! cross-checked against pydna in `reference/python/tests/xcheck_clone.py`
//! rather than argued from this comment.

use crate::{rc, Dseq, End};

/// Knobs for a ligation.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Refuse rather than enumerate beyond this many fragments.
    ///
    /// The search is over orderings and orientations, so it grows as `n!·2ⁿ` —
    /// the same wall [`crate::assembly::Options::max_fragments`] hits, for the
    /// same reason, and it is set to the same number so the two operations
    /// refuse at the same size rather than for different reasons.
    pub max_fragments: usize,
    /// Join blunt ends to blunt ends.
    ///
    /// OFF by default, and that is a judgement about what a wrong answer costs.
    /// Blunt ligation is real and routine, but every blunt end is compatible
    /// with every other, so switching it on turns a digest with two blunt
    /// fragments into a combinatorial spray of products, most of which nobody
    /// intends. A sticky-only search says "these three arrangements work"; a
    /// blunt-inclusive one says "here are forty", and the user must then find
    /// the one they meant. Ask for it deliberately.
    pub blunt: bool,
    /// Report linear products as well as circular ones.
    ///
    /// Off by default because a plasmid cloning wants a circle: a linear
    /// product is usually an intermediate or a dead end, and listing every
    /// partial join alongside the real answer buries it.
    pub linear: bool,
    /// How many fragment PICKS [`subclone`] will try before refusing.
    ///
    /// The choice of one fragment per digest multiplies: two digests of twelve
    /// and eighteen fragments is 216 combinations, and each of those runs a full
    /// `n!·2ⁿ` ligation search inside it. `max_fragments` bounds the inner
    /// search and says nothing about the outer one, so this is a second wall for
    /// a second dimension rather than a duplicate of the first.
    ///
    /// Four thousand and ninety-six. A real subcloning is two or three digests
    /// of a handful of fragments each; anything past this is a user who has
    /// ticked every enzyme in the table, and refusing with the number in the
    /// message is more use to them than a frozen window.
    pub max_combinations: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            max_fragments: 8,
            blunt: false,
            linear: false,
            max_combinations: 4096,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LigateError {
    NotEnoughFragments,
    TooManyFragments {
        given: usize,
        max: usize,
    },
    /// A circle cannot be a starting fragment: it has no ends to ligate.
    CircularInput {
        index: usize,
    },
    /// A [`subclone`] digest with nothing in it.
    ///
    /// Named rather than answered with an empty result. "One fragment from each
    /// digest" cannot be done when one digest has none, and a silent `Ok(vec![])`
    /// reads as "your vector and insert do not fit together" — which sends the
    /// user to choose different enzymes for a problem that is in the OTHER
    /// molecule, where they did not look.
    EmptyPool {
        pool: usize,
    },
    CircularPoolInput {
        pool: usize,
        index: usize,
    },
    TooManyCombinations {
        given: usize,
        max: usize,
    },
}

impl std::fmt::Display for LigateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LigateError::NotEnoughFragments => {
                write!(f, "a ligation needs at least one fragment")
            }
            LigateError::TooManyFragments { given, max } => write!(
                f,
                "{given} fragments is more than this search will enumerate (max {max}); \
                 it grows as n! * 2^n"
            ),
            LigateError::CircularInput { index } => write!(
                f,
                "fragment {index} is circular, so it has no ends to ligate; cut it first"
            ),
            LigateError::EmptyPool { pool } => write!(
                f,
                "digest {pool} produced no fragments, so there is nothing to take from it"
            ),
            LigateError::CircularPoolInput { pool, index } => write!(
                f,
                "fragment {index} of digest {pool} is circular, so it has no ends to ligate; \
                 cut it first"
            ),
            LigateError::TooManyCombinations { given, max } => write!(
                f,
                "{given} ways to pick one fragment from each digest is more than this search \
                 will try (max {max}); cut with fewer enzymes"
            ),
        }
    }
}

impl std::error::Error for LigateError {}

/// One way the fragments can be joined.
#[derive(Debug, Clone, PartialEq)]
pub struct Product {
    pub seq: Dseq,
    /// The fragments in the order used, and whether each was flipped.
    pub order: Vec<(usize, bool)>,
    /// The end sealed at each junction, in the same order. For a circular
    /// product the last entry is the junction that closes the circle.
    pub junctions: Vec<End>,
}

impl Product {
    /// The identity of the finished molecule, so two routes to the same
    /// construct collapse to one answer.
    ///
    /// Delegates to [`crate::assembly::Product`]'s rule rather than restating
    /// it: `cdseguid` for a circle because it is invariant to rotation and
    /// strand, `ldseguid` for a linear one because it is invariant to strand
    /// only, and a marked raw fallback when an ambiguity code makes both
    /// impossible. A second copy of that decision would be a second place for
    /// it to drift.
    pub fn identity(&self) -> String {
        crate::assembly::Product {
            seq: self.seq.clone(),
            order: self.order.clone(),
            junctions: Vec::new(),
        }
        .identity()
    }
}

/// The molecule end-for-end: the same duplex read from the other strand.
///
/// Watson and crick swap as strings — each is already 5'->3', so neither is
/// re-complemented — and the new offset is the right-hand one, `w + ovhg - c`,
/// unchanged in sign. The end that was on the right becomes the left end and
/// keeps its shape.
///
/// Pinned by the two cases in
/// `the_flip_offset_matches_pydna_on_the_cases_that_fix_its_sign`, and checked
/// against pydna's `reverse_complement` field by field.
pub fn flip(d: &Dseq) -> Dseq {
    let (w, c) = (d.watson.len() as i64, d.crick.len() as i64);
    Dseq {
        watson: d.crick.clone(),
        crick: d.watson.clone(),
        // Exactly the `d = w + ovhg - c` that `right_end` computes, NOT its
        // negation — the end that was on the right becomes the left one, and it
        // keeps its shape rather than inverting it. Derived by re-coordinating:
        // the old watson becomes the new crick, whose 3' end lands `w` in from
        // the new right edge, so `-new_ovhg = R - w` with `R = c - ovhg`.
        //
        // The negation looked plausible and is wrong, which is why the two
        // fixed points below are asserted rather than assumed: a blunt fragment
        // must flip to `ovhg == 0`, and a fragment cut once out of a circle has
        // the same overhang at both ends, so flipping must leave `ovhg`
        // unchanged. The negation passes the first and fails the second.
        ovhg: w + d.ovhg - c,
        circular: d.circular,
    }
}

/// Whether these two ends may be sealed, honouring the blunt policy.
fn joins(left: &End, right: &End, opts: &Options) -> bool {
    if matches!(left, End::Blunt) && matches!(right, End::Blunt) && !opts.blunt {
        return false;
    }
    left.ligates_with(right)
}

/// Seal `b` onto the right of `a`. `None` when the ends cannot be joined.
///
/// The junction is `a`'s right end against `b`'s left end. Neither may be a
/// circle: a circle has no ends, and silently treating one as linear here is
/// how a tool invents a construct nobody asked for.
pub fn join(a: &Dseq, b: &Dseq, opts: &Options) -> Option<Dseq> {
    if a.circular || b.circular {
        return None;
    }
    if !joins(&a.right_end(), &b.left_end(), opts) {
        return None;
    }
    Some(Dseq {
        watson: format!("{}{}", a.watson, b.watson),
        crick: format!("{}{}", b.crick, a.crick),
        ovhg: a.ovhg,
        circular: false,
    })
}

/// Close a linear molecule into a circle by sealing its own two ends.
///
/// `None` when they cannot be joined. A circular `Dseq` in this representation
/// is canonical — `ovhg == 0`, crick the reverse complement of watson — and
/// watson is already exactly one turn of the circle, running from one nick all
/// the way round to the same nick, so closing it is a re-labelling and not a
/// re-splicing. That is why re-closing a molecule you have just cut gives the
/// bases back unchanged.
pub fn looped(a: &Dseq, opts: &Options) -> Option<Dseq> {
    if a.circular {
        return Some(a.clone());
    }
    if !joins(&a.right_end(), &a.left_end(), opts) {
        return None;
    }
    Some(Dseq {
        crick: rc(&a.watson),
        watson: a.watson.clone(),
        ovhg: 0,
        circular: true,
    })
}

/// The depth-first search over orderings and orientations.
///
/// A struct rather than nine loose parameters: the fragments, the options and
/// the two accumulators are the same for every level of the recursion, and only
/// the four that change are passed down.
struct Search<'a> {
    frags: &'a [Dseq],
    opts: &'a Options,
    out: Vec<Product>,
    seen: std::collections::BTreeSet<String>,
}

impl Search<'_> {
    fn walk(
        &mut self,
        used: &mut Vec<bool>,
        order: &mut Vec<(usize, bool)>,
        run: &Dseq,
        junctions: &mut Vec<End>,
        pinned: bool,
    ) {
        if order.len() == self.frags.len() {
            // Closed: the run's own two ends seal.
            if let Some(c) = looped(run, self.opts) {
                let mut js = junctions.clone();
                js.push(run.right_end());
                let p = Product {
                    seq: c,
                    order: order.clone(),
                    junctions: js,
                };
                if self.seen.insert(p.identity()) {
                    self.out.push(p);
                }
            }
            // Linear products only from the unpinned pass: with fragment 0
            // pinned first, a linear arrangement that does not start there is
            // unreachable, and reporting the ones that do would be an arbitrary
            // subset rather than an answer.
            if self.opts.linear && !pinned {
                let p = Product {
                    seq: run.clone(),
                    order: order.clone(),
                    junctions: junctions.clone(),
                };
                if self.seen.insert(format!("linear:{}", p.identity())) {
                    self.out.push(p);
                }
            }
            return;
        }
        for i in 0..self.frags.len() {
            if used[i] {
                continue;
            }
            for &flipped in &[false, true] {
                let next = if flipped {
                    flip(&self.frags[i])
                } else {
                    self.frags[i].clone()
                };
                let Some(joined) = join(run, &next, self.opts) else {
                    continue;
                };
                used[i] = true;
                order.push((i, flipped));
                junctions.push(run.right_end());
                self.walk(used, order, &joined, junctions, pinned);
                junctions.pop();
                order.pop();
                used[i] = false;
            }
        }
    }
}

/// Every distinct molecule these fragments can be ligated into.
///
/// Circular products by default; linear ones too with [`Options::linear`].
/// Results are deduplicated by [`Product::identity`], so the same construct
/// reached by a different route or read from the other strand is reported once.
///
/// Every fragment must be used. A ligation that quietly dropped one would be
/// answering a question the user did not ask — "what can I make from SOME of
/// this" — and the fragment left out is usually the insert.
pub fn ligate(fragments: &[Dseq], opts: &Options) -> Result<Vec<Product>, LigateError> {
    if fragments.is_empty() {
        return Err(LigateError::NotEnoughFragments);
    }
    if fragments.len() > opts.max_fragments {
        return Err(LigateError::TooManyFragments {
            given: fragments.len(),
            max: opts.max_fragments,
        });
    }
    if let Some(i) = fragments.iter().position(|f| f.circular) {
        return Err(LigateError::CircularInput { index: i });
    }

    let n = fragments.len();
    let mut s = Search {
        frags: fragments,
        opts,
        out: Vec::new(),
        seen: std::collections::BTreeSet::new(),
    };
    let mut order: Vec<(usize, bool)> = Vec::new();
    let mut used = vec![false; n];

    // Fragment 0 is pinned first and unflipped. A circular product has no
    // starting point and no preferred strand, so every arrangement is reachable
    // with it fixed, and fixing it is what stops n rotations of one answer from
    // being reported as n answers. For linear products the pin would lose real
    // arrangements, so they are enumerated separately below.
    // Circular search: fragment 0 pinned, unflipped.
    used[0] = true;
    order.push((0, false));
    let mut junctions: Vec<End> = Vec::new();
    s.walk(&mut used, &mut order, &fragments[0], &mut junctions, true);
    order.pop();
    used[0] = false;

    // A single fragment closing on itself is the commonest ligation there is —
    // a vector cut once and re-closed — and the walk above already covers it,
    // since `order.len() == 1 == frags.len()` on entry.

    if opts.linear {
        for start in 0..n {
            for &flipped in &[false, true] {
                used[start] = true;
                order.push((start, flipped));
                let run = if flipped {
                    flip(&fragments[start])
                } else {
                    fragments[start].clone()
                };
                let mut js: Vec<End> = Vec::new();
                s.walk(&mut used, &mut order, &run, &mut js, false);
                order.pop();
                used[start] = false;
            }
        }
    }

    Ok(s.out)
}

/// One construct, and every way of picking fragments that builds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Construct {
    /// The molecule, and how the chosen fragments were arranged.
    ///
    /// `product.order[k].0` IS A POOL INDEX, not a fragment index, and that is
    /// not a coincidence to be documented away: exactly one fragment is taken
    /// from each digest, so the slice handed to [`ligate`] is one fragment per
    /// pool in pool order, and `order` indexes it. Which fragment came from that
    /// pool is in [`Construct::routes`].
    pub product: Product,
    /// Each entry is one fragment index per pool, in pool order.
    ///
    /// More than one when different picks build the same molecule — two halves
    /// of a palindromic digest, say. Reported rather than collapsed to whichever
    /// the loop met first, because "these two choices give you the same plasmid"
    /// is the answer, and picking one arbitrarily would present a coincidence as
    /// a fact about the vector.
    pub routes: Vec<Vec<usize>>,
}

/// Every construct that uses exactly one fragment from each digest.
///
/// THIS IS SUBCLONING. Cut the vector, cut the insert, take one piece of each,
/// ligate. [`ligate`] cannot answer it and should not: every fragment must be
/// used there, deliberately, because a ligation that quietly dropped one would
/// be answering "what can I make from SOME of this" — and the fragment left out
/// is usually the insert. The choice is made HERE instead, one combination at a
/// time, and each combination goes through `ligate` unchanged.
///
/// Results are grouped by [`Product::identity`], so a molecule reachable by two
/// different picks is one construct with two routes rather than two answers.
///
/// A combination that cannot be sealed simply contributes nothing. That is not
/// an error and is the ordinary case: most pairs of fragments from two digests
/// have no compatible ends, which is exactly what makes the ones that do worth
/// reporting.
pub fn subclone(pools: &[Vec<Dseq>], opts: &Options) -> Result<Vec<Construct>, LigateError> {
    if pools.is_empty() {
        return Err(LigateError::NotEnoughFragments);
    }
    if pools.len() > opts.max_fragments {
        return Err(LigateError::TooManyFragments {
            given: pools.len(),
            max: opts.max_fragments,
        });
    }
    // Checked HERE and not left to `ligate` to hit combination by combination.
    // An empty or circular pool makes every one of the combinations below fail
    // identically, so discovering it inside the loop would report it as "no
    // construct is possible" — a sentence about the enzymes, when the fault is
    // that one of the molecules was never cut.
    for (i, p) in pools.iter().enumerate() {
        if p.is_empty() {
            return Err(LigateError::EmptyPool { pool: i });
        }
        if let Some(j) = p.iter().position(|f| f.circular) {
            return Err(LigateError::CircularPoolInput { pool: i, index: j });
        }
    }
    // `checked_mul`, because the product of a few pool sizes overflows `usize`
    // long before it becomes slow, and a wrapped count would pass the very
    // guard it is being computed for.
    let combos = pools
        .iter()
        .try_fold(1usize, |a, p| a.checked_mul(p.len()))
        .unwrap_or(usize::MAX);
    if combos > opts.max_combinations {
        return Err(LigateError::TooManyCombinations {
            given: combos,
            max: opts.max_combinations,
        });
    }

    let mut out: Vec<Construct> = Vec::new();
    let mut index: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut pick = vec![0usize; pools.len()];
    loop {
        let frags: Vec<Dseq> = pools
            .iter()
            .zip(&pick)
            .map(|(p, &i)| p[i].clone())
            .collect();
        // `?` and not a swallowed error. Every cause `ligate` can refuse for is
        // ruled out above, so one arriving here is a disagreement between the
        // two functions' preconditions and must be heard rather than absorbed
        // once per combination.
        for p in ligate(&frags, opts)? {
            let id = p.identity();
            match index.get(&id) {
                Some(&k) => out[k].routes.push(pick.clone()),
                None => {
                    index.insert(id, out.len());
                    out.push(Construct {
                        product: p,
                        routes: vec![pick.clone()],
                    });
                }
            }
        }
        // The odometer. Last pool turns fastest, so the routes come out in
        // pool-major order and two runs over the same input agree.
        let mut k = pools.len();
        loop {
            if k == 0 {
                return Ok(out);
            }
            k -= 1;
            pick[k] += 1;
            if pick[k] < pools[k].len() {
                break;
            }
            pick[k] = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::try_cut;
    use pl_enzymes::by_name;

    fn enz(n: &str) -> &'static pl_enzymes::Enzyme {
        by_name(n).expect("in the table")
    }

    /// The fixed points that pin `flip`'s sign, which the first version had
    /// backwards.
    ///
    /// Cross-checked against pydna field by field — same representation, so
    /// `watson`, `crick` and `ovhg` compare directly rather than through a
    /// summary. The run covered 7 molecules across BamHI, BglII, EcoRI, EcoRV,
    /// PstI and KpnI: cut 12/12, flip 12/12 (against pydna's
    /// `reverse_complement`), join 12/12 (against its `__add__`) and loop 2/2
    /// (against `looped`), with ZERO disagreements.
    ///
    /// Pinned here rather than re-run, for the reason the enzyme oracle is
    /// pinned: pydna is not a build dependency, and a check that skips when its
    /// oracle is absent passes for the wrong reason.
    #[test]
    fn the_flip_offset_matches_pydna_on_the_cases_that_fix_its_sign() {
        // A fragment cut once out of a circle has the SAME overhang at both
        // ends, so flipping must leave `ovhg` alone. The negated formula gives
        // +4 here and this is the assertion that caught it.
        let m = Dseq::new("AAAAGGATCCTTTTGCGCGCATATATCCCGGGAAAATTTTCCCC", true);
        let f = &try_cut(&m, enz("BamHI")).expect("cuts")[0];
        assert_eq!(f.ovhg, -4);
        assert_eq!(flip(f).ovhg, -4, "flip inverted an overhang it should keep");
        assert_eq!(flip(f).watson, f.crick, "strands did not exchange");
        assert_eq!(flip(f).crick, f.watson);

        // Blunt flips to blunt: satisfied by BOTH the right formula and the
        // wrong one, which is why it cannot be the only case.
        let b = Dseq::new("ACGTACGT", false);
        assert_eq!(flip(&b).ovhg, 0);

        // An asymmetric fragment: 5' overhang on the left, blunt on the right,
        // flips to blunt-left and overhang-right.
        let a = Dseq::from_parts("GATCCAAAA", "TTTTGGATC", -4, false);
        assert_eq!(
            a.left_end(),
            End::Overhang {
                five_prime: true,
                bases: "GATC".into()
            }
        );
        let g = flip(&a);
        assert_eq!(
            g.right_end(),
            a.left_end(),
            "the end changed shape when flipped"
        );
        assert_eq!(g.left_end(), a.right_end());
    }

    /// THE OPERATION THIS WHOLE CRATE IS FOR, and which nothing could execute
    /// until now: cut a vector, cut an insert, put the insert in the vector.
    ///
    /// PROVEN TO FAIL against 8c41d59 — `subclone` does not exist there, and the
    /// reason it had to is not an omission but a deliberate contract. `ligate`
    /// uses EVERY fragment, so handing it all four pieces of the two digests
    /// asks for a four-piece construct and never returns the two-piece one the
    /// user wants. Ligating just the two chosen pieces requires choosing them,
    /// and the choosing is the missing operation.
    ///
    /// The construct is checked BY SEQUENCE against one built by hand from the
    /// two halves, not by length and not by fragment count. A wrong arrangement
    /// — insert flipped, vector flipped, the two overhangs swapped — has exactly
    /// the same length as the right one, so length is the measurement that
    /// cannot fail.
    #[test]
    fn an_insert_cut_from_one_plasmid_goes_into_a_vector_cut_from_another() {
        // EcoRI is G^AATTC and BamHI is G^GATCC, so each fragment below carries
        // one AATT overhang and one GATC overhang and can only anneal the one
        // way round. That is what makes a directional subcloning directional.
        let vector = Dseq::new(
            "GAATTCTTTTTTTTTTTTTTTTTTTTGGATCCAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            true,
        );
        let donor = Dseq::new(
            "GAATTCCCCCCCCCCCCCCCCCCGGATCCGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
            true,
        );
        let cutters = [enz("EcoRI"), enz("BamHI")];
        let vfrags = crate::digest(&vector, cutters);
        let dfrags = crate::digest(&donor, cutters);
        assert_eq!(
            (vfrags.len(), dfrags.len()),
            (2, 2),
            "the premise: two cuts in each circle give two pieces each"
        );

        let out =
            subclone(&[vfrags.clone(), dfrags.clone()], &Options::default()).expect("a subcloning");
        // Every fragment here has one AATT end and one GATC end, so all four
        // pairings close. That is the honest answer and the panel's job is to
        // show which is which — not this function's to guess.
        assert_eq!(out.len(), 4, "one construct per pairing");

        // THE ONE THE USER WANTED, identified the way a person would: the
        // vector's backbone plus the donor's insert.
        let backbone = vfrags
            .iter()
            .max_by_key(|f| f.len())
            .expect("a longest vector fragment");
        let insert = dfrags
            .iter()
            .min_by_key(|f| f.len())
            .expect("a shortest donor fragment");
        let want = looped(
            &join(backbone, insert, &Options::default()).expect("the ends seal"),
            &Options::default(),
        )
        .expect("and the circle closes");
        let want_id = crate::assembly::Product {
            seq: want.clone(),
            order: vec![],
            junctions: vec![],
        }
        .identity();
        let got = out
            .iter()
            .find(|c| c.product.identity() == want_id)
            .expect("the vector-plus-insert construct is not among the answers");
        assert!(got.product.seq.circular);
        // MINUS EIGHT, and the eight is the point rather than a fudge. A sticky
        // fragment's `len()` counts its overhang bases, and each of the two
        // junctions here is one four-base overhang carried on one strand of each
        // piece — present in both counts before the join and once after. A
        // construct that came out at `backbone + insert` would have written each
        // overhang twice, which is eight duplicated bases inside a plasmid.
        assert_eq!(
            got.product.seq.len(),
            backbone.len() + insert.len() - 8,
            "the construct is not the two chosen pieces annealed at their overhangs"
        );
        // And it says WHICH pieces, which is the whole point of `routes`: a
        // construct nobody can trace back to a band on a gel cannot be built.
        let route = &got.routes[0];
        assert_eq!(route.len(), 2, "one fragment chosen from each digest");
        assert_eq!(
            vfrags[route[0]].len(),
            backbone.len(),
            "the route names the wrong vector fragment"
        );
        assert_eq!(
            dfrags[route[1]].len(),
            insert.len(),
            "the route names the wrong donor fragment"
        );
        // `order` indexes POOLS, not fragments, and a reader who assumes
        // otherwise gets a plausible wrong answer rather than an error.
        let mut pools: Vec<usize> = got.product.order.iter().map(|(i, _)| *i).collect();
        pools.sort_unstable();
        assert_eq!(pools, vec![0, 1], "each pool contributed exactly once");
    }

    /// A vector and an insert cut with enzymes that leave incompatible ends
    /// give nothing, and that is a result rather than an error.
    ///
    /// The other half of the test above, and it is not decoration: a `subclone`
    /// that returned every pairing regardless of the ends would pass the whole
    /// of the previous test — four constructs, one of them the wanted one — and
    /// be wrong about the only thing a ligation depends on.
    #[test]
    fn ends_that_cannot_be_sealed_produce_no_construct_rather_than_an_error() {
        // PstI leaves a 3' TGCA overhang; EcoRI leaves 5' AATT. Neither anneals
        // to the other, and neither anneals to itself here because each circle
        // is cut once and its two ends are the pair that just came apart.
        let vector = Dseq::new("GAATTCTTTTTTTTTTTTTTTTTTTTTTTTAAAACCCCGGGG", true);
        let donor = Dseq::new("CTGCAGCCCCCCCCCCCCCCCCCCCCCCCCAAAATTTTGGGG", true);
        let vfrags = crate::digest(&vector, [enz("EcoRI")]);
        let dfrags = crate::digest(&donor, [enz("PstI")]);
        assert_eq!((vfrags.len(), dfrags.len()), (1, 1), "the premise");
        let out = subclone(&[vfrags, dfrags], &Options::default()).expect("not an error");
        assert!(
            out.is_empty(),
            "an AATT overhang was sealed to a TGCA one, which no ligase does"
        );
    }

    /// The refusals, each named, because every one of them is a different thing
    /// for the user to do about it.
    #[test]
    fn a_subcloning_that_cannot_be_attempted_says_which_molecule_is_at_fault() {
        let opts = Options::default();
        let frag = try_cut(
            &Dseq::new("AAAAGGATCCTTTTGCGCGCATATATCCCGGG", true),
            enz("BamHI"),
        )
        .expect("cuts");
        assert_eq!(subclone(&[], &opts), Err(LigateError::NotEnoughFragments));
        // An empty digest must NOT come back as "these do not fit together":
        // that sentence sends the user to change enzymes on the molecule they
        // are looking at, when the fault is in the one they are not.
        assert_eq!(
            subclone(&[frag.clone(), Vec::new()], &opts),
            Err(LigateError::EmptyPool { pool: 1 })
        );
        // A circle has no ends. Reported per pool, so the message can name the
        // molecule that was never cut.
        assert_eq!(
            subclone(
                &[frag.clone(), vec![Dseq::new("ACGTACGTACGT", true)]],
                &opts
            ),
            Err(LigateError::CircularPoolInput { pool: 1, index: 0 })
        );
        // And the second wall, for the second dimension. `max_fragments` bounds
        // the search inside one combination and says nothing about how many
        // combinations there are.
        let wide = vec![frag[0].clone(); 100];
        assert_eq!(
            subclone(&[wide.clone(), wide], &opts),
            Err(LigateError::TooManyCombinations {
                given: 10_000,
                max: opts.max_combinations
            })
        );
    }

    /// docs/PLAN.md §6's stated validation criterion, which until now nothing
    /// in this crate could execute: cut a plasmid and put it back together.
    #[test]
    fn a_complete_digest_religates_to_the_molecule_it_came_from() {
        // A circle with exactly one BamHI site.
        let seq = "AAAAGGATCCTTTTGCGCGCATATATCCCGGGAAAATTTTCCCC";
        let plasmid = Dseq::new(seq, true);
        let frags = try_cut(&plasmid, enz("BamHI")).expect("one site");
        assert_eq!(frags.len(), 1, "one cut in a circle gives one linear piece");

        let products = ligate(&frags, &Options::default()).expect("a ligation");
        assert_eq!(products.len(), 1, "one fragment, one way to close it");
        let back = &products[0].seq;
        assert!(back.circular);
        assert_eq!(
            back.len(),
            plasmid.len(),
            "religation changed the length: {} -> {}",
            plasmid.len(),
            back.len()
        );
        // Identity is rotation- and strand-invariant, which is the only fair
        // comparison: the religated circle need not start where the original did.
        let a = crate::assembly::Product {
            seq: plasmid.clone(),
            order: vec![],
            junctions: vec![],
        };
        assert_eq!(
            products[0].identity(),
            a.identity(),
            "not the same molecule"
        );
    }

    /// The reason this module exists: a vector cut with one enzyme takes an
    /// insert cut with a different one, because the ENDS match.
    #[test]
    fn a_bamhi_vector_accepts_a_bglii_insert() {
        let vector = Dseq::new("GGATCCAAAAAAAAAAAAAAAAAAAACCCCCCCCCC", true);
        let insert = Dseq::new("AGATCTGGGGGGGGGGGGGGGGAGATCT", true);
        let v = try_cut(&vector, enz("BamHI")).expect("cuts");
        let i = try_cut(&insert, enz("BglII")).expect("cuts");
        assert_eq!(v.len(), 1);
        // Two BglII sites in a circle give two fragments; take the one that is
        // not the tiny spacer.
        let insert_frag = i
            .iter()
            .max_by_key(|f| f.len())
            .expect("a largest fragment")
            .clone();

        let products = ligate(&[v[0].clone(), insert_frag], &Options::default()).expect("ligation");
        assert!(
            !products.is_empty(),
            "a BamHI end and a BglII end leave the same GATC overhang and must join"
        );
        for p in &products {
            assert!(p.seq.circular);
            assert_eq!(p.order.len(), 2, "both fragments must be used");
        }
    }

    /// Ends that cannot anneal do not produce a molecule, however much a user
    /// would like them to.
    #[test]
    fn incompatible_ends_make_nothing() {
        // EcoRI leaves AATT, BamHI leaves GATC.
        let a = Dseq::new("GAATTCAAAAAAAAAAAAAAAAAAAA", true);
        let b = Dseq::new("GGATCCTTTTTTTTTTTTTTTTTTTT", true);
        let fa = try_cut(&a, enz("EcoRI")).expect("cuts");
        let fb = try_cut(&b, enz("BamHI")).expect("cuts");
        let products = ligate(&[fa[0].clone(), fb[0].clone()], &Options::default()).expect("ran");
        assert!(
            products.is_empty(),
            "AATT and GATC ends must not be joined: {} product(s)",
            products.len()
        );
    }

    /// Blunt ends are compatible with everything, so they are opt-in.
    #[test]
    fn blunt_joins_are_off_until_asked_for() {
        let m = Dseq::new("GATATCAAAAAAAAAAAAAAAAAAAAGATATC", true);
        let frags = try_cut(&m, enz("EcoRV")).expect("blunt cutter, two sites");
        assert!(frags.iter().all(|f| f.left_end() == End::Blunt));

        let off = ligate(&frags, &Options::default()).expect("ran");
        assert!(off.is_empty(), "blunt ligation must not happen by default");

        let on = ligate(
            &frags,
            &Options {
                blunt: true,
                ..Default::default()
            },
        )
        .expect("ran");
        assert!(!on.is_empty(), "with blunt on, the pieces must re-close");
    }

    /// Flipping is an involution and preserves the molecule.
    #[test]
    fn flipping_twice_is_the_identity_and_moves_the_end_to_the_other_side() {
        let m = Dseq::new("GGATCCAAAAAAAAAAAAAAAAAAAAGGATCC", true);
        for f in try_cut(&m, enz("BamHI")).expect("cuts") {
            assert_eq!(flip(&flip(&f)), f, "flip is not an involution");
            // A 5' overhang on the left becomes one on the right.
            let g = flip(&f);
            assert_eq!(f.left_end(), g.right_end(), "the ends did not swap sides");
            assert_eq!(f.right_end(), g.left_end());
            assert_eq!(f.len(), g.len(), "flipping changed the length");
        }
        // Blunt flips to blunt with the offset unchanged at zero.
        let b = Dseq::new("ACGTACGT", false);
        assert_eq!(flip(&b).ovhg, 0);
    }

    #[test]
    fn a_circle_has_no_ends_and_says_so() {
        let c = Dseq::new("ACGTACGTACGT", true);
        assert_eq!(
            ligate(&[c], &Options::default()),
            Err(LigateError::CircularInput { index: 0 })
        );
        assert!(matches!(
            ligate(&[], &Options::default()),
            Err(LigateError::NotEnoughFragments)
        ));
    }
}
