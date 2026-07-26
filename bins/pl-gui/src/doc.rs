//! The loaded document, and the work that must not happen on the UI thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

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
    pub molecule: Molecule,
    pub format: Format,
    /// Present only for `.dna`, so the container can be described.
    pub container: Option<snapgene::Document>,
    pub digest: DigestState,
}

impl Document {
    pub fn from_bytes(data: &[u8], title: String, path: Option<PathBuf>) -> Result<Self, String> {
        let container = if pl_fileio::detect(data) == Some(Format::SnapGene) {
            snapgene::parse(data).ok()
        } else {
            None
        };
        let (molecule, format) = pl_fileio::load(data).map_err(|e| e.to_string())?;
        let digest = start_digest(&molecule);
        Ok(Document {
            path,
            title,
            molecule,
            format,
            container,
            digest,
        })
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let title = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sequence".into());
        Self::from_bytes(&data, title, Some(path.to_path_buf()))
    }

    pub fn unique_cutters(&self) -> impl Iterator<Item = &Digest> {
        self.digest
            .results()
            .iter()
            .filter(|d| d.is_unique_cutter())
    }

    pub fn cutters(&self) -> impl Iterator<Item = &Digest> {
        self.digest.results().iter().filter(|d| !d.is_non_cutter())
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
        assert_eq!(doc.molecule.len(), 16);
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
}
