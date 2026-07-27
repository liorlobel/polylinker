//! Motif search across the library: the query nothing else offers.
//!
//! SnapGene searches sequence within the file you have open. Explorer searches
//! file names. Neither answers "which of my three thousand plasmids contains
//! this oligo" — which is the question that gets asked when a construct has
//! gone missing, and the reason this feature exists.
//!
//! Every clause below is a place where a tool of this kind normally lies, so
//! each is stated as a contract and tested as one.
//!
//! - **Degenerate patterns.** IUPAC codes throughout, matched through
//!   [`crate::nibble`], which is the same predicate `pl_enzymes` uses.
//! - **Asymmetric, inherited not re-decided.** Pattern `N` matches subject `A`;
//!   pattern `A` does *not* match subject `N`, because an unknown base is not
//!   evidence of a site. A record containing `N` can therefore silently *lose*
//!   a hit, which is why the ambiguous-base count is carried into every
//!   coverage report rather than left for the user to guess at.
//! - **Both strands, always.** The reverse complement of the *pattern* is
//!   scanned as a second pattern against the same forward store — never a
//!   reverse-complemented copy of a 24 Mbase corpus.
//! - **Palindromes are not double-reported.** Collapsed on masks, so `GAATTC`
//!   yields one hit per site rather than two.
//! - **Origin wrapping.** Circular molecules get `n` starts; a hit may end
//!   before it begins, and that span is reported wrapped rather than clamped.
//! - **Undeclared topology scans as circular**, a strict superset of the linear
//!   scan whose extra members are exactly the wrapping hits, and each such hit
//!   says so.
//! - **A pattern longer than the molecule matches nothing**, matching
//!   `pl_enzymes::cut_positions` rather than Biopython.

use crate::nibble;
use crate::Row;

/// Which strand a hit was found on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strand {
    Forward,
    Reverse,
    /// The pattern is its own reverse complement, so the site is both. Reported
    /// once — a palindromic site found twice is a miscount, not thoroughness.
    Both,
}

impl Strand {
    pub fn as_str(self) -> &'static str {
        match self {
            Strand::Forward => "+",
            Strand::Reverse => "-",
            Strand::Both => "±",
        }
    }
}

/// One occurrence, in the coordinates the rest of the product uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    /// 1-based inclusive, always on the plus strand.
    pub start: u64,
    /// 1-based inclusive. **Less than `start` when the hit wraps the origin**;
    /// a caller that clamps this has drawn the wrong picture.
    pub end: u64,
    pub strand: Strand,
    /// Did this hit cross the origin?
    pub wrapped: bool,
    /// Was it only found because an undeclared topology was scanned as
    /// circular? Such a hit is real if the molecule really is circular, and the
    /// file never said. Reported, never hidden and never silently asserted.
    pub assumed_circular: bool,
}

/// Why a pattern could not be searched for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MotifError {
    Empty,
    /// `.0` is the 0-based offset, `.1` the offending byte.
    NotACode(usize, u8),
}

impl std::fmt::Display for MotifError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MotifError::Empty => write!(f, "the pattern is empty"),
            MotifError::NotACode(i, b) => write!(
                f,
                "byte {} is {:?}, which is not an IUPAC nucleotide code and can never match",
                i + 1,
                *b as char
            ),
        }
    }
}

/// A validated pattern, and what searching for it will actually do.
///
/// Built once and reused across every record. It also exists so the UI can say
/// what it is about to search for *before* it searches — an empty result is
/// only legible as "searched and absent" if the user can see what was asked.
#[derive(Debug, Clone)]
pub struct Motif {
    /// As typed, for display.
    pub text: String,
    masks: Vec<u8>,
    rc_masks: Vec<u8>,
    /// Its own reverse complement, so one scan answers both strands.
    pub palindromic: bool,
    /// Positions carrying more than one base, for the "degenerate: W = A|T"
    /// note. Empty for a fully specified pattern.
    pub degenerate_at: Vec<usize>,
}

impl Motif {
    /// Validate a pattern, or say exactly which byte cannot ever match.
    ///
    /// Refusing is the whole point. `5'-GAATTC-3'` pasted from a supplier's
    /// order form, or a trailing space, describes nothing — and returning a
    /// clean empty result for it is the same silent failure this design
    /// rejected FTS5 for, in a smaller costume.
    pub fn new(pattern: &str) -> Result<Motif, MotifError> {
        let bytes = pattern.as_bytes();
        if bytes.is_empty() {
            return Err(MotifError::Empty);
        }
        let masks = nibble::pattern_masks(bytes).map_err(|(i, b)| MotifError::NotACode(i, b))?;
        let rc_masks = nibble::masks_reverse_complement(&masks);
        let degenerate_at = masks
            .iter()
            .enumerate()
            .filter(|(_, &m)| m.count_ones() > 1)
            .map(|(i, _)| i)
            .collect();
        Ok(Motif {
            text: pattern.to_string(),
            palindromic: nibble::is_palindrome(&masks),
            masks,
            rc_masks,
            degenerate_at,
        })
    }

    pub fn len(&self) -> usize {
        self.masks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.masks.is_empty()
    }

    /// A one-line statement of what a search will do, for the result header.
    ///
    /// This is the anti-silent-failure affordance: it makes an empty result
    /// legible as "searched and absent" rather than "did not search".
    pub fn describe(&self) -> String {
        let mut s = format!("{} ({} bp", self.text, self.len());
        if !self.degenerate_at.is_empty() {
            let codes: Vec<String> = self
                .degenerate_at
                .iter()
                .map(|&i| {
                    let m = self.masks[i];
                    let letters: String =
                        [(0b0001, 'A'), (0b0010, 'C'), (0b0100, 'G'), (0b1000, 'T')]
                            .iter()
                            .filter(|(bit, _)| m & bit != 0)
                            .map(|(_, c)| *c)
                            .collect::<Vec<char>>()
                            .iter()
                            .map(|c| c.to_string())
                            .collect::<Vec<String>>()
                            .join("|");
                    format!("{} = {letters}", nibble::base_for(m) as char)
                })
                .collect();
            // Distinct codes only; `NNNNN` should not print five times.
            let mut seen: Vec<String> = Vec::new();
            for c in codes {
                if !seen.contains(&c) {
                    seen.push(c);
                }
            }
            s.push_str(&format!(", degenerate: {}", seen.join(", ")));
        }
        s.push_str(if self.palindromic {
            ", palindromic"
        } else {
            ", not palindromic"
        });
        s.push_str(") — both strands");
        s
    }
}

/// Every hit of `motif` in one record's packed sequence.
///
/// `packed` is the whole library store; `row` names the slice. Hits come back
/// ordered by `(start, strand)`, which is what makes a whole-library result
/// deterministic without a sort at the end.
pub fn find_in_row(motif: &Motif, packed: &[u8], row: &Row) -> Vec<Hit> {
    if !row.state.searchable() || row.seq_bases == 0 {
        return Vec::new();
    }
    let n = row.seq_bases as usize;
    let circular = row.topology.scan_as_circular();
    let assumed = !row.topology.declared();
    let k = motif.len();

    // The store is shared, so offset into it rather than copying the record
    // out. `seq_off` is in bases; the nibble accessor takes base indices.
    let base = row.seq_off as usize;
    let slice = Slice { packed, base };

    let mut out: Vec<Hit> = Vec::new();
    let push = |starts: Vec<u64>, strand: Strand, out: &mut Vec<Hit>| {
        for s in starts {
            let i = (s - 1) as usize;
            let wrapped = circular && i + k > n;
            out.push(Hit {
                start: s,
                end: if circular {
                    ((i + k - 1) % n) as u64 + 1
                } else {
                    (i + k) as u64
                },
                strand,
                wrapped,
                assumed_circular: assumed,
            });
        }
    };

    if motif.palindromic {
        // One scan. Two would return every site twice, and "GAATTC found in
        // 2,576 files" reads exactly the same whether each site was counted
        // once or twice.
        push(
            slice.find(&motif.masks, n, circular),
            Strand::Both,
            &mut out,
        );
    } else {
        push(
            slice.find(&motif.masks, n, circular),
            Strand::Forward,
            &mut out,
        );
        push(
            slice.find(&motif.rc_masks, n, circular),
            Strand::Reverse,
            &mut out,
        );
    }
    out.sort_by_key(|h| (h.start, h.strand as u8));
    out
}

/// A record's window onto the shared packed store.
struct Slice<'a> {
    packed: &'a [u8],
    base: usize,
}

impl Slice<'_> {
    fn find(&self, masks: &[u8], n: usize, circular: bool) -> Vec<u64> {
        let k = masks.len();
        if n == 0 || k == 0 || k > n {
            return Vec::new();
        }
        let starts = if circular { n } else { n - k + 1 };
        let mut out = Vec::new();
        for i in 0..starts {
            let hit = (0..k).all(|j| {
                let idx = if circular { (i + j) % n } else { i + j };
                let s = nibble::mask_at(self.packed, self.base + idx);
                s != 0 && (s & !masks[j]) == 0
            });
            if hit {
                out.push(i as u64 + 1);
            }
        }
        out
    }
}

/// The bases a hit covers, for showing it in context or checking it.
pub fn hit_bases(packed: &[u8], row: &Row, hit: &Hit, k: usize) -> Vec<u8> {
    let n = row.seq_bases as usize;
    let i = (hit.start - 1) as usize;
    (0..k)
        .map(|j| {
            let idx = if row.topology.scan_as_circular() {
                (i + j) % n
            } else {
                i + j
            };
            nibble::base_for(nibble::mask_at(packed, row.seq_off as usize + idx))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{State, Topology};
    use pl_core::iupac::reverse_complement;

    fn row_of(seq: &[u8], topology: Topology, off: u64) -> Row {
        Row {
            state: State::Ok,
            topology,
            seq_off: off,
            seq_bases: seq.len() as u64,
            length: seq.len() as u64,
            ..Default::default()
        }
    }

    /// Pack one record at offset 0.
    fn one(seq: &[u8], topology: Topology) -> (Vec<u8>, Row) {
        (nibble::pack(seq), row_of(seq, topology, 0))
    }

    fn rng(state: &mut u64) -> u64 {
        *state ^= *state >> 12;
        *state ^= *state << 25;
        *state ^= *state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn random_seq(state: &mut u64, n: usize, alphabet: &[u8]) -> Vec<u8> {
        (0..n)
            .map(|_| alphabet[(rng(state) % alphabet.len() as u64) as usize])
            .collect()
    }

    /// Test 3: palindromes must not double-report.
    ///
    /// A hit *count* passes whether each site was counted once or twice, so the
    /// assertion is against `pl_enzymes`, whose answer agrees with Biopython.
    #[test]
    fn a_palindromic_site_is_reported_once_and_agrees_with_the_enzyme_scan() {
        let mut st = 0xabc_1234_5678_9001u64;
        for enzyme in pl_enzymes::ENZYMES {
            // Plant the site a few times in random sequence, on both a circle
            // and a line.
            let mut seq = random_seq(&mut st, 200, b"ACGT");
            for at in [10usize, 90, 150] {
                seq[at..at + enzyme.site.len()].copy_from_slice(enzyme.site.as_bytes());
            }
            for topo in [Topology::Circular, Topology::Linear] {
                let (packed, row) = one(&seq, topo);
                let motif = Motif::new(enzyme.site).unwrap();
                let hits = find_in_row(&motif, &packed, &row);

                let pl_topo = if topo == Topology::Circular {
                    pl_core::Topology::Circular
                } else {
                    pl_core::Topology::Linear
                };
                // `cut_positions` maps starts through cut_offset; compare
                // against an enzyme with offset zero so both report starts.
                let mut zeroed = *enzyme;
                zeroed.cut_offset = 0;
                let want = pl_enzymes::cut_positions(&seq, pl_topo, &zeroed);

                let got: Vec<u64> = hits.iter().map(|h| h.start).collect();
                assert_eq!(
                    got, want,
                    "{} on a {:?}: motif search and cut_positions disagree",
                    enzyme.name, topo
                );
                // Every shipped site is palindromic, so every hit is Both.
                assert!(
                    hits.iter().all(|h| h.strand == Strand::Both),
                    "{} should report ± once, not + and - separately",
                    enzyme.name
                );
            }
        }
    }

    /// Test 4: a minus-strand hit at the wrong coordinate is invisible in a
    /// count, so assert the bases rather than the index.
    #[test]
    fn a_reverse_hit_covers_bases_that_reverse_complement_to_the_pattern() {
        let mut st = 0x1111_2222_3333_4444u64;
        for _ in 0..400 {
            let seq = random_seq(&mut st, 60, b"ACGT");
            let pn = 4 + (rng(&mut st) % 5) as usize;
            let pattern = random_seq(&mut st, pn, b"ACGT");
            let text = String::from_utf8(pattern.clone()).unwrap();
            let motif = Motif::new(&text).unwrap();
            let (packed, row) = one(&seq, Topology::Circular);
            for h in find_in_row(&motif, &packed, &row) {
                let bases = hit_bases(&packed, &row, &h, motif.len());
                match h.strand {
                    Strand::Forward => assert_eq!(bases, pattern),
                    Strand::Reverse => assert_eq!(reverse_complement(&bases), pattern),
                    Strand::Both => {
                        assert_eq!(bases, pattern);
                        assert_eq!(reverse_complement(&bases), pattern);
                    }
                }
            }
        }
    }

    #[test]
    fn an_asymmetric_motif_finds_the_minus_strand_copy_the_enzyme_scan_would_miss() {
        // The concrete case: `cut_positions` scans forward only, which is right
        // for a palindromic restriction site and wrong for a user's oligo.
        //          1234567890123456789012
        //          A A A A A T G G G G G C C C A T T T T T
        let seq = b"AAAAATGGGGGCCCATTTTT";
        let (packed, row) = one(seq, Topology::Linear);
        let motif = Motif::new("ATG").unwrap();
        assert!(!motif.palindromic);
        let hits = find_in_row(&motif, &packed, &row);
        let fwd: Vec<u64> = hits
            .iter()
            .filter(|h| h.strand == Strand::Forward)
            .map(|h| h.start)
            .collect();
        let rev: Vec<u64> = hits
            .iter()
            .filter(|h| h.strand == Strand::Reverse)
            .map(|h| h.start)
            .collect();
        assert_eq!(fwd, vec![5], "the plus-strand ATG");
        assert_eq!(rev, vec![14], "CAT at 14..16 is ATG on the minus strand");
    }

    /// Test 5: origin wrapping that works on one molecule and nowhere else.
    #[test]
    fn rotating_a_circle_moves_every_hit_by_exactly_the_rotation() {
        let mut st = 0x9999_8888_7777_6666u64;
        for _ in 0..120 {
            let n = 30 + (rng(&mut st) % 40) as usize;
            let pn = 3 + (rng(&mut st) % 4) as usize;
            let seq = random_seq(&mut st, n, b"ACGT");
            let pattern = random_seq(&mut st, pn, b"ACGTRYWN");
            let motif = Motif::new(&String::from_utf8(pattern).unwrap()).unwrap();

            let (packed, row) = one(&seq, Topology::Circular);
            let base: Vec<(u64, Strand)> = find_in_row(&motif, &packed, &row)
                .iter()
                .map(|h| (h.start, h.strand))
                .collect();

            for r in 0..n {
                let mut rot = seq[r..].to_vec();
                rot.extend_from_slice(&seq[..r]);
                let (rp, rr) = one(&rot, Topology::Circular);
                let got: Vec<(u64, Strand)> = find_in_row(&motif, &rp, &rr)
                    .iter()
                    .map(|h| (h.start, h.strand))
                    .collect();
                let mut want: Vec<(u64, Strand)> = base
                    .iter()
                    .map(|&(p, s)| (((p - 1 + (n - r) as u64) % n as u64) + 1, s))
                    .collect();
                want.sort_by_key(|&(p, s)| (p, s as u8));
                assert_eq!(got, want, "rotation {r}");
            }
        }
    }

    #[test]
    fn a_hit_across_the_origin_ends_before_it_starts_and_says_so() {
        //         1234567890
        let seq = b"TTCXXXXGAA";
        let (packed, row) = one(seq, Topology::Circular);
        let motif = Motif::new("GAATTC").unwrap();
        let hits = find_in_row(&motif, &packed, &row);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].start, 8);
        assert_eq!(hits[0].end, 3, "a wrapped hit ends before it starts");
        assert!(hits[0].wrapped);
        assert!(!hits[0].assumed_circular, "circular was declared");

        // On a line the same coordinates describe nothing.
        let (packed, row) = one(seq, Topology::Linear);
        assert!(find_in_row(&motif, &packed, &row).is_empty());
    }

    #[test]
    fn an_undeclared_topology_finds_the_wrapping_hit_and_flags_it() {
        // The whole reason `Undeclared` exists. A Plasmidsaurus assembly of a
        // plasmid arrives as FASTA, at an arbitrary rotation, declaring
        // nothing. Read as linear, the origin-straddling site is lost.
        let seq = b"TTCXXXXGAA";
        let (packed, row) = one(seq, Topology::Undeclared);
        let motif = Motif::new("GAATTC").unwrap();
        let hits = find_in_row(&motif, &packed, &row);
        assert_eq!(hits.len(), 1, "an undeclared record is scanned as circular");
        assert!(hits[0].wrapped);
        assert!(
            hits[0].assumed_circular,
            "the file never said it was circular, and the hit must say so"
        );
    }

    /// Test 7: pattern validation.
    #[test]
    fn a_pattern_that_can_never_match_is_an_error_not_an_empty_result() {
        assert_eq!(Motif::new("").unwrap_err(), MotifError::Empty);
        assert_eq!(
            Motif::new("5'-GAATTC-3'").unwrap_err(),
            MotifError::NotACode(0, b'5')
        );
        assert_eq!(
            Motif::new("GAATTC GGATCC").unwrap_err(),
            MotifError::NotACode(6, b' ')
        );
        // And the message names the byte and its 1-based position.
        let msg = Motif::new("GAATTX").unwrap_err().to_string();
        assert!(msg.contains("byte 6"), "{msg}");
        assert!(msg.contains("'X'"), "{msg}");
    }

    #[test]
    fn records_without_bases_are_never_searched_and_never_match() {
        let seq = b"GAATTC";
        let packed = nibble::pack(seq);
        let motif = Motif::new("GAATTC").unwrap();
        for state in [
            State::NoBases,
            State::AnnotationTrack,
            State::NotASequenceFile,
            State::Unreadable,
            State::NotDownloaded,
            State::TooLarge,
            State::SuspectParse,
        ] {
            let mut row = row_of(seq, Topology::Circular, 0);
            row.state = state;
            assert!(
                find_in_row(&motif, &packed, &row).is_empty(),
                "{} must not be searched",
                state.as_str()
            );
        }
    }

    #[test]
    fn a_pattern_longer_than_the_record_matches_nothing() {
        // Matching `cut_positions`, not Biopython: no site binds a molecule
        // shorter than itself, however many times it would wrap.
        for topo in [Topology::Circular, Topology::Linear, Topology::Undeclared] {
            let (packed, row) = one(b"GAAT", topo);
            let motif = Motif::new("GAATTC").unwrap();
            assert!(find_in_row(&motif, &packed, &row).is_empty(), "{topo:?}");
        }
    }

    #[test]
    fn a_record_is_searched_at_its_own_offset_in_the_shared_store() {
        // The deepest risk in the whole feature: a wrong offset makes the index
        // answer confidently about the wrong molecule, and nothing in a
        // self-consistent round-trip can see it.
        let a = b"AAAAAAAAAA";
        let b = b"GGGAATTCGG";
        let mut all = a.to_vec();
        all.extend_from_slice(b);
        let packed = nibble::pack(&all);

        let ra = row_of(a, Topology::Linear, 0);
        let rb = row_of(b, Topology::Linear, a.len() as u64);
        let motif = Motif::new("GAATTC").unwrap();

        assert!(
            find_in_row(&motif, &packed, &ra).is_empty(),
            "record A has no site"
        );
        let hits = find_in_row(&motif, &packed, &rb);
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].start, 3,
            "coordinates are record-local, not store-global"
        );
        assert_eq!(hit_bases(&packed, &rb, &hits[0], 6), b"GAATTC".to_vec());
    }

    #[test]
    fn an_odd_offset_still_reads_the_right_nibbles() {
        // Two bases share a byte, so a record starting at an odd base index
        // reads the high nibble first. An off-by-one here would silently shift
        // every sequence in the library by one base.
        let a = b"AAAAA"; // odd length, so B starts at base index 5
        let b = b"GGGAATTCGG";
        let mut all = a.to_vec();
        all.extend_from_slice(b);
        let packed = nibble::pack(&all);
        let rb = row_of(b, Topology::Linear, a.len() as u64);
        let motif = Motif::new("GAATTC").unwrap();
        let hits = find_in_row(&motif, &packed, &rb);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].start, 3);
        assert_eq!(hit_bases(&packed, &rb, &hits[0], 6), b"GAATTC".to_vec());
    }

    #[test]
    fn the_header_says_what_will_actually_be_searched() {
        let m = Motif::new("GGWCC").unwrap();
        let d = m.describe();
        assert!(d.contains("GGWCC"), "{d}");
        assert!(d.contains("5 bp"), "{d}");
        assert!(d.contains("W = A|T"), "{d}");
        assert!(d.contains(", palindromic"), "{d}");
        assert!(d.contains("both strands"), "{d}");

        let m = Motif::new("GGTCTC").unwrap();
        let d = m.describe();
        assert!(d.contains("not palindromic"), "{d}");
        assert!(!d.contains("degenerate"), "{d}");

        // A repeated code is named once, not five times.
        let m = Motif::new("CCANNNNNTGG").unwrap();
        let d = m.describe();
        assert_eq!(d.matches("N = A|C|G|T").count(), 1, "{d}");
    }
}
