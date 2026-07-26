//! Translation, for the protein half of auto-annotation.
//!
//! `docs/PLAN.md` §7.7 step 5: a codon-optimised resistance marker shares almost
//! no nucleotide identity with the version in the database, but its protein is
//! unchanged. Six-frame translation is what makes those resolve, and it is the
//! single biggest reason the annotator finds things a nucleotide-only matcher
//! misses.
//!
//! # Why the tables are stored as NCBI's four-line form
//!
//! Sixty-four hand-written codon entries is exactly the kind of recalled
//! constant that this project has already been bitten by more than once. The
//! compact form below is copied as a single string from the NCBI genetic code
//! specification, and its index is *derived* rather than written out, so a
//! transcription slip shows up as a whole shifted block rather than one wrong
//! amino acid. It is also diffable against the NCBI page by eye.
//!
//! `reference/python/tests/xcheck_translate.py` checks every codon of every
//! table implemented here against Biopython, which is the actual guarantee.

use crate::iupac;

/// Bases in NCBI's table order. The index of a codon is
/// `16*b1 + 4*b2 + b3` over this alphabet.
const ORDER: [u8; 4] = *b"TCAG";

/// NCBI translation table 1 — the standard genetic code.
///
/// <https://www.ncbi.nlm.nih.gov/Taxonomy/Utils/wprintgc.cgi>
pub const STANDARD: &str = "FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG";

/// Which codons may start a CDS in table 1, same ordering as [`STANDARD`].
///
/// Kept because §7.7 allows "1–2 missing terminal codons" for fusion
/// constructs, which needs to know what a plausible start looks like — `GTG`
/// and `TTG` are genuine bacterial starts and appear in real markers.
pub const STANDARD_STARTS: &str =
    "---M------**--*----M---------------M----------------------------";

/// A genetic code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Code {
    /// NCBI transl_table number, as it appears in GenBank.
    pub id: u8,
    aas: &'static str,
    starts: &'static str,
}

/// Table 1. The only one auto-annotation uses; the others exist so that a
/// GenBank `/transl_table=` qualifier can be honoured rather than ignored.
pub const TABLE1: Code = Code {
    id: 1,
    aas: STANDARD,
    starts: STANDARD_STARTS,
};

/// Table 11 — bacterial, archaeal and plant plastid. Identical amino acids to
/// table 1; it differs only in permitting more initiation codons, which matters
/// for exactly the bacterial markers plasmids are full of.
pub const TABLE11: Code = Code {
    id: 11,
    aas: STANDARD,
    starts: "---M------**--*----M------------MMMM---------------M------------",
};

/// Table 2 — vertebrate mitochondrial. Present so mitochondrial constructs are
/// not silently mistranslated.
pub const TABLE2: Code = Code {
    id: 2,
    aas: "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSS**VVVVAAAADDEEGGGG",
    starts: "----------**--------------------MMMM----------**---M------------",
};

/// Look up a code by its GenBank `transl_table` number.
pub fn table(id: u8) -> Option<Code> {
    match id {
        1 => Some(TABLE1),
        2 => Some(TABLE2),
        11 => Some(TABLE11),
        _ => None,
    }
}

/// The index of a codon in the NCBI ordering, or `None` if any base is not a
/// plain `ACGT`.
///
/// Ambiguity codes deliberately do not resolve here even when they could (`GGN`
/// is unambiguously glycine). Auto-annotation compares translated frames for
/// equality, and quietly inventing an amino acid from an ambiguous codon would
/// manufacture agreement that the sequence does not support.
fn codon_index(codon: &[u8]) -> Option<usize> {
    if codon.len() != 3 {
        return None;
    }
    let mut idx = 0usize;
    for &b in codon {
        let up = b.to_ascii_uppercase();
        let up = if up == b'U' { b'T' } else { up };
        let pos = ORDER.iter().position(|&o| o == up)?;
        idx = idx * 4 + pos;
    }
    Some(idx)
}

impl Code {
    /// Translate one codon. `None` becomes `X`, so the output length is always
    /// `seq.len() / 3` and a caller can index into it without special cases.
    pub fn codon(&self, codon: &[u8]) -> u8 {
        match codon_index(codon) {
            Some(i) => self.aas.as_bytes()[i],
            None => b'X',
        }
    }

    /// Is this a valid initiation codon for this code?
    pub fn is_start(&self, codon: &[u8]) -> bool {
        match codon_index(codon) {
            Some(i) => self.starts.as_bytes()[i] == b'M',
            None => false,
        }
    }

    pub fn is_stop(&self, codon: &[u8]) -> bool {
        self.codon(codon) == b'*'
    }

    /// Translate a sequence in frame 0. A trailing partial codon is dropped.
    ///
    /// Stops are emitted as `*` rather than truncating the output — the caller
    /// decides what a stop means, because for auto-annotation an internal stop
    /// is evidence about a *frame*, not a reason to stop reading.
    pub fn translate(&self, seq: &[u8]) -> Vec<u8> {
        seq.chunks_exact(3).map(|c| self.codon(c)).collect()
    }
}

/// One reading frame of a molecule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// 0, 1 or 2 — the offset this frame starts at.
    pub offset: usize,
    /// `false` for the given strand, `true` for its reverse complement.
    pub reverse: bool,
    pub protein: Vec<u8>,
}

impl Frame {
    /// Map an index in `protein` back to the base offset in the *original*
    /// forward sequence of length `len`.
    ///
    /// The reverse frames are the fiddly half, and getting this wrong places
    /// every protein hit on the wrong strand at the wrong coordinate — so it is
    /// tested directly rather than only through the annotator.
    pub fn to_source(&self, aa_index: usize, len: usize) -> (usize, usize) {
        let start_in_frame = self.offset + aa_index * 3;
        if self.reverse {
            // The frame runs along the reverse complement, so a residue that
            // begins at `start_in_frame` there ends at the mirrored position.
            let end = len - start_in_frame;
            (end.saturating_sub(3), end)
        } else {
            (start_in_frame, start_in_frame + 3)
        }
    }
}

/// All six reading frames of `seq`.
pub fn six_frames(seq: &[u8], code: Code) -> Vec<Frame> {
    let rc = iupac::reverse_complement(seq);
    let mut out = Vec::with_capacity(6);
    for reverse in [false, true] {
        let src: &[u8] = if reverse { &rc } else { seq };
        for offset in 0..3 {
            if src.len() < offset {
                continue;
            }
            out.push(Frame {
                offset,
                reverse,
                protein: code.translate(&src[offset..]),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_the_right_shape() {
        // A truncated or padded table string would silently shift every codon
        // after the damage, so assert the length before trusting any lookup.
        assert_eq!(STANDARD.len(), 64);
        assert_eq!(STANDARD_STARTS.len(), 64);
        assert_eq!(TABLE2.aas.len(), 64);
        assert_eq!(TABLE2.starts.len(), 64);
        assert_eq!(TABLE11.starts.len(), 64);
    }

    #[test]
    fn codon_index_follows_ncbi_ordering() {
        // Spot-check the derivation itself: T,C,A,G with the third base fastest.
        assert_eq!(codon_index(b"TTT"), Some(0));
        assert_eq!(codon_index(b"TTC"), Some(1));
        assert_eq!(codon_index(b"TTA"), Some(2));
        assert_eq!(codon_index(b"TTG"), Some(3));
        assert_eq!(codon_index(b"TCT"), Some(4));
        assert_eq!(codon_index(b"GGG"), Some(63));
        assert_eq!(codon_index(b"ATG"), Some(35));
    }

    #[test]
    fn the_landmarks_of_the_standard_code() {
        let c = TABLE1;
        assert_eq!(c.codon(b"ATG"), b'M');
        assert_eq!(c.codon(b"TGG"), b'W'); // the only Trp codon
        assert_eq!(c.codon(b"TAA"), b'*');
        assert_eq!(c.codon(b"TAG"), b'*');
        assert_eq!(c.codon(b"TGA"), b'*');
        assert_eq!(c.codon(b"GGG"), b'G');
        assert_eq!(c.codon(b"AAA"), b'K');
        assert!(c.is_start(b"ATG"));
        assert!(!c.is_start(b"AAA"));
    }

    #[test]
    fn every_codon_of_the_standard_code_is_assigned() {
        // 61 sense codons and 3 stops, no gaps, no X.
        let mut stops = 0;
        for &b1 in ORDER.iter() {
            for &b2 in ORDER.iter() {
                for &b3 in ORDER.iter() {
                    let aa = TABLE1.codon(&[b1, b2, b3]);
                    assert!(
                        aa.is_ascii_uppercase() || aa == b'*',
                        "{}{}{} gave {}",
                        b1 as char,
                        b2 as char,
                        b3 as char,
                        aa as char
                    );
                    assert_ne!(aa, b'X');
                    if aa == b'*' {
                        stops += 1;
                    }
                }
            }
        }
        assert_eq!(stops, 3, "the standard code has exactly three stop codons");
    }

    #[test]
    fn table_11_differs_from_table_1_only_in_starts() {
        assert_eq!(TABLE1.aas, TABLE11.aas);
        assert_ne!(TABLE1.starts, TABLE11.starts);
        // GTG and TTG initiate in bacteria; this is why plasmid markers need it.
        assert!(TABLE11.is_start(b"GTG"));
        assert!(TABLE11.is_start(b"TTG"));
        assert!(!TABLE1.is_start(b"GTG"));
    }

    #[test]
    fn ambiguity_does_not_invent_an_amino_acid() {
        // GGN is unambiguously glycine, and we still refuse. See `codon_index`.
        assert_eq!(TABLE1.codon(b"GGN"), b'X');
        assert_eq!(TABLE1.codon(b"NNN"), b'X');
        assert_eq!(TABLE1.codon(b"AT"), b'X');
    }

    #[test]
    fn rna_translates_as_dna_does() {
        assert_eq!(TABLE1.codon(b"AUG"), b'M');
        assert_eq!(TABLE1.translate(b"AUGUUU"), b"MF".to_vec());
    }

    #[test]
    fn case_is_irrelevant_to_translation() {
        assert_eq!(TABLE1.translate(b"atgttt"), b"MF".to_vec());
        assert_eq!(TABLE1.translate(b"AtGtTt"), b"MF".to_vec());
    }

    #[test]
    fn a_trailing_partial_codon_is_dropped() {
        assert_eq!(TABLE1.translate(b"ATGT"), b"M".to_vec());
        assert_eq!(TABLE1.translate(b"AT"), b"".to_vec());
        assert_eq!(TABLE1.translate(b""), b"".to_vec());
    }

    #[test]
    fn six_frames_are_six_and_cover_both_strands() {
        let f = six_frames(b"ATGAAACCCGGGTTTTAA", TABLE1);
        assert_eq!(f.len(), 6);
        assert_eq!(f[0].protein, b"MKPGF*".to_vec());
        assert_eq!(f.iter().filter(|x| x.reverse).count(), 3);
        for (i, fr) in f.iter().enumerate() {
            assert_eq!(fr.offset, i % 3);
        }
    }

    #[test]
    fn a_frame_maps_back_to_the_coordinates_it_came_from() {
        let seq = b"AAATGAAACCCGGGTTTTAA";
        // Locate the start codon rather than asserting a hand-counted index.
        // Counting is the step that has been wrong before in this project; the
        // code under test has not been.
        let at = seq.windows(3).position(|w| w == b"ATG").unwrap();
        let frames = six_frames(seq, TABLE1);

        // Residue `i` of the frame with offset `at % 3` covers `at`.
        let f = frames
            .iter()
            .find(|x| !x.reverse && x.offset == at % 3)
            .unwrap();
        let aa = at / 3;
        assert_eq!(f.protein[aa], b'M');
        assert_eq!(f.to_source(aa, seq.len()), (at, at + 3));
        assert_eq!(&seq[at..at + 3], b"ATG");

        // And on the reverse strand the mapping has to mirror.
        let rc = iupac::reverse_complement(seq);
        let r = frames.iter().find(|x| x.reverse && x.offset == 0).unwrap();
        let (s, e) = r.to_source(0, seq.len());
        assert_eq!(e - s, 3);
        assert_eq!(&rc[0..3], &iupac::reverse_complement(&seq[s..e])[..]);
    }

    #[test]
    fn translating_a_real_marker_start_looks_like_a_protein() {
        // The first codons of TEM-1 beta-lactamase (AmpR) as they appear in
        // pUC19. Not a memory test: the point is only that frame 0 of a real
        // CDS begins with M and carries no internal stop this early.
        let p = TABLE1.translate(b"ATGAGTATTCAACATTTCCGTGTCGCCCTTATTCC");
        assert_eq!(p[0], b'M');
        assert!(!p[..p.len() - 1].contains(&b'*'));
    }
}
