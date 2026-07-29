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
    /// Records the file held. Only the first is shown, and a viewer that does
    /// not say so is indistinguishable from a file with fewer records in it.
    pub records_in_file: usize,
    /// Location forms the reader could not represent, reported not dropped.
    pub unrepresentable_locations: Vec<String>,
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
        Ok(Document {
            path,
            title,
            log: OpLog::new(molecule),
            format,
            container,
            digest,
            records_in_file: report.records,
            unrepresentable_locations: report.unrepresentable_locations,
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
            log: OpLog::new(mol),
            format: Format::GenBank,
            container: None,
            records_in_file: 1,
            unrepresentable_locations: Vec::new(),
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
        self.log.apply(kind, "you")?;
        self.restart_digest();
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), OpError> {
        self.log.undo()?;
        self.restart_digest();
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), OpError> {
        self.log.redo()?;
        self.restart_digest();
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
        self.restart_digest();
        Ok(())
    }

    /// Start a fresh digest, and tell the previous one to give up.
    fn restart_digest(&mut self) {
        self.digest.cancel();
        self.digest = start_digest(self.log.current());
    }

    /// Has anything been edited since the file was opened?
    pub fn edited(&self) -> bool {
        !self.log.all_ops().is_empty()
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
        if i > 0 && (s.len() - i) % 3 == 0 {
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
            log: OpLog::new(mol),
            format: Format::GenBank,
            container: None,
            records_in_file: 1,
            unrepresentable_locations: Vec::new(),
        }
    }

    #[test]
    fn an_edit_is_undoable_and_leaves_the_molecule_as_it_was() {
        // The whole reason the op log exists, and it was wired to nothing.
        // NOT a palindrome: AAAACCCCGGGGTTTT is its own reverse complement,
        // so a revcomp test on it passes no matter what the code does.
        const SEQ: &str = "AAAACCCCGGGGTTTTAAGG";
        let mut d = doc_of(SEQ, true);
        assert!(!d.edited());
        assert!(!d.log.can_undo());

        d.apply(OpKind::ReverseComplement).unwrap();
        assert!(d.edited());
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
        assert!(!d.edited(), "a refused edit must not enter the history");
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
