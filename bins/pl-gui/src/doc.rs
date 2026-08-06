//! The loaded document, and the work that must not happen on the UI thread.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use pl_core::oplog::{OpError, OpKind, OpLog};
use pl_core::{Molecule, Topology};
use pl_enzymes::methylation::SiteEffect;
use pl_enzymes::Digest;
use pl_fileio::{snapgene, Format};

/// Everything one worker produces about a molecule.
///
/// The verdicts live here rather than being derived at paint time. The Enzymes
/// tab draws one row per cutting enzyme and each row shows a methylation
/// verdict; computing that verdict needs the enzyme's first [`pl_enzymes::CutSite`],
/// and asking `cut_sites` for it in the row builder put a full-molecule scan
/// back on the UI thread — 58 of them per frame, measured at 1.58 s per frame
/// on the 4.6 Mb NC_000913.3, which is the whole of the work this worker exists
/// to take away. The worker already had those sites in hand and threw them
/// away.
pub struct Digested {
    pub results: Vec<Digest>,
    /// Parallel to `results`: the methylation verdict at that enzyme's first
    /// site, or `None` if it does not cut or nothing methylates its site.
    verdicts: Vec<Option<SiteEffect>>,
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
    /// The methylation verdict for `results()[i]`.
    ///
    /// A field read, deliberately. See [`Digested`] for what it cost when this
    /// was a scan.
    pub fn verdict(&self, i: usize) -> Option<SiteEffect> {
        match self {
            DigestState::Done(v) => v.verdicts.get(i).copied().flatten(),
            _ => None,
        }
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
            seq_version: 0,
            orf_params: (11, 30),
            #[cfg(test)]
            orf_spawns: 0,
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
            seq_version: 0,
            orf_params: (11, 30),
            orf_spawns: 0,
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
                // The verdict is a property of the *site*, so it is asked at
                // the first site the scan found rather than reconstructed from
                // a cut position — the two disagree wherever a site wraps the
                // origin.
                verdicts.push(sites.first().and_then(|s| {
                    pl_enzymes::methylation::site_effect(
                        e,
                        &seq,
                        (s.site_start - 1) as usize,
                        topology,
                        &meth,
                    )
                }));
                results.push(Digest {
                    enzyme: e,
                    positions,
                });
                all_sites.push(sites);
            }
            // Send failing means the document was replaced; that is fine.
            let _ = tx.send(Digested {
                results,
                verdicts,
                sites: all_sites,
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
mod tests {
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
            seq_version: 0,
            orf_params: (11, 30),
            orf_spawns: 0,
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
    /// `cut_positions`, so that it can keep the first `CutSite` from the one
    /// scan it already pays for. This pins the derivation to the crate: the
    /// sort and the dedup are not cosmetic — on a circle two sites at different
    /// starts can nick the same bond — and the verdict has to be asked at the
    /// *site*, which is not recoverable from the cut position when the site
    /// wraps the origin.
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
            let want = pl_enzymes::cut_sites(&mol.seq, mol.topology, dg.enzyme)
                .into_iter()
                .next()
                .and_then(|s| {
                    pl_enzymes::methylation::site_effect(
                        dg.enzyme,
                        &mol.seq,
                        (s.site_start - 1) as usize,
                        mol.topology,
                        &mol.methylation,
                    )
                });
            assert_eq!(d.digest.verdict(i), want, "{} verdict", dg.enzyme.name);
        }

        let i = d
            .digest
            .results()
            .iter()
            .position(|x| x.enzyme.name == "ApaI")
            .expect("ApaI ships");
        let v = d.digest.verdict(i).expect("Dcm blocks this wrapped site");
        assert_eq!(v.effect, pl_enzymes::methylation::Effect::Blocked);
        assert_eq!(v.methylase, pl_enzymes::methylation::Methylase::Dcm);
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
