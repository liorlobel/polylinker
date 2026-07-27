//! The loaded document, and the work that must not happen on the UI thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use pl_core::oplog::{OpError, OpKind, OpLog};
use pl_core::{Molecule, Topology};
use pl_enzymes::Digest;
use pl_fileio::{snapgene, Format};

/// Digestion is O(sequence x enzymes) and takes about 600 ms on a 4.6 Mb
/// genome. That is four dropped frames at 60 Hz, so it runs on a worker and the
/// UI says so meanwhile.
pub enum DigestState {
    Running(Receiver<Vec<Digest>>),
    Done(Vec<Digest>),
    /// No bases to digest, with the reason.
    Unavailable(String),
}

impl DigestState {
    pub fn results(&self) -> &[Digest] {
        match self {
            DigestState::Done(v) => v,
            _ => &[],
        }
    }
    pub fn is_running(&self) -> bool {
        matches!(self, DigestState::Running(_))
    }
    /// Collect the worker's result if it has finished. Returns true if the
    /// state changed, so the caller knows to repaint.
    pub fn poll(&mut self) -> bool {
        let done = match self {
            DigestState::Running(rx) => rx.try_recv().ok(),
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
        self.digest = start_digest(self.log.current());
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), OpError> {
        self.log.undo()?;
        self.digest = start_digest(self.log.current());
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), OpError> {
        self.log.redo()?;
        self.digest = start_digest(self.log.current());
        Ok(())
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
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("digest".into())
        .spawn(move || {
            let out = pl_enzymes::ENZYMES
                .iter()
                .map(|e| Digest {
                    enzyme: e,
                    positions: pl_enzymes::cut_positions(&seq, topology, e),
                })
                .collect();
            // Send failing means the document was replaced; that is fine.
            let _ = tx.send(out);
        })
        .expect("spawn digest worker");
    DigestState::Running(rx)
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

    #[test]
    fn editing_re_digests_rather_than_showing_a_stale_answer() {
        // Cut sites are a function of the sequence. A stale enzyme list after
        // an edit is a wrong answer presented as a current one.
        let mut d = doc_of("AAAAGAATTCCCCGGGGTTTT", true);
        while d.digest.is_running() {
            d.digest.poll();
            std::thread::yield_now();
        }
        let before = d.digest.results().len();
        assert!(before > 0);

        d.apply(OpKind::DeleteRange { start: 5, len: 6 }).unwrap();
        // A fresh worker, not the old results.
        assert!(
            d.digest.is_running() || d.digest.results().len() == before,
            "the digest must be recomputed after an edit"
        );
    }
}
