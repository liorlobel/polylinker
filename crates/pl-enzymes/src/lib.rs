//! Restriction enzymes and digestion.
//!
//! # Cut coordinates
//!
//! A cut is reported as the **1-based position of the base immediately 3' of
//! the nick on the top strand** — the same convention Biopython's
//! `Restriction` module uses. EcoRI on `GAATTC` starting at 1 cuts `G^AATTC`
//! and is reported as 2.
//!
//! Picking someone else's convention on purpose makes cross-validation a
//! straight equality check instead of an argument about off-by-one.
//!
//! # The enzyme table
//!
//! A small set of textbook Type IIP enzymes, transcribed independently from
//! primary literature and catalogue chemistry. No vendor database is
//! reproduced. Real use wants REBASE, which carries its own licence terms and
//! belongs in a separate data package — see `docs/PLAN.md` §8.

use pl_core::{iupac, Molecule, Topology};

/// A Type IIP restriction enzyme: palindromic site, fixed cut offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Enzyme {
    pub name: &'static str,
    /// Recognition site, IUPAC, 5'->3' on the top strand.
    pub site: &'static str,
    /// Bases into the site at which the top strand is cut.
    /// `GAATTC` with offset 1 is `G^AATTC`.
    pub cut_offset: u8,
}

impl Enzyme {
    pub fn len(&self) -> usize {
        self.site.len()
    }
    pub fn is_empty(&self) -> bool {
        self.site.is_empty()
    }
    /// Blunt when the top-strand nick sits at the centre of the site.
    pub fn is_blunt(&self) -> bool {
        self.site.len() % 2 == 0 && self.cut_offset as usize == self.site.len() / 2
    }
}

/// A textbook set of Type IIP enzymes, sorted by name.
pub const ENZYMES: &[Enzyme] = &[
    Enzyme {
        name: "AatII",
        site: "GACGTC",
        cut_offset: 5,
    },
    Enzyme {
        name: "AflII",
        site: "CTTAAG",
        cut_offset: 1,
    },
    Enzyme {
        name: "AgeI",
        site: "ACCGGT",
        cut_offset: 1,
    },
    Enzyme {
        name: "ApaI",
        site: "GGGCCC",
        cut_offset: 5,
    },
    Enzyme {
        name: "AscI",
        site: "GGCGCGCC",
        cut_offset: 2,
    },
    Enzyme {
        name: "AvrII",
        site: "CCTAGG",
        cut_offset: 1,
    },
    Enzyme {
        name: "BamHI",
        site: "GGATCC",
        cut_offset: 1,
    },
    Enzyme {
        name: "BclI",
        site: "TGATCA",
        cut_offset: 1,
    },
    Enzyme {
        name: "BglII",
        site: "AGATCT",
        cut_offset: 1,
    },
    Enzyme {
        name: "BsiWI",
        site: "CGTACG",
        cut_offset: 1,
    },
    Enzyme {
        name: "BspEI",
        site: "TCCGGA",
        cut_offset: 1,
    },
    Enzyme {
        name: "BsrGI",
        site: "TGTACA",
        cut_offset: 1,
    },
    Enzyme {
        name: "BstBI",
        site: "TTCGAA",
        cut_offset: 2,
    },
    Enzyme {
        name: "ClaI",
        site: "ATCGAT",
        cut_offset: 2,
    },
    Enzyme {
        name: "DraI",
        site: "TTTAAA",
        cut_offset: 3,
    },
    Enzyme {
        name: "EagI",
        site: "CGGCCG",
        cut_offset: 1,
    },
    Enzyme {
        name: "EcoRI",
        site: "GAATTC",
        cut_offset: 1,
    },
    Enzyme {
        name: "EcoRV",
        site: "GATATC",
        cut_offset: 3,
    },
    Enzyme {
        name: "FseI",
        site: "GGCCGGCC",
        cut_offset: 6,
    },
    Enzyme {
        name: "HindIII",
        site: "AAGCTT",
        cut_offset: 1,
    },
    Enzyme {
        name: "HpaI",
        site: "GTTAAC",
        cut_offset: 3,
    },
    Enzyme {
        name: "KpnI",
        site: "GGTACC",
        cut_offset: 5,
    },
    Enzyme {
        name: "MfeI",
        site: "CAATTG",
        cut_offset: 1,
    },
    Enzyme {
        name: "MluI",
        site: "ACGCGT",
        cut_offset: 1,
    },
    Enzyme {
        name: "NcoI",
        site: "CCATGG",
        cut_offset: 1,
    },
    Enzyme {
        name: "NdeI",
        site: "CATATG",
        cut_offset: 2,
    },
    Enzyme {
        name: "NheI",
        site: "GCTAGC",
        cut_offset: 1,
    },
    Enzyme {
        name: "NotI",
        site: "GCGGCCGC",
        cut_offset: 2,
    },
    Enzyme {
        name: "NruI",
        site: "TCGCGA",
        cut_offset: 3,
    },
    Enzyme {
        name: "NsiI",
        site: "ATGCAT",
        cut_offset: 5,
    },
    Enzyme {
        name: "PacI",
        site: "TTAATTAA",
        cut_offset: 5,
    },
    Enzyme {
        name: "PmeI",
        site: "GTTTAAAC",
        cut_offset: 4,
    },
    Enzyme {
        name: "PstI",
        site: "CTGCAG",
        cut_offset: 5,
    },
    Enzyme {
        name: "PvuI",
        site: "CGATCG",
        cut_offset: 4,
    },
    Enzyme {
        name: "PvuII",
        site: "CAGCTG",
        cut_offset: 3,
    },
    Enzyme {
        name: "SacI",
        site: "GAGCTC",
        cut_offset: 5,
    },
    Enzyme {
        name: "SacII",
        site: "CCGCGG",
        cut_offset: 4,
    },
    Enzyme {
        name: "SalI",
        site: "GTCGAC",
        cut_offset: 1,
    },
    Enzyme {
        name: "SbfI",
        site: "CCTGCAGG",
        cut_offset: 6,
    },
    Enzyme {
        name: "ScaI",
        site: "AGTACT",
        cut_offset: 3,
    },
    Enzyme {
        name: "SmaI",
        site: "CCCGGG",
        cut_offset: 3,
    },
    Enzyme {
        name: "SnaBI",
        site: "TACGTA",
        cut_offset: 3,
    },
    Enzyme {
        name: "SpeI",
        site: "ACTAGT",
        cut_offset: 1,
    },
    Enzyme {
        name: "SphI",
        site: "GCATGC",
        cut_offset: 5,
    },
    Enzyme {
        name: "SspI",
        site: "AATATT",
        cut_offset: 3,
    },
    Enzyme {
        name: "StuI",
        site: "AGGCCT",
        cut_offset: 3,
    },
    Enzyme {
        name: "SwaI",
        site: "ATTTAAAT",
        cut_offset: 4,
    },
    Enzyme {
        name: "XbaI",
        site: "TCTAGA",
        cut_offset: 1,
    },
    Enzyme {
        name: "XhoI",
        site: "CTCGAG",
        cut_offset: 1,
    },
    Enzyme {
        name: "XmaI",
        site: "CCCGGG",
        cut_offset: 1,
    },
];

pub fn by_name(name: &str) -> Option<&'static Enzyme> {
    ENZYMES.iter().find(|e| e.name.eq_ignore_ascii_case(name))
}

/// Every cut an enzyme makes, as 1-based positions (see module docs).
///
/// On a circular molecule, sites spanning the origin are found and their cut
/// positions wrapped into `1..=n`. Missing that is the classic plasmid bug:
/// a unique cutter is reported as a non-cutter purely because the site
/// happens to straddle base 1.
pub fn cut_positions(seq: &[u8], topology: Topology, enzyme: &Enzyme) -> Vec<u64> {
    let n = seq.len();
    let k = enzyme.site.len();
    if n == 0 || k == 0 || k > n {
        return Vec::new();
    }
    let pattern = enzyme.site.as_bytes();
    let circular = topology.is_circular();

    // Scanning `n` starts on a circle vs `n - k + 1` on a line is the whole
    // of the wraparound handling; indices are taken modulo n.
    let starts = if circular { n } else { n - k + 1 };
    let mut out = Vec::new();
    for i in 0..starts {
        let hit = (0..k).all(|j| {
            let idx = if circular { (i + j) % n } else { i + j };
            iupac::matches(pattern[j], seq[idx])
        });
        if hit {
            // 0-based index of the base 3' of the nick, then to 1-based.
            let cut0 = (i + enzyme.cut_offset as usize) % n;
            out.push(cut0 as u64 + 1);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// One enzyme's result over a molecule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digest {
    pub enzyme: &'static Enzyme,
    pub positions: Vec<u64>,
}

impl Digest {
    pub fn count(&self) -> usize {
        self.positions.len()
    }
    pub fn is_unique_cutter(&self) -> bool {
        self.positions.len() == 1
    }
    pub fn is_non_cutter(&self) -> bool {
        self.positions.is_empty()
    }

    /// Fragment lengths produced by this enzyme alone.
    ///
    /// A circular molecule cut once linearises to a single full-length
    /// fragment; cut *k* times it yields *k* fragments. A linear molecule cut
    /// *k* times yields *k + 1*.
    pub fn fragments(&self, len: u64, topology: Topology) -> Vec<u64> {
        if len == 0 || self.positions.is_empty() {
            return if len == 0 { Vec::new() } else { vec![len] };
        }
        let p = &self.positions;
        if topology.is_circular() {
            if p.len() == 1 {
                return vec![len];
            }
            let mut out: Vec<u64> = p.windows(2).map(|w| w[1] - w[0]).collect();
            out.push(len - p[p.len() - 1] + p[0]);
            out.sort_unstable_by(|a, b| b.cmp(a));
            out
        } else {
            let mut out = vec![p[0] - 1];
            out.extend(p.windows(2).map(|w| w[1] - w[0]));
            out.push(len - p[p.len() - 1] + 1);
            out.retain(|&f| f > 0);
            out.sort_unstable_by(|a, b| b.cmp(a));
            out
        }
    }
}

/// Digest a molecule with every enzyme in the default set.
pub fn digest_all(mol: &Molecule) -> Vec<Digest> {
    digest_with(mol, ENZYMES)
}

pub fn digest_with(mol: &Molecule, enzymes: &'static [Enzyme]) -> Vec<Digest> {
    enzymes
        .iter()
        .map(|e| Digest {
            enzyme: e,
            positions: cut_positions(&mol.seq, mol.topology, e),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cuts(seq: &[u8], topo: Topology, name: &str) -> Vec<u64> {
        cut_positions(seq, topo, by_name(name).unwrap())
    }

    #[test]
    fn ecori_cut_uses_the_biopython_convention() {
        // G^AATTC at position 1 => the base after the nick is 2.
        assert_eq!(cuts(b"GAATTC", Topology::Linear, "EcoRI"), vec![2]);
    }

    #[test]
    fn blunt_cutter_sits_mid_site() {
        // EcoRV GAT^ATC
        assert_eq!(cuts(b"GATATC", Topology::Linear, "EcoRV"), vec![4]);
        assert!(by_name("EcoRV").unwrap().is_blunt());
        assert!(!by_name("EcoRI").unwrap().is_blunt());
    }

    #[test]
    fn site_is_found_regardless_of_case() {
        assert_eq!(cuts(b"gaattc", Topology::Linear, "EcoRI"), vec![2]);
        assert_eq!(cuts(b"GaAtTc", Topology::Linear, "EcoRI"), vec![2]);
    }

    #[test]
    fn ambiguous_bases_do_not_conjure_sites() {
        // N in the subject is not evidence of a site.
        assert!(cuts(b"GAATTN", Topology::Linear, "EcoRI").is_empty());
        assert!(cuts(b"NNNNNN", Topology::Linear, "EcoRI").is_empty());
    }

    #[test]
    fn origin_spanning_site_is_found_on_a_circle_and_missed_on_a_line() {
        // 15 bp. 1-based: 1..3 = TTC, 4..13 = G x10, 14..15 = AA.
        // Read circularly from 13, the bases are G A A T T C -- an EcoRI site
        // straddling the origin at positions 13,14,15,1,2,3.
        let seq = b"TTCGGGGGGGGGGAA";
        assert_eq!(seq.len(), 15);
        assert!(
            cuts(seq, Topology::Linear, "EcoRI").is_empty(),
            "linearly there is no site, and claiming one would be worse than missing it"
        );

        let c = cuts(seq, Topology::Circular, "EcoRI");
        assert_eq!(c.len(), 1, "a unique cutter hidden at the origin");
        // G^AATTC nicks after position 13, so the base 3' of the nick is 14.
        assert_eq!(c, vec![14]);
    }

    #[test]
    fn circular_fragments_sum_to_the_molecule() {
        let seq = b"GAATTCAAAAAAAAAAGAATTCAAAAAAAAAA";
        let d = Digest {
            enzyme: by_name("EcoRI").unwrap(),
            positions: cut_positions(seq, Topology::Circular, by_name("EcoRI").unwrap()),
        };
        assert_eq!(d.count(), 2);
        let f = d.fragments(seq.len() as u64, Topology::Circular);
        assert_eq!(f.len(), 2);
        assert_eq!(f.iter().sum::<u64>(), seq.len() as u64);
    }

    #[test]
    fn single_cut_circle_linearises_to_one_full_length_fragment() {
        let seq = b"GAATTCAAAAAAAAAA";
        let e = by_name("EcoRI").unwrap();
        let d = Digest {
            enzyme: e,
            positions: cut_positions(seq, Topology::Circular, e),
        };
        assert_eq!(d.count(), 1);
        assert_eq!(d.fragments(seq.len() as u64, Topology::Circular), vec![16]);
    }

    #[test]
    fn linear_fragments_sum_to_the_molecule() {
        let seq = b"AAAGAATTCAAAGAATTCAAA";
        let e = by_name("EcoRI").unwrap();
        let d = Digest {
            enzyme: e,
            positions: cut_positions(seq, Topology::Linear, e),
        };
        let f = d.fragments(seq.len() as u64, Topology::Linear);
        assert_eq!(f.len(), 3);
        assert_eq!(f.iter().sum::<u64>(), seq.len() as u64);
    }

    #[test]
    fn isoschizomers_share_a_site_but_not_a_cut() {
        // SmaI CCC^GGG (blunt) vs XmaI C^CCGGG (sticky): same site, different nick.
        assert_eq!(cuts(b"CCCGGG", Topology::Linear, "SmaI"), vec![4]);
        assert_eq!(cuts(b"CCCGGG", Topology::Linear, "XmaI"), vec![2]);
    }

    #[test]
    fn table_is_sorted_and_free_of_duplicates() {
        let mut names: Vec<_> = ENZYMES.iter().map(|e| e.name).collect();
        let original = names.clone();
        names.sort_unstable();
        assert_eq!(names, original, "keep the table sorted for reviewability");
        names.dedup();
        assert_eq!(names.len(), ENZYMES.len(), "duplicate enzyme name");
    }

    #[test]
    fn every_cut_offset_lies_within_its_site() {
        for e in ENZYMES {
            assert!(
                (e.cut_offset as usize) <= e.site.len(),
                "{} cuts outside its own recognition site",
                e.name
            );
            assert!(
                e.site.bytes().all(|b| iupac::code_mask(b) != 0),
                "{} has a non-IUPAC character in its site",
                e.name
            );
        }
    }
}
