//! Sequence file formats for Polylinker.
//!
//! Formats are detected from **content, not from the file extension**. In a
//! real corpus the extension lies often enough to matter: `.ab1` files that
//! are actually SCF or ZTR, `.gb` files that are FASTA, `.seq` that could be
//! anything. Sniffing costs one read of the first few bytes.

pub mod fasta;
pub mod genbank;
pub mod snapgene;
pub mod xml;

use pl_core::Molecule;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    SnapGene,
    GenBank,
    Fasta,
    /// Recognised, but a chromatogram rather than a sequence file. Named so
    /// the user gets "that is an ABIF trace" instead of "unknown file".
    Abif,
    Scf,
    Ztr,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::SnapGene => "SnapGene .dna",
            Format::GenBank => "GenBank",
            Format::Fasta => "FASTA",
            Format::Abif => "ABIF chromatogram",
            Format::Scf => "SCF chromatogram",
            Format::Ztr => "ZTR chromatogram",
        }
    }
    pub fn is_sequence_file(self) -> bool {
        matches!(self, Format::SnapGene | Format::GenBank | Format::Fasta)
    }
}

/// Identify a file from its leading bytes.
pub fn detect(data: &[u8]) -> Option<Format> {
    if data.len() >= 13 && data[0] == snapgene::block::HEADER && &data[5..13] == snapgene::MAGIC {
        return Some(Format::SnapGene);
    }
    match &data[..data.len().min(4)] {
        b"ABIF" => return Some(Format::Abif),
        b".scf" => return Some(Format::Scf),
        [0xAE, b'Z', b'T', b'R'] => return Some(Format::Ztr),
        _ => {}
    }
    // Text formats: look at the first few KB only.
    let head = String::from_utf8_lossy(&data[..data.len().min(8192)]);
    if head
        .lines()
        .any(|l| l.starts_with("LOCUS ") || l == "LOCUS")
        || head.lines().any(|l| l.starts_with("ORIGIN"))
    {
        return Some(Format::GenBank);
    }
    if head.trim_start().starts_with('>') {
        return Some(Format::Fasta);
    }
    None
}

#[derive(Debug)]
pub enum LoadError {
    Unrecognised,
    NotASequenceFile(Format),
    SnapGene(snapgene::Error),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Unrecognised => write!(
                f,
                "unrecognised format -- expected SnapGene .dna, GenBank or FASTA"
            ),
            LoadError::NotASequenceFile(fmt) => {
                write!(f, "that is {}, not a sequence file", fmt.name())
            }
            LoadError::SnapGene(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// What a file contained, beyond the molecule that was returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadReport {
    /// Records present in the file. `load` returns only the first.
    ///
    /// Multi-record files are ordinary — a genome writes one record per contig
    /// — and there was no channel to say so. A 124-record `.gbk` came back as
    /// one molecule with 1,879 features missing, and `pl convert` then wrote
    /// that truncated molecule back out. 8 of 303 GenBank files and 351 FASTA
    /// files in this project's corpus have more than one record.
    pub records: usize,
}

impl LoadReport {
    /// Did the file hold more than we returned?
    pub fn truncated(&self) -> bool {
        self.records > 1
    }
}

/// Load the first record of a file.
///
/// Prefer [`load_with_report`] where the caller can tell the user what was
/// left behind; this exists because most callers genuinely want one molecule.
pub fn load(data: &[u8]) -> Result<(Molecule, Format), LoadError> {
    load_with_report(data).map(|(m, f, _)| (m, f))
}

/// Load the first record, and say what else the file held.
pub fn load_with_report(data: &[u8]) -> Result<(Molecule, Format, LoadReport), LoadError> {
    match detect(data) {
        Some(Format::SnapGene) => {
            let doc = snapgene::parse(data).map_err(LoadError::SnapGene)?;
            Ok((doc.molecule, Format::SnapGene, LoadReport { records: 1 }))
        }
        Some(Format::GenBank) => {
            let text = String::from_utf8_lossy(data);
            let all = genbank::parse_all(&text);
            let records = all.len();
            Ok((
                all.into_iter().next().unwrap_or_default(),
                Format::GenBank,
                LoadReport { records },
            ))
        }
        Some(Format::Fasta) => {
            let text = String::from_utf8_lossy(data);
            let all = fasta::parse_all(&text);
            let records = all.len();
            Ok((
                all.into_iter().next().unwrap_or_default(),
                Format::Fasta,
                LoadReport { records },
            ))
        }
        Some(other) => Err(LoadError::NotASequenceFile(other)),
        None => Err(LoadError::Unrecognised),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_multi_record_file_reports_what_it_held() {
        // `load` returns record 1 and used to have no way to say so. A
        // 124-record .gbk came back as one molecule with 1,879 features gone,
        // and `pl convert` then wrote that truncated molecule back out.
        // Built from lines rather than one literal: GenBank is column-sensitive
        // and a stray indent on ORIGIN silently produces an empty sequence.
        let record = |name: &str, bases: &str| {
            [
                format!(
                    "LOCUS       {name:<16}           4 bp    DNA     linear   SYN 27-JUL-2026"
                ),
                "ORIGIN".to_string(),
                format!("        1 {bases}"),
                "//".to_string(),
            ]
            .join("\n")
        };
        let two = format!("{}\n{}\n", record("one", "acgt"), record("two", "tttt"));
        let (mol, fmt, report) = load_with_report(two.as_bytes()).unwrap();
        assert_eq!(fmt, Format::GenBank);
        assert_eq!(mol.seq, b"acgt".to_vec(), "the first record is returned");
        assert_eq!(report.records, 2);
        assert!(report.truncated());

        let fasta = ">a
ACGT
>b
TTTT
>c
GGGG
";
        let (_, _, r) = load_with_report(fasta.as_bytes()).unwrap();
        assert_eq!(r.records, 3);
        assert!(r.truncated());

        // A single-record file is not truncated.
        let one = format!("{}\n", record("one", "acgt"));
        assert!(!load_with_report(one.as_bytes()).unwrap().2.truncated());
    }

    #[test]
    fn detects_by_content_not_extension() {
        let mut dna = vec![snapgene::block::HEADER, 0, 0, 0, 14];
        dna.extend_from_slice(snapgene::MAGIC);
        dna.extend_from_slice(&[0, 1, 0, 15, 0, 19]);
        assert_eq!(detect(&dna), Some(Format::SnapGene));

        assert_eq!(detect(b">seq\nACGT\n"), Some(Format::Fasta));
        assert_eq!(
            detect(b"LOCUS       x   10 bp DNA linear SYN 01-JAN-2026\n"),
            Some(Format::GenBank)
        );
        assert_eq!(detect(b"ABIF\x00\x01"), Some(Format::Abif));
        assert_eq!(detect(b".scf\x00"), Some(Format::Scf));
        assert_eq!(detect(&[0xAE, b'Z', b'T', b'R']), Some(Format::Ztr));
        assert_eq!(detect(b"random bytes"), None);
        assert_eq!(detect(b""), None);
    }

    #[test]
    fn chromatograms_get_a_useful_error_not_a_generic_one() {
        let e = load(b"ABIF\x00\x01\x02\x03").unwrap_err();
        assert!(e.to_string().contains("ABIF"), "got: {e}");
    }

    #[test]
    fn a_genbank_file_without_a_locus_line_is_still_recognised() {
        // SnapGene writes a LOCUS line Biopython rejects; be liberal on read.
        assert_eq!(
            detect(b"LOCUS       Annotations   19-MAR-2018\nORIGIN\n//\n"),
            Some(Format::GenBank)
        );
    }
}
