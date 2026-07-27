//! Open reading frames.
//!
//! # What counts as one
//!
//! A run that **begins at a start codon** for the chosen genetic code and ends
//! at the first in-frame stop. Not "the longest stretch between two stops",
//! which is the other common definition and answers a different question: a
//! stop-to-stop run has no start, so it is not a thing that could be
//! translated, and reporting one as an ORF puts a protein on a map that the
//! ribosome would never make.
//!
//! Which codons may start is the *table's* business, and it varies a lot.
//! Table 1 allows `ATG`, `CTG` and `TTG`; table 11 — the bacterial one, which
//! is what most plasmids want — allows seven, including `GTG` and `ATT`. A tool
//! that hard-codes `ATG` misses real bacterial genes; `tet(A)` starts `GTG`,
//! which this project has already been caught by once.
//!
//! # Incomplete frames are reported, not dropped
//!
//! On a linear molecule a reading frame can run off the end without ever
//! meeting a stop. That is a real and common thing — a cloned fragment, a
//! partial read — and [`Orf::complete`] says which kind you have. Dropping them
//! silently would hide exactly the ORFs a user is trying to find when they
//! sequence a fragment.
//!
//! # Circular molecules
//!
//! A frame wraps. On a circle there is no end to run off, so every ORF either
//! meets a stop or runs the whole way round, and a gene straddling the origin
//! is found like any other.
//!
//! One consequence is easy to get wrong: **if the length is not a multiple of
//! three, the three reading frames are not distinct.** Stepping by codons from
//! any position eventually reaches *every* position, so a 5,386 bp circle has
//! one cycle, not three. Running three separate frames over it reports every ORF
//! three times. That case runs a single pass here and labels each ORF by its
//! offset from the origin, because the alternative — triplicating every hit —
//! is the kind of wrong that looks like thoroughness.

use crate::translate::Code;
use crate::Strand;

/// One open reading frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orf {
    /// 1-based inclusive, on the plus strand, **including** the start codon and
    /// the stop codon when there is one.
    ///
    /// For a reverse-strand ORF these still read low-to-high on the plus
    /// strand; `strand` says which way it is read. On a circular molecule
    /// `end < start` means it crosses the origin.
    pub start: u64,
    pub end: u64,
    pub strand: Strand,
    /// 0, 1 or 2 — the offset of this frame from the start of the strand it is
    /// read on.
    pub frame: u8,
    /// Amino acids, excluding the stop.
    pub aa_len: usize,
    /// The start codon actually used, which is not always `ATG`.
    pub start_codon: [u8; 3],
    /// Did it reach a stop codon?
    ///
    /// `false` means the frame ran off the end of a linear molecule. Such an
    /// ORF is real and is reported; it just is not a whole gene.
    pub complete: bool,
    /// Did it cross the origin of a circular molecule?
    pub wrapped: bool,
}

impl Orf {
    /// Length in bases, including the stop codon when present.
    pub fn bases(&self) -> usize {
        self.aa_len * 3 + if self.complete { 3 } else { 0 }
    }
}

/// Settings for a search.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Shortest ORF worth reporting, in amino acids.
    ///
    /// 30 by default. Any threshold is arbitrary; this one is conventional and
    /// keeps a 5 kb plasmid from producing hundreds of three-residue "genes"
    /// that bury the real ones.
    pub min_aa: usize,
    /// Report frames that run off the end of a linear molecule.
    pub include_incomplete: bool,
    /// Require the *table's* start codons. With this off, any in-frame stretch
    /// ending at a stop is reported, beginning right after the previous stop —
    /// the stop-to-stop reading, offered because some workflows want it, but
    /// never the default.
    pub require_start: bool,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            min_aa: 30,
            include_incomplete: true,
            require_start: true,
        }
    }
}

/// Every ORF in all six frames.
///
/// Results are ordered by `(start, strand, frame)`, so two runs over the same
/// molecule agree byte for byte.
pub fn find_orfs(seq: &[u8], code: Code, circular: bool, p: &Params) -> Vec<Orf> {
    let n = seq.len();
    let mut out = Vec::new();
    if n < 3 {
        return out;
    }

    let rc = crate::iupac::reverse_complement(seq);
    // Codons before a circular frame arrives back where it began. When 3 does
    // not divide the length that is every position, not a third of them — see
    // the module docs.
    let per_turn = if n % 3 == 0 { n / 3 } else { n };
    let merged = circular && n % 3 != 0;
    let frames: &[u8] = if merged { &[0] } else { &[0, 1, 2] };

    for strand in [Strand::Forward, Strand::Reverse] {
        let s: &[u8] = if strand == Strand::Forward { seq } else { &rc };
        for &frame in frames {
            // Where to begin.
            //
            // On a circle the origin is an arbitrary cut, and it may fall in
            // the middle of a gene. Starting the scan there and taking the
            // first start codon it meets reports whichever start happens to sit
            // nearest the origin — so rotating the same molecule changes the
            // answer, and the ORF a user sees depends on where the file was
            // linearised. Synchronising to a stop first removes the origin from
            // the result entirely: from just past a stop, the next start is the
            // real one.
            let sync = if circular {
                let mut at = None;
                for j in 0..per_turn {
                    let i = frame as usize + 3 * j;
                    if code.is_stop(&[s[i % n], s[(i + 1) % n], s[(i + 2) % n]]) {
                        at = Some(i);
                        break;
                    }
                }
                at
            } else {
                None
            };

            let mut i = match sync {
                Some(at) => at + 3,
                None => frame as usize,
            };
            let mut k = 0usize; // codons consumed
            let mut open: Option<(usize, [u8; 3])> = None;
            loop {
                if circular {
                    // Exactly one turn. Starting just past a stop, the last
                    // codon of that turn *is* that stop again, so anything
                    // still open closes there and nothing needs a second lap.
                    // A frame with no stop at all has nothing to synchronise on
                    // and is handled after the loop.
                    if k >= per_turn {
                        break;
                    }
                } else if i + 3 > n {
                    break;
                }
                let c3 = if circular {
                    [s[i % n], s[(i + 1) % n], s[(i + 2) % n]]
                } else {
                    [s[i], s[i + 1], s[i + 2]]
                };

                if let Some((from, start_codon)) = open {
                    if code.is_stop(&c3) {
                        open = None;
                        let aa_len = (i - from) / 3;
                        if aa_len >= p.min_aa {
                            out.push(make(
                                from,
                                i + 3,
                                aa_len,
                                strand,
                                frame,
                                start_codon,
                                true,
                                n,
                                circular,
                                merged,
                            ));
                        }
                    }
                } else if !circular || k < per_turn {
                    let is_start = if p.require_start {
                        code.is_start(&c3)
                    } else {
                        !code.is_stop(&c3)
                    };
                    if is_start {
                        open = Some((i, c3));
                    }
                }

                i += 3;
                k += 1;
            }

            // A frame that ran off the end of a linear molecule.
            //
            // The circular case is deliberately *not* reported here. A frame
            // with no stop anywhere on a circle has no defensible start: every
            // start codon in it is equally first, and which one a scan reaches
            // depends only on where the file happened to be cut. Reporting one
            // makes the answer change when the same plasmid is rotated.
            // [`stopless_frames`] names those frames instead, which is the part
            // that is actually true.
            if let Some((from, start_codon)) = open {
                if p.include_incomplete && !circular {
                    let usable = ((n - from) / 3) * 3;
                    let aa_len = usable / 3;
                    if aa_len >= p.min_aa {
                        out.push(make(
                            from,
                            from + usable,
                            aa_len,
                            strand,
                            frame,
                            start_codon,
                            false,
                            n,
                            circular,
                            merged,
                        ));
                    }
                }
            }
        }
    }
    out.sort_by_key(|o| (o.start, o.strand as u8, o.frame, o.end));
    out.dedup();
    out
}

/// Reading frames with no stop codon anywhere on a circular molecule.
///
/// Returned as `(strand, frame)`. Such a frame translates for ever, so it has
/// no ORF — but it is a real and reportable property of the molecule, and
/// saying so is better than either inventing an arbitrary start for it or
/// dropping the fact silently. Always empty for a linear molecule, which ends
/// whether or not it stops.
pub fn stopless_frames(seq: &[u8], code: Code, circular: bool) -> Vec<(Strand, u8)> {
    let n = seq.len();
    if !circular || n < 3 {
        return Vec::new();
    }
    let per_turn = if n % 3 == 0 { n / 3 } else { n };
    let merged = n % 3 != 0;
    let frames: &[u8] = if merged { &[0] } else { &[0, 1, 2] };
    let rc = crate::iupac::reverse_complement(seq);
    let mut out = Vec::new();
    for strand in [Strand::Forward, Strand::Reverse] {
        let s: &[u8] = if strand == Strand::Forward { seq } else { &rc };
        for &frame in frames {
            let any = (0..per_turn).any(|j| {
                let i = frame as usize + 3 * j;
                code.is_stop(&[s[i % n], s[(i + 1) % n], s[(i + 2) % n]])
            });
            if !any {
                out.push((strand, frame));
            }
        }
    }
    out
}

/// Map a hit on the searched strand back to plus-strand coordinates.
#[allow(clippy::too_many_arguments)]
fn make(
    from: usize,
    to: usize,
    aa_len: usize,
    strand: Strand,
    frame: u8,
    start_codon: [u8; 3],
    complete: bool,
    n: usize,
    circular: bool,
    merged: bool,
) -> Orf {
    // Whether it crosses the origin, asked of the span itself. `to > n` is not
    // the same question: the scan may begin its turn late in the molecule, so a
    // span can have indices past `n` without ever passing position 1.
    let wrapped = circular && (from % n) + (to - from) > n;
    // On the reverse strand, index `k` of the reverse complement is plus-strand
    // index `n - 1 - k`. The span therefore flips end for end, which is why the
    // coordinates are computed rather than copied — getting this wrong puts
    // every reverse ORF at the mirror image of where it is.
    let (s0, e0) = if strand == Strand::Forward {
        (from % n, (to - 1) % n)
    } else {
        ((n - 1 - ((to - 1) % n)) % n, (n - 1 - (from % n)) % n)
    };
    Orf {
        start: s0 as u64 + 1,
        end: e0 as u64 + 1,
        strand,
        // When the frames merge there is no frame number to report, so the
        // offset from the origin stands in for one.
        frame: if merged { (from % n % 3) as u8 } else { frame },
        aa_len,
        start_codon,
        complete,
        wrapped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::{all_tables, table, TABLE1, TABLE11};
    use crate::Strand;

    fn p(min_aa: usize) -> Params {
        Params {
            min_aa,
            ..Default::default()
        }
    }

    /// Filler for the sense codons of a fixture gene.
    ///
    /// GC-only on purpose. `GCT` repeated spells `CTG` in frame 1 — a start
    /// codon in table 1 — so a fixture meant to contain one gene quietly
    /// contains a second ORF, and a test that counts them fails for a reason
    /// that has nothing to do with the code under test. That is not a
    /// hypothetical: it is what the first draft of this file did.
    const FILLER: &str = "GCC";

    /// The guard for the above, asserted rather than recalled.
    #[test]
    fn the_filler_these_fixtures_use_cannot_fabricate_an_orf() {
        let run = FILLER.repeat(8);
        let rc = crate::iupac::reverse_complement(run.as_bytes());
        for s in [run.as_bytes(), rc.as_slice()] {
            for f in 0..3 {
                for c in s[f..].chunks_exact(3) {
                    for code in crate::translate::all_tables() {
                        assert!(
                            !code.is_start(c),
                            "filler spells the start {} in table {}",
                            String::from_utf8_lossy(c),
                            code.id
                        );
                        assert!(
                            !code.is_stop(c),
                            "filler spells the stop {} in table {}",
                            String::from_utf8_lossy(c),
                            code.id
                        );
                    }
                }
            }
        }
    }

    /// Build a coding sequence: start, `n` sense codons, stop.
    fn gene(start: &str, n: usize, stop: &str) -> String {
        format!("{start}{}{stop}", FILLER.repeat(n))
    }

    #[test]
    fn a_simple_gene_is_found_with_its_start_and_stop_included() {
        let g = gene("ATG", 40, "TAA");
        let seq = format!("TTTTT{g}TTTTT");
        let orfs = find_orfs(seq.as_bytes(), TABLE1, false, &p(10));
        let fwd: Vec<&Orf> = orfs
            .iter()
            .filter(|o| o.strand == Strand::Forward)
            .collect();
        assert_eq!(fwd.len(), 1, "{orfs:?}");
        let o = fwd[0];
        assert_eq!(o.start, 6, "1-based start of the ATG");
        assert_eq!(o.end, 5 + g.len() as u64, "through the stop codon");
        assert_eq!(o.aa_len, 41, "40 GCT plus the initiator");
        assert!(o.complete);
        assert_eq!(&o.start_codon, b"ATG");
    }

    #[test]
    fn a_reverse_strand_gene_lands_where_it_really_is() {
        // The coordinate flip: index k of the reverse complement is plus-strand
        // index n-1-k, so the span turns end for end. Getting it wrong puts
        // every reverse ORF at the mirror image of its true position, which
        // still looks like a plausible ORF.
        let g = gene("ATG", 40, "TAA");
        let rc = String::from_utf8(crate::iupac::reverse_complement(g.as_bytes())).unwrap();
        let seq = format!("TTTTT{rc}TTTTT");
        let orfs = find_orfs(seq.as_bytes(), TABLE1, false, &p(10));
        let rev: Vec<&Orf> = orfs
            .iter()
            .filter(|o| o.strand == Strand::Reverse)
            .collect();
        assert_eq!(rev.len(), 1, "{orfs:?}");
        assert_eq!(rev[0].start, 6);
        assert_eq!(rev[0].end, 5 + g.len() as u64);
        assert_eq!(rev[0].aa_len, 41);

        // And the bases really do translate: read the plus-strand span, reverse
        // complement it, and it must be the gene we planted.
        let span = &seq.as_bytes()[(rev[0].start - 1) as usize..rev[0].end as usize];
        assert_eq!(
            String::from_utf8(crate::iupac::reverse_complement(span)).unwrap(),
            g
        );
    }

    #[test]
    fn a_bacterial_start_codon_is_found_only_with_a_table_that_allows_it() {
        // `tet(A)` starts GTG. A tool that hard-codes ATG misses real genes,
        // and this project has already been caught by that once.
        //
        // Table 11 lists GTG as an initiator; table 1 does not. (Table 2 does
        // too, which is why it is not the contrast here — that was checked
        // against the shipped tables rather than recalled.)
        assert!(TABLE11.is_start(b"GTG") && !TABLE1.is_start(b"GTG"));

        let g = gene("GTG", 40, "TAA");
        let seq = format!("TTTTT{g}TTTTT");

        let with11 = find_orfs(seq.as_bytes(), TABLE11, false, &p(10));
        assert!(
            with11
                .iter()
                .any(|o| o.strand == Strand::Forward && o.start == 6 && &o.start_codon == b"GTG"),
            "table 11 allows GTG: {with11:?}"
        );
        let with1 = find_orfs(seq.as_bytes(), TABLE1, false, &p(10));
        assert!(
            !with1
                .iter()
                .any(|o| o.strand == Strand::Forward && o.start == 6),
            "table 1 must not start here: {with1:?}"
        );
    }

    #[test]
    fn thirteen_tables_read_through_tga_and_the_orf_ends_somewhere_else() {
        // The fact that makes honouring /transl_table= a correctness matter and
        // not a nicety: in 13 of the 27 codes TGA is not a stop. Translate a
        // mitochondrial construct with table 1 and the protein ends early; use
        // the right table and the ORF runs on to the real stop. Both look
        // entirely plausible.
        let readthrough: Vec<u8> = all_tables()
            .filter(|c| !c.is_stop(b"TGA"))
            .map(|c| c.id)
            .collect();
        assert_eq!(
            readthrough,
            vec![2, 3, 4, 5, 9, 10, 13, 14, 21, 24, 25, 31, 33]
        );

        let seq = format!("ATG{}TGA{}TAA", FILLER.repeat(40), FILLER.repeat(40));
        let short = find_orfs(seq.as_bytes(), TABLE1, false, &p(10));
        let long = find_orfs(seq.as_bytes(), table(2).unwrap(), false, &p(10));
        let at1 = |v: &[Orf]| {
            v.iter()
                .find(|o| o.strand == Strand::Forward && o.start == 1)
                .map(|o| o.aa_len)
        };
        assert_eq!(at1(&short), Some(41), "table 1 stops at the TGA");
        assert_eq!(at1(&long), Some(82), "table 2 reads through it");
    }

    #[test]
    fn a_frame_that_runs_off_the_end_is_reported_as_incomplete() {
        // A cloned fragment or a partial read. Dropping these hides exactly the
        // ORFs someone sequencing a fragment is looking for.
        let seq = format!("ATG{}", FILLER.repeat(50));
        let orfs = find_orfs(seq.as_bytes(), TABLE1, false, &p(10));
        let o = orfs
            .iter()
            .find(|o| o.strand == Strand::Forward && o.start == 1)
            .expect("the frame from base 1");
        assert!(!o.complete, "it never meets a stop");
        assert_eq!(o.aa_len, 51);

        let strict = Params {
            include_incomplete: false,
            ..p(10)
        };
        assert!(find_orfs(seq.as_bytes(), TABLE1, false, &strict)
            .iter()
            .all(|o| o.complete));
    }

    #[test]
    fn a_gene_across_the_origin_is_found_on_a_circle() {
        // The case a linear reading cannot see at all.
        let g = gene("ATG", 30, "TAA");
        // Rotate so the gene straddles the origin: put its second half first.
        let half = g.len() / 2;
        let rotated = format!("{}TTTTTTTTT{}", &g[half..], &g[..half]);
        let n = rotated.len() as u64;
        let circ = find_orfs(rotated.as_bytes(), TABLE1, true, &p(10));
        let lin = find_orfs(rotated.as_bytes(), TABLE1, false, &p(10));

        // The gene begins 1-based 58 and ends at 48, having gone round.
        let o = circ
            .iter()
            .find(|o| o.strand == Strand::Forward && o.start == 58)
            .unwrap_or_else(|| panic!("the planted gene: {circ:?}"));
        assert!(o.wrapped && o.complete);
        assert_eq!(o.end, 48, "end < start: it crossed the origin");
        assert_eq!(o.aa_len, 31);

        // Read the span the way the coordinates say to, and it must be the gene
        // that was planted — the check that a plausible-looking wrap is the
        // right wrap.
        let b = rotated.as_bytes();
        let span: Vec<u8> = (0..o.bases())
            .map(|j| b[((o.start - 1 + j as u64) % n) as usize])
            .collect();
        assert_eq!(String::from_utf8(span).unwrap(), g);

        // And the linear reading cannot see it at all, which is the point.
        assert!(
            !lin.iter().any(|o| o.start == 58 && o.complete),
            "a linear molecule has no way round: {lin:?}"
        );
    }

    #[test]
    fn the_minimum_length_keeps_a_plasmid_from_drowning_in_tiny_orfs() {
        let seq = format!("ATG{}TAA", FILLER).repeat(20); // 2-aa ORFs, over and over
        assert!(find_orfs(seq.as_bytes(), TABLE1, false, &p(30)).is_empty());
        let tiny = find_orfs(seq.as_bytes(), TABLE1, false, &p(1));
        assert!(!tiny.is_empty(), "with no floor they are all reported");
    }

    #[test]
    fn stop_to_stop_reading_is_available_but_is_not_the_default() {
        // A stop-to-stop run has no start codon, so it is not something a
        // ribosome would make. Offered, never assumed.
        let seq = FILLER.repeat(60);
        assert!(
            find_orfs(seq.as_bytes(), TABLE1, false, &p(10)).is_empty(),
            "no start codon, so no ORF by default"
        );
        let loose = Params {
            require_start: false,
            ..p(10)
        };
        assert!(!find_orfs(seq.as_bytes(), TABLE1, false, &loose).is_empty());
    }

    #[test]
    fn a_circular_frame_with_no_stop_is_named_rather_than_given_a_start() {
        // Every start codon in such a frame is equally first; which one a scan
        // reaches depends only on where the file was cut. Reporting one makes
        // the answer change when the plasmid is rotated, so the frame is named
        // instead. GC-only filler cannot spell a stop in any table.
        let seq = format!("ATG{}", FILLER.repeat(59)); // 180 bp, no stop
        assert_eq!(seq.len() % 3, 0);
        let circ = find_orfs(seq.as_bytes(), TABLE1, true, &p(10));
        assert!(
            circ.iter().all(|o| o.complete),
            "no stopless ORF on a circle: {circ:?}"
        );

        let named = stopless_frames(seq.as_bytes(), TABLE1, true);
        assert!(!named.is_empty(), "the frames themselves are reported");
        assert!(stopless_frames(seq.as_bytes(), TABLE1, false).is_empty());

        // The linear reading does report it: a line has an end to run off.
        let lin = find_orfs(seq.as_bytes(), TABLE1, false, &p(10));
        assert!(lin.iter().any(|o| o.start == 1 && !o.complete));
    }

    #[test]
    fn rotating_a_circle_moves_every_orf_by_exactly_the_rotation() {
        // The invariant that caught the origin bug: the scan used to begin at
        // the arbitrary origin and take the first start it met, so a gene whose
        // real start lay before the cut was reported from the wrong place —
        // and the same plasmid, rotated, gave a different answer.
        let seq = format!(
            "{}{}{}",
            gene("ATG", 20, "TAA"),
            "GGCACGTTCAGGCATTAGCCAGGCTTGACAT",
            gene("GTG", 14, "TGA")
        );
        let n = seq.len();
        let base = find_orfs(seq.as_bytes(), TABLE11, true, &p(5));
        assert!(base.len() >= 2, "{base:?}");

        for r in [1, 2, 3, 7, 31, 64, n - 1] {
            let rot = format!("{}{}", &seq[n - r..], &seq[..n - r]);
            let got = find_orfs(rot.as_bytes(), TABLE11, true, &p(5));
            let want: Vec<(u64, u64, usize, bool)> = base
                .iter()
                .map(|o| {
                    (
                        (o.start - 1 + r as u64) % n as u64 + 1,
                        (o.end - 1 + r as u64) % n as u64 + 1,
                        o.aa_len,
                        o.strand == Strand::Reverse,
                    )
                })
                .collect();
            let mut want = want;
            want.sort();
            let mut have: Vec<(u64, u64, usize, bool)> = got
                .iter()
                .map(|o| (o.start, o.end, o.aa_len, o.strand == Strand::Reverse))
                .collect();
            have.sort();
            assert_eq!(have, want, "rotating by {r} changed the answer");
        }
    }

    #[test]
    fn the_search_is_deterministic() {
        let seq = format!("{}{}", gene("ATG", 40, "TAA"), gene("GTG", 35, "TGA"));
        let first = find_orfs(seq.as_bytes(), TABLE11, true, &p(10));
        for _ in 0..5 {
            assert_eq!(find_orfs(seq.as_bytes(), TABLE11, true, &p(10)), first);
        }
    }

    #[test]
    fn a_sequence_too_short_to_hold_a_codon_finds_nothing() {
        for s in ["", "A", "AT"] {
            assert!(find_orfs(s.as_bytes(), TABLE1, false, &p(1)).is_empty());
            assert!(find_orfs(s.as_bytes(), TABLE1, true, &p(1)).is_empty());
        }
    }
}
