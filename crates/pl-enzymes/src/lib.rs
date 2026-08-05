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

use pl_core::{iupac, Molecule, Strand, Topology};

/// Whether two cut ends can be ligated to each other.
///
/// Three-valued on purpose. A two-valued answer would have to call `BstXI` to
/// `BstXI` either compatible — which is false, its `NNNN` overhangs agree only
/// when the DNA makes them — or incompatible, which is false the other way. The
/// same is true of every Type IIS enzyme, and Golden Gate exists precisely
/// because those ends are chosen per construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compatibility {
    /// Same polarity and the same fixed bases, or both blunt.
    Always,
    /// They may anneal; the DNA decides. A Type IIS end, or an IUPAC code in
    /// the overhang.
    Sequence,
    /// Opposite polarity, different lengths, or bases that cannot agree.
    Never,
}

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
    /// the centre of an even-length site means blunt) is right for all 50
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
    /// Does this enzyme cut OUTSIDE its recognition site? A Type IIS enzyme.
    ///
    /// Read off `fst5`, whose own doc already states the rule: `BsaI` is
    /// `GGTCTCN^NNNN`, so `fst5` is 7 on a six-base site. The alternative is a
    /// second, hand-maintained list of names, and a list is a thing that goes
    /// out of date against the table beside it.
    ///
    /// Here rather than in a caller because it is a fact about an enzyme, and
    /// Golden Gate rests on it: the whole method works because the recognition
    /// site leaves with the cut, so the junction carries no scar and cannot be
    /// re-cut. It is the difference between offering the handful of enzymes that
    /// can do it and offering fifty that cannot.
    pub fn cuts_outside_its_site(&self) -> bool {
        self.fst5 as usize > self.site.len()
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

    /// Where the single-stranded end begins, as an offset into the site.
    ///
    /// The two nicks bracket the overhang, and which one comes first is what the
    /// sign of `ovhg` records: a 5' overhang starts at the top nick, a 3' one at
    /// the bottom nick.
    fn overhang_start(&self) -> usize {
        if self.ovhg > 0 {
            self.bottom_cut().max(0) as usize
        } else {
            self.fst5 as usize
        }
    }

    /// The bases of the overhang this enzyme leaves, read off its own site.
    ///
    /// `Some("")` for a blunt cutter. **`None` when the enzyme cuts outside its
    /// site**: `BsaI` leaves whatever four bases happen to sit one past `GGTCTC`,
    /// so the answer is a property of the DNA and not of the enzyme, and saying
    /// otherwise is how a Golden Gate assembly gets designed wrong.
    ///
    /// The string may carry IUPAC codes. `BstXI` is `CCANNNNNNTGG` and its
    /// overhang is four of those `N`s — fixed in *position* by the enzyme and not
    /// in *identity* — which is why [`Enzyme::ligates_with`] has three answers
    /// and not two.
    pub fn overhang_seq(&self) -> Option<&'static str> {
        if self.cuts_outside_site() {
            return None;
        }
        let start = self.overhang_start();
        self.site.get(start..start + self.overhang_len())
    }

    /// Can an end left by this enzyme be ligated to one left by `other`?
    ///
    /// This is the question behind "the polylinker has no `BglII` site, can I use
    /// the `BamHI` one?" — and behind every subcloning that combines two
    /// different digests. Compatibility is decided by the single-stranded end
    /// alone, so enzymes with quite different recognition sites are
    /// interchangeable at the junction: `BamHI`, `BglII` and `BclI` all leave
    /// `GATC`.
    ///
    /// Three answers, not two, because two of the reasons an overhang is not
    /// fixed are ordinary:
    ///
    /// - [`Compatibility::Always`] — same polarity, same fixed bases, or both
    ///   blunt. Blunt ligates to blunt whatever the sequences are.
    /// - [`Compatibility::Sequence`] — the ends *may* anneal, and whether they do
    ///   depends on the DNA: a Type IIS enzyme, or an overhang carrying an IUPAC
    ///   code. `BstXI` to `BstXI` is this, not `Always`.
    /// - [`Compatibility::Never`] — opposite polarity, different lengths, or
    ///   bases that cannot agree at some position.
    ///
    /// Polarity is checked before the bases and that is not a formality:
    /// `KpnI` leaves `GTAC` as a 3' overhang and `BsrGI` leaves `GTAC` as a 5'
    /// one. The same four letters, and they cannot anneal to each other.
    pub fn ligates_with(&self, other: &Enzyme) -> Compatibility {
        // A 5' end cannot anneal to a 3' end, and neither anneals to a blunt
        // one, whatever the bases say.
        if self.ovhg.signum() != other.ovhg.signum() {
            return Compatibility::Never;
        }
        if self.is_blunt() {
            return Compatibility::Always;
        }
        if self.overhang_len() != other.overhang_len() {
            return Compatibility::Never;
        }
        let (a, b) = match (self.overhang_seq(), other.overhang_seq()) {
            (Some(a), Some(b)) => (a, b),
            // At least one is a Type IIS end: the bases are not ours to know.
            _ => return Compatibility::Sequence,
        };
        let mut certain = true;
        for (p, q) in a.bytes().zip(b.bytes()) {
            let (mp, mq) = (iupac::code_mask(p), iupac::code_mask(q));
            // Disjoint sets: no DNA can satisfy both.
            if mp & mq == 0 {
                return Compatibility::Never;
            }
            // They can agree, but only one arrangement of a degenerate site
            // does, so this is a fact about a molecule and not about a pair of
            // enzymes.
            if mp.count_ones() > 1 || mq.count_ones() > 1 {
                certain = false;
            }
        }
        if certain {
            Compatibility::Always
        } else {
            Compatibility::Sequence
        }
    }

    /// Every catalogued enzyme whose ends always ligate to this one.
    ///
    /// Includes the enzyme itself — cutting twice with `BamHI` and re-closing is
    /// the commonest case of all — and excludes anything whose compatibility
    /// depends on the sequence, because a list a user reads as "these are
    /// interchangeable" must not contain a maybe. Use [`Enzyme::ligates_with`]
    /// for those.
    pub fn partners(&self) -> Vec<&'static Enzyme> {
        ENZYMES
            .iter()
            .filter(|e| self.ligates_with(e) == Compatibility::Always)
            .collect()
    }

    /// The top strand across a junction made by ligating this enzyme's end to
    /// `other`'s, when the bases are knowable.
    ///
    /// What it is for: telling whether the junction can be cut again. Ligating
    /// `BamHI` to `BglII` gives `GGATCT`, which neither enzyme cuts — the
    /// standard way to join two fragments and not have the seam re-open — while
    /// `BamHI` to `BamHI` gives `GGATCC` straight back.
    ///
    /// `None` unless [`Compatibility::Always`] holds, since a junction whose
    /// bases depend on the insert is not a string this function can honestly
    /// return.
    ///
    /// Only the two half-sites are included. Whether a LONGER site spans the
    /// seam depends on the flanking DNA, which lives in the construct and not in
    /// the enzyme, so a caller checking for regenerated sites should search the
    /// assembled molecule rather than this string alone.
    pub fn junction(&self, other: &Enzyme) -> Option<String> {
        if self.ligates_with(other) != Compatibility::Always {
            return None;
        }
        let left = self.site.get(..self.overhang_start())?;
        let overhang = self.overhang_seq()?;
        let right = other
            .site
            .get(other.overhang_start() + other.overhang_len()..)?;
        Some(format!("{left}{overhang}{right}"))
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
            // "6+ cutters" was wrong in the way that matters. This set means
            // `specificity() >= 6` — recognition-site LENGTH — while every
            // heading beside it ("22 unique cutters", "18 cut more than once")
            // is a cut COUNT, so half of biologists read it as the far end of
            // the same axis. Renamed here rather than later because this chip
            // now alters the map as well as the list.
            //
            // The "+" is not decoration, and dropping it was a second and
            // smaller error in the same line: `admits` tests `specificity() >=
            // 6`, and the table holds four seven-base and seven eight-base
            // cutters. PmeI is GTTTAAAC, and "6-base sites" says of it something
            // the set does not.
            EnzymeSet::SixPlus => "6+ base sites",
            EnzymeSet::UniqueSixPlus => "Unique 6+ base",
        }
    }

    /// Whether this set can turn anything away from a given digest.
    ///
    /// False means the chip is inert on this molecule — clicking it changes
    /// neither the list nor the map, and the user gets no feedback that the
    /// control did nothing. That is the state `SixPlus` and `UniqueSixPlus` are
    /// permanently in against the built-in table, where every enzyme has a
    /// 6-base or longer site (asserted by
    /// `specificity_counts_specified_bases_not_raw_length`, so a four-cutter
    /// makes this start discriminating loudly rather than
    /// silently). It is a fact about the DATA and not about the code, so the UI
    /// asks the data rather than special-casing two variants.
    pub fn discriminates(self, results: &[Digest]) -> bool {
        results.iter().any(|d| d.count() > 0 && !self.admits(d))
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
/// 58 entries: 50 Type IIP, plus the eight Type IIS enzymes Golden Gate needs.
/// Sites and cut geometry were verified against Biopython's REBASE-derived
/// tables, which agreed with every one of the 50 already here; the `ovhg`
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

/// One cut, together with the match that produced it.
///
/// [`cut_positions`] throws `site_start` away, and a caller that needs it back
/// cannot recover it: for a non-palindromic Type IIS enzyme the two strands map
/// a match to a cut through *different* offsets (`start + fst5` forward,
/// `start + k - bottom_cut` reverse), so `position - fst5` is simply the wrong
/// answer for half the hits. Methylation sensitivity is asked about the *site*,
/// not the cut, so anything calling `methylation::site_effect` needs this
/// rather than a reconstruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutSite {
    /// 1-based position of the base immediately 3' of the top-strand nick.
    pub position: u64,
    /// 1-based position of the first base of the recognition-site match.
    ///
    /// On a circle this is a real index in `1..=n` even when the site wraps the
    /// origin, so it can be greater than `position`.
    pub site_start: u64,
    /// Which strand the enzyme bound. Only `Forward` and `Reverse` occur.
    pub strand: Strand,
}

/// Every cut an enzyme makes, as 1-based positions (see module docs).
///
/// On a circular molecule, sites spanning the origin are found and their cut
/// positions wrapped into `1..=n`. Missing that is the classic plasmid bug:
/// a unique cutter is reported as a non-cutter purely because the site
/// happens to straddle base 1.
pub fn cut_positions(seq: &[u8], topology: Topology, enzyme: &Enzyme) -> Vec<u64> {
    // Two sites at different starts can nick the same bond once the offset has
    // wrapped, so the sort and dedup stay. `find_all` returns ascending starts;
    // the mapped cuts need not be ascending.
    let mut out: Vec<u64> = cut_sites(seq, topology, enzyme)
        .into_iter()
        .map(|c| c.position)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Every cut an enzyme makes, each still carrying the site that produced it.
///
/// Unlike [`cut_positions`] this is neither sorted nor deduplicated: two
/// distinct sites nicking the same bond are two binding events, and collapsing
/// them here would lose the second site.
pub fn cut_sites(seq: &[u8], topology: Topology, enzyme: &Enzyme) -> Vec<CutSite> {
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

    // A cut needs *both* nicks, and on a linear molecule both have to land on a
    // phosphodiester bond that exists.
    //
    // `cut0` is the top-strand nick, expressed as the 0-based index of the base
    // 3' of it, so the bond it breaks is the one between bases `cut0 - 1` and
    // `cut0`: real for `1 <= cut0 <= n - 1` and nowhere else. Its partner sits
    // `-ovhg` away at `cut0 - ovhg` on both search paths — forward, the bottom
    // nick is `bottom_cut - fst5 = -ovhg` past the top one; reverse, the
    // reported nick is the bottom one and the enzyme's own top nick is
    // `-ovhg` the other side of it — and it has to be a real bond too.
    //
    // Checking only `cut0`, and only against `0..n`, invented cuts at both ends
    // of every linear molecule, in a window exactly `|ovhg|` wide:
    //
    //   * `AAAAAAAAAGGTCTCAAAAA` (20 bp, linear) reported BsaI at 17, but the
    //     bottom nick would have to fall between bases 20 and 21. The molecule
    //     is nicked, not cleaved, and `fragments` duly showed two bands — 16
    //     and 4 — where a gel shows one 20 bp species. Site starts 10..=13 all
    //     did this.
    //   * `TTTTTGAGACCTTTTTTTTT` (20 bp, linear) reported BsaI at 1, and there
    //     is no bond 5' of base 1. That is a bottom-strand-only nick, yet
    //     `pl digest` filed BsaI as a *unique cutter* — a linearisation
    //     candidate — while `fragments` returned a single full-length 20-mer,
    //     so the fragment list silently contradicted the cut list.
    //
    // Biopython reports no cut for either, which is the biology: an enzyme that
    // binds near the end and reaches past it binds, and finds nothing to cut.
    // Only Type IIS enzymes can reach this window; every Type IIP entry in the
    // table has `1 <= fst5 <= len - 1`, which puts both of its nicks inside the
    // molecule at every match position, so nothing about the 50 Type IIP
    // enzymes' behaviour changes here.
    let bond_exists = |bond: i64| (1..n as i64).contains(&bond);
    let ovhg = enzyme.ovhg as i64;
    let place = |cut0: i64| -> Option<u64> {
        if circular {
            // Wrapped back into 1..=n. Note this project never had Biopython's
            // too-short-doubling bug that `docs/PLAN.md` §7.1 warns about: the
            // *site* search walks the circle itself, so a Type IIS enzyme
            // reaching 11 bases past its site is found and placed correctly
            // however close to the origin it sits. Every bond on a circle
            // exists, so there is nothing to reject.
            Some(cut0.rem_euclid(n as i64) as u64 + 1)
        } else if bond_exists(cut0) && bond_exists(cut0 - ovhg) {
            Some(cut0 as u64 + 1)
        } else {
            None
        }
    };

    let mut out: Vec<CutSite> = iupac::find_all(enzyme.site.as_bytes(), seq, circular)
        .into_iter()
        // `find_all` gives the 1-based start of the site; the nick is `fst5`
        // further along.
        .filter_map(|start| {
            place(start as i64 - 1 + enzyme.fst5 as i64).map(|position| CutSite {
                position,
                site_start: start,
                strand: Strand::Forward,
            })
        })
        .collect();

    if antisense {
        out.extend(
            iupac::find_all(&rc_site, seq, circular)
                .into_iter()
                .filter_map(|start| {
                    place(start as i64 - 1 - back).map(|position| CutSite {
                        position,
                        site_start: start,
                        strand: Strand::Reverse,
                    })
                }),
        );
    }

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
        fragments_from_cuts(&self.positions, len, topology)
    }
}

/// Fragment lengths from a set of cut positions, descending.
///
/// Split out of [`Digest::fragments`] so a **combined** digest — several
/// enzymes in one tube, which is what a diagnostic double digest is — can merge
/// their positions and get the real pattern. Running each enzyme separately and
/// concatenating the results gives a different and wrong answer: a double
/// digest makes fragments shorter than either single digest does, and that is
/// usually the whole reason for doing it.
///
/// `positions` need not be sorted or unique. Two enzymes cutting at the same
/// base — an isoschizomer pair, or overlapping sites — make one cut and not
/// two, so duplicates collapse rather than producing a phantom zero-length
/// fragment.
pub fn fragments_from_cuts(positions: &[u64], len: u64, topology: Topology) -> Vec<u64> {
    if len == 0 {
        return Vec::new();
    }
    let mut p: Vec<u64> = positions.to_vec();
    p.sort_unstable();
    p.dedup();
    if p.is_empty() {
        return vec![len];
    }
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

    /// A doc comment that names a test must name one that exists.
    ///
    /// PROVEN TO FAIL at 713bd3b: two doc comments in this file
    /// (`Sets::discriminates` and `a_filter_that_can_hide_nothing_can_be_asked`)
    /// said the six-base invariant was "asserted by" and "pinned by" a test
    /// called specificity-counts-only-specified-bases. A repo-wide grep found
    /// that name in exactly those two sentences and nowhere else — there was no
    /// such test. The invariant is real and it is pinned, by
    /// `specificity_counts_specified_bases_not_raw_length`; what was broken was
    /// the pointer, which is the worse failure of the two, because a reader
    /// checking whether the claim is guarded greps the cited name, finds
    /// nothing, and cannot tell an unguarded claim from a renamed test.
    ///
    /// (The stale name is written with hyphens above so that this doc comment
    /// does not trip the rule it describes — which it did on the first run.)
    ///
    /// The rule: any backticked all-lowercase snake_case token of three or more
    /// underscores appearing in a `///` line of this file is read as naming an
    /// item in this file, and must be defined here. That shape is this
    /// project's test-naming convention and nothing else in the file uses it —
    /// at 713bd3b the scan yielded exactly one token, the phantom.
    #[test]
    fn every_test_a_doc_comment_names_is_defined_here() {
        const SRC: &str = include_str!("lib.rs");

        let defined: std::collections::BTreeSet<&str> = SRC
            .lines()
            .filter_map(|l| l.trim_start().strip_prefix("fn "))
            .map(|rest| rest.split(['(', '<', ' ']).next().unwrap_or(""))
            .collect();

        let mut checked = 0usize;
        for line in SRC.lines().filter(|l| l.trim_start().starts_with("///")) {
            for tok in line.split('`').skip(1).step_by(2) {
                let snake = !tok.is_empty()
                    && tok
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                    && tok.matches('_').count() >= 3;
                if !snake {
                    continue;
                }
                checked += 1;
                assert!(
                    defined.contains(tok),
                    "a doc comment cites `{tok}`, which is defined nowhere in \
                     this file — either the test was renamed or it never existed"
                );
            }
        }
        // Without this the whole test degrades to a loop that never runs, which
        // is the shape of defect this file's audit exists to catch.
        assert!(
            checked > 0,
            "no doc comment in this file names a test any more; if that is \
             deliberate, delete this test rather than leaving it green over an \
             empty loop"
        );
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
        // site means blunt -- is right for all 50 Type IIP enzymes here and
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

    /// Two of the five chips cannot change anything, on any molecule, against
    /// the table this program ships — and a UX pass renamed both of them on the
    /// stated grounds that they now matter more.
    ///
    /// The set definitions are older than that pass and the no-op is documented
    /// and pinned by `specificity_counts_specified_bases_not_raw_length`. What was missing
    /// is a way for the UI to ASK, so a user who clicks "6+ base sites" is told
    /// the control did nothing instead of inferring it from a picture that did
    /// not move. Asked of the digest, so a table that gains a four-cutter makes
    /// the chips live with no change at the call site.
    #[test]
    fn a_filter_that_can_hide_nothing_can_be_asked() {
        let mol = mol(
            "AAAAGAATTCCCCGGATCCTTTTAAGCTTGGGGGAATTCCCCC",
            Topology::Circular,
        );
        let results = digest_all(&mol);
        assert!(
            results.iter().filter(|d| d.count() > 0).count() >= 3,
            "the premise: something must cut"
        );
        assert!(
            EnzymeSet::Unique.discriminates(&results),
            "this molecule has a multi-cutter, so Unique hides something"
        );
        // Every enzyme in the shipped table has a 6-base or longer site, so
        // these two admit every cutter there is.
        assert!(!EnzymeSet::SixPlus.discriminates(&results));
        assert!(!EnzymeSet::All.discriminates(&results));
        // And the label says "6+", not "6": the set is `specificity() >= 6` and
        // the table holds seven- and eight-base cutters. PmeI is GTTTAAAC.
        assert!(EnzymeSet::SixPlus.label().contains("6+"));
        assert!(EnzymeSet::UniqueSixPlus.label().contains("6+"));
        assert_eq!(by_name("PmeI").unwrap().specificity(), 8);
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

    #[test]
    fn a_combined_digest_cuts_at_the_union_of_its_enzymes() {
        // A double digest is one tube. Running each enzyme separately and
        // stacking the fragment lists gives a different and wrong answer: the
        // real fragments are shorter than either single digest makes, which is
        // usually the whole reason for doing it.
        let len = 1000u64;
        let a = [100u64, 500];
        let b = [300u64, 800];
        let single_a = fragments_from_cuts(&a, len, Topology::Circular);
        let single_b = fragments_from_cuts(&b, len, Topology::Circular);
        let both = fragments_from_cuts(&[100, 500, 300, 800], len, Topology::Circular);
        assert_eq!(both, vec![300, 300, 200, 200]);
        assert!(
            both[0] < single_a[0] && both[0] < single_b[0],
            "the double digest's largest fragment is smaller than either single one"
        );
    }

    #[test]
    fn fragments_always_add_up_to_the_molecule() {
        // Checked on a real plasmid first: pACYC184-Ppho-fab2-6his cut with
        // EcoRI and BamHI gives 4358+3009+3028+2129+1869+1254 = 15647, its
        // exact length. A digest that loses or invents bases is wrong in a way
        // that still looks like a plausible gel.
        for topology in [Topology::Circular, Topology::Linear] {
            for cuts in [
                vec![],
                vec![1u64],
                vec![500],
                vec![1, 999],
                vec![100, 500, 300, 800],
                vec![7, 7, 7],
                vec![1, 2, 3, 998, 999, 1000],
            ] {
                let f = fragments_from_cuts(&cuts, 1000, topology);
                let sum: u64 = f.iter().sum();
                assert_eq!(
                    sum, 1000,
                    "{topology:?} with cuts {cuts:?} produced {f:?} summing to {sum}"
                );
                assert!(f.windows(2).all(|w| w[0] >= w[1]), "descending: {f:?}");
                assert!(f.iter().all(|x| *x > 0), "no empty fragments: {f:?}");
            }
        }
    }

    #[test]
    fn two_enzymes_cutting_the_same_base_make_one_cut() {
        // Isoschizomers, or two sites that happen to cut at the same place.
        // Counting the cut twice inserts a zero-length fragment, which is a
        // band that does not exist.
        let one = fragments_from_cuts(&[250, 750], 1000, Topology::Circular);
        let twice = fragments_from_cuts(&[250, 250, 750, 750], 1000, Topology::Circular);
        assert_eq!(one, twice);
        assert_eq!(one, vec![500, 500]);
    }

    #[test]
    fn an_unsorted_cut_list_gives_the_same_answer_as_a_sorted_one() {
        // Callers merge positions from several enzymes and have no reason to
        // sort first.
        let sorted = fragments_from_cuts(&[100, 300, 500, 800], 1000, Topology::Circular);
        let jumbled = fragments_from_cuts(&[800, 100, 500, 300], 1000, Topology::Circular);
        assert_eq!(sorted, jumbled);
    }

    #[test]
    fn a_type_iis_bottom_nick_running_off_the_3_prime_end_is_not_a_cut() {
        // BsaI 9 bases into a 20 bp linear duplex: the top nick lands on a real
        // bond (between bases 16 and 17) but the bottom nick would have to fall
        // between bases 20 and 21. The enzyme binds, reaches past the end and
        // nicks one strand. That is not a double-strand break, and Biopython
        // agrees it is not a cut.
        //
        // Reported as a cut it was worse than merely wrong: `fragments` turned
        // it into two gel bands, 16 and 4, for a duplex that runs off the gel
        // as a single 20 bp species, and `pl_clone::cut` handed back a 4-base
        // watson with an empty crick as though it were a ligatable fragment.
        let seq = b"AAAAAAAAAGGTCTCAAAAA";
        assert_eq!(seq.len(), 20);
        assert!(
            cuts(seq, Topology::Linear, "BsaI").is_empty(),
            "the bottom nick has no bond to break"
        );

        // The window is exactly |ovhg| = 4 wide. Every start inside it was
        // wrong in the same way, so pin the whole window rather than one case.
        for pad in 9..=12 {
            let mut s = vec![b'A'; pad];
            s.extend_from_slice(b"GGTCTC");
            s.resize(20, b'A');
            assert!(
                cuts(&s, Topology::Linear, "BsaI").is_empty(),
                "BsaI at 0-based {pad} of 20 reaches past the 3' end"
            );
        }
    }

    #[test]
    fn a_bottom_strand_only_nick_at_the_5_prime_end_is_not_a_cut() {
        // The mirror image, on the antisense path. `GAGACC` is the reverse
        // complement of `GGTCTC`, so BsaI binds the bottom strand here and
        // reaches leftwards: its bottom nick lands on the real bond between
        // bases 4 and 5, and its top nick falls off the 5' end entirely.
        //
        // The old guard admitted `cut0 == 0` and reported this as a cut at
        // position 1, where there is no bond. `pl digest` then filed BsaI as a
        // UNIQUE cutter -- i.e. offered it as a linearisation candidate for a
        // molecule it cannot linearise -- while `Digest::fragments` returned a
        // single full-length 20-mer, so the fragment list and the cut list
        // disagreed with each other and neither said so.
        let seq = b"TTTTTGAGACCTTTTTTTTT";
        assert_eq!(seq.len(), 20);
        let e = by_name("BsaI").unwrap();
        let positions = cut_positions(seq, Topology::Linear, e);
        assert!(positions.is_empty(), "got {positions:?}, expected no cut");

        // And the two answers now agree, which is the property that failed.
        let d = Digest {
            enzyme: e,
            positions,
        };
        assert!(!d.is_unique_cutter(), "not a linearisation candidate");
        assert_eq!(
            d.fragments(20, Topology::Linear),
            vec![20],
            "one band, and the cut list must say so too"
        );
    }

    #[test]
    fn a_type_iis_with_both_nicks_inside_a_linear_molecule_still_cuts() {
        // The control for the two tests above: one base further from each end
        // and both nicks land on real bonds, so the cut is real and must
        // survive. Tightening the guard by one too many would silence these.
        //
        // Forward, 0-based 8 of 20: top nick between bases 15 and 16, bottom
        // nick between 19 and 20 -- the last bond there is.
        let fwd = b"AAAAAAAAGGTCTCAAAAAA";
        assert_eq!(fwd.len(), 20);
        assert_eq!(cuts(fwd, Topology::Linear, "BsaI"), vec![16]);

        // Antisense, 0-based 6 of 20: top nick between bases 1 and 2, bottom
        // nick between 5 and 6. A lopsided cut, but a cut.
        let rev = b"TTTTTTGAGACCTTTTTTTT";
        assert_eq!(rev.len(), 20);
        assert_eq!(cuts(rev, Topology::Linear, "BsaI"), vec![2]);
    }

    #[test]
    fn every_type_iip_enzyme_still_cuts_a_padded_copy_of_its_own_site() {
        // The over-correction control. The end rule bounds *both* nicks, and a
        // Type IIP enzyme's two nicks are both inside its site, so no entry in
        // the table may lose a cut to it. If this ever fails, the guard has
        // stopped being about the ends of the molecule.
        let pad = 20; // wider than the longest reach in the table (AarI, 11)
        for e in ENZYMES {
            if e.cuts_outside_site() {
                continue;
            }
            let flank = "A".repeat(pad);
            let s = format!("{flank}{}{flank}", e.site);
            let want = pad as u64 + e.fst5 as u64 + 1;
            assert!(
                cut_positions(s.as_bytes(), Topology::Linear, e).contains(&want),
                "{} lost the cut at {want} in its own padded site",
                e.name
            );
        }
    }

    #[test]
    fn a_circle_has_no_ends_so_no_type_iis_cut_is_ever_dropped() {
        // The second control. The bond that does not exist is a property of a
        // linear molecule; on a circle every bond exists, including the one
        // that closes the origin. The same fixture that yields nothing linearly
        // must yield a cut circularly, or the end rule has leaked across the
        // topology boundary and plasmids would start losing sites.
        let seq = b"AAAAAAAAAGGTCTCAAAAA";
        assert!(cuts(seq, Topology::Linear, "BsaI").is_empty());
        assert_eq!(
            cuts(seq, Topology::Circular, "BsaI"),
            vec![17],
            "the bottom nick at 20 simply wraps to the origin on a 20 bp circle"
        );
    }

    #[test]
    fn a_cut_carries_the_site_that_produced_it_even_across_the_origin() {
        // Why `CutSite` exists. The GUI needs a site start to ask
        // `methylation::site_effect`, and reconstructing one from the cut
        // position is not possible: the reverse-strand path maps a match
        // through `start + k - bottom_cut`, not `start + fst5`, and
        // `cut_positions` sorts and dedups so a caller cannot even tell which
        // path a given position came from.
        //
        // The concrete failure: circular `CGATAAAAAAAAAGAT` carries a ClaI site
        // starting 0-based at 14 and wrapping the origin, cut correctly at 1.
        // Subtracting fst5 from that cut and clamping at 0 gave site start 0,
        // whose window contains no `GATC`, so a Dam-blocked enzyme was shown as
        // an unblocked unique cutter -- the panel offering to linearise a
        // plasmid with an enzyme that will not cut it.
        let seq = b"CGATAAAAAAAAAGAT";
        assert_eq!(seq.len(), 16);
        let e = by_name("ClaI").unwrap();

        let sites = cut_sites(seq, Topology::Circular, e);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].position, 1);
        assert_eq!(
            sites[0].site_start, 15,
            "1-based; the site wraps the origin"
        );
        assert_eq!(sites[0].strand, Strand::Forward);
        assert_eq!(
            cut_positions(seq, Topology::Circular, e),
            vec![sites[0].position],
            "the two entry points must not disagree"
        );

        let dam = pl_core::Methylation {
            dam: true,
            dcm: false,
            cpg: false,
            ecoki: false,
        };
        let verdict = methylation::site_effect(
            e,
            seq,
            sites[0].site_start as usize - 1,
            Topology::Circular,
            &dam,
        );
        assert_eq!(
            verdict.map(|v| v.effect),
            Some(methylation::Effect::Blocked),
            "GATC spans indices 13,14,15,0 with both methylated adenines in the site"
        );

        // The clamp this replaces, reproduced, so the difference is on record.
        let clamped = (sites[0].position as usize)
            .saturating_sub(1)
            .saturating_sub(e.fst5 as usize);
        assert_eq!(clamped, 0, "the old recovery landed here");
        assert_eq!(
            methylation::site_effect(e, seq, clamped, Topology::Circular, &dam),
            None,
            "and saw no methylation at all"
        );
    }

    #[test]
    fn a_reverse_strand_cut_is_labelled_as_one() {
        // The control for the above: a forward site start must not be reported
        // for a match the enzyme made on the bottom strand, or the site start
        // would be right by accident on palindromes and wrong on every Type IIS.
        let seq = b"TTTTTTTTTTGAGACCTTTTTTTTTT";
        let e = by_name("BsaI").unwrap();
        let sites = cut_sites(seq, Topology::Linear, e);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].strand, Strand::Reverse);
        assert_eq!(sites[0].site_start, 11, "1-based start of GAGACC");
        // 0-based 10, minus back = bottom_cut - k = 11 - 6 = 5, so cut0 = 5.
        assert_eq!(sites[0].position, 6);
        assert_ne!(
            sites[0].site_start as i64,
            sites[0].position as i64 - e.fst5 as i64,
            "position - fst5 is the wrong recovery on this strand, which is the point"
        );
    }

    #[test]
    fn the_table_holds_fifty_type_iip_enzymes_and_eight_type_iis() {
        // The prose said 51 Type IIP in four places (this file three times and
        // PROVENANCE.md), while `methylation.rs`, the `checked >= 50` assertion
        // there, and the independently transcribed oracle in
        // `reference/python/tests/validate_digest.py` all said 50. Nothing read
        // the number, so nothing failed; the cost was that anyone auditing the
        // `ovhg` column against the stated verification scope came up one
        // enzyme short and could not tell which claim was wrong. Pinned here so
        // the number in the docs has something executable behind it.
        let iis = ENZYMES.iter().filter(|e| e.cuts_outside_site()).count();
        assert_eq!(ENZYMES.len(), 58);
        assert_eq!(iis, 8);
        assert_eq!(ENZYMES.len() - iis, 50, "the count the docs must quote");
    }

    // -----------------------------------------------------------------------
    // compatible ends
    // -----------------------------------------------------------------------

    fn e(name: &str) -> &'static Enzyme {
        by_name(name).unwrap_or_else(|| panic!("{name} is not in the catalogue"))
    }

    /// Every catalogued overhang, against Biopython's own `ovhgseq`.
    ///
    /// Not a restatement: we derive the overhang by reading it off the site
    /// between the two nicks, Biopython carries REBASE's tabulated value. The
    /// run that produced these numbers compared all 58 — 50 Type IIP agreed on
    /// site, sign and bases with **zero** disagreements, and for the 8 Type IIS
    /// Biopython reports `fst5 > len(site)` and an `ovhgseq` of literally
    /// `NNNN`/`NNN`, which is its way of saying what `None` says here.
    ///
    /// Pinned rather than re-run: Biopython is not a build dependency, and a
    /// test that silently skips when an oracle is absent is a test that passes
    /// for the wrong reason. `BspQI` and `SapI` are in the list because their
    /// overhang is THREE bases, so a length check has something to fail on.
    #[test]
    fn the_overhangs_match_biopython_including_the_three_base_ones() {
        for (name, ovhg, want) in [
            ("AatII", 4i8, Some("ACGT")),
            ("AflII", -4, Some("TTAA")),
            ("ApaI", 4, Some("GGCC")),
            ("AscI", -4, Some("CGCG")),
            ("BsiWI", -4, Some("GTAC")),
            ("BstBI", -2, Some("CG")),
            ("ClaI", -2, Some("CG")),
            ("MfeI", -4, Some("AATT")),
            ("NcoI", -4, Some("CATG")),
            ("NdeI", -2, Some("TA")),
            ("NotI", -4, Some("GGCC")),
            ("SacII", 2, Some("GC")),
            ("SbfI", 4, Some("TGCA")),
            ("SphI", 4, Some("CATG")),
            // Type IIS: three bases, not four, and unknowable either way.
            ("BspQI", -3, None),
            ("SapI", -3, None),
            ("AarI", -4, None),
            ("PaqCI", -4, None),
        ] {
            let x = e(name);
            assert_eq!(x.ovhg, ovhg, "{name} overhang sign/length");
            assert_eq!(x.overhang_seq(), want, "{name} overhang bases");
        }
        // A three-base end cannot ligate to a four-base one, whatever the bases.
        assert_eq!(e("SapI").overhang_len(), 3);
        assert_eq!(e("BsaI").overhang_len(), 4);
        assert_eq!(e("SapI").ligates_with(e("BsaI")), Compatibility::Never);
    }

    /// The overhangs, against what the enzymes are known to leave.
    ///
    /// Read off the site and the two nicks rather than tabulated, so this is the
    /// check that the arithmetic agrees with the literature. `KpnI` and `BsrGI`
    /// are both here because they are the pair that makes polarity matter.
    #[test]
    fn the_overhang_each_enzyme_leaves_is_the_one_the_catalogues_publish() {
        for (name, want) in [
            ("BamHI", "GATC"),
            ("BglII", "GATC"),
            ("BclI", "GATC"),
            ("SalI", "TCGA"),
            ("XhoI", "TCGA"),
            ("NheI", "CTAG"),
            ("XbaI", "CTAG"),
            ("SpeI", "CTAG"),
            ("AvrII", "CTAG"),
            ("AgeI", "CCGG"),
            ("XmaI", "CCGG"),
            ("BspEI", "CCGG"),
            ("EcoRI", "AATT"),
            ("HindIII", "AGCT"),
            ("KpnI", "GTAC"),  // 3'
            ("BsrGI", "GTAC"), // 5' — same letters, other strand
            ("PstI", "TGCA"),  // 3'
            ("EcoRV", ""),     // blunt
            ("SmaI", ""),
        ] {
            assert_eq!(e(name).overhang_seq(), Some(want), "{name}");
        }
        // A Type IIS enzyme has no answer to give: its overhang is four bases of
        // the insert, not of the enzyme.
        for name in ["BsaI", "BsmBI", "BbsI", "Esp3I", "SapI", "AarI"] {
            assert_eq!(e(name).overhang_seq(), None, "{name}");
        }
    }

    /// The families a cloner treats as interchangeable, and the junction that
    /// decides whether the seam can be cut open again.
    #[test]
    fn compatible_families_ligate_and_their_junctions_say_what_survives() {
        for family in [
            vec!["BamHI", "BglII", "BclI"],
            vec!["SalI", "XhoI"],
            vec!["NheI", "XbaI", "SpeI", "AvrII"],
            vec!["AgeI", "XmaI", "BspEI"],
        ] {
            for a in &family {
                for b in &family {
                    assert_eq!(
                        e(a).ligates_with(e(b)),
                        Compatibility::Always,
                        "{a} and {b} leave the same overhang and must ligate"
                    );
                }
            }
        }

        // Cutting with one enzyme and re-closing puts the site back.
        assert_eq!(e("BamHI").junction(e("BamHI")).as_deref(), Some("GGATCC"));
        assert_eq!(e("KpnI").junction(e("KpnI")).as_deref(), Some("GGTACC"));
        assert_eq!(e("EcoRV").junction(e("EcoRV")).as_deref(), Some("GATATC"));

        // Joining two DIFFERENT members of a family destroys both sites, which
        // is the reason to do it: the seam cannot re-open.
        for (a, b, seam) in [
            ("BamHI", "BglII", "GGATCT"),
            ("BamHI", "BclI", "GGATCA"),
            ("XbaI", "SpeI", "TCTAGT"),
            ("SalI", "XhoI", "GTCGAG"),
            ("AgeI", "XmaI", "ACCGGG"),
        ] {
            let j = e(a)
                .junction(e(b))
                .expect("compatible ends make a junction");
            assert_eq!(j, seam, "{a}+{b}");
            for cutter in [a, b] {
                assert!(
                    cut_positions(j.as_bytes(), Topology::Linear, e(cutter)).is_empty(),
                    "{cutter} still cuts the {a}+{b} junction {j}"
                );
            }
        }
        // The control: the same machinery DOES find the site when it is there,
        // so "no cuts" above is a fact and not a broken search.
        assert_eq!(
            cut_positions(b"GGATCC", Topology::Linear, e("BamHI")).len(),
            1,
            "the search finds a real site, so finding none is meaningful"
        );
    }

    /// The same four bases on opposite strands. This is the case a length-and-
    /// letters comparison gets wrong, and it is a real trap: `KpnI` and `BsrGI`
    /// both read `GTAC` in a catalogue.
    #[test]
    fn the_same_bases_at_opposite_polarity_never_ligate() {
        let (kpn, bsrg) = (e("KpnI"), e("BsrGI"));
        assert_eq!(kpn.overhang_seq(), bsrg.overhang_seq());
        assert!(kpn.is_three_prime_overhang() && bsrg.is_five_prime_overhang());
        assert_eq!(kpn.ligates_with(bsrg), Compatibility::Never);
        assert_eq!(bsrg.ligates_with(kpn), Compatibility::Never);
        assert_eq!(kpn.junction(bsrg), None);
        // And a sticky end does not ligate to a blunt one.
        assert_eq!(e("BamHI").ligates_with(e("EcoRV")), Compatibility::Never);
        // Different bases, same polarity and length: also never.
        assert_eq!(e("BamHI").ligates_with(e("EcoRI")), Compatibility::Never);
        // Two 3' four-base ends whose bases simply differ.
        assert_eq!(e("SacI").overhang_seq(), Some("AGCT"));
        assert_eq!(e("PstI").overhang_seq(), Some("TGCA"));
        assert_eq!(e("SacI").ligates_with(e("PstI")), Compatibility::Never);
        // Same bases, DIFFERENT lengths: ClaI leaves CG over two bases, AscI
        // CGCG over four, and a two-base end cannot fill a four-base gap.
        assert_eq!(e("ClaI").overhang_len(), 2);
        assert_eq!(e("AscI").overhang_len(), 4);
        assert_eq!(e("ClaI").ligates_with(e("AscI")), Compatibility::Never);
    }

    /// An end whose bases the enzyme does not fix is a maybe, and must say so.
    #[test]
    fn an_unfixed_overhang_is_answered_with_sequence_not_with_yes() {
        // Type IIS: the whole basis of Golden Gate is that these are chosen per
        // construct. Claiming BsaI always ligates to BsaI would be the error
        // that silently scrambles an assembly.
        for name in ["BsaI", "BsmBI", "BbsI", "Esp3I"] {
            assert_eq!(
                e(name).ligates_with(e(name)),
                Compatibility::Sequence,
                "{name} to itself"
            );
            assert_eq!(e(name).junction(e(name)), None, "{name}");
            assert!(
                !e(name).partners().iter().any(|p| p.name == name),
                "{name} must not appear in a list of certain partners"
            );
        }
        // An IUPAC code inside the overhang is the same kind of maybe, reached
        // by a different route: BstXI's site fixes WHERE its overhang is and not
        // WHAT it says. Constructed rather than looked up, following the
        // precedent in `specificity`'s own test — the table has no interrupted
        // palindrome — which also means this branch is unreachable from the
        // shipped catalogue today and must still be right for the day it is not.
        let bst = Enzyme {
            name: "BstXI",
            site: "CCANNNNNNTGG",
            fst5: 8,
            ovhg: 4,
        };
        assert_eq!(bst.overhang_seq(), Some("NNNN"));
        assert_eq!(bst.ligates_with(&bst), Compatibility::Sequence);
        assert_eq!(bst.junction(&bst), None);
        // Sequence-dependent is not the same as impossible: an N can be a G, so
        // against another 3' four-base end the answer is "the DNA decides".
        assert_eq!(bst.ligates_with(e("KpnI")), Compatibility::Sequence);
        assert_eq!(bst.ligates_with(e("PstI")), Compatibility::Sequence);
        // Polarity is still settled before the bases are ever consulted, so an
        // N does not make it compatible with everything: BamHI is 5', BstXI 3'.
        assert_eq!(bst.ligates_with(e("BamHI")), Compatibility::Never);
        assert_eq!(bst.ligates_with(e("EcoRV")), Compatibility::Never);
    }

    /// Blunt is blunt: every flush end joins every other, and the junction is
    /// simply the two halves.
    #[test]
    fn any_blunt_end_ligates_to_any_other() {
        let blunt: Vec<&Enzyme> = ENZYMES.iter().filter(|x| x.is_blunt()).collect();
        assert!(blunt.len() >= 8, "only {} blunt cutters", blunt.len());
        for a in &blunt {
            for b in &blunt {
                assert_eq!(
                    a.ligates_with(b),
                    Compatibility::Always,
                    "{} {}",
                    a.name,
                    b.name
                );
            }
            assert!(
                a.partners().len() >= blunt.len(),
                "{} should list every blunt cutter",
                a.name
            );
        }
        assert_eq!(e("SmaI").junction(e("EcoRV")).as_deref(), Some("CCCATC"));
    }

    /// Whole-catalogue invariants, so a new enzyme cannot quietly break the
    /// relation.
    #[test]
    fn compatibility_is_symmetric_reflexive_where_it_can_be_and_free_of_maybes() {
        for a in ENZYMES {
            // Symmetric: ligation does not care which fragment you name first.
            for b in ENZYMES {
                assert_eq!(
                    a.ligates_with(b),
                    b.ligates_with(a),
                    "{} vs {} is not symmetric",
                    a.name,
                    b.name
                );
            }
            // An enzyme is never INCOMPATIBLE with itself: two ends off the same
            // cut always re-close, even when the bases are the insert's.
            assert_ne!(
                a.ligates_with(a),
                Compatibility::Never,
                "{} cannot re-close its own cut",
                a.name
            );
            // `partners` is the list a user reads as "interchangeable", so it
            // carries only certainties, and every entry survives a re-check.
            for p in a.partners() {
                assert_eq!(a.ligates_with(p), Compatibility::Always);
                assert_eq!(a.overhang_len(), p.overhang_len());
                assert_eq!(a.ovhg.signum(), p.ovhg.signum());
            }
        }
        // The families are found by search, not asserted into existence.
        let gatc: Vec<&str> = e("BamHI").partners().iter().map(|x| x.name).collect();
        assert_eq!(gatc, vec!["BamHI", "BclI", "BglII"]);
    }
}
