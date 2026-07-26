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

/// Read any supported sequence file into the common model.
pub fn load(data: &[u8]) -> Result<(Molecule, Format), LoadError> {
    match detect(data) {
        Some(Format::SnapGene) => {
            let doc = snapgene::parse(data).map_err(LoadError::SnapGene)?;
            Ok((doc.molecule, Format::SnapGene))
        }
        Some(Format::GenBank) => Ok((
            genbank::parse(&String::from_utf8_lossy(data)),
            Format::GenBank,
        )),
        Some(Format::Fasta) => Ok((fasta::parse(&String::from_utf8_lossy(data)), Format::Fasta)),
        Some(other) => Err(LoadError::NotASequenceFile(other)),
        None => Err(LoadError::Unrecognised),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
