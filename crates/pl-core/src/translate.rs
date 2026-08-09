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
//! Tables 1, 2 and 11 were checked codon by codon against Biopython 1.87 —
//! all 64 amino acids and all 64 start flags, zero mismatches — and *because*
//! they agreed, the remaining 24 were generated from the same source rather
//! than typed out. All 27 are now compared against Biopython on every run by
//! `reference/python/tests/xcheck_translate.py`, which is wired into the gate
//! and was verified to fail: flipping one residue in table 24 is caught and
//! named exactly. `the_tables_match_their_published_values` below still pins
//! the three literals, so the tables do not depend on Python being installed.
//!
//! An earlier version of this comment cited a cross-check that did not exist,
//! which left TABLE2 and TABLE11's alternative starts pinned by nothing but a
//! length check. The script named above is real; if it is ever removed, this
//! paragraph is a lie again.
//!
//! # A codon can be both a stop and an amino acid
//!
//! In tables 27, 28 and 31, termination is context-dependent, and NCBI records
//! that by putting the residue in the AAs line and the stop in the Starts line.
//! So [`Code::is_stop`] reads the *Starts* line — the amino-acid line is wrong
//! for those three, and it was what this module used at first.

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
    name: &'static str,
}

/// Every NCBI genetic code, in the compact form NCBI publishes them in.
///
/// All 27, generated from Biopython's `Bio.Data.CodonTable` rather than typed
/// out — and the three that were already here (1, 2 and 11) were checked
/// against it first and agreed exactly, which is what made generating the rest
/// from the same place reasonable. A mistyped amino acid in a table nobody
/// uses often is invisible until the day someone translates a mitochondrial
/// construct.
///
/// **13 of the 27 do not treat `TGA` as a stop.** Honouring a GenBank
/// `/transl_table=` qualifier is therefore not a nicety: with the wrong table a
/// protein reads through its own stop codon, or ends early, and the translation
/// looks perfectly plausible either way.
const TABLES: &[(u8, &str, &str, &str)] = &[
    (1, "FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "---M------**--*----M---------------M----------------------------", "Standard"),
    (2, "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSS**VVVVAAAADDEEGGGG", "----------**--------------------MMMM----------**---M------------", "Vertebrate Mitochondrial"),
    (3, "FFLLSSSSYY**CCWWTTTTPPPPHHQQRRRRIIMMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "----------**----------------------MM---------------M------------", "Yeast Mitochondrial"),
    (4, "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "--MM------**-------M------------MMMM---------------M------------", "Mold Mitochondrial, Protozoan Mitochondrial, Coelenterate Mitochondrial, Mycoplasma, Spiroplasma"),
    (5, "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSSSSVVVVAAAADDEEGGGG", "---M------**--------------------MMMM---------------M------------", "Invertebrate Mitochondrial"),
    (6, "FFLLSSSSYYQQCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "--------------*--------------------M----------------------------", "Ciliate Nuclear, Dasycladacean Nuclear, Hexamita Nuclear"),
    (9, "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNNKSSSSVVVVAAAADDEEGGGG", "----------**-----------------------M---------------M------------", "Echinoderm Mitochondrial, Flatworm Mitochondrial"),
    (10, "FFLLSSSSYY**CCCWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "----------**-----------------------M----------------------------", "Euplotid Nuclear"),
    (11, "FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "---M------**--*----M------------MMMM---------------M------------", "Bacterial, Archaeal, Plant Plastid"),
    (12, "FFLLSSSSYY**CC*WLLLSPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "----------**--*----M---------------M----------------------------", "Alternative Yeast Nuclear"),
    (13, "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSSGGVVVVAAAADDEEGGGG", "---M------**----------------------MM---------------M------------", "Ascidian Mitochondrial"),
    (14, "FFLLSSSSYYY*CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNNKSSSSVVVVAAAADDEEGGGG", "-----------*-----------------------M----------------------------", "Alternative Flatworm Mitochondrial"),
    (15, "FFLLSSSSYY*QCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "----------*---*--------------------M----------------------------", "Blepharisma Macronuclear"),
    (16, "FFLLSSSSYY*LCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "----------*---*--------------------M----------------------------", "Chlorophycean Mitochondrial"),
    (21, "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSSSSVVVVAAAADDEEGGGG", "----------**-----------------------M---------------M------------", "Trematode Mitochondrial"),
    (22, "FFLLSS*SYY*LCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "------*---*---*--------------------M----------------------------", "Scenedesmus obliquus Mitochondrial"),
    (23, "FF*LSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "--*-------**--*-----------------M--M---------------M------------", "Thraustochytrium Mitochondrial"),
    (24, "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSSKVVVVAAAADDEEGGGG", "---M------**-------M---------------M---------------M------------", "Pterobranchia Mitochondrial"),
    (25, "FFLLSSSSYY**CCGWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "---M------**-----------------------M---------------M------------", "Candidate Division SR1, Gracilibacteria"),
    (26, "FFLLSSSSYY**CC*WLLLAPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "----------**--*----M---------------M----------------------------", "Pachysolen tannophilus Nuclear"),
    (27, "FFLLSSSSYYQQCCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "--------------*--------------------M----------------------------", "Karyorelict Nuclear"),
    (28, "FFLLSSSSYYQQCCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "----------**--*--------------------M----------------------------", "Condylostoma Nuclear"),
    (29, "FFLLSSSSYYYYCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "--------------*--------------------M----------------------------", "Mesodinium Nuclear"),
    (30, "FFLLSSSSYYEECC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "--------------*--------------------M----------------------------", "Peritrich Nuclear"),
    (31, "FFLLSSSSYYEECCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "----------**-----------------------M----------------------------", "Blastocrithidia Nuclear"),
    (32, "FFLLSSSSYY*WCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG", "---M------*---*----M------------MMMM---------------M------------", "Balanophoraceae Plastid"),
    (33, "FFLLSSSSYYY*CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSSKVVVVAAAADDEEGGGG", "---M-------*-------M---------------M---------------M------------", "Cephalodiscidae Mitochondrial"),
];

/// Table 1 — the standard code.
pub const TABLE1: Code = Code {
    id: 1,
    aas: STANDARD,
    starts: STANDARD_STARTS,
    name: "Standard",
};

/// Table 11 — bacterial, archaeal and plant plastid. Identical amino acids to
/// table 1; it differs only in permitting more initiation codons, which matters
/// for exactly the bacterial markers plasmids are full of.
pub const TABLE11: Code = Code {
    id: 11,
    aas: STANDARD,
    starts: "---M------**--*----M------------MMMM---------------M------------",
    name: "Bacterial, Archaeal, Plant Plastid",
};

/// Table 2 — vertebrate mitochondrial. Present so mitochondrial constructs are
/// not silently mistranslated.
pub const TABLE2: Code = Code {
    id: 2,
    aas: "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSS**VVVVAAAADDEEGGGG",
    starts: "----------**--------------------MMMM----------**---M------------",
    name: "Vertebrate Mitochondrial",
};

/// Look up a code by its GenBank `transl_table` number.
pub fn table(id: u8) -> Option<Code> {
    TABLES
        .iter()
        .find(|(i, ..)| *i == id)
        .map(|&(id, aas, starts, name)| Code {
            id,
            aas,
            starts,
            name,
        })
}

/// Every code, in NCBI order.
pub fn all_tables() -> impl Iterator<Item = Code> {
    TABLES.iter().map(|&(id, aas, starts, name)| Code {
        id,
        aas,
        starts,
        name,
    })
}

/// The index of a codon in the NCBI ordering, or `None` if any base is not
/// `A`, `C`, `G`, `T` or `U`, in either case.
///
/// The fast path for a codon that is already fully determined. Ambiguity codes
/// are resolved one level up, in [`codon_resolutions`], under the rule that a
/// codon means whatever every base it allows agrees on.
///
/// This used to carry the opposite rationale — "ambiguity codes deliberately do
/// not resolve here even when they could (`GGN` is unambiguously glycine) …
/// quietly inventing an amino acid from an ambiguous codon would manufacture
/// agreement that the sequence does not support". That rule was reversed
/// deliberately, because it made `is_stop` read straight through `TAR` and an
/// ORF's own protein disagree with where the ORF ended; see
/// `ambiguity_resolves_only_when_every_reading_agrees` below. `GGN` is now
/// glycine, and this function is not where that happens.
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

/// Every concrete codon an ambiguous codon stands for, as NCBI indices.
///
/// Empty when any position is not a nucleotide code at all. `TAR` expands to
/// `TAA` and `TAG`; `NNN` to all 64.
///
/// This exists because a codon carrying an ambiguity code still has a
/// determinate meaning whenever every base it allows agrees. `TAR` terminates
/// under both A and G, so it terminates — and treating it as "not a stop"
/// because it is not one of the 64 literal codons made ORF finding read
/// straight through it.
fn codon_resolutions(codon: &[u8]) -> Vec<usize> {
    if codon.len() != 3 {
        return Vec::new();
    }
    // A concrete codon is its own single resolution, and this keeps the common
    // case on the same code path it has always used.
    if let Some(i) = codon_index(codon) {
        return vec![i];
    }
    // `code_mask`'s bits are A, C, G, T in that order; the NCBI slot for each
    // is looked up in `ORDER` rather than written out, for the reason this
    // module's own header gives — a second hand-written copy of the ordering is
    // a second thing that can be silently wrong.
    const MASK_BITS: [u8; 4] = *b"ACGT";
    let mut out = vec![0usize];
    for &b in codon {
        let mask = iupac::code_mask(b);
        if mask == 0 {
            return Vec::new();
        }
        let mut next = Vec::with_capacity(out.len() * 2);
        for (bit, &base) in MASK_BITS.iter().enumerate() {
            if mask & (1 << bit) == 0 {
                continue;
            }
            let slot = ORDER
                .iter()
                .position(|&o| o == base)
                .expect("ORDER covers A, C, G and T");
            for &acc in &out {
                next.push(acc * 4 + slot);
            }
        }
        out = next;
    }
    out.sort_unstable();
    out
}

impl Code {
    /// NCBI's name for this code, for a UI that has to say which one it used.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The 64 amino acids, in NCBI's codon order (`TCAG` on each position).
    pub fn amino_acids(&self) -> &'static str {
        self.aas
    }

    /// `M` where a codon may initiate, `-` where it may not, same order.
    pub fn start_codons(&self) -> &'static str {
        self.starts
    }

    /// Translate one codon. `None` becomes `X`, so the output length is always
    /// `seq.len() / 3` and a caller can index into it without special cases.
    /// An ambiguous codon translates whenever every base it allows gives the
    /// same residue — `CTN` is leucine under all four, and GenBank and
    /// Biopython both translate it that way. Otherwise `X`, which is the honest
    /// answer for a position that could be two different amino acids.
    ///
    /// Without this, a codon `is_stop` calls a terminator would translate to
    /// `X`, so an ORF's own protein would disagree with where the ORF ended.
    pub fn codon(&self, codon: &[u8]) -> u8 {
        let r = codon_resolutions(codon);
        match r.first() {
            None => b'X',
            Some(&first) => {
                let aa = self.aas.as_bytes()[first];
                if r.iter().all(|&i| self.aas.as_bytes()[i] == aa) {
                    aa
                } else {
                    b'X'
                }
            }
        }
    }

    /// Is this a valid initiation codon for this code?
    /// Same all-resolutions rule as [`Code::is_stop`]: an ambiguous codon
    /// initiates only if every base it allows initiates. `ATG` does; `ATN` does
    /// not, since `ATC` is isoleucine in table 1.
    pub fn is_start(&self, codon: &[u8]) -> bool {
        let r = codon_resolutions(codon);
        !r.is_empty() && r.iter().all(|&i| self.starts.as_bytes()[i] == b'M')
    }

    /// Does this codon terminate translation?
    ///
    /// Read off the *Starts* line, not the amino-acid line, because the two
    /// disagree in three tables and only the Starts line is right in all 27.
    /// In the Karyorelict (27), Condylostoma (28) and Blastocrithidia (31)
    /// nuclear codes a codon is **both** a stop and an amino acid; NCBI writes
    /// the residue in the AAs line and marks termination in the Starts line, so
    /// `TGA` in table 27 is `W` *and* a terminator. Asking the amino-acid line
    /// whether something is a stop gets all three of those codes wrong, in
    /// opposite directions depending on which line you trust.
    /// An ambiguous codon terminates only if it terminates under **every** base
    /// it allows: `TAR` is `TAA` or `TAG`, both stops in table 1, so it is a
    /// stop. `TAN` is not, because `TAC` is tyrosine. Reading it as "not a
    /// stop" because it is not one of the 64 literal codons made `find_orfs`
    /// run straight through a codon that terminates under every reading of it.
    pub fn is_stop(&self, codon: &[u8]) -> bool {
        let r = codon_resolutions(codon);
        !r.is_empty() && r.iter().all(|&i| self.starts.as_bytes()[i] == b'*')
    }

    /// A codon that both terminates and encodes a residue.
    ///
    /// True for six codons across tables 27, 28 and 31, and nowhere else.
    /// Whether one of them actually stops translation depends on context this
    /// crate does not have — in these organisms, on how close the poly(A) tail
    /// is — so an ORF that ends at one is a guess. Callers that care should say
    /// so rather than present the boundary as settled.
    pub fn is_ambiguous_stop(&self, codon: &[u8]) -> bool {
        let r = codon_resolutions(codon);
        !r.is_empty()
            && r.iter().all(|&i| self.starts.as_bytes()[i] == b'*')
            && r.iter().all(|&i| self.aas.as_bytes()[i] != b'*')
    }

    /// Translate a sequence in frame 0. A trailing partial codon is dropped.
    ///
    /// Stops are emitted as `*` rather than truncating the output — the caller
    /// decides what a stop means, because for auto-annotation an internal stop
    /// is evidence about a *frame*, not a reason to stop reading.
    pub fn translate(&self, seq: &[u8]) -> Vec<u8> {
        seq.chunks_exact(3).map(|c| self.codon(c)).collect()
    }

    /// Translate a span that is known to *begin at an initiator*, the way
    /// GenBank writes `/translation`: the first residue is `M` whatever the
    /// codon spells.
    ///
    /// [`Code::translate`] is a raw per-codon primitive and is deliberately so —
    /// it has no idea whether position 0 is an initiation site. This is the
    /// wrapper for callers that do know, and it is a different answer, not a
    /// cosmetic one: `tet(A)` starts `GTG`, and table 11 — the default for
    /// plasmid work, chosen precisely so GTG- and TTG-started bacterial markers
    /// are found — translates that codon as valine. Printing `VKPNIPLI...` for a
    /// protein whose reference is `MKPNIPLI...` gives anyone who pastes it into
    /// BLAST a leading mismatch.
    ///
    /// The substitution is conditioned on [`Code::is_start`] rather than applied
    /// to whatever sits first, because a stop-to-stop reading (`require_start`
    /// off) hands over spans whose first codon is an ordinary sense codon, and
    /// calling one of those Met would be an invention.
    pub fn translate_cds(&self, seq: &[u8]) -> Vec<u8> {
        let mut out = self.translate(seq);
        if seq.len() >= 3 && self.is_start(&seq[..3]) {
            if let Some(first) = out.first_mut() {
                *first = b'M';
            }
        }
        out
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
            //
            // `saturating_sub`: now that all six frames are always produced, a
            // frame with `offset > len` is constructible (a 1 bp sequence has a
            // frame at offset 2), and `len - start_in_frame` underflowed on it.
            // Such a frame has no residues, so this is unreachable through
            // `protein`, but the method is public and must not panic.
            let end = len.saturating_sub(start_in_frame);
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
            // Always six. The guard here used to `continue`, so a 0 bp input
            // got two frames and a 1 bp input four — from a function called
            // `six_frames` whose documentation promises all six. A frame that
            // starts past the end is simply empty, which is the honest answer;
            // skipping it silently changes the shape of the result.
            let start = offset.min(src.len());
            out.push(Frame {
                offset,
                reverse,
                protein: code.translate(&src[start..]),
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
    fn the_named_constants_are_the_same_tables_the_lookup_returns() {
        // TABLE1/2/11 are spelled out separately for callers that want one
        // without a lookup, so they can drift from the generated array —
        // silently, and only for whoever used the constant.
        for (named, id) in [(TABLE1, 1u8), (TABLE2, 2), (TABLE11, 11)] {
            let looked_up = table(id).expect("a shipped table");
            assert_eq!(named.aas, looked_up.aas, "table {id} amino acids");
            assert_eq!(named.starts, looked_up.starts, "table {id} starts");
            assert_eq!(named.id, looked_up.id);
            assert_eq!(named.name, looked_up.name, "table {id} name");
        }
    }

    #[test]
    fn a_codon_can_be_both_a_stop_and_an_amino_acid() {
        // Tables 27, 28 and 31 encode context-dependent termination: NCBI puts
        // the residue in the AAs line and the stop in the Starts line. Reading
        // stops off the AAs line — which this module used to do — makes those
        // three codes translate through their own stop codons.
        let t27 = table(27).expect("Karyorelict Nuclear");
        assert!(t27.is_stop(b"TGA"), "it terminates");
        assert_eq!(t27.codon(b"TGA"), b'W', "and it is tryptophan");
        assert!(t27.is_ambiguous_stop(b"TGA"));

        let ambiguous: Vec<(u8, usize)> = all_tables()
            .map(|c| {
                (
                    c.id,
                    [b"TAA", b"TAG", b"TGA"]
                        .iter()
                        .filter(|x| c.is_ambiguous_stop(**x))
                        .count(),
                )
            })
            .filter(|(_, n)| *n > 0)
            .collect();
        assert_eq!(ambiguous, vec![(27, 1), (28, 3), (31, 2)]);

        // Everywhere else the two lines agree, so nothing else changes.
        for c in all_tables() {
            for codon in [b"TAA", b"TAG", b"TGA"] {
                if !c.is_ambiguous_stop(codon) {
                    assert_eq!(
                        c.is_stop(codon),
                        c.codon(codon) == b'*',
                        "table {} disagrees with itself about {}",
                        c.id,
                        String::from_utf8_lossy(codon)
                    );
                }
            }
        }
    }

    #[test]
    fn the_tables_match_their_published_values() {
        // The tables are the whole module, and a silent character change in one
        // would mistranslate quietly rather than fail loudly. Pinned here as
        // exact literals, transcribed from the NCBI genetic code tables and
        // confirmed against Biopython 1.87.
        assert_eq!(
            TABLE1.aas,
            "FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG"
        );
        assert_eq!(
            TABLE1.starts,
            "---M------**--*----M---------------M----------------------------"
        );
        // Table 11 is table 1's amino acids with more initiation codons.
        assert_eq!(TABLE11.aas, TABLE1.aas);
        assert_eq!(
            TABLE11.starts,
            "---M------**--*----M------------MMMM---------------M------------"
        );
        assert_eq!(
            TABLE2.aas,
            "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSS**VVVVAAAADDEEGGGG"
        );
        assert_eq!(
            TABLE2.starts,
            "----------**--------------------MMMM----------**---M------------"
        );

        // Landmarks that would survive a shifted block but not a wrong table.
        assert_eq!(TABLE2.codon(b"AGA"), b'*', "vertebrate mito: AGA is a stop");
        assert_eq!(TABLE2.codon(b"TGA"), b'W', "vertebrate mito: TGA is Trp");
        assert_eq!(TABLE2.codon(b"ATA"), b'M', "vertebrate mito: ATA is Met");
        // Table 1 already initiates at TTG, CTG and ATG; table 11 adds the
        // four below. Getting that split wrong in either direction is exactly
        // what a shifted table looks like.
        for c in [b"TTG", b"CTG", b"ATG"] {
            assert!(TABLE1.is_start(c), "table 1 should initiate at {c:?}");
            assert!(TABLE11.is_start(c));
        }
        for c in [b"ATT", b"ATC", b"ATA", b"GTG"] {
            assert!(TABLE11.is_start(c), "table 11 should initiate at {c:?}");
            assert!(!TABLE1.is_start(c), "table 1 should not initiate at {c:?}");
        }
        // Exactly three starts in table 1, seven in table 11.
        let count = |code: Code| code.starts.bytes().filter(|&b| b == b'M').count();
        assert_eq!(count(TABLE1), 3);
        assert_eq!(count(TABLE11), 7);
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
    fn ambiguity_resolves_only_when_every_reading_agrees() {
        // **This reverses an earlier decision, deliberately.** The previous rule
        // was "GGN is unambiguously glycine, and we still refuse", on the same
        // instinct as `iupac::matches` — an unknown base is not evidence.
        //
        // That instinct is right about *evidence* and wrong here. `matches`
        // refuses to let an unknown subject satisfy a pattern, which is a claim
        // about missing information. `GGN` is not missing information about the
        // residue: every base N allows gives glycine, so glycine is a deduction
        // rather than a guess. GenBank and Biopython both translate it that way.
        //
        // What forced the change is `is_stop`. `TAR` terminates under both A and
        // G, so it must be a stop or `find_orfs` runs straight through it — and
        // if it is a stop while `codon` returns `X`, an ORF's own protein
        // disagrees with the boundary the same table just drew.
        assert_eq!(TABLE1.codon(b"GGN"), b'G');
        assert_eq!(TABLE1.codon(b"CTN"), b'L');
        assert_eq!(TABLE1.codon(b"TAR"), b'*', "TAA and TAG are both stops");

        // Still `X` wherever the readings genuinely disagree, or the input is
        // not a codon at all.
        assert_eq!(TABLE1.codon(b"NNN"), b'X');
        assert_eq!(TABLE1.codon(b"TAN"), b'X', "TAC is tyrosine, TAA is a stop");
        assert_eq!(TABLE1.codon(b"AT"), b'X');
        assert_eq!(TABLE1.codon(b"AT-"), b'X');
    }

    #[test]
    fn codon_index_documents_the_rule_this_module_now_follows() {
        // The companion to the test above, for the doc comment rather than the
        // behaviour. `codon_index`'s `///` block kept the *rejected* rule's
        // justification — "quietly inventing an amino acid from an ambiguous
        // codon would manufacture agreement that the sequence does not support"
        // — after the reversal above, and it is the entry point to the whole
        // lookup: a maintainer reading it concludes the module refuses to
        // resolve ambiguity, which makes `TAR`-as-a-stop look like a bug to be
        // "fixed" back into `find_orfs` running straight through it. Asserted
        // rather than eyeballed, because nothing else in the build reads doc
        // comments.
        //
        // The rationale is allowed to survive as *history*, which is how the
        // reversal is explained; what it may not do is stand as the current
        // rule.
        assert_eq!(TABLE1.codon(b"GGN"), b'G', "the rule the doc must describe");

        let src = include_str!("translate.rs");
        let head = src
            .find("/// The index of a codon in the NCBI ordering")
            .expect("codon_index still describes itself");
        let item = src[head..]
            .find("fn codon_index(")
            .expect("codon_index is still here")
            + head;
        let doc = &src[head..item];
        assert!(
            doc.contains("[`codon_resolutions`]"),
            "the doc has to say where ambiguity IS resolved: {doc}"
        );
        if let Some(stale) = doc.find("deliberately do not resolve here") {
            let history = doc
                .find("used to carry the opposite rationale")
                .expect("the rejected rule may only appear as history");
            assert!(
                history < stale,
                "the rejected rule must be marked as the past, not stated as the rule"
            );
        }

        // The same block's alphabet claim. `None` "if any base is not a plain
        // `ACGT`" was false: the lookup folds case and maps U to T first, which
        // is what makes `rna_translates_as_dna_does` pass.
        assert_eq!(codon_index(b"AUG"), codon_index(b"ATG"));
        assert_eq!(codon_index(b"aug"), codon_index(b"ATG"));
        assert!(codon_index(b"ATG").is_some());
        assert!(
            doc.contains("`U`") && doc.contains("in either case"),
            "the accepted alphabet is A, C, G, T or U, in either case: {doc}"
        );
    }

    #[test]
    fn an_ambiguous_codon_stops_only_if_every_reading_stops() {
        // The bug this was found by: `find_orfs` uses `is_stop` as its only
        // terminator test, so an ORF read straight through `TAR` — a codon that
        // terminates under both bases it allows — and ran on to the next
        // literal stop, reporting a protein that does not exist.
        assert!(TABLE1.is_stop(b"TAR"), "TAA and TAG");
        assert!(TABLE1.is_stop(b"TRA"), "TAA and TGA");
        assert!(!TABLE1.is_stop(b"TAN"), "TAC and TAT are tyrosine");
        assert!(!TABLE1.is_stop(b"NNN"));
        assert!(!TABLE1.is_stop(b"TA-"), "not a codon at all");

        // The same rule for initiation, so `ATN` does not start an ORF.
        assert!(TABLE1.is_start(b"ATG"));
        assert!(!TABLE1.is_start(b"ATN"), "ATC and ATT are isoleucine");
        assert!(
            TABLE11.is_start(b"NTG"),
            "GTG, TTG, CTG and ATG all initiate"
        );
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
    fn six_frames_are_six_at_every_length() {
        // A function called `six_frames` returned two frames for a 0 bp input
        // and four for a 1 bp one, because the guard `continue`d instead of
        // clamping. A frame starting past the end is empty, which is the
        // honest answer; omitting it changes the shape of the result.
        for n in 0..8 {
            let seq: Vec<u8> = std::iter::repeat_n(b'A', n).collect();
            let f = six_frames(&seq, TABLE1);
            assert_eq!(f.len(), 6, "{n} bp gave {} frames", f.len());
            assert_eq!(f.iter().filter(|x| x.reverse).count(), 3);
            for (i, fr) in f.iter().enumerate() {
                assert_eq!(fr.offset, i % 3);
            }
            // ...and mapping back never panics, even for a frame that starts
            // past the end.
            for fr in &f {
                let _ = fr.to_source(0, n);
                let _ = fr.to_source(5, n);
            }
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

    #[test]
    fn an_alternative_initiator_translates_as_met_the_way_genbank_writes_it() {
        // `tet(A)` starts GTG, and table 11 is the default here precisely so
        // GTG- and TTG-started bacterial markers are found at all. Reporting
        // that first residue as valine gives anyone pasting the protein into
        // BLAST a leading mismatch against the reference — the project's own
        // features.tsv stores `MKPNIPLI...` for PLF:0006, not `VKPNIPLI...`.
        let cds = format!("GTG{}TAA", "GCC".repeat(20));
        let got = TABLE11.translate_cds(cds.as_bytes());
        assert_eq!(got[0], b'M', "{}", String::from_utf8_lossy(&got));
        assert_eq!(
            String::from_utf8_lossy(&got),
            format!("M{}*", "A".repeat(20))
        );

        // The raw primitive is unchanged: it does not know whether position 0
        // is an initiation site, and it is not this wrapper's job to teach it.
        assert_eq!(TABLE11.translate(cds.as_bytes())[0], b'V');

        // Only the FIRST codon. An internal GTG is a valine like any other.
        let internal = format!("ATGGTG{}TAA", "GCC".repeat(5));
        let got = TABLE11.translate_cds(internal.as_bytes());
        assert_eq!((got[0], got[1]), (b'M', b'V'));

        // And a span that does not begin at an initiator is left alone, which
        // is what a stop-to-stop reading (`require_start` off) hands over.
        let not_a_start = format!("CCC{}TAA", "GCC".repeat(5));
        assert!(!TABLE11.is_start(b"CCC"));
        assert_eq!(TABLE11.translate_cds(not_a_start.as_bytes())[0], b'P');
    }
}
