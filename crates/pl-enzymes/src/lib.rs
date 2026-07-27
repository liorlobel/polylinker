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

pub mod methylation;

use pl_core::{iupac, Molecule, Topology};

/// A restriction enzyme, in Biopython's coordinates.
///
/// Two numbers describe every cut this project can make — Type IIP, Type IIS
/// cutting at a distance, blunt, 5' overhang, 3' overhang:
///
/// ```text
/// top    = match_start + fst5
/// bottom = top - ovhg
/// ```
///
/// `docs/PLAN.md` §7.1 specifies exactly this and adds "do not invent
/// another", which is worth heeding: the previous model stored only the
/// top-strand offset and *derived* the bottom one by mirroring it about the
/// centre of the site. That is true for a palindrome and false for every
/// Type IIS enzyme, whose two nicks are both outside the site and are not
/// symmetric about anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Enzyme {
    pub name: &'static str,
    /// Recognition site, IUPAC, 5'->3' on the top strand.
    pub site: &'static str,
    /// Bases from the start of the match to the top-strand nick.
    ///
    /// `GAATTC` with `fst5 = 1` is `G^AATTC`. For a Type IIS enzyme this
    /// exceeds the site length: `BsaI` is `GGTCTCN^NNNN`, so `fst5 = 7` on a
    /// six-base site.
    pub fst5: u8,
    /// The overhang left behind: **negative for 5', positive for 3', zero for
    /// blunt** — Biopython's sign convention.
    ///
    /// Stored rather than inferred. The geometric guess it replaces (a nick at
    /// the centre of an even-length site means blunt) is right for all 51
    /// Type IIP enzymes here and wrong for 5 of Biopython's 389, and it cannot
    /// be right at all for an enzyme that cuts outside its site.
    pub ovhg: i8,
}

impl Enzyme {
    pub fn len(&self) -> usize {
        self.site.len()
    }
    pub fn is_empty(&self) -> bool {
        self.site.is_empty()
    }
    /// Blunt ends, read off the stored overhang rather than guessed at.
    pub fn is_blunt(&self) -> bool {
        self.ovhg == 0
    }
    /// Leaves a 5' overhang — the sticky end most cloning uses.
    pub fn is_five_prime_overhang(&self) -> bool {
        self.ovhg < 0
    }
    pub fn is_three_prime_overhang(&self) -> bool {
        self.ovhg > 0
    }
    /// Length of the single-stranded end, whichever kind it is.
    pub fn overhang_len(&self) -> usize {
        self.ovhg.unsigned_abs() as usize
    }
    /// Does this enzyme cut outside its recognition site?
    ///
    /// The defining property of a Type IIS enzyme, and the reason the overhang
    /// a cut leaves depends on the *sequence* rather than on the enzyme: `BsaI`
    /// leaves whatever four bases happen to sit one past its site, which is the
    /// whole basis of Golden Gate assembly.
    pub fn cuts_outside_site(&self) -> bool {
        self.fst5 as usize >= self.site.len()
    }
    /// The bottom-strand nick, as an offset from the start of the match.
    pub fn bottom_cut(&self) -> i64 {
        self.fst5 as i64 - self.ovhg as i64
    }

    /// How many bases of the site are actually specified.
    ///
    /// Not the same as [`Enzyme::len`], and the difference is what "6-cutter"
    /// means. `BstXI` is `CCANNNNNNTGG` — twelve bases long, but the six `N`s
    /// constrain nothing, so it cuts about as often as a six-cutter and is
    /// classified as one. Counting raw length instead would file it with the
    /// rare cutters and quietly mislead someone choosing an enzyme.
    pub fn specificity(&self) -> usize {
        self.site
            .bytes()
            .filter(|b| !b.eq_ignore_ascii_case(&b'N'))
            .count()
    }
}

/// A named subset of the enzyme list.
///
/// Every tool in this category offers these, and every one of them defaults to
/// showing a subset. `docs/PLAN.md` item 33 is blunt about the cost: hiding
/// sites behind a default filter is the one documented case of this software
/// category costing a user a month of bench time. So the filter exists — it is
/// genuinely useful — and [`Visibility`] exists beside it so that what the
/// filter hides can never be silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnzymeSet {
    /// Everything that cuts at all.
    All,
    /// Cuts exactly once. The set you want when linearising.
    Unique,
    /// Cuts once or twice. Two sites still give a usable excision.
    UniqueAndDual,
    /// Recognition specificity of six or more bases — the rarer cutters.
    SixPlus,
    /// Both: cuts once, and is a six-or-more-base cutter.
    UniqueSixPlus,
}

impl EnzymeSet {
    pub const ALL: [EnzymeSet; 5] = [
        EnzymeSet::All,
        EnzymeSet::Unique,
        EnzymeSet::UniqueAndDual,
        EnzymeSet::SixPlus,
        EnzymeSet::UniqueSixPlus,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EnzymeSet::All => "All cutters",
            EnzymeSet::Unique => "Unique",
            EnzymeSet::UniqueAndDual => "Unique & dual",
            EnzymeSet::SixPlus => "6+ cutters",
            EnzymeSet::UniqueSixPlus => "Unique 6+",
        }
    }

    /// Does this set include a given result?
    ///
    /// Non-cutters are in no set: an enzyme with no sites is not something the
    /// filter is hiding, it is something the molecule does not contain.
    pub fn admits(self, d: &Digest) -> bool {
        let n = d.count();
        if n == 0 {
            return false;
        }
        let six = d.enzyme.specificity() >= 6;
        match self {
            EnzymeSet::All => true,
            EnzymeSet::Unique => n == 1,
            EnzymeSet::UniqueAndDual => n <= 2,
            EnzymeSet::SixPlus => six,
            EnzymeSet::UniqueSixPlus => n == 1 && six,
        }
    }
}

/// What a filter is showing, and what it is not.
///
/// The counts a user needs in order to know whether the panel in front of them
/// is the whole story. `hidden_sites` is deliberately a count of *sites*, not
/// of enzymes: "3 enzymes hidden" understates a case where those three cut
/// fourteen times between them, and it is the cut you did not know about that
/// ruins the experiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Visibility {
    pub shown_enzymes: usize,
    pub shown_sites: usize,
    pub hidden_enzymes: usize,
    pub hidden_sites: usize,
    /// Enzymes with no site in this molecule. Not hidden — absent.
    pub non_cutters: usize,
}

impl Visibility {
    pub fn of(results: &[Digest], set: EnzymeSet) -> Visibility {
        let mut v = Visibility::default();
        for d in results {
            let n = d.count();
            if n == 0 {
                v.non_cutters += 1;
            } else if set.admits(d) {
                v.shown_enzymes += 1;
                v.shown_sites += n;
            } else {
                v.hidden_enzymes += 1;
                v.hidden_sites += n;
            }
        }
        v
    }

    /// Is the panel showing less than the whole truth?
    pub fn hides_anything(&self) -> bool {
        self.hidden_sites > 0
    }
}

/// The shipped enzymes, sorted by name.
///
/// Type IIP throughout, plus the eight Type IIS enzymes Golden Gate needs.
/// Sites and cut geometry were verified against Biopython's REBASE-derived
/// tables, which agreed with every one of the 51 already here; the `ovhg`
/// column and the Type IIS entries were taken from the same place. These are
/// published facts about enzymes, not a copied database — see `PROVENANCE.md`.
pub const ENZYMES: &[Enzyme] = &[
    Enzyme {
        name: "AarI",
        site: "CACCTGC",
        fst5: 11, // Type IIS: cuts outside its site
        ovhg: -4,
    },
    Enzyme {
        name: "AatII",
        site: "GACGTC",
        fst5: 5,
        ovhg: 4,
    },
    Enzyme {
        name: "AflII",
        site: "CTTAAG",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "AgeI",
        site: "ACCGGT",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "ApaI",
        site: "GGGCCC",
        fst5: 5,
        ovhg: 4,
    },
    Enzyme {
        name: "AscI",
        site: "GGCGCGCC",
        fst5: 2,
        ovhg: -4,
    },
    Enzyme {
        name: "AvrII",
        site: "CCTAGG",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "BamHI",
        site: "GGATCC",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "BbsI",
        site: "GAAGAC",
        fst5: 8, // Type IIS: cuts outside its site
        ovhg: -4,
    },
    Enzyme {
        name: "BclI",
        site: "TGATCA",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "BglII",
        site: "AGATCT",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "BsaI",
        site: "GGTCTC",
        fst5: 7, // Type IIS: cuts outside its site
        ovhg: -4,
    },
    Enzyme {
        name: "BsiWI",
        site: "CGTACG",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "BsmBI",
        site: "CGTCTC",
        fst5: 7, // Type IIS: cuts outside its site
        ovhg: -4,
    },
    Enzyme {
        name: "BspEI",
        site: "TCCGGA",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "BspQI",
        site: "GCTCTTC",
        fst5: 8, // Type IIS: cuts outside its site
        ovhg: -3,
    },
    Enzyme {
        name: "BsrGI",
        site: "TGTACA",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "BstBI",
        site: "TTCGAA",
        fst5: 2,
        ovhg: -2,
    },
    Enzyme {
        name: "ClaI",
        site: "ATCGAT",
        fst5: 2,
        ovhg: -2,
    },
    Enzyme {
        name: "DraI",
        site: "TTTAAA",
        fst5: 3,
        ovhg: 0,
    },
    Enzyme {
        name: "EagI",
        site: "CGGCCG",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "EcoRI",
        site: "GAATTC",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "EcoRV",
        site: "GATATC",
        fst5: 3,
        ovhg: 0,
    },
    Enzyme {
        name: "Esp3I",
        site: "CGTCTC",
        fst5: 7, // Type IIS: cuts outside its site
        ovhg: -4,
    },
    Enzyme {
        name: "FseI",
        site: "GGCCGGCC",
        fst5: 6,
        ovhg: 4,
    },
    Enzyme {
        name: "HindIII",
        site: "AAGCTT",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "HpaI",
        site: "GTTAAC",
        fst5: 3,
        ovhg: 0,
    },
    Enzyme {
        name: "KpnI",
        site: "GGTACC",
        fst5: 5,
        ovhg: 4,
    },
    Enzyme {
        name: "MfeI",
        site: "CAATTG",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "MluI",
        site: "ACGCGT",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "NcoI",
        site: "CCATGG",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "NdeI",
        site: "CATATG",
        fst5: 2,
        ovhg: -2,
    },
    Enzyme {
        name: "NheI",
        site: "GCTAGC",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "NotI",
        site: "GCGGCCGC",
        fst5: 2,
        ovhg: -4,
    },
    Enzyme {
        name: "NruI",
        site: "TCGCGA",
        fst5: 3,
        ovhg: 0,
    },
    Enzyme {
        name: "NsiI",
        site: "ATGCAT",
        fst5: 5,
        ovhg: 4,
    },
    Enzyme {
        name: "PacI",
        site: "TTAATTAA",
        fst5: 5,
        ovhg: 2,
    },
    Enzyme {
        name: "PaqCI",
        site: "CACCTGC",
        fst5: 11, // Type IIS: cuts outside its site
        ovhg: -4,
    },
    Enzyme {
        name: "PmeI",
        site: "GTTTAAAC",
        fst5: 4,
        ovhg: 0,
    },
    Enzyme {
        name: "PstI",
        site: "CTGCAG",
        fst5: 5,
        ovhg: 4,
    },
    Enzyme {
        name: "PvuI",
        site: "CGATCG",
        fst5: 4,
        ovhg: 2,
    },
    Enzyme {
        name: "PvuII",
        site: "CAGCTG",
        fst5: 3,
        ovhg: 0,
    },
    Enzyme {
        name: "SacI",
        site: "GAGCTC",
        fst5: 5,
        ovhg: 4,
    },
    Enzyme {
        name: "SacII",
        site: "CCGCGG",
        fst5: 4,
        ovhg: 2,
    },
    Enzyme {
        name: "SalI",
        site: "GTCGAC",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "SapI",
        site: "GCTCTTC",
        fst5: 8, // Type IIS: cuts outside its site
        ovhg: -3,
    },
    Enzyme {
        name: "SbfI",
        site: "CCTGCAGG",
        fst5: 6,
        ovhg: 4,
    },
    Enzyme {
        name: "ScaI",
        site: "AGTACT",
        fst5: 3,
        ovhg: 0,
    },
    Enzyme {
        name: "SmaI",
        site: "CCCGGG",
        fst5: 3,
        ovhg: 0,
    },
    Enzyme {
        name: "SnaBI",
        site: "TACGTA",
        fst5: 3,
        ovhg: 0,
    },
    Enzyme {
        name: "SpeI",
        site: "ACTAGT",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "SphI",
        site: "GCATGC",
        fst5: 5,
        ovhg: 4,
    },
    Enzyme {
        name: "SspI",
        site: "AATATT",
        fst5: 3,
        ovhg: 0,
    },
    Enzyme {
        name: "StuI",
        site: "AGGCCT",
        fst5: 3,
        ovhg: 0,
    },
    Enzyme {
        name: "SwaI",
        site: "ATTTAAAT",
        fst5: 4,
        ovhg: 0,
    },
    Enzyme {
        name: "XbaI",
        site: "TCTAGA",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "XhoI",
        site: "CTCGAG",
        fst5: 1,
        ovhg: -4,
    },
    Enzyme {
        name: "XmaI",
        site: "CCCGGG",
        fst5: 1,
        ovhg: -4,
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
    // A circular molecule shorter than the recognition site has no site.
    //
    // A deliberate, documented divergence from Biopython, which reports a cut
    // here: circular `CGTA` with BsiWI (`CGTACG`) gives Bio `[2]` and us `[]`.
    // Biopython searches a doubled string, so the site wraps the molecule more
    // than once and matches. No restriction enzyme binds a 6 bp site on a 4 bp
    // circle, so ours is the biology and Biopython's is an artifact of its
    // implementation. Pinned by `a_circle_shorter_than_the_site_has_no_site`
    // so nobody "restores parity" and makes it wrong.
    //
    // For every n >= k the two agree exactly — 25,400 compared positions,
    // including origin-straddling and tandem sites.
    if n == 0 || k == 0 || k > n {
        return Vec::new();
    }
    // The scan itself is `pl_core::iupac::find_all`, so the library's motif
    // search runs the same code this function's Biopython oracle covers rather
    // than a second implementation of it. Scanning `n` starts on a circle
    // against `n - k + 1` on a line — the whole of the wraparound handling —
    // lives there now.
    //
    // The `k > n` guard above stays here as well: it is this function's
    // documented divergence, and a reader looking for it should find it at the
    // place that owns it.
    let circular = topology.is_circular();

    // Sites on the bottom strand are cut too.
    //
    // For a palindrome the two searches find the same places, so scanning
    // forward alone was complete — and stayed complete for as long as every
    // shipped enzyme was palindromic. A Type IIS enzyme is not: `BsaI` binds
    // `GGTCTC` on either strand, and on the bottom strand it reaches the other
    // way, so its minus-strand sites are real cuts at different coordinates.
    // Biopython finds them; we did not, and on one 180 kb contig that was five
    // cuts reported where there are eight.
    let rc_site = iupac::reverse_complement(enzyme.site.as_bytes());
    let antisense = !iupac::is_palindrome_masks(enzyme.site.as_bytes());

    // Where the *bottom*-strand nick of an antisense match lands on the top
    // strand. In the enzyme's own frame it nicks `bottom_cut` bases past its
    // site start, and that frame runs the other way, so from the 0-based start
    // `i` of the reverse-complemented site the nick is at
    // `i - (bottom_cut - k)`.
    let k = enzyme.site.len() as i64;
    let back = enzyme.bottom_cut() - k;

    let mut out: Vec<u64> = iupac::find_all(enzyme.site.as_bytes(), seq, circular)
        .into_iter()
        // `find_all` gives the 1-based start of the site; the nick is `fst5`
        // further along.
        .filter_map(|start| {
            let cut0 = start as i64 - 1 + enzyme.fst5 as i64;
            if circular {
                // Wrapped back into 1..=n. Note this project never had
                // Biopython's too-short-doubling bug that `docs/PLAN.md` §7.1
                // warns about: the *site* search walks the circle itself, so a
                // Type IIS enzyme reaching 11 bases past its site is found and
                // placed correctly however close to the origin it sits.
                Some(cut0.rem_euclid(n as i64) as u64 + 1)
            } else if (0..n as i64).contains(&cut0) {
                Some(cut0 as u64 + 1)
            } else {
                // A Type IIS enzyme can bind near the end of a linear molecule
                // and reach past it. It binds; there is nothing there to cut.
                // Reporting a wrapped position would invent a cut on a
                // molecule with no other end.
                None
            }
        })
        .collect();

    if antisense {
        out.extend(
            iupac::find_all(&rc_site, seq, circular)
                .into_iter()
                .filter_map(|start| {
                    let cut0 = start as i64 - 1 - back;
                    if circular {
                        Some(cut0.rem_euclid(n as i64) as u64 + 1)
                    } else if (0..n as i64).contains(&cut0) {
                        Some(cut0 as u64 + 1)
                    } else {
                        None
                    }
                }),
        );
    }

    // Two sites at different starts can nick the same bond once the offset has
    // wrapped, so the sort and dedup stay. `find_all` returns ascending starts;
    // the mapped cuts need not be ascending.
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

    /// The property `docs/PLAN.md` §7.12.3 singles out as "where origin bugs
    /// live": on a circle there is no privileged starting point, so rotating
    /// the sequence must move every cut site by the same amount and create or
    /// destroy none.
    #[test]
    fn the_cut_site_set_of_a_circle_is_invariant_under_rotation() {
        let seqs = [
            "GAATTCAAAAGGATCCTTTTAAGCTTGGGGCTGCAGCCCC",
            // A site deliberately straddling the origin: TTC....GAA reads
            // GAATTC across the join.
            "TTCGGGGGGGGGGAA",
            "ACGTACGTACGTACGTACGTACGT",
            "GGATCCGGATCCGGATCC",
        ];
        for seq in seqs {
            let n = seq.len() as u64;
            for e in ENZYMES {
                let base = cut_positions(seq.as_bytes(), Topology::Circular, e);
                for k in 1..seq.len() {
                    let rotated = pl_core::seguid::rotate(seq, k as isize);
                    let got = cut_positions(rotated.as_bytes(), Topology::Circular, e);

                    // A cut at 1-based p in the original sits at
                    // ((p - 1 - k) mod n) + 1 after rotating k off the front.
                    let mut want: Vec<u64> = base
                        .iter()
                        .map(|&p| (p as i64 - 1 - k as i64).rem_euclid(n as i64) as u64 + 1)
                        .collect();
                    want.sort_unstable();

                    assert_eq!(
                        got,
                        want,
                        "{} on {seq:?} rotated by {k}: {} sites became {}",
                        e.name,
                        base.len(),
                        got.len()
                    );
                }
            }
        }
    }

    #[test]
    fn a_linear_molecule_is_not_rotation_invariant_and_should_not_be() {
        // The counterpart to the property above: rotating a linear molecule
        // changes what it is, so its sites are free to change. If this ever
        // started passing, circular handling would have leaked into the
        // linear path.
        let seq = "TTCGGGGGGGGGGAA";
        let e = by_name("EcoRI").unwrap();
        assert!(cut_positions(seq.as_bytes(), Topology::Linear, e).is_empty());

        // Rotating 12 off the front brings the site into the middle:
        // "GAA" + "TTCGGGGGGGGG" == "GAATTCGGGGGGGGG".
        let rotated = pl_core::seguid::rotate(seq, 12);
        assert_eq!(rotated, "GAATTCGGGGGGGGG");
        assert_eq!(
            cut_positions(rotated.as_bytes(), Topology::Linear, e),
            vec![2]
        );
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
    fn a_type_iip_enzyme_cuts_inside_its_site_and_a_type_iis_outside() {
        // This replaced a blanket "every cut lies within its site". That was a
        // true statement about a table of Type IIP enzymes and a false one
        // about restriction enzymes: cutting at a distance is the defining
        // property of Type IIS, and it is the whole basis of Golden Gate.
        let mut iis = 0;
        for e in ENZYMES {
            assert!(
                e.site.bytes().all(|b| iupac::code_mask(b) != 0),
                "{} has a non-IUPAC character in its site",
                e.name
            );
            if e.cuts_outside_site() {
                iis += 1;
                assert!(
                    e.ovhg != 0,
                    "{}: a Type IIS enzyme leaving blunt ends would be unusable for assembly",
                    e.name
                );
            } else {
                assert!(
                    (e.fst5 as usize) <= e.site.len(),
                    "{} nicks at {} on a {}-base site but is not marked as cutting outside it",
                    e.name,
                    e.fst5,
                    e.site.len()
                );
            }
            // The bottom-strand nick must be somewhere real.
            assert!(
                e.bottom_cut() >= 0,
                "{}: bottom cut before the site",
                e.name
            );
        }
        assert!(
            iis >= 8,
            "the Type IIS enzymes Golden Gate needs are missing"
        );
    }

    #[test]
    fn the_overhang_is_stored_rather_than_guessed_from_geometry() {
        // The heuristic this replaced -- a nick at the centre of an even-length
        // site means blunt -- is right for all 51 Type IIP enzymes here and
        // wrong for 5 of Biopython's 389. It also cannot be right at all for an
        // enzyme that cuts outside its site, which is why it had to go before
        // Type IIS could be added.
        for e in ENZYMES {
            let guess = e.site.len() % 2 == 0 && e.fst5 as usize == e.site.len() / 2;
            if e.cuts_outside_site() {
                assert!(!e.is_blunt(), "{} should have sticky ends", e.name);
                continue;
            }
            assert_eq!(
                guess,
                e.is_blunt(),
                "{}: the old geometric guess and the stored overhang disagree",
                e.name
            );
        }
        // And the three kinds are exclusive and exhaustive.
        for e in ENZYMES {
            let kinds = [
                e.is_blunt(),
                e.is_five_prime_overhang(),
                e.is_three_prime_overhang(),
            ];
            assert_eq!(kinds.iter().filter(|k| **k).count(), 1, "{}", e.name);
            assert_eq!(e.overhang_len() == 0, e.is_blunt(), "{}", e.name);
        }
    }

    fn mol(seq: &str, topology: Topology) -> Molecule {
        Molecule {
            seq: seq.as_bytes().to_vec(),
            topology,
            ..Default::default()
        }
    }

    #[test]
    fn a_circle_shorter_than_the_site_has_no_site() {
        // Biopython says otherwise, and Biopython is wrong here — see the note
        // in `cut_positions`. Recorded as a test rather than left as a silent
        // difference from the oracle we otherwise match exactly.
        for (name, seq) in [("BsiWI", "CGTA"), ("PacI", "TTAA"), ("NotI", "CCGCGG")] {
            let e = by_name(name).unwrap();
            assert!(seq.len() < e.site.len());
            assert!(
                cut_positions(seq.as_bytes(), Topology::Circular, e).is_empty(),
                "{name} cannot bind its site on a {} bp circle",
                seq.len()
            );
        }
        // Exactly at the boundary the site does exist, and is found once.
        let e = by_name("EcoRI").unwrap();
        assert_eq!(
            cut_positions(b"GAATTC", Topology::Circular, e),
            vec![2],
            "n == k is a real site and must still be found"
        );
    }

    #[test]
    fn specificity_counts_specified_bases_not_raw_length() {
        // BstXI is CCANNNNNNTGG: twelve long, six specified. It cuts about as
        // often as a six-cutter and belongs with them; classifying by raw
        // length would file it with the rare cutters and mislead someone
        // choosing an enzyme.
        for e in ENZYMES {
            assert!(e.specificity() <= e.len());
            assert!(e.specificity() > 0, "{} specifies nothing", e.name);
        }
        assert_eq!(by_name("EcoRI").unwrap().specificity(), 6);
        assert_eq!(by_name("NotI").unwrap().specificity(), 8);

        // The interrupted-palindrome case, tested on a constructed enzyme
        // because our table does not yet contain one. BstXI is CCANNNNNNTGG:
        // twelve long, six specified.
        let bstxi = Enzyme {
            name: "BstXI",
            site: "CCANNNNNNTGG",
            fst5: 8,
            ovhg: 4,
        };
        assert_eq!(bstxi.len(), 12);
        assert_eq!(bstxi.specificity(), 6, "the six Ns constrain nothing");

        // Every enzyme currently in the table is a 6+ cutter with no ambiguity
        // codes, so `SixPlus` admits every cutter and hides nothing. That is
        // correct rather than broken, and pinned here so that adding a
        // four-cutter makes this fail loudly and the set starts discriminating
        // on purpose rather than by accident.
        assert!(
            ENZYMES.iter().all(|e| e.specificity() >= 6 && e.len() == e.specificity()),
            "the table has gained a short or ambiguous cutter; the 6+ set now              discriminates, so check the UI copy and this test together"
        );
    }

    #[test]
    fn the_sets_partition_the_cutters() {
        // Whatever the filter is, shown + hidden must equal every enzyme that
        // cuts. If that ever fails, some sites are in no category at all and
        // the badge cannot be trusted.
        let mol = mol(
            "AAAAGAATTCCCCGGATCCTTTTAAGCTTGGGGGAATTCCCCC",
            Topology::Circular,
        );
        let results = digest_all(&mol);
        let cutters = results.iter().filter(|d| d.count() > 0).count();
        for set in EnzymeSet::ALL {
            let v = Visibility::of(&results, set);
            assert_eq!(
                v.shown_enzymes + v.hidden_enzymes,
                cutters,
                "{} loses enzymes",
                set.label()
            );
            assert_eq!(
                v.shown_enzymes + v.hidden_enzymes + v.non_cutters,
                results.len(),
                "{} loses enzymes entirely",
                set.label()
            );
            let total_sites: usize = results.iter().map(|d| d.count()).sum();
            assert_eq!(
                v.shown_sites + v.hidden_sites,
                total_sites,
                "{} loses sites",
                set.label()
            );
        }
    }

    #[test]
    fn all_hides_nothing_and_the_others_may() {
        let mol = mol(
            "AAAAGAATTCCCCGGATCCTTTTAAGCTTGGGGGAATTCCCCC",
            Topology::Circular,
        );
        let results = digest_all(&mol);

        let all = Visibility::of(&results, EnzymeSet::All);
        assert!(!all.hides_anything(), "'All' must never hide a site");
        assert_eq!(all.hidden_sites, 0);

        // EcoRI appears twice here, so 'Unique' must hide it -- and must say
        // how many SITES, not how many enzymes.
        let uniq = Visibility::of(&results, EnzymeSet::Unique);
        assert!(uniq.hides_anything());
        assert!(
            uniq.hidden_sites >= uniq.hidden_enzymes,
            "sites cannot be fewer than the enzymes hiding them"
        );
    }

    #[test]
    fn a_non_cutter_is_absent_not_hidden() {
        // An enzyme with no site is not something the filter is concealing.
        // Counting it as hidden would put a permanent, meaningless warning on
        // every molecule, and a warning that is always on is not a warning.
        let mol = mol("GAATTC", Topology::Linear);
        let results = digest_all(&mol);
        let v = Visibility::of(&results, EnzymeSet::Unique);
        assert!(v.non_cutters > 0);
        assert_eq!(v.hidden_sites, 0, "nothing is hidden here");
        assert!(!v.hides_anything());
    }

    #[test]
    fn unique_and_dual_admits_one_and_two_but_not_three() {
        let e = by_name("EcoRI").unwrap();
        let mk = |n: usize| Digest {
            enzyme: e,
            positions: (0..n as u64).collect(),
        };
        assert!(EnzymeSet::Unique.admits(&mk(1)));
        assert!(!EnzymeSet::Unique.admits(&mk(2)));
        assert!(EnzymeSet::UniqueAndDual.admits(&mk(1)));
        assert!(EnzymeSet::UniqueAndDual.admits(&mk(2)));
        assert!(!EnzymeSet::UniqueAndDual.admits(&mk(3)));
        // Nothing admits a non-cutter.
        for set in EnzymeSet::ALL {
            assert!(!set.admits(&mk(0)), "{} admitted a non-cutter", set.label());
        }
    }
}
