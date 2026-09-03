//! The loaded document, and the work that must not happen on the UI thread.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

use pl_core::oplog::{OpError, OpKind, OpLog};
use pl_core::{Molecule, Topology};
use pl_enzymes::methylation::SiteEffect;
use pl_enzymes::Digest;
use pl_features::annotate::Annotation;
use pl_fileio::{snapgene, Format};

/// Everything one worker produces about a molecule.
///
/// The verdicts live here rather than being derived at paint time. The Enzymes
/// tab draws one row per cutting enzyme and each row shows a methylation
/// verdict; computing that verdict needs the enzyme's [`pl_enzymes::CutSite`]s,
/// and asking `cut_sites` for them in the row builder put a full-molecule scan
/// back on the UI thread — 58 of them per frame, measured at 1.58 s per frame
/// on the 4.6 Mb NC_000913.3, which is the whole of the work this worker exists
/// to take away. The worker already had those sites in hand and threw them
/// away.
pub struct Digested {
    pub results: Vec<Digest>,
    /// Parallel to `results`: what methylation does to that enzyme's sites, or
    /// `None` if it does not cut or nothing methylates any of them.
    verdicts: Vec<Option<Methylated>>,
    /// Parallel to `results`: every match, with the cut it produced.
    ///
    /// Kept for the same reason the verdicts are. The sequence view draws the
    /// recognition site as a bracket and the cut as a chevron, and they are
    /// different objects: EcoRI is `G^AATTC`, so `CutSite::position` is
    /// `site_start + 1`. `Digest` carries only the cut positions, and
    /// `pl-enzymes`' own docstring says `site_start` is *not* recoverable from
    /// one — the two strands map a match to a cut through different offsets, so
    /// `position - fst5` is the wrong answer for half the hits. The worker
    /// already had these in hand and threw them away.
    sites: Vec<Vec<pl_enzymes::CutSite>>,
    /// Parallel to `sites`, entry for entry: what methylation does to THAT
    /// site. See [`Methylated`] for why one verdict per enzyme was not enough.
    site_effects: Vec<Vec<Option<SiteEffect>>>,
}

/// What methylation does to one enzyme's sites on one molecule.
///
/// # PER SITE, and then summarised — never one site standing in for the enzyme
///
/// `methylation.rs` states the rule as *for each candidate site*, and it has to
/// be per site: whether Dam blocks a ClaI site depends on the base beside it
/// (`ATCGAT` preceded by `G`, or followed by `C`, makes an overlapping `GATC` —
/// about 44% of random sites), so two sites of one enzyme on one plasmid
/// routinely disagree.
///
/// This used to be the verdict at `sites.first()`, and `cut_sites` returns
/// forward matches in ascending coordinate, so *first* meant *lowest*. On a
/// circle the origin is an arbitrary cut: `Edit → Set origin here`, or opening
/// the same plasmid from a differently linearised file, permutes which site is
/// lowest and so flipped every answer the app gave — the strikethrough on the
/// Enzymes row, the "Dam blocked" chip, whether the gel would seed that lane,
/// and the caveat printed under the lane. Same molecule, two contradictory
/// bench predictions, decided by where the file happened to be cut. The mirror
/// case was as bad: an enzyme with four sites of which only the lowest was
/// blocked was struck through as if it did not cut at all.
///
/// `bins/pl/src/main.rs`' `pl design` tail path already did this per site, with
/// `let live = cuts - dead`; this is the same arithmetic for the GUI's four
/// surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Methylated {
    /// The most severe thing methylation does to any one of these sites.
    ///
    /// Reduced with a TOTAL ORDER over (effect, methylase) rather than by
    /// taking the first — see [`Digested`]'s history. A rule that depended on
    /// which site the scan met first would have reintroduced exactly the
    /// origin-dependence this type exists to remove.
    pub worst: SiteEffect,
    /// Sites this enzyme reads on the molecule. Never zero: nothing builds a
    /// `Methylated` for an enzyme with no affected site.
    pub total: usize,
    /// Of those, how many will not cut at all in this preparation.
    pub blocked: usize,
    /// Of those, how many methylation touches in any way — blocked, impaired,
    /// or sources-disagree.
    pub affected: usize,
}

impl Methylated {
    /// Does methylation stop this enzyme cutting the molecule at all?
    ///
    /// The question the strikethrough and the gel's seeding rule are actually
    /// asking. An enzyme with one blocked site out of four still cuts, still
    /// gives a real digest, and must not be drawn as dead.
    pub fn all_blocked(&self) -> bool {
        self.blocked == self.total
    }

    /// Sites that still cut in this preparation.
    pub fn live(&self) -> usize {
        self.total.saturating_sub(self.blocked)
    }

    /// The Enzymes-tab chip: the methylase, what it does, and TO HOW MANY OF
    /// HOW MANY. The count is the half that was missing — "Dam blocked" beside
    /// an enzyme with four sites, one of them blocked, is a true phrase and a
    /// false statement.
    pub fn chip(&self) -> String {
        let m = self.worst.methylase.name();
        let e = self.worst.effect.as_str();
        let n = if self.worst.effect == pl_enzymes::methylation::Effect::Blocked {
            self.blocked
        } else {
            self.affected
        };
        if self.total == 1 {
            format!("{m} {e}")
        } else if n == self.total {
            format!("{m} {e} · all {} sites", self.total)
        } else {
            format!("{m} {e} · {n} of {} sites", self.total)
        }
    }

    /// `1 of its 3 sites`, or `its only site`. One place, so the gel's four
    /// sentences cannot count differently from each other.
    pub fn of_sites(&self, n: usize) -> String {
        if self.total == 1 {
            "its only site".into()
        } else if n == self.total {
            format!("all {} of its sites", self.total)
        } else {
            format!("{n} of its {} sites", self.total)
        }
    }
}

/// A TOTAL order on site effects, worst last.
///
/// Effect first, exactly as `methylation::site_effect` ranks the rules at one
/// site; the methylase only breaks ties, and only so that an enzyme whose sites
/// are blocked by two different methylases names the same one however the
/// molecule is rotated.
fn severity(e: SiteEffect) -> (u8, u8) {
    use pl_enzymes::methylation::{Effect, Methylase};
    (
        match e.effect {
            Effect::Blocked => 2,
            Effect::Impaired => 1,
            Effect::Unknown => 0,
        },
        match e.methylase {
            Methylase::Dam => 2,
            Methylase::Dcm => 1,
            Methylase::Cpg => 0,
        },
    )
}

/// What methylation does to each of `sites`, and the summary of that.
///
/// ONE implementation, called by the digest worker and by anything that checks
/// it, so that two surfaces cannot count the same molecule differently.
///
/// `sites` rather than the enzyme's own scan, because the caller has already
/// paid for the scan: `cut_sites` walks the whole molecule and is what this
/// worker exists to keep off the UI thread.
pub(crate) fn methylation_at(
    e: &pl_enzymes::Enzyme,
    seq: &[u8],
    topology: Topology,
    meth: &pl_core::Methylation,
    sites: &[pl_enzymes::CutSite],
) -> (Vec<Option<SiteEffect>>, Option<Methylated>) {
    let effects: Vec<Option<SiteEffect>> = sites
        .iter()
        .map(|s| {
            // `site_start`, not a cut position: `pl-enzymes` documents that the
            // two strands map a match to a cut through different offsets, so
            // `position - fst5` is the wrong answer for half the hits, and they
            // disagree wherever a site wraps the origin.
            pl_enzymes::methylation::site_effect(
                e,
                seq,
                (s.site_start - 1) as usize,
                topology,
                meth,
            )
        })
        .collect();
    let summary = summarise(&effects);
    (effects, summary)
}

/// Reduce one enzyme's per-site effects to what the surfaces need.
///
/// `None` when nothing is affected, which is what every consumer treats as
/// "methylation has nothing to say about this enzyme here".
fn summarise(effects: &[Option<SiteEffect>]) -> Option<Methylated> {
    let worst = effects
        .iter()
        .flatten()
        .copied()
        .max_by_key(|e| severity(*e))?;
    Some(Methylated {
        worst,
        total: effects.len(),
        blocked: effects
            .iter()
            .flatten()
            .filter(|e| e.effect == pl_enzymes::methylation::Effect::Blocked)
            .count(),
        affected: effects.iter().flatten().count(),
    })
}

/// Digestion is O(sequence x enzymes). Measured at 1,712 ms for all 58 enzymes
/// on the 4.6 Mb NC_000913.3 in the benchmark corpus — this docstring said
/// "about 600 ms" until somebody timed it — so it runs on a worker and the UI
/// says so meanwhile.
pub enum DigestState {
    Running {
        rx: Receiver<Digested>,
        /// Set when a later edit supersedes this scan.
        ///
        /// Dropping the `Receiver` alone does not stop the worker: its `send`
        /// fails, but only *after* it has finished the whole scan. Measured on
        /// a 4.6 Mb genome, one full 58-enzyme digest is 1,712 ms of CPU — the
        /// docstring's "about 600 ms" is off by 2.9x — and simulated typing
        /// spawned 30 workers with 16 live at once, 29 of whose results were
        /// superseded before anyone could see them, still draining 2.2 s after
        /// the last keystroke. Coalescing cuts the spawn rate about a
        /// hundredfold; this is what stops the ones that do get superseded from
        /// burning a core to produce an answer nobody will read.
        cancel: Arc<AtomicBool>,
    },
    Done(Digested),
    /// No bases to digest, with the reason.
    Unavailable(String),
}

impl DigestState {
    pub fn results(&self) -> &[Digest] {
        match self {
            DigestState::Done(v) => &v.results,
            _ => &[],
        }
    }
    /// The methylation verdict for `results()[i]`, over ALL of its sites.
    ///
    /// A field read, deliberately. See [`Digested`] for what it cost when this
    /// was a scan.
    pub fn verdict(&self, i: usize) -> Option<Methylated> {
        match self {
            DigestState::Done(v) => v.verdicts.get(i).copied().flatten(),
            _ => None,
        }
    }

    /// The verdict at ONE cut of `results()[i]`, for a surface that is naming
    /// that coordinate.
    ///
    /// The map's tooltip prints a site's own position and then a methylation
    /// tag beside it; with an enzyme-wide verdict there, hovering an unblocked
    /// site read `ClaI  2,000 / ATCGAT · Dam blocked` because a *different*
    /// site was the blocked one. A false statement bound to a named coordinate
    /// is worse than a missing one, so this asks about that cut.
    ///
    /// Two sites at different starts can nick the same bond on a circle, which
    /// is why `Digest::positions` is deduplicated; when they do, the worse of
    /// the two verdicts is the honest answer for the bond.
    pub fn site_verdict(&self, i: usize, position: u64) -> Option<SiteEffect> {
        let DigestState::Done(v) = self else {
            return None;
        };
        let sites = v.sites.get(i)?;
        let effects = v.site_effects.get(i)?;
        sites
            .iter()
            .zip(effects)
            .filter(|(s, _)| s.position == position)
            .filter_map(|(_, e)| *e)
            .max_by_key(|e| severity(*e))
    }
    /// Every match for `results()[i]`, cut and site both. See [`Digested`].
    pub fn sites(&self, i: usize) -> &[pl_enzymes::CutSite] {
        match self {
            DigestState::Done(v) => v.sites.get(i).map(|v| v.as_slice()).unwrap_or(&[]),
            _ => &[],
        }
    }
    pub fn is_running(&self) -> bool {
        matches!(self, DigestState::Running { .. })
    }
    /// Tell whatever is still scanning that its answer is no longer wanted.
    pub fn cancel(&self) {
        if let DigestState::Running { cancel, .. } = self {
            cancel.store(true, Ordering::Relaxed);
        }
    }
    /// Collect the worker's result if it has finished. Returns true if the
    /// state changed, so the caller knows to repaint.
    pub fn poll(&mut self) -> bool {
        let done = match self {
            DigestState::Running { rx, .. } => rx.try_recv().ok(),
            _ => None,
        };
        if let Some(v) = done {
            *self = DigestState::Done(v);
            return true;
        }
        false
    }
}

/// Everything one ORF worker produces about a molecule.
pub struct Orfs {
    pub orfs: Vec<pl_core::orf::Orf>,
    /// The same interval structure the feature ribbons use, over the ORF spans,
    /// with the lane PINNED to the frame.
    ///
    /// Built on the worker: 19,552 spans is about 1.1 ms of tree building, and
    /// that is not work a frame should discover it has to do. `Iv::feat` here
    /// indexes `orfs`, not `Molecule::features`.
    pub index: crate::annot::AnnotIndex,
    /// ORFs that lap the molecule and so cannot be drawn as an interval.
    ///
    /// On a circle whose length is not a multiple of three the three frames are
    /// one cycle of `n` codons, so one turn walks `3n` bases and an ORF can be
    /// longer than the molecule it sits on. A `[lo, hi)` interval cannot say
    /// that: it would hold one lap, draw one lap, and look entirely normal.
    /// Counted and named instead. Every molecule in this project's vocabulary
    /// is in that case — pKoV 8,117 (n%3=2), pUC19 2,686 (1), pBR322 4,361 (2),
    /// NC_000913.3 4,641,652 (1) — and it is reachable on a small GC-rich
    /// circle, which is exactly a codon-optimised gene block in pUC19.
    pub lapping: usize,
    /// Frames with no stop codon anywhere on a circle. `pl_core::orf` reports
    /// them separately because such a frame translates for ever and so has no
    /// ORF at all — and without saying so, an empty strip and a stop-free frame
    /// look identical.
    pub stopless: Vec<(pl_core::Strand, u8)>,
    pub code: u8,
    pub min_aa: usize,
}

/// The ORF scan, on the same shape as [`DigestState`] and deliberately not on a
/// second one.
///
/// Measured on the 4,641,652 bp `NC_000913.3`: a six-frame scan at
/// `min_aa = 30` is 300-420 ms against 1,322-1,712 ms for the 58-enzyme digest
/// on the same machine. ORFs are about 30% of a scan this application already
/// runs on a worker, on the same trigger; there is no argument for a different
/// mechanism.
///
/// `Off` is the one state the digest does not have, and it is the important
/// one: the ORF strip is off by default, so a user who never turns it on never
/// spawns a thread and never pays a millisecond.
pub enum OrfState {
    /// Never asked for. Costs nothing.
    Off,
    Running {
        rx: Receiver<Orfs>,
        /// Set when a later edit supersedes this scan. See
        /// [`pl_core::orf::find_orfs_until`] for why dropping the receiver is
        /// not enough.
        cancel: Arc<AtomicBool>,
    },
    Done(Orfs),
    /// No bases to scan, with the reason — the digest's own three refusals.
    Unavailable(String),
}

impl OrfState {
    pub fn done(&self) -> Option<&Orfs> {
        match self {
            OrfState::Done(v) => Some(v),
            _ => None,
        }
    }
    pub fn is_running(&self) -> bool {
        matches!(self, OrfState::Running { .. })
    }
    pub fn is_off(&self) -> bool {
        matches!(self, OrfState::Off)
    }
    pub fn cancel(&self) {
        if let OrfState::Running { cancel, .. } = self {
            cancel.store(true, Ordering::Relaxed);
        }
    }
    /// Collect the worker's result if it has finished. Returns true if the
    /// state changed, so the caller knows to repaint.
    pub fn poll(&mut self) -> bool {
        let done = match self {
            OrfState::Running { rx, .. } => rx.try_recv().ok(),
            _ => None,
        };
        if let Some(v) = done {
            *self = OrfState::Done(v);
            return true;
        }
        false
    }
}

/// Everything one annotation worker produces about a molecule.
///
/// **These are PROPOSALS and the document does not contain them.** Nothing here
/// is in `OpLog`, nothing here is saved, and nothing here is drawn as a feature
/// of the molecule. `features/SIGNOFF.tsv` gates which records a curator has
/// approved, an approval lapses the moment the row changes, and `pl annotate`
/// reports rather than writes; the desktop equivalent of all three is that the
/// user accepts, one at a time or all at once, and only then does an `OpKind`
/// exist. An implementation that wrote these into the molecule on open would
/// demo better and would be asserting on the user's behalf.
pub struct Proposals {
    /// The database `hits[i].record` indexes into.
    ///
    /// `'static` because it is loaded once for the process — see
    /// [`crate::featuredb`] — and carried HERE rather than looked up again at
    /// paint time, because "which table" is half of what a record index means.
    /// A hit found against the reviewed subset and resolved against the full
    /// table names a different feature, plausibly, with nothing wrong-looking
    /// on screen.
    pub db: &'static pl_features::Db,
    pub hits: Vec<Annotation>,
    /// Records neither index can reach, by name.
    ///
    /// `pl annotate` writes these to stderr. A window has no stderr anybody
    /// reads, and `Annotator::unseedable`'s own doc says why they must not be
    /// swallowed: "a caller that believes it searched the whole database when
    /// it did not will report a confident empty result."
    pub unseedable: Vec<String>,
    /// Whether the unreviewed rows were searched too. See
    /// [`Document::start_proposals`].
    pub unreviewed: bool,
}

impl Proposals {
    /// The database row a hit came from.
    pub fn record(&self, a: &Annotation) -> &'static pl_features::Record {
        &self.db.records[a.record]
    }

    /// The hits worth showing, given whether partial matches are wanted.
    ///
    /// `pl annotate` hides `is_fragment` hits unless `--fragments` is passed,
    /// and this is the same rule with the same default. A fragment is a hit
    /// whose coverage fell below `Config::fragment_coverage` — a piece of a
    /// feature, not the feature — and the commonest one is a promoter or an
    /// origin clipped by whatever the cloning did. Worth being able to see;
    /// wrong to offer first, because the name is the whole feature's name.
    pub fn shown(&self, fragments: bool) -> impl Iterator<Item = &Annotation> {
        self.hits
            .iter()
            .filter(move |a| fragments || !a.is_fragment)
    }

    /// How many hits the fragment rule is holding back.
    pub fn fragments(&self) -> usize {
        self.hits.iter().filter(|a| a.is_fragment).count()
    }
}

/// The annotation scan, on [`OrfState`]'s shape and deliberately not on a third
/// one.
///
/// Measured on this machine, release build, as whole-process `pl annotate`
/// times less a `pl --version` floor of about 30 ms: 25 ms for a 10 kb plasmid
/// (of which ~4 ms parses the tables and ~13 ms builds the indexes, both of
/// which this application pays once for the process rather than once per scan),
/// 115 ms at 400 kb, 315 ms at 1.2 Mb, and 4.2 s at 4.64 Mb.
///
/// Against the 58-enzyme digest this document already starts unconditionally
/// on every open, timed the same way through `pl digest`: about 10 ms at 10 kb
/// and 110 ms at 400 kb, so at plasmid scale the two are the same order of
/// work. It is at genome scale that they part company, and there annotation is
/// the more expensive: 4.2 s against the 1,712 ms this file records above for
/// the digest at 4.6 Mb. Either way it is not work a frame may do.
///
/// # The one thing this could not do that the ORF scan can — until 2026-09-03
///
/// **STOP.** [`pl_core::orf::find_orfs_until`] takes a predicate and checks it
/// as it goes, so a superseded ORF scan abandons the genome within a few
/// codons. From 2026-08-06 to 2026-09-03
/// [`pl_features::annotate::Annotator::annotate`] took no such hook, so
/// `cancel` here was checked exactly twice — before the scan started and
/// before the answer was sent — and a worker superseded in between ran to
/// completion. At 4.64 Mb that was 4.2 s of a core produced for nobody. What
/// bounded it was the same thing that bounds the digest's spawn rate: a run of
/// typing is ONE operation, because `App::settle` coalesces it, so the trigger
/// is committed edits and not keystrokes. The note here said the hook belonged
/// in `pl-features` beside `find_orfs_until`'s, not here, and that is where it
/// went: [`pl_features::annotate::Annotator::annotate_until`] polls the same
/// kind of predicate at every loop boundary the scan has, and `spawn_proposals`
/// hands it `cancel`. What it still cannot interrupt is one pass between two of
/// those boundaries — a seed or chain pass over one strand of the doubled text,
/// a six-frame translation, one chain's verification — so "within a few
/// codons" is the ORF scan's promise and not this one's. How long such a pass
/// takes at 4.64 Mb was not measured for this note.
///
/// `Off` is what a document starts in and what it costs: a user who switches
/// automatic annotation off never spawns this thread and never pays a
/// millisecond.
pub enum ProposalState {
    /// Nobody has asked. Costs nothing.
    Off,
    Running {
        rx: Receiver<Proposals>,
        /// Set when a later edit supersedes this scan. See the note above for
        /// exactly how much good it does, and since when.
        cancel: Arc<AtomicBool>,
    },
    Done(Proposals),
    /// No bases to search, with the reason — the digest's own three refusals.
    Unavailable(String),
    /// The worker went away without answering.
    ///
    /// A state the digest and the ORF scan do not have, and it is not
    /// defensive furniture: [`pl_features::annotate::Annotator::new`] PANICS,
    /// by documented design, on a database carrying a protein reference shorter
    /// than its floor. A panicking worker drops its `Sender`, and
    /// `try_recv().ok()` — which is how the other two poll — cannot tell that
    /// from "not finished yet", so the panel would show a spinner for ever
    /// while the reason sat in a stderr no GUI user has. `Err(Disconnected)` is
    /// the only observable that distinguishes them.
    Failed(String),
}

impl ProposalState {
    pub fn done(&self) -> Option<&Proposals> {
        match self {
            ProposalState::Done(v) => Some(v),
            _ => None,
        }
    }
    pub fn done_mut(&mut self) -> Option<&mut Proposals> {
        match self {
            ProposalState::Done(v) => Some(v),
            _ => None,
        }
    }
    pub fn is_running(&self) -> bool {
        matches!(self, ProposalState::Running { .. })
    }
    pub fn is_off(&self) -> bool {
        matches!(self, ProposalState::Off)
    }
    pub fn cancel(&self) {
        if let ProposalState::Running { cancel, .. } = self {
            cancel.store(true, Ordering::Relaxed);
        }
    }
    /// Collect the worker's result if it has finished. Returns true if the
    /// state changed, so the caller knows to repaint.
    pub fn poll(&mut self) -> bool {
        let next = match self {
            ProposalState::Running { rx, .. } => match rx.try_recv() {
                Ok(v) => Some(ProposalState::Done(v)),
                Err(TryRecvError::Empty) => None,
                // See `Failed`. Reachable only through a worker that died,
                // because a cancelled one is always dropped along with the
                // receiver that would observe it.
                Err(TryRecvError::Disconnected) => Some(ProposalState::Failed(
                    "the annotation worker stopped without answering; the feature database \
                     may be unreadable"
                        .into(),
                )),
            },
            _ => None,
        };
        if let Some(s) = next {
            *self = s;
            return true;
        }
        false
    }
}

pub struct Document {
    pub path: Option<PathBuf>,
    pub title: String,
    /// The document *is* its history.
    ///
    /// `docs/PLAN.md` ADR-2: undo and history are the same mechanism, and it
    /// "cannot be retrofitted". The log was built, tested and wired to nothing
    /// for a week; this is the wiring. Nothing here mutates a molecule
    /// directly — every change goes through [`Document::apply`], so every
    /// change is undoable and appears in the history panel.
    pub log: OpLog,
    pub format: Format,
    /// Present only for `.dna`, so the container can be described.
    pub container: Option<snapgene::Document>,
    pub digest: DigestState,
    /// The open-reading-frame scan, if anyone has asked for one.
    pub orfs: OrfState,
    /// What the feature database thinks is in here — as PROPOSALS, never as
    /// features. See [`Proposals`].
    pub proposals: ProposalState,
    /// Bumped whenever the BASES or the topology may have moved, and left alone
    /// when only the annotations did.
    ///
    /// The ORF cache is keyed on this and not on the log cursor. `apply`,
    /// `undo`, `redo` and `seek` all restart the digest unconditionally, so
    /// adding a feature to the 4.6 Mb genome already throws away a 1,322 ms
    /// enzyme scan for a change that cannot move a cut site; hanging a 400 ms
    /// ORF scan off the same trigger would make that 1.7 s of waste per feature
    /// edit. Three of the nine `OpKind`s cannot touch a base or the topology
    /// and the match below is exhaustive, so a tenth is a compile error rather
    /// than a silently stale strip.
    pub seq_version: u64,
    /// The `(table, min_aa)` the current or last ORF scan was asked for.
    ///
    /// Kept here because `OrfState::Running` does not carry them and an edit
    /// mid-scan has to restart the same question, not the default one.
    orf_params: (u8, usize),
    /// Whether the current or last annotation scan searched the unreviewed rows.
    ///
    /// `orf_params`' twin, for the same reason: `ProposalState::Running` does
    /// not carry it, and `Running` is exactly the state in which the user can
    /// tick the box.
    proposal_scope: bool,
    /// How many ORF workers this document has started.
    ///
    /// Test-only, and it exists because the defect it pins is invisible to every
    /// other observable: a scan cancelled and respawned on every frame looks
    /// exactly like a scan that is merely slow, right up until it never
    /// finishes. Only a spawn count can say "one question, asked once" rather
    /// than "an answer eventually arrived". Per document and not a process-wide
    /// static, because the suite runs its tests in parallel in one process.
    #[cfg(test)]
    pub orf_spawns: usize,
    /// How many times this document has ASKED for an annotation scan.
    ///
    /// Asked, not started: a molecule with no bases is answered by
    /// `spawn_proposals` without a thread, and that still counts, because the
    /// property being measured is "one question, asked once" and re-asking a
    /// refusal sixty times a second is the same defect with a cheaper body.
    /// `orf_spawns` counts the same way.
    ///
    /// Test-only, and it exists for `orf_spawns`' reason twice over: the defect
    /// it pins is invisible to every other observable, and until 2026-09-03
    /// THIS worker could not be stopped mid-scan (see [`ProposalState`]), so
    /// the same mistake here burned a whole core per frame on a genome rather
    /// than a few codons' worth. It can be stopped now, which makes the count
    /// no less necessary: a scan cancelled and respawned every frame still
    /// never finishes, it just does so more cheaply.
    #[cfg(test)]
    pub proposal_spawns: usize,
    /// Records the file held. Only the first is shown, and a viewer that does
    /// not say so is indistinguishable from a file with fewer records in it.
    pub records_in_file: usize,
    /// Location forms the reader could not represent, reported not dropped.
    pub unrepresentable_locations: Vec<String>,
    /// The log cursor at which this document was last written to a file, or
    /// `None` for a document that has never been written at all.
    ///
    /// A cursor, never an op count. [`pl_core::oplog::OpLog::path`] shrinks on
    /// undo and regrows when the next edit forks from the same parent, so
    /// "circularise, undo, reverse-complement" is back at length 1 holding a
    /// different molecule — the collision `Autosaved` in `main.rs` already
    /// documents having shipped once, in the recovery file. Content addressing
    /// is what makes the cursor an identity: two different edits from one
    /// parent cannot share it.
    ///
    /// The nesting is load-bearing and reads badly: outer `None` = never
    /// written; `Some(None)` = written, and what was written was the base
    /// state.
    pub saved: Option<Option<pl_core::oplog::OpId>>,
}

impl Document {
    pub fn from_bytes(data: &[u8], title: String, path: Option<PathBuf>) -> Result<Self, String> {
        let container = if pl_fileio::detect(data) == Some(Format::SnapGene) {
            snapgene::parse(data).ok()
        } else {
            None
        };
        let (molecule, format, report) =
            pl_fileio::load_with_report(data).map_err(|e| e.to_string())?;
        let digest = start_digest(&molecule);
        // Set here rather than at each call site so `adopt` cannot forget it —
        // which matters, because `adopt`'s own docstring exists because "two of
        // the four places that used to assign it forgot the second half".
        //
        // `path.is_some()` means the document was read from that file at the
        // base of an empty log, so the file holds it: opening a file and
        // closing it must never prompt. `path.is_none()` covers the three
        // callers that are genuinely unsaved — the recovery-banner restore
        // (which drops the path deliberately), a file dropped as bytes with no
        // path, and `of_molecule` in tests. A restored crash draft sitting at
        // cursor `None` with zero edits IS unsaved work, and a predicate that
        // only compared cursors would call it clean and let it be closed.
        let saved = path.is_some().then_some(None);
        Ok(Document {
            path,
            title,
            log: OpLog::new(molecule),
            format,
            container,
            digest,
            orfs: OrfState::Off,
            proposals: ProposalState::Off,
            seq_version: 0,
            orf_params: (11, 30),
            proposal_scope: false,
            #[cfg(test)]
            orf_spawns: 0,
            #[cfg(test)]
            proposal_spawns: 0,
            records_in_file: report.records,
            unrepresentable_locations: report.unrepresentable_locations,
            saved,
        })
    }

    /// A document around a molecule that came from no file.
    ///
    /// Test-only, and it exists because the editing model in `seqedit` needs
    /// fixtures with awkward topologies and origin-crossing features that no
    /// short file literal expresses cleanly. It goes through the same
    /// `OpLog::new` as everything else, so the log is not special-cased.
    #[cfg(test)]
    pub fn of_molecule(mol: Molecule) -> Self {
        Document {
            path: None,
            title: "test".into(),
            digest: start_digest(&mol),
            orfs: OrfState::Off,
            proposals: ProposalState::Off,
            seq_version: 0,
            orf_params: (11, 30),
            proposal_scope: false,
            orf_spawns: 0,
            proposal_spawns: 0,
            log: OpLog::new(mol),
            format: Format::GenBank,
            container: None,
            records_in_file: 1,
            unrepresentable_locations: Vec::new(),
            saved: None,
        }
    }

    /// The molecule as it stands. Always the log's current state.
    pub fn molecule(&self) -> &Molecule {
        self.log.current()
    }

    /// Apply an edit, recording it.
    ///
    /// Returns the error rather than swallowing it: the log refuses an
    /// operation that would leave the annotations describing something the
    /// sequence does not contain, and the user needs to be told which edit was
    /// refused and why. Re-digests, because cut sites are a function of the
    /// sequence and a stale enzyme list after an edit is a wrong answer
    /// presented as a current one.
    pub fn apply(&mut self, kind: OpKind) -> Result<(), OpError> {
        let moved = moves_bases(&kind);
        self.log.apply(kind, "you")?;
        self.restart_scans(moved);
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), OpError> {
        self.log.undo()?;
        // Conservatively: a step through the history can land anywhere, and an
        // ORF list that survived an undo of a deletion would name coordinates
        // that no longer exist.
        self.restart_scans(true);
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), OpError> {
        self.log.redo()?;
        self.restart_scans(true);
        Ok(())
    }

    /// Move to any point in the log, on any branch.
    ///
    /// The GUI needs this for the one gesture that is two operations: deleting
    /// across the origin is a rotate and then a range op, and stepping back over
    /// only the second leaves a plasmid that is whole, plausible, and renumbered
    /// — a state the user never asked for and never saw.
    pub fn seek(&mut self, to: Option<pl_core::oplog::OpId>) -> Result<(), OpError> {
        self.log.seek(to)?;
        self.restart_scans(true);
        Ok(())
    }

    /// Start a fresh digest, and tell the previous one to give up.
    ///
    /// The ORF scan restarts only when the bases or the topology may have
    /// moved, and only if anyone had asked for one — `OrfState::Off` stays off.
    /// The annotation scan follows exactly that rule, and its reason is
    /// stronger than a stale strip: a proposal is a NAME AT A COORDINATE that a
    /// user is about to click Accept on. Held across an insertion it would put
    /// `AmpR` on bases that are no longer AmpR, and the user's own file would
    /// then carry the wrong claim with a provenance note vouching for it. So
    /// the old list is thrown away rather than remapped — `remap_annotations`
    /// exists for things the document contains, and these are proposals about a
    /// molecule that has changed.
    fn restart_scans(&mut self, bases_moved: bool) {
        self.digest.cancel();
        self.digest = start_digest(self.log.current());
        if bases_moved {
            self.seq_version = self.seq_version.wrapping_add(1);
            if !self.orfs.is_off() {
                let (code, min_aa) = self.orf_params;
                self.orfs.cancel();
                self.orfs = spawn_orfs(self.log.current(), code, min_aa);
                #[cfg(test)]
                {
                    self.orf_spawns += 1;
                }
            }
            if !self.proposals.is_off() {
                self.proposals.cancel();
                self.proposals = spawn_proposals(self.log.current(), self.proposal_scope);
                #[cfg(test)]
                {
                    self.proposal_spawns += 1;
                }
            }
        }
    }

    /// Ask for an ORF scan of the current molecule, cancelling any running one.
    ///
    /// Idempotent against the question IN FLIGHT, not merely against a finished
    /// one, and the word *finished* was the whole defect. `main.rs`'s
    /// `refresh_orfs` calls this at the top of the Sequence tab on EVERY frame,
    /// and a running scan asks for a repaint every 80 ms; a version that only
    /// early-returned on `Done` therefore cancelled and respawned the worker on
    /// every frame, so any molecule whose scan outlives one repaint tick never
    /// finished at all. Measured on a 4,641,652 bp genome whose scan is 425 ms:
    /// the header read "ORFs: scanning…" at 15 s, 20 s and 60 s, one core stayed
    /// pinned at 110% whether or not the window had focus (22,031 ms of CPU over
    /// 20 s), the working set climbed 232 -> 470 MB, and the answer — the same
    /// 66,527 ORFs `pl orfs` prints — appeared the instant the user switched to
    /// another tab so that this function stopped being called. The tick beats
    /// the scan above about 0.87 Mb at rest and far below that while the pointer
    /// is moving, when the effective interval is the display frame.
    ///
    /// `orf_params` and not the finished scan's own fields, because `Running`
    /// does not carry them and `Running` is exactly the state that was missing.
    /// `Unavailable` is covered too: it is an answer, and respawning a worker to
    /// be told again that there are no bases is the same loop with a cheaper
    /// body.
    pub fn start_orfs(&mut self, code: u8, min_aa: usize) {
        if !self.orfs.is_off() && self.orf_params == (code, min_aa) {
            return;
        }
        self.orf_params = (code, min_aa);
        self.orfs.cancel();
        self.orfs = spawn_orfs(self.log.current(), code, min_aa);
        #[cfg(test)]
        {
            self.orf_spawns += 1;
        }
    }

    /// Ask the feature database what is in this molecule, cancelling any
    /// running scan asked at a different scope.
    ///
    /// `start_orfs`' contract, verbatim, and its defect is worth re-reading
    /// before touching this: idempotent against the question IN FLIGHT and not
    /// merely against a finished one. `main.rs`'s `refresh_proposals` calls this
    /// on EVERY frame, and a running scan asks for a repaint every 80 ms, so a
    /// version that only early-returned on `Done` would cancel and respawn on
    /// every frame — and until 2026-09-03, unlike the ORF worker, this one
    /// could not be stopped mid-scan, so the superseded copies would not exit
    /// either. At 4.64 Mb, where one scan is 4.2 s, that was a core per frame
    /// with no answer ever arriving; with `annotate_until` it is a scan
    /// abandoned per frame with no answer ever arriving, which is the same
    /// defect. `Unavailable` and `Failed` are covered by the same predicate:
    /// both are answers, and respawning a worker to be told again that there
    /// are no bases is the same loop with a cheaper body.
    ///
    /// `unreviewed` says whether the rows no curator has signed off were
    /// searched too. It is a scope and not a filter — the two annotators index
    /// different tables, so changing it has to re-run the scan, which is why it
    /// is here rather than at paint time like the fragment rule.
    pub fn start_proposals(&mut self, unreviewed: bool) {
        if !self.proposals.is_off() && self.proposal_scope == unreviewed {
            return;
        }
        self.proposal_scope = unreviewed;
        self.proposals.cancel();
        self.proposals = spawn_proposals(self.log.current(), unreviewed);
        #[cfg(test)]
        {
            self.proposal_spawns += 1;
        }
    }

    /// Collect a finished annotation scan. See [`ProposalState::poll`].
    pub fn poll_proposals(&mut self) -> bool {
        self.proposals.poll()
    }

    /// Stop scanning and forget the proposals. Nothing to hold.
    pub fn stop_proposals(&mut self) {
        self.proposals.cancel();
        self.proposals = ProposalState::Off;
    }

    /// Collect a finished ORF scan. See [`OrfState::poll`].
    pub fn poll_orfs(&mut self) -> bool {
        self.orfs.poll()
    }

    /// Stop scanning and forget the answer. The strip is off; nothing to hold.
    pub fn stop_orfs(&mut self) {
        self.orfs.cancel();
        self.orfs = OrfState::Off;
    }

    /// Has any operation ever been recorded, on any branch?
    ///
    /// **Not a dirty flag**, and it must never be used as one. `all_ops`
    /// deliberately keeps abandoned branches, so this is true forever after the
    /// first keystroke — after an undo back to the base, and after a save. It
    /// was `edited()`, its one consumer was the toolbar dot, and it was the
    /// wrong predicate there: a guard or a marker keyed on it fires when nothing
    /// has changed, which is exactly how a guard becomes a reflex click. Use
    /// [`Document::unsaved`] for that question.
    ///
    /// `#[cfg(test)]` because after the dot moved to `unsaved()` the only honest
    /// remaining use is a test asserting that an operation did or did not enter
    /// the log. Left named for what it measures so it cannot be mistaken again.
    #[cfg(test)]
    pub fn has_history(&self) -> bool {
        !self.log.all_ops().is_empty()
    }

    /// Is anything on screen absent from every file on disk?
    ///
    /// The comparison is against the last successful *write*, not the file the
    /// document was opened from — those coincide until the first save. Undo
    /// back to the opening state returns `cursor()` to `None`, which equals
    /// `saved` for a file-backed document, so the guard does not fire; redo
    /// forward makes it dirty again. Both fall out of the definition and need
    /// no special case.
    pub fn unsaved(&self) -> bool {
        self.saved != Some(self.log.cursor())
    }

    /// How many operations stand between the last write and the screen.
    ///
    /// `None` when the saved cursor is not an ancestor of the current one —
    /// reachable by saving and then seeking onto another branch — in which case
    /// the count genuinely does not exist and the caller must say "changes"
    /// rather than invent a number.
    pub fn unsaved_ops(&self) -> Option<usize> {
        self.log.distance_from(self.saved.flatten())
    }

    /// Record that the state at the cursor is now on disk.
    ///
    /// Called only on a successful write. A failed write that marked a document
    /// clean is a data-loss bug wearing the costume of the fix.
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.log.cursor());
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let title = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sequence".into());
        Self::from_bytes(&data, title, Some(path.to_path_buf()))
    }

    /// What the given set is showing, and what it is not.
    ///
    /// Replaces the old `unique_cutters`/`cutters` pair. Those partitioned the
    /// results but had no notion of a filter, so nothing could report what a
    /// filter concealed — which is the whole point of `docs/PLAN.md` item 33.
    pub fn visibility(&self, set: pl_enzymes::EnzymeSet) -> pl_enzymes::Visibility {
        pl_enzymes::Visibility::of(self.digest.results(), set)
    }
}

/// Could this operation move a base or change the topology?
///
/// Exhaustive on purpose — no `_` arm — so a new `OpKind` cannot quietly join
/// the "annotations only" side. Rotate renumbers every coordinate and
/// SetTopology decides whether a reading frame wraps, so both are on the
/// sequence side even though neither adds or removes a base.
fn moves_bases(kind: &OpKind) -> bool {
    match kind {
        OpKind::InsertAt { .. }
        | OpKind::DeleteRange { .. }
        | OpKind::ReplaceRange { .. }
        | OpKind::SetTopology(_)
        | OpKind::Rotate { .. }
        | OpKind::ReverseComplement => true,
        OpKind::SetFeature { .. } | OpKind::RemoveFeature { .. } | OpKind::SetMethylation(_) => {
            false
        }
    }
}

/// `start_digest`'s shape, with the enzyme loop replaced by one ORF scan.
///
/// Every structural choice here is copied rather than reinvented: the same
/// `mol.seq.clone()` (about a millisecond at 4.6 Mb, against the 400 ms scan it
/// feeds), the same `channel()`, the same `Arc<AtomicBool>`, the same named
/// thread, and the same "send failing means the document was replaced; that is
/// fine".
fn spawn_orfs(mol: &Molecule, code_id: u8, min_aa: usize) -> OrfState {
    if mol.seq.is_empty() {
        return OrfState::Unavailable(if mol.sequence_absent() {
            "this file declares a length but carries no bases".into()
        } else if mol.is_annotation_track() {
            "this is an annotation track; it has no sequence".into()
        } else {
            "no sequence".into()
        });
    }
    // Disbelieved here as well as in `settings.rs`, because this is the last
    // place before `find_orfs` and a table that does not exist has no
    // defensible fallback further in.
    let code = pl_core::translate::table(code_id).unwrap_or(pl_core::translate::TABLE11);
    let seq = mol.seq.clone();
    let circular = mol.topology.is_circular();
    let params = pl_core::orf::Params {
        min_aa,
        // Deliberately not exposed. Measured at 200 kb: 3,214 ORFs by default
        // against 11,087 nested, and 618,782 at 4.64 Mb — a strip nobody can
        // read and 24.8 MB to hold it.
        nested: false,
        ..Default::default()
    };
    let (tx, rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    std::thread::Builder::new()
        .name("orfs".into())
        .spawn(move || {
            let Some(orfs) = pl_core::orf::find_orfs_until(&seq, code, circular, &params, &|| {
                flag.load(Ordering::Relaxed)
            }) else {
                // Superseded. Sending a partial list would draw a molecule with
                // fewer genes on it than it has, which is indistinguishable
                // from the truth.
                return;
            };
            let stopless = pl_core::orf::stopless_frames(&seq, code, circular);
            let mut spans: Vec<crate::annot::Span> = Vec::with_capacity(orfs.len());
            let mut lapping = 0usize;
            for (i, o) in orfs.iter().enumerate() {
                if o.laps > 0 {
                    lapping += 1;
                    continue;
                }
                spans.push(crate::annot::Span {
                    start: o.start,
                    end: o.end,
                    item: i as u32,
                    sub: 0,
                    // Frame, never greedy. 0..2 are the forward frames and
                    // 3..5 the reverse ones, so a strip row means one thing
                    // over the whole molecule.
                    lane: Some(if o.strand.is_reverse() {
                        3 + o.frame.min(2)
                    } else {
                        o.frame.min(2)
                    }),
                });
            }
            // Grouped by item, which `of_spans` requires; one span each, so
            // enumerating in order already satisfies it.
            let index = crate::annot::AnnotIndex::of_spans(
                &spans,
                seq.len() as u64,
                circular,
                6,
                // The ORF index is keyed by the document's `seq_version` in
                // `Document`, not by this field, which exists for the feature
                // index's identity check. Nothing compares it.
                (0, None),
            );
            let _ = tx.send(Orfs {
                orfs,
                index,
                lapping,
                stopless,
                code: code.id,
                min_aa,
            });
        })
        .expect("spawn orf worker");
    OrfState::Running { rx, cancel }
}

/// `spawn_orfs`' shape, with the ORF search replaced by one annotation pass.
///
/// Copied rather than reinvented, down to the named thread, the "send
/// failing means the document was replaced; that is fine", and — since
/// 2026-09-03 — the `_until` call with the flag closed over. Until then the
/// cancel flag could only be read at the two ends of the scan
/// ([`ProposalState`] records what that cost). The one thing that still
/// differs is written down where it matters: the worker builds a MINIMAL
/// query molecule rather than cloning the document's.
fn spawn_proposals(mol: &Molecule, unreviewed: bool) -> ProposalState {
    if mol.seq.is_empty() {
        return ProposalState::Unavailable(if mol.sequence_absent() {
            "this file declares a length but carries no bases".into()
        } else if mol.is_annotation_track() {
            "this is an annotation track; it has no sequence".into()
        } else {
            "no sequence".into()
        });
    }
    // `seq` and `topology` and nothing else, which is everything
    // `Annotator::annotate` reads. A whole `mol.clone()` would drag this
    // document's features, primers and notes onto the worker to be ignored —
    // and worse, it would put the CURRENT annotations in front of a function
    // whose entire output is a proposal about them, which is the sort of
    // accidental coupling that later reads as intent.
    let query = Molecule {
        seq: mol.seq.clone(),
        topology: mol.topology,
        ..Molecule::default()
    };
    let (tx, rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    std::thread::Builder::new()
        .name("annotate".into())
        .spawn(move || {
            // Before the index build as well as inside the scan: a worker
            // superseded during another worker's 17 ms build has no reason to
            // pay for one of its own.
            if flag.load(Ordering::Relaxed) {
                return;
            }
            // The first call for the process parses the tables and builds the
            // indexes — about 17 ms, measured — and it happens HERE, on a
            // worker, so no frame ever pays it. Every later call is a lookup.
            let annotator = crate::featuredb::annotator(unreviewed);
            // `annotate_until`, on `find_orfs_until`'s shape (see `spawn_orfs`
            // above): the flag is polled at every loop boundary the scan has,
            // and `None` is "superseded". Dropping the answer rather than
            // sending it, so a list of coordinates in a molecule that has
            // since been edited cannot reach a screen with an Accept button
            // beside it. Until 2026-09-03 this was `annotate` plus a second
            // `flag.load` after it, and a superseded worker ran the whole
            // genome first.
            let Some(hits) = annotator.annotate_until(&query, &|| flag.load(Ordering::Relaxed))
            else {
                return;
            };
            let unseedable = annotator
                .unseedable()
                .iter()
                .map(|r| r.name.clone())
                .collect();
            // Send failing means the document was replaced; that is fine.
            let _ = tx.send(Proposals {
                db: crate::featuredb::db(unreviewed),
                hits,
                unseedable,
                unreviewed,
            });
        })
        .expect("spawn annotate worker");
    ProposalState::Running { rx, cancel }
}

fn start_digest(mol: &Molecule) -> DigestState {
    if mol.seq.is_empty() {
        return DigestState::Unavailable(if mol.sequence_absent() {
            "this file declares a length but carries no bases".into()
        } else if mol.is_annotation_track() {
            "this is an annotation track; it has no sequence".into()
        } else {
            "no sequence".into()
        });
    }

    // The worker owns copies. Cloning a 4.6 Mb sequence costs about a
    // millisecond, far less than the scan it feeds.
    let seq = mol.seq.clone();
    let topology = mol.topology;
    // Nothing in the UI edits the methylation flags — they come off the file —
    // so the worker's copy cannot go stale without the document changing, and a
    // document change restarts the digest.
    let meth = mol.methylation;
    let (tx, rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    std::thread::Builder::new()
        .name("digest".into())
        .spawn(move || {
            // Checked once per enzyme: 58 relaxed loads against a scan that
            // takes seconds, so the check itself is free, and a superseded
            // worker stops within one enzyme instead of finishing the genome.
            let mut results = Vec::with_capacity(pl_enzymes::ENZYMES.len());
            let mut verdicts = Vec::with_capacity(pl_enzymes::ENZYMES.len());
            let mut all_sites = Vec::with_capacity(pl_enzymes::ENZYMES.len());
            let mut all_effects = Vec::with_capacity(pl_enzymes::ENZYMES.len());
            for e in pl_enzymes::ENZYMES.iter() {
                if flag.load(Ordering::Relaxed) {
                    return;
                }
                // `cut_sites` rather than `cut_positions`, because
                // `cut_positions` *is* `cut_sites` with the sites mapped away
                // and the Enzymes tab needs the first one back. Calling both
                // would double this worker's cost; deriving the positions here
                // costs a sort. The two lines below are `cut_positions`' whole
                // body and `cut_positions_and_verdicts_agree_with_the_crate`
                // pins them to it, because the dedup is not cosmetic: on a
                // circle two sites at different starts can nick the same bond.
                let sites = pl_enzymes::cut_sites(&seq, topology, e);
                let mut positions: Vec<u64> = sites.iter().map(|c| c.position).collect();
                positions.sort_unstable();
                positions.dedup();
                // EVERY site, not the first. The verdict is a property of the
                // site — so it is asked with `site_start` rather than
                // reconstructed from a cut position, which disagrees wherever a
                // site wraps the origin — and it is asked of all of them,
                // because two sites of one enzyme routinely differ and the
                // lowest-coordinate one is chosen by where the file was
                // linearised. See [`Methylated`].
                //
                // The cost is one `site_effect` per SITE instead of per enzyme.
                // That is a scan of a window the width of the recognition site
                // plus its flanks, over the handful of rules naming this enzyme;
                // the `cut_sites` call above it, which walks the whole molecule,
                // is what this worker is actually paying for.
                let (effects, verdict) = methylation_at(e, &seq, topology, &meth, &sites);
                verdicts.push(verdict);
                results.push(Digest {
                    enzyme: e,
                    positions,
                });
                all_sites.push(sites);
                all_effects.push(effects);
            }
            // Send failing means the document was replaced; that is fine.
            let _ = tx.send(Digested {
                results,
                verdicts,
                sites: all_sites,
                site_effects: all_effects,
            });
        })
        .expect("spawn digest worker");
    DigestState::Running { rx, cancel }
}

/// Human-readable summary of what a molecule is, for the title bar.
pub fn describe(mol: &Molecule, format: Format) -> String {
    let topo = match mol.topology {
        Topology::Circular => "circular",
        Topology::Linear => "linear",
    };
    if mol.sequence_absent() {
        format!(
            "{} · {} bp declared, no bases · {topo}",
            format.name(),
            fmt_int(mol.span())
        )
    } else if mol.is_annotation_track() {
        format!("{} · annotation track, no sequence", format.name())
    } else {
        format!("{} · {} bp · {topo}", format.name(), fmt_int(mol.len()))
    }
}

/// Thousands separators without pulling in a formatting crate.
pub fn fmt_int(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn integers_get_separators() {
        assert_eq!(fmt_int(0), "0");
        assert_eq!(fmt_int(999), "999");
        assert_eq!(fmt_int(1000), "1,000");
        assert_eq!(fmt_int(4_641_652), "4,641,652");
    }

    #[test]
    fn a_fasta_document_loads_and_digests() {
        let doc = Document::from_bytes(b">x\nGAATTCaaaaGGATCC\n", "x.fa".into(), None).unwrap();
        assert_eq!(doc.molecule().len(), 16);
        assert!(doc.digest.is_running() || !doc.digest.results().is_empty());
    }

    #[test]
    fn an_unreadable_file_reports_the_cores_message() {
        let e = match Document::from_bytes(b"not a sequence", "x".into(), None) {
            Err(e) => e,
            Ok(_) => panic!("that is not a sequence file"),
        };
        assert!(e.contains("unrecognised"), "{e}");
    }

    #[test]
    fn a_chromatogram_is_named_rather_than_rejected_vaguely() {
        let e = match Document::from_bytes(b"ABIF\x00\x01\x02\x03", "x.ab1".into(), None) {
            Err(e) => e,
            Ok(_) => panic!("a chromatogram is not a sequence file"),
        };
        assert!(e.contains("ABIF"), "{e}");
    }

    #[test]
    fn a_file_with_no_bases_says_why_it_cannot_be_digested() {
        let gb = "LOCUS       x                      100 bp    DNA     linear   SYN 01-JAN-2026\n\
                  FEATURES             Location/Qualifiers\n\
                  \x20    gene            1..10\n\
                  ORIGIN\n//\n";
        let doc = Document::from_bytes(gb.as_bytes(), "x.gb".into(), None).unwrap();
        match &doc.digest {
            DigestState::Unavailable(why) => assert!(why.contains("no bases"), "{why}"),
            _ => panic!("expected Unavailable"),
        }
    }

    /// A small circular molecule with one feature, as `Document` would hold it.
    fn doc_of(seq: &str, circular: bool) -> Document {
        let mut mol = Molecule {
            seq: seq.as_bytes().to_vec(),
            topology: if circular {
                Topology::Circular
            } else {
                Topology::Linear
            },
            ..Default::default()
        };
        let mut f = pl_core::Feature::new("gg", "misc_feature");
        f.segments.push(pl_core::Segment::new(9, 12));
        mol.features.push(f);
        Document {
            path: None,
            title: "test".into(),
            digest: start_digest(&mol),
            orfs: OrfState::Off,
            proposals: ProposalState::Off,
            seq_version: 0,
            orf_params: (11, 30),
            proposal_scope: false,
            orf_spawns: 0,
            proposal_spawns: 0,
            log: OpLog::new(mol),
            format: Format::GenBank,
            container: None,
            records_in_file: 1,
            unrepresentable_locations: Vec::new(),
            saved: None,
        }
    }

    #[test]
    fn an_edit_is_undoable_and_leaves_the_molecule_as_it_was() {
        // The whole reason the op log exists, and it was wired to nothing.
        // NOT a palindrome: AAAACCCCGGGGTTTT is its own reverse complement,
        // so a revcomp test on it passes no matter what the code does.
        const SEQ: &str = "AAAACCCCGGGGTTTTAAGG";
        let mut d = doc_of(SEQ, true);
        assert!(!d.has_history());
        assert!(!d.log.can_undo());

        d.apply(OpKind::ReverseComplement).unwrap();
        assert!(d.has_history());
        assert!(d.log.can_undo());
        assert_eq!(
            d.molecule().seq,
            pl_core::reverse_complement(SEQ.as_bytes()),
            "the sequence really changed"
        );

        d.undo().unwrap();
        assert_eq!(d.molecule().seq, SEQ.as_bytes().to_vec());
        assert!(d.log.can_redo(), "the redo branch must still be there");

        d.redo().unwrap();
        assert_eq!(
            d.molecule().seq,
            pl_core::reverse_complement(SEQ.as_bytes())
        );
    }

    #[test]
    fn a_refused_edit_changes_nothing() {
        // The log declines an edit that would leave the annotations describing
        // something the sequence does not contain. The document must come back
        // untouched, not half-applied.
        let mut d = doc_of("AAAACCCCGGGGTTTT", false);
        let before = d.molecule().clone();
        let err = d.apply(OpKind::Rotate { origin: 5 }).unwrap_err();
        assert!(matches!(err, OpError::NotCircular));
        assert_eq!(d.molecule().seq, before.seq);
        assert_eq!(d.molecule().features, before.features);
        assert!(
            !d.has_history(),
            "a refused edit must not enter the history"
        );
    }

    #[test]
    fn a_new_edit_after_an_undo_forks_rather_than_truncating() {
        // Every other editor in this category throws the redo branch away at
        // this moment, and that is where people lose an afternoon.
        let mut d = doc_of("AAAACCCCGGGGTTTT", true);
        d.apply(OpKind::ReverseComplement).unwrap();
        d.undo().unwrap();
        d.apply(OpKind::SetTopology(Topology::Linear)).unwrap();

        assert_eq!(
            d.log.all_ops().len(),
            2,
            "the abandoned branch is still recorded"
        );
        assert_eq!(d.log.path().len(), 1, "but it is not on the current path");
        // The branch is at the base, which `forks()` cannot express since it
        // returns op ids; the two ops sharing a `None` parent are the record.
        let roots = d
            .log
            .all_ops()
            .iter()
            .filter(|o| o.parent.is_none())
            .count();
        assert_eq!(roots, 2, "both first-edits are kept");
    }

    /// Where EcoRI cuts, once the worker has finished.
    ///
    /// The answer has to be read off a *position*, not off `results().len()`.
    /// `start_digest` emits one `Digest` per entry of the fixed `ENZYMES`
    /// table whether or not the enzyme cuts anything, so the length of the
    /// results is a compile-time constant that carries no information about
    /// the sequence at all: it is `ENZYMES.len()` in `Done` and 0 in the other
    /// two states. Asserting on it cannot tell a recomputed digest from a
    /// stale one — which is exactly what the test below used to do.
    fn ecori_sites(d: &mut Document) -> Vec<u64> {
        while d.digest.is_running() {
            d.digest.poll();
            std::thread::yield_now();
        }
        d.digest
            .results()
            .iter()
            .find(|x| x.enzyme.name == "EcoRI")
            .expect("EcoRI is in the shipped table")
            .positions
            .clone()
    }

    #[test]
    fn editing_re_digests_rather_than_showing_a_stale_answer() {
        // Cut sites are a function of the sequence. A stale enzyme list after
        // an edit is a wrong answer presented as a current one.
        //
        // The edit deletes the one GAATTC, so the enzyme list before and after
        // genuinely disagree. The previous form of this test asserted
        // `is_running() || results().len() == before`, and both disjuncts hold
        // in every reachable state: deleting the re-digest from `apply` left
        // `Done(old)`, whose length is still `ENZYMES.len()`. It survived the
        // exact mutation it exists to catch.
        let mut d = doc_of("AAAAGAATTCCCCGGGGTTTT", true);
        assert_eq!(ecori_sites(&mut d), vec![6], "the premise: one EcoRI site");

        d.apply(OpKind::DeleteRange { start: 5, len: 6 }).unwrap();
        assert!(d.digest.is_running(), "a fresh worker, not the old results");
        assert!(
            ecori_sites(&mut d).is_empty(),
            "the site was deleted, so the digest must no longer report it"
        );
    }

    /// The worker derives `positions` itself instead of calling
    /// `cut_positions`, so that it can keep the `CutSite`s from the one scan it
    /// already pays for. This pins the derivation to the crate: the sort and
    /// the dedup are not cosmetic — on a circle two sites at different starts
    /// can nick the same bond — and the verdict has to be asked at the *site*,
    /// which is not recoverable from the cut position when the site wraps the
    /// origin.
    #[test]
    fn the_workers_positions_and_verdicts_match_the_crate() {
        // ApaI's GGGCCC starts at 0-based 15 on this 20 bp circle and runs off
        // the end, and Dcm blocks it there.
        let mut mol = Molecule {
            seq: b"CAAAAAAAAAAACCAGGGCC".to_vec(),
            topology: Topology::Circular,
            ..Default::default()
        };
        mol.methylation.dcm = true;
        let mut d = Document::of_molecule(mol.clone());
        while d.digest.is_running() {
            d.digest.poll();
            std::thread::yield_now();
        }

        for (i, dg) in d.digest.results().iter().enumerate() {
            assert_eq!(
                dg.positions,
                pl_enzymes::cut_positions(&mol.seq, mol.topology, dg.enzyme),
                "{} positions",
                dg.enzyme.name
            );
            let sites = pl_enzymes::cut_sites(&mol.seq, mol.topology, dg.enzyme);
            let want: Vec<Option<SiteEffect>> = sites
                .iter()
                .map(|s| {
                    pl_enzymes::methylation::site_effect(
                        dg.enzyme,
                        &mol.seq,
                        (s.site_start - 1) as usize,
                        mol.topology,
                        &mol.methylation,
                    )
                })
                .collect();
            assert_eq!(
                d.digest.verdict(i),
                summarise(&want),
                "{} verdict",
                dg.enzyme.name
            );
            // And the per-cut answers, which is what the map's tooltip prints
            // beside a coordinate.
            for s in &sites {
                assert_eq!(
                    d.digest.site_verdict(i, s.position),
                    sites
                        .iter()
                        .zip(&want)
                        .filter(|(o, _)| o.position == s.position)
                        .filter_map(|(_, e)| *e)
                        .max_by_key(|e| severity(*e)),
                    "{} at {}",
                    dg.enzyme.name,
                    s.position
                );
            }
        }

        let i = d
            .digest
            .results()
            .iter()
            .position(|x| x.enzyme.name == "ApaI")
            .expect("ApaI ships");
        let v = d.digest.verdict(i).expect("Dcm blocks this wrapped site");
        assert_eq!(v.worst.effect, pl_enzymes::methylation::Effect::Blocked);
        assert_eq!(v.worst.methylase, pl_enzymes::methylation::Methylase::Dcm);
        assert_eq!((v.total, v.blocked), (1, 1), "one site, and it is blocked");
        assert!(v.all_blocked());
    }

    /// A 3 kb circular plasmid with TWO ClaI sites, of which Dam blocks exactly
    /// one. The fixture the first-site verdict could not describe.
    ///
    /// ClaI reads `ATCGAT` and Dam is `Scope::AnyOverlap`, so a site is blocked
    /// only when a `GATC` actually overlaps it: preceded by `G`, or followed by
    /// `C`. That is about 44% of random sites, which is why an enzyme with a
    /// mixture of blocked and live sites is the ordinary case rather than a
    /// contrived one.
    ///
    /// The filler is `A` and `C` only, so neither `ATCGAT` nor `GATC` can
    /// appear anywhere by accident: the molecule has exactly the two sites put
    /// into it, and the ClaI Cpg rule — which is `Scope::Unconditional` and
    /// would block both — is off, as it is by default and in every plasmid
    /// grown in E. coli.
    pub(crate) fn two_clai_sites_one_dam_blocked() -> Molecule {
        let mut seq = b"AAAAC".repeat(600);
        // 0-based 300, as `AAA ATCGAT AAA`: no overlapping GATC, so it cuts.
        seq[297..309].copy_from_slice(b"AAAATCGATAAA");
        // 0-based 2000, as `AAG ATCGAT AAA`: the G in front makes `GATC` across
        // the site's first three bases, so Dam blocks this one.
        seq[1997..2009].copy_from_slice(b"AAGATCGATAAA");
        let mut mol = Molecule {
            seq,
            topology: Topology::Circular,
            ..Default::default()
        };
        mol.methylation.dam = true;
        mol
    }

    /// PROVEN TO FAIL before this change, in both directions and by rotation
    /// alone.
    ///
    /// The verdict was `sites.first()`, and `cut_sites` returns ascending
    /// coordinates, so *first* meant *lowest*. On this plasmid as linearised
    /// the lowest ClaI site is the LIVE one, so the app said ClaI was
    /// unaffected: no strikethrough, no chip, no gel caveat, and the lane drawn
    /// as fact. Rotate the same molecule — `Edit → Set origin here`, or simply
    /// open the same plasmid from a differently linearised file — and the
    /// blocked site becomes the lowest, so every one of those answers inverted
    /// and ClaI was struck through as a non-cutter. Same molecule, two
    /// contradictory bench predictions, decided by where the file was cut.
    #[test]
    fn a_methylation_verdict_covers_every_site_and_does_not_move_with_the_origin() {
        use pl_enzymes::methylation::{Effect, Methylase};
        let plain = two_clai_sites_one_dam_blocked();
        let mut rotated = plain.clone();
        // Past the first site and short of the second, so the blocked site is
        // the one with the lower coordinate afterwards and neither is split.
        rotated.seq.rotate_left(1_900);

        let mut seen: Vec<Methylated> = Vec::new();
        for (label, mol) in [("as linearised", &plain), ("rotated", &rotated)] {
            let mut d = Document::of_molecule(mol.clone());
            while d.digest.is_running() {
                d.digest.poll();
                std::thread::yield_now();
            }
            let i = d
                .digest
                .results()
                .iter()
                .position(|x| x.enzyme.name == "ClaI")
                .expect("ClaI ships");
            let cuts = d.digest.results()[i].positions.clone();
            assert_eq!(cuts.len(), 2, "{label}: the fixture needs two ClaI sites");

            let v = d
                .digest
                .verdict(i)
                .unwrap_or_else(|| panic!("{label}: Dam blocks one of the two sites"));
            assert_eq!(
                (v.total, v.blocked, v.affected),
                (2, 1, 1),
                "{label}: {v:?}"
            );
            assert_eq!(v.worst.effect, Effect::Blocked, "{label}");
            assert_eq!(v.worst.methylase, Methylase::Dam, "{label}");
            // ClaI still cuts this plasmid, once. Striking the row through, or
            // refusing to seed the lane, would be calling a live enzyme dead.
            assert!(!v.all_blocked(), "{label}: {v:?}");
            assert_eq!(v.live(), 1, "{label}");
            assert!(v.chip().contains("1 of 2 sites"), "{label}: {}", v.chip());

            // AND PER CUT, because the map's tooltip prints this beside a
            // coordinate. Exactly one of the two cuts is the blocked one, and
            // the other must say nothing rather than inherit its neighbour's
            // verdict.
            let per_cut: Vec<Option<SiteEffect>> =
                cuts.iter().map(|p| d.digest.site_verdict(i, *p)).collect();
            assert_eq!(
                per_cut.iter().filter(|e| e.is_some()).count(),
                1,
                "{label}: {per_cut:?}"
            );
            assert_eq!(
                per_cut
                    .iter()
                    .flatten()
                    .map(|e| e.effect)
                    .collect::<Vec<_>>(),
                vec![Effect::Blocked],
                "{label}"
            );
            seen.push(v);
        }
        assert_eq!(
            seen[0], seen[1],
            "rotating the molecule changed what the app says about Dam blocking"
        );

        // THE CONTROL. Take the G away from in front of the second site and
        // nothing is blocked at all, so the verdict is not boilerplate that
        // any two-site enzyme would collect.
        let mut live = plain.clone();
        live.seq[1999] = b'A';
        let mut d = Document::of_molecule(live);
        while d.digest.is_running() {
            d.digest.poll();
            std::thread::yield_now();
        }
        let i = d
            .digest
            .results()
            .iter()
            .position(|x| x.enzyme.name == "ClaI")
            .expect("ClaI ships");
        assert_eq!(d.digest.results()[i].positions.len(), 2);
        assert_eq!(d.digest.verdict(i), None);
    }

    /// The Enzymes tab draws one row per cutting enzyme and every row shows a
    /// methylation verdict. Recovering that verdict used to mean a full
    /// `cut_sites` scan of the whole molecule *per row, per frame* — 58 of them,
    /// measured at 1.58 s on the 4.6 Mb NC_000913.3, which is `doc.rs`'s entire
    /// digest put back on the UI thread. This asserts the ratio rather than an
    /// absolute time, so it means the same thing on a slow machine.
    #[test]
    fn a_frame_of_the_enzymes_tab_does_not_rescan_the_molecule_per_row() {
        // 120 kb: a fosmid, well past the ~45 kb where 58 scans stop fitting in
        // a frame and well short of the genomes where it becomes seconds.
        let n = 120_000usize;
        let mut seq = Vec::with_capacity(n);
        let mut s = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..n {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            seq.push(b"ACGT"[(s >> 33) as usize & 3]);
        }
        let mol = Molecule {
            seq: seq.clone(),
            topology: Topology::Circular,
            ..Default::default()
        };
        let mut d = Document::of_molecule(mol);
        while d.digest.is_running() {
            d.digest.poll();
            std::thread::yield_now();
        }
        let rows = d.digest.results().len();
        assert_eq!(rows, pl_enzymes::ENZYMES.len(), "one row per enzyme");

        // Twenty frames of the tab, every row asking for its verdict.
        let t = std::time::Instant::now();
        let mut seen = 0usize;
        for _ in 0..20 {
            for i in 0..rows {
                if d.digest.verdict(i).is_some() {
                    seen += 1;
                }
            }
        }
        let twenty_frames = t.elapsed();
        std::hint::black_box(seen);

        // One frame the way it was done before: a full-molecule scan per row.
        let t = std::time::Instant::now();
        for dg in d.digest.results() {
            std::hint::black_box(
                pl_enzymes::cut_sites(&seq, Topology::Circular, dg.enzyme)
                    .into_iter()
                    .next(),
            );
        }
        let one_old_frame = t.elapsed();

        assert!(
            twenty_frames * 10 < one_old_frame,
            "twenty frames of verdict reads took {twenty_frames:?}; one frame of the \
             old per-row rescan took {one_old_frame:?} — the verdict is being recomputed"
        );
    }

    #[test]
    fn undo_and_redo_re_digest_as_well() {
        // Same reason, and neither had any coverage at all: the enzyme list
        // after an undo has to describe the molecule the undo brought back.
        let mut d = doc_of("AAAAGAATTCCCCGGGGTTTT", true);
        assert_eq!(ecori_sites(&mut d), vec![6]);
        d.apply(OpKind::DeleteRange { start: 5, len: 6 }).unwrap();
        assert!(ecori_sites(&mut d).is_empty());

        d.undo().unwrap();
        assert!(d.digest.is_running(), "undo starts a fresh digest");
        assert_eq!(ecori_sites(&mut d), vec![6], "the site is back");

        d.redo().unwrap();
        assert!(d.digest.is_running(), "and so does redo");
        assert!(ecori_sites(&mut d).is_empty());
    }
}
