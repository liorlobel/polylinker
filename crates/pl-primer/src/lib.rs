//! Where a primer anneals to a template, and what part of it does not.
//!
//! # The split that everything downstream depends on
//!
//! A primer has two parts, and conflating them is the mistake that makes every
//! later feature wrong:
//!
//! - the **footprint**, the 3' portion that actually pairs with the template;
//! - the **tail**, the 5' portion that does not — a restriction site being
//!   added, a Gibson arm, an att site, a barcode.
//!
//! `docs/PLAN.md` §7.3 is explicit that these are separate objects, and the
//! consequence that bites first is thermodynamic: **a 5' tail must not
//! contribute to Tm.** A 20 nt primer with a 20 nt Gibson arm is a 40-mer whose
//! annealing temperature is that of the 20-mer. Computing Tm over the whole
//! oligo reports a number ten degrees too high, the PCR is run too hot, and
//! nothing amplifies.
//!
//! # How a site is found
//!
//! A **3'-anchored seed**, because that is the end a polymerase extends from: a
//! primer mismatched at its 3' terminus does not prime, however well the rest
//! of it pairs. The seed is the last `seed_len` bases (default 14, matching
//! pydna's `limit` of 13 and SnapGene's behaviour), matched exactly, and the
//! footprint is then extended 5' allowing **isolated** mismatches.
//!
//! "Isolated" is doing real work. Extending with *free* mismatches, as a naive
//! reading of §7.3 suggests, would call a 20 nt Gibson arm an annealed region
//! with twenty mismatches — the exact conflation the footprint/tail split
//! exists to prevent. Two adjacent mismatches end the footprint, and everything
//! 5' of that is tail.

use pl_core::iupac::{matches, reverse_complement};
use pl_thermo::{tm, Method};

/// Which strand of the template the primer anneals to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strand {
    /// The primer reads as the plus strand; extension runs left to right.
    Forward,
    /// The primer reads as the minus strand; extension runs right to left.
    Reverse,
}

impl Strand {
    pub fn as_str(self) -> &'static str {
        match self {
            Strand::Forward => "+",
            Strand::Reverse => "-",
        }
    }
}

/// Search settings.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Length of the exactly-matched 3' seed.
    ///
    /// 14 by default. pydna uses 13; SnapGene's exact default is not published.
    pub seed_len: usize,
    /// Allow one mismatch inside the seed, as long as it is not the 3'-terminal
    /// base. Off by default: a seed is meant to be the part you are sure of.
    pub seed_mismatch: bool,
    /// Allow isolated mismatches when extending the footprint 5'.
    ///
    /// On by default, because a site-directed mutagenesis primer carries the
    /// change it is introducing and still anneals. Turn it **off** for
    /// behaviour identical to pydna and SnapGene, which stop the footprint at
    /// the first mismatch.
    ///
    /// The difference is not academic: with a random 5' tail, the base next to
    /// the footprint mismatches and the one beyond it matches by chance about a
    /// quarter of the time, so a lenient extension quietly absorbs two bases of
    /// tail into the footprint. That was six of eighty cases in the pydna
    /// differential, and it is why the flag exists rather than the rule being
    /// argued about.
    ///
    /// A footprint found this way carries a mismatch, and [`Binding::tm`] is
    /// then `None`: the nearest-neighbour model behind it describes a perfect
    /// duplex, and a perfect-duplex number for an imperfect footprint is wrong
    /// in the direction that kills the reaction.
    pub extend_mismatches: bool,
    /// Thermodynamics for the footprint Tm.
    pub tm_method: Method,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            seed_len: 14,
            seed_mismatch: false,
            extend_mismatches: true,
            tm_method: Method::default(),
        }
    }
}

/// One place a primer anneals.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    /// 1-based, inclusive, on the template's **plus strand**, whichever strand
    /// the primer anneals to.
    pub start: u64,
    pub end: u64,
    pub strand: Strand,
    /// The part of the primer that pairs, 5'->3' as the primer reads.
    pub footprint: Vec<u8>,
    /// The 5' part that does not pair. **Never contributes to Tm.**
    pub tail: Vec<u8>,
    /// Mismatched positions within the footprint, indexed from its 5' end.
    pub mismatches: Vec<usize>,
    /// Melting temperature of the footprint alone, in Celsius.
    ///
    /// `None` when the footprint is too short, holds an ambiguity code, or
    /// **carries a mismatch** — each of which is reported rather than guessed
    /// at.
    ///
    /// The mismatch case is the one that bites, and it was wrong here until
    /// 2026-07-29. [`pl_thermo`] models a *perfect* duplex and has no
    /// internal-mismatch parameters at all, so computing a Tm over a footprint
    /// that the `mismatches` field beside it says is imperfect answers a
    /// different question, and answers it **hot**. On the 31 bp template
    /// `TGCACGAATGAGAACAGAACCACAAATGGTG` the mutagenic primer
    /// `GCATGAGAACAGAACCACAA` reported 50.5 C where a mismatch-aware
    /// nearest-neighbour model gives 40.2 C — and, the tell, 2.8 C *above* the
    /// 47.7 C of its perfectly-paired parent `GAATGAGAACAGAACCACAA`.
    /// Introducing a mismatch cannot raise a melting temperature. Ten degrees
    /// hot is exactly the failure this module's header names: the PCR is run
    /// too hot and nothing amplifies.
    ///
    /// Making this number right rather than absent means mismatch
    /// nearest-neighbour tables, which `pl-thermo` does not have; until it
    /// does, the honest answer is no answer.
    pub tm: Option<f64>,
}

impl Binding {
    /// Does this primer carry a 5' tail?
    pub fn has_tail(&self) -> bool {
        !self.tail.is_empty()
    }
    pub fn footprint_str(&self) -> String {
        String::from_utf8_lossy(&self.footprint).to_string()
    }
    pub fn tail_str(&self) -> String {
        String::from_utf8_lossy(&self.tail).to_string()
    }
}

/// Extend a 3'-anchored match 5' along the template.
///
/// `p` and `t` are the primer and the template region, both oriented so that
/// index 0 is the 5' end of the primer. Returns the number of bases of the
/// primer that form the footprint, and the mismatched offsets within it.
///
/// Walks from the 3' end backwards and stops at two adjacent mismatches — the
/// point at which "annealed but imperfect" becomes "a tail that happens to
/// share a base".
fn extend(p: &[u8], t: &[u8], lenient: bool) -> (usize, Vec<usize>) {
    let n = p.len().min(t.len());
    let mut kept = 0usize;
    let mut mism: Vec<usize> = Vec::new();
    let mut prev_mismatch = false;
    for k in 0..n {
        let pi = p.len() - 1 - k;
        let ti = t.len() - 1 - k;
        let ok = matches(p[pi], t[ti]);
        if !ok {
            if !lenient {
                break;
            }
            if prev_mismatch {
                // Two in a row: the footprint ended one base ago.
                mism.pop();
                kept -= 1;
                break;
            }
            prev_mismatch = true;
            mism.push(pi);
        } else {
            prev_mismatch = false;
        }
        kept += 1;
    }
    // A footprint may not begin on a mismatch; that base belongs to the tail.
    while kept > 0 {
        let first = p.len() - kept;
        if mism.contains(&first) {
            mism.retain(|&m| m != first);
            kept -= 1;
        } else {
            break;
        }
    }
    let offset = p.len() - kept;
    (kept, mism.into_iter().map(|m| m - offset).rev().collect())
}

/// Every place `primer` anneals to `template`.
///
/// `circular` lets a site straddle the origin, which is the normal case for a
/// primer designed against a plasmid map that someone later rotated.
pub fn find_bindings(primer: &[u8], template: &[u8], circular: bool, p: &Params) -> Vec<Binding> {
    let mut out = Vec::new();
    let n = template.len();
    if primer.len() < p.seed_len || n == 0 {
        return out;
    }

    // The template as a doubled string for a circle, so a footprint that
    // crosses the origin is contiguous to look at; positions come back modulo n.
    let ext: Vec<u8> = if circular {
        let mut v = template.to_vec();
        v.extend_from_slice(template);
        v
    } else {
        template.to_vec()
    };

    for (strand, oriented) in [
        (Strand::Forward, primer.to_vec()),
        (Strand::Reverse, reverse_complement(primer)),
    ] {
        // For the reverse strand the primer, read on the plus strand, runs
        // backwards — so its 3' end is at the *left* of the match.
        let seed_from_left = strand == Strand::Reverse;
        let seed: Vec<u8> = if seed_from_left {
            oriented[..p.seed_len].to_vec()
        } else {
            oriented[oriented.len() - p.seed_len..].to_vec()
        };

        let last = if circular {
            n
        } else {
            n.saturating_sub(seed.len()) + 1
        };
        for i in 0..last {
            if i + seed.len() > ext.len() {
                break;
            }
            let window = &ext[i..i + seed.len()];
            let mism = seed
                .iter()
                .zip(window)
                .filter(|(a, b)| !matches(**a, **b))
                .count();
            let seed_ok = if p.seed_mismatch {
                // Never at the 3'-terminal base: a primer mismatched there does
                // not prime, whatever the rest of it does.
                let term = if seed_from_left { 0 } else { seed.len() - 1 };
                mism <= 1 && matches(seed[term], window[term])
            } else {
                mism == 0
            };
            if !seed_ok {
                continue;
            }

            // Extend 5' from the seed.
            let (footprint_len, mismatches, start0) = if seed_from_left {
                // Extend rightwards along the plus strand, which is 5'-ward on
                // the primer.
                //
                // Never more than one turn of the circle -- the same clamp the
                // 5'-extension branch below carries, which this branch was
                // missing until 2026-07-29. `ext` is the *doubled* template, so
                // `ext.len() - i` is more than one turn at every seed hit, and a
                // primer longer than the plasmid paired with template bases it
                // had already consumed. On the 20 bp circle
                // ACGGTTACCAGTTGCATCGA the 26 nt primer
                // AACCGTTCGATGCAACTGGTAACCGT came back as a 26 nt footprint
                // with no tail at 61.1 C, where the same 26 bases read on the
                // other strand -- which has clamped since 2026-07-28 -- give a
                // 20 nt footprint, a 6 nt tail and 54.7 C. It broke the
                // coordinates too: the reported span 1..6 is six bases against
                // a twenty-six base footprint. Marking a molecule circular must
                // not lengthen a footprint past the molecule nor delete a real
                // tail; the linear answer is the right one here.
                //
                // On a line `ext.len() == n`, so `.min(n)` is already implied by
                // `ext.len() - i` and needs no `circular` test of its own.
                let avail = (ext.len() - i).min(oriented.len()).min(n);
                let region: Vec<u8> = ext[i..i + avail].to_vec();
                let rp: Vec<u8> = oriented[..avail].iter().rev().copied().collect();
                let rt: Vec<u8> = region.iter().rev().copied().collect();
                let (k, m) = extend(&rp, &rt, p.extend_mismatches);
                (k, m, i)
            } else {
                let back = i + seed.len();
                // How much template we may look at 5' of the seed. On a line
                // that is bounded by the start of the template; on a circle it
                // is not, because "before position 1" is the end of the plasmid.
                //
                // Clamping the window at index 0 of the doubled buffer -- which
                // this did until 2026-07-28 -- does not merely shorten the
                // search, it *fabricates a tail*. For the 31 bp circle
                // CAAATGGTGTGCACGAATGAGAACAGAACCA and the primer
                // AACCACAAATGGTGTGCAC, a perfect 19/19 match to positions
                // 27..31 + 1..14, the seed hit at i = 0 could see only 14
                // template bases, so the five bases before the origin fell out
                // of the window: the footprint came back as the bare 14 nt
                // seed, five genuinely-annealing bases were reported as a 5'
                // tail, start was 1 instead of 27, and the reported Tm was the
                // 14-mer's -- 40.0 C against the true 51.2 C. Too *cold*, so
                // the anneal step is run 11 degrees under and primes wherever
                // it likes rather than failing loudly. Rotating the same
                // plasmid so the site did not cross the origin gave the right
                // answer, which is what made it an origin-dependent result.
                //
                // The reverse branch above never had this bug: it extends
                // rightwards, and the doubled buffer already extends that way.
                // The asymmetry was the tell.
                let avail = if circular {
                    // Never more than one turn of the circle, or a primer
                    // longer than the plasmid would pair with the same bases
                    // twice.
                    oriented.len().min(n)
                } else {
                    back.min(oriented.len())
                };
                let region: Vec<u8> = if circular {
                    // `back + n >= avail` because `avail <= n`, so this does
                    // not underflow the way `back - avail` does.
                    (0..avail)
                        .map(|d| template[(back + n - avail + d) % n])
                        .collect()
                } else {
                    ext[back - avail..back].to_vec()
                };
                let (k, m) = extend(
                    &oriented[oriented.len() - avail..],
                    &region,
                    p.extend_mismatches,
                );
                let start0 = if circular {
                    // Same reason: the footprint may begin before the origin.
                    (back + n - k) % n
                } else {
                    back - k
                };
                (k, m, start0)
            };
            if footprint_len < p.seed_len {
                continue;
            }
            if start0 >= n && circular {
                continue; // the same site, found again on the second copy
            }

            let footprint: Vec<u8> = if seed_from_left {
                // On the primer's own reading, the footprint is its 3' end.
                primer[primer.len() - footprint_len..].to_vec()
            } else {
                primer[primer.len() - footprint_len..].to_vec()
            };
            let tail: Vec<u8> = primer[..primer.len() - footprint_len].to_vec();

            let end0 = start0 + footprint_len - 1;
            out.push(Binding {
                start: (start0 % n) as u64 + 1,
                end: (end0 % n) as u64 + 1,
                strand,
                // A footprint holding a mismatch is not a perfect duplex, and
                // `pl_thermo` knows no other kind, so its number would be a
                // different question's answer -- roughly ten degrees hot, the
                // direction that kills the reaction. Refuse rather than guess;
                // see `Binding::tm`.
                tm: if mismatches.is_empty() {
                    tm(&footprint, &p.tm_method).ok().map(|t| t.tm)
                } else {
                    None
                },
                footprint,
                tail,
                mismatches,
            });
        }
    }
    out.sort_by_key(|b| (b.start, b.strand as u8, b.end));
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A base that is definitely not the one already there.
    ///
    /// Writing a literal is how three tests in this module silently became
    /// no-ops when the fixture changed: `primer[1] = b'A'` where the base was
    /// already `A` mutates nothing and asserts nothing.
    fn flip(b: u8) -> u8 {
        match b {
            b'A' => b'C',
            b'C' => b'A',
            b'G' => b'T',
            _ => b'G',
        }
    }

    fn p() -> Params {
        Params {
            seed_len: 10,
            ..Default::default()
        }
    }

    #[test]
    fn a_perfect_primer_anneals_where_it_should_with_no_tail() {
        // A deliberately non-repetitive template. The first fixture here was
        // `TTTTTGGATCCACGTACGTACGGGGCCCTTT`, and `ACGTACGTACG` is nearly its
        // own reverse complement -- `ACGT` is a palindrome, so any run of it
        // is one -- which gave the primer a genuine second site on the other
        // strand and made every count in this module look wrong. The detector
        // was right; the fixture was pathological.
        //                1234567890123456789012345678901
        let template = b"TGCACGAATGAGAACAGAACCACAAATGGTG";
        let primer = b"GAATGAGAACAGAACCA";
        let b = find_bindings(primer, template, false, &p());
        assert_eq!(b.len(), 1, "{b:?}");
        assert_eq!(b[0].strand, Strand::Forward);
        assert_eq!(b[0].start, 6);
        assert_eq!(b[0].footprint, primer.to_vec());
        assert!(!b[0].has_tail());
        assert!(b[0].mismatches.is_empty());
        assert!(b[0].tm.is_some());
    }

    #[test]
    fn a_five_prime_tail_is_split_off_and_does_not_reach_the_tm() {
        // The case the footprint/tail split exists for: a Gibson arm or an
        // added restriction site pairs with nothing, and counting it in the Tm
        // reports a number far too high, the PCR is run too hot, and nothing
        // amplifies.
        let template = b"TGCACGAATGAGAACAGAACCACAAATGGTG";
        let core = b"GAGAACAGAACCA";
        let tail = b"GAATTCGCGGCCGC";
        let mut primer = tail.to_vec();
        primer.extend_from_slice(core);

        let b = find_bindings(&primer, template, false, &p());
        assert_eq!(b.len(), 1, "{b:?}");
        assert_eq!(b[0].footprint, core.to_vec());
        assert_eq!(b[0].tail, tail.to_vec());
        assert!(b[0].has_tail());

        // The Tm is the footprint's, not the whole oligo's, and they are far
        // apart.
        let whole = tm(&primer, &Method::default()).unwrap().tm;
        let foot = tm(core, &Method::default()).unwrap().tm;
        assert!(
            whole - foot > 8.0,
            "the fixture must make the difference obvious: {whole} vs {foot}"
        );
        assert!((b[0].tm.unwrap() - foot).abs() < 1e-9);
    }

    #[test]
    fn a_reverse_primer_is_found_and_its_coordinates_are_on_the_plus_strand() {
        let template = b"TGCACGAATGAGAACAGAACCACAAATGGTG";
        // The reverse complement of positions 6..22.
        let region = &template[5..22];
        let primer = reverse_complement(region);
        let b = find_bindings(&primer, template, false, &p());
        assert_eq!(b.len(), 1, "{b:?}");
        assert_eq!(b[0].strand, Strand::Reverse);
        assert_eq!(b[0].start, 6);
        assert_eq!(b[0].end, 22);
        assert!(!b[0].has_tail());
    }

    #[test]
    fn the_three_prime_terminal_base_must_match() {
        // A polymerase extends from the 3' end. A primer mismatched there does
        // not prime, however well the rest of it pairs -- and reporting it as a
        // binding site would predict a product that never appears.
        let template = b"TGCACGAATGAGAACAGAACCACAAATGGTG";
        let mut primer = b"GAATGAGAACAGAACCA".to_vec();
        let last = primer.len() - 1;
        primer[last] = flip(primer[last]);
        assert!(
            find_bindings(&primer, template, false, &p()).is_empty(),
            "a 3'-mismatched primer must not be reported as annealing"
        );

        // With seed mismatches allowed it is still refused, because the rule is
        // about the terminal base specifically.
        let lenient = Params {
            seed_mismatch: true,
            ..p()
        };
        assert!(find_bindings(&primer, template, false, &lenient).is_empty());
    }

    #[test]
    fn an_isolated_internal_mismatch_extends_the_footprint_but_is_recorded() {
        // Site-directed mutagenesis: the primer carries the change it is
        // introducing, and still anneals.
        let template = b"TGCACGAATGAGAACAGAACCACAAATGGTG";
        let mut primer = b"GAATGAGAACAGAACCA".to_vec();
        primer[1] = flip(primer[1]); // one base in from the 5' end
        let b = find_bindings(&primer, template, false, &p());
        assert_eq!(b.len(), 1, "{b:?}");
        assert_eq!(b[0].mismatches.len(), 1, "{:?}", b[0]);
        assert!(!b[0].has_tail(), "an isolated mismatch is not a tail");
    }

    #[test]
    fn two_adjacent_mismatches_end_the_footprint() {
        // The rule that stops a 20 nt Gibson arm being called "annealed with
        // twenty mismatches", which is the conflation this module exists to
        // prevent.
        let template = b"TGCACGAATGAGAACAGAACCACAAATGGTG";
        let mut primer = b"GAATGAGAACAGAACCA".to_vec();
        primer[3] = flip(primer[3]);
        primer[4] = flip(primer[4]);
        let b = find_bindings(&primer, template, false, &p());
        assert_eq!(b.len(), 1, "{b:?}");
        assert!(b[0].has_tail(), "the mismatched run belongs to the tail");
        assert!(
            b[0].footprint.len() < primer.len(),
            "footprint {} vs primer {}",
            b[0].footprint.len(),
            primer.len()
        );
        // And the footprint never begins on a mismatch.
        assert!(!b[0].mismatches.contains(&0));
    }

    #[test]
    fn a_site_across_the_origin_is_found_on_a_circle_and_not_on_a_line() {
        // The template above, rotated so the primer's site straddles the
        // origin: bases 19..31 followed by 1..18.
        //                 1234567890123456789012345678901
        let template = b"CAAATGGTGTGCACGAATGAGAACAGAACCA";
        let primer = b"GAACCACAAATGGTG";
        let circ = find_bindings(primer, template, true, &p());
        let lin = find_bindings(primer, template, false, &p());
        assert!(
            circ.len() > lin.len(),
            "a circle should find at least the wrapping site: {circ:?} vs {lin:?}"
        );
        assert!(circ.iter().any(|b| b.end < b.start), "a wrapped footprint");
    }

    #[test]
    fn a_footprint_reaching_back_over_the_origin_is_not_reported_as_a_tail() {
        // The one existing circular test happens to exercise the case where the
        // *seed itself* straddles the origin, which was always right because
        // `back = i + seed_len` then runs past `n` and the window is big enough.
        // The broken case is the other one: the seed lies wholly after the
        // origin and only the 5' extension needs bases from before it.
        //
        // This primer is a perfect 19/19 match to positions 27..31 + 1..14. The
        // clamp at index 0 of the doubled buffer returned start 1, a bare 14 nt
        // footprint and a fabricated 5 nt tail "AACCA", with the 14-mer's Tm
        // (40.0 C) standing in for the real 19-mer's (51.2 C) -- eleven degrees
        // *low*, so the anneal step is run cold and primes everywhere.
        //                1234567890123456789012345678901
        let template = b"CAAATGGTGTGCACGAATGAGAACAGAACCA";
        let primer = b"AACCACAAATGGTGTGCAC";
        // Params::default(), not p(): seed_len 14 is what ships, and the bug
        // needs the seed to be shorter than the primer.
        let b = find_bindings(primer, template, true, &Default::default());
        assert_eq!(b.len(), 1, "{b:?}");
        assert_eq!(b[0].strand, Strand::Forward);
        assert_eq!(b[0].start, 27, "the site begins before the origin");
        assert_eq!(b[0].end, 14, "and wraps past it");
        assert_eq!(b[0].footprint, primer.to_vec(), "all 19 bases pair");
        assert!(!b[0].has_tail(), "fabricated tail {:?}", b[0].tail_str());
        let whole = tm(primer, &Method::default()).unwrap().tm;
        assert!(
            (b[0].tm.unwrap() - whole).abs() < 1e-9,
            "the Tm must be the 19-mer's, got {:?} against {whole}",
            b[0].tm
        );

        // Control: the clamp is *correct* on a line. There really is no
        // template before position 1, so those five bases really are a tail,
        // and the linear answer must not change.
        let lin = find_bindings(primer, template, false, &Default::default());
        assert_eq!(lin.len(), 1, "{lin:?}");
        assert_eq!(lin[0].start, 1);
        assert_eq!(lin[0].footprint.len(), 14);
        assert_eq!(lin[0].tail_str(), "AACCA");
    }

    #[test]
    fn rotating_the_plasmid_does_not_change_where_a_primer_anneals() {
        // The same molecule with the origin moved: an answer that depends on
        // where someone happened to rotate the map is not an answer.
        let primer = b"AACCACAAATGGTGTGCAC";
        let across = b"CAAATGGTGTGCACGAATGAGAACAGAACCA";
        let clear = b"AACCACAAATGGTGTGCACGAATGAGAACAG";

        let a = find_bindings(primer, across, true, &Default::default());
        let c = find_bindings(primer, clear, true, &Default::default());
        assert_eq!(a.len(), 1, "{a:?}");
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].start, 1, "the control site does not cross the origin");
        assert_eq!(c[0].end, 19);
        assert_eq!(a[0].footprint, c[0].footprint);
        assert_eq!(a[0].tail, c[0].tail);
        assert_eq!(a[0].tm, c[0].tm);
        // The span is the same length whichever side of the origin it starts.
        assert_eq!(a[0].footprint.len(), 19);
    }

    #[test]
    fn a_primer_shorter_than_the_seed_finds_nothing_rather_than_everything() {
        let template = b"TGCACGAATGAGAACAGAACCACAAATGGTG";
        assert!(find_bindings(b"GAATGA", template, false, &p()).is_empty());
        assert!(find_bindings(b"", template, false, &p()).is_empty());
        assert!(find_bindings(b"GAATGAGAACAGAACCA", b"", false, &p()).is_empty());
    }

    #[test]
    fn a_footprint_never_begins_on_a_mismatch() {
        // The trim at the end of `extend`, which nothing exercised until this
        // test existed: disabling it left every other test passing.
        //
        // The case is a primer whose 5'-most base is the only mismatch. The
        // extension loop runs out of primer rather than hitting two adjacent
        // mismatches, so without the trim the footprint would start on a base
        // that does not pair -- and a footprint is, by definition, the part
        // that pairs.
        let template = b"TGCACGAATGAGAACAGAACCACAAATGGTG";
        let mut primer = b"GAATGAGAACAGAACCA".to_vec();
        primer[0] = flip(primer[0]);

        let b = find_bindings(&primer, template, false, &p());
        assert_eq!(b.len(), 1, "{b:?}");
        assert!(
            !b[0].mismatches.contains(&0),
            "the footprint begins on a mismatch: {:?}",
            b[0]
        );
        assert_eq!(b[0].tail, vec![primer[0]], "that base is the tail");
        assert_eq!(b[0].footprint, primer[1..].to_vec());
    }

    #[test]
    fn strict_extension_stops_at_the_first_mismatch() {
        // The mode that matches pydna and SnapGene. With a random 5' tail the
        // base next to the footprint mismatches and the one beyond it matches
        // by chance often enough that a lenient extension absorbs two bases of
        // tail -- six of eighty cases in the differential.
        let template = b"TGCACGAATGAGAACAGAACCACAAATGGTG";
        let mut primer = b"GAATGAGAACAGAACCA".to_vec();
        primer[1] = flip(primer[1]);

        let lenient = find_bindings(&primer, template, false, &p());
        assert_eq!(lenient[0].mismatches.len(), 1);
        assert!(!lenient[0].has_tail());

        let strict = Params {
            extend_mismatches: false,
            ..p()
        };
        let strict = find_bindings(&primer, template, false, &strict);
        assert_eq!(strict.len(), 1, "{strict:?}");
        assert!(
            strict[0].mismatches.is_empty(),
            "strict mode reports no mismatch inside the footprint"
        );
        assert!(
            strict[0].has_tail(),
            "everything 5' of the mismatch is tail"
        );
        assert!(strict[0].footprint.len() < lenient[0].footprint.len());
    }

    /// PROVEN TO FAIL at dfd6ac9: `tm` was computed over the footprint
    /// unconditionally, so this reported `Some(45.73)` and the final assertion
    /// -- that the mismatched primer's perfect-duplex number is *hotter* than
    /// the perfectly-paired parent's -- documents why that number was not
    /// merely imprecise.
    #[test]
    fn a_mismatched_footprint_gets_no_tm_rather_than_a_perfect_duplex_one() {
        // The site-directed mutagenesis case, which is what `extend_mismatches`
        // is on by default for. The footprint pairs everywhere but one base;
        // `pl_thermo` has no internal-mismatch parameters, so its answer for
        // these bases is the answer for a duplex that does not exist.
        let template = b"TGCACGAATGAGAACAGAACCACAAATGGTG";
        let parent = b"GAATGAGAACAGAACCA".to_vec();
        let mut primer = parent.clone();
        primer[1] = flip(primer[1]); // one base in from the 5' end

        let m = find_bindings(&primer, template, false, &p());
        assert_eq!(m.len(), 1, "{m:?}");
        assert_eq!(m[0].mismatches.len(), 1, "{:?}", m[0]);
        assert_eq!(
            m[0].footprint, primer,
            "the mismatch is inside the footprint, not split off as tail"
        );
        assert!(
            m[0].tm.is_none(),
            "a mismatched footprint is not a perfect duplex, so it has no \
             perfect-duplex Tm; got {:?} C",
            m[0].tm
        );

        // A perfect footprint still gets one -- the refusal is about the
        // mismatch, not about giving up on Tm.
        let ok = find_bindings(&parent, template, false, &p());
        assert_eq!(ok.len(), 1, "{ok:?}");
        let parent_tm = ok[0].tm.expect("a perfect footprint keeps its Tm");

        // And the number that used to be reported was wrong in the direction
        // the module header calls fatal: introducing a mismatch made it go UP.
        let as_if_perfect = tm(&m[0].footprint, &Method::default()).unwrap().tm;
        assert!(
            as_if_perfect > parent_tm + 2.0,
            "the fixture must make the error unmistakable: the mismatched \
             primer's perfect-duplex Tm is {as_if_perfect} C against the \
             parent's {parent_tm} C"
        );
    }

    /// PROVEN TO FAIL at dfd6ac9: the reverse branch's `avail` was
    /// `(ext.len() - i).min(oriented.len())` with no one-turn clamp, so this
    /// reported a 26 nt footprint with an empty tail on a 20 bp molecule, span
    /// `1..6`, at 61.1 C.
    #[test]
    fn a_primer_longer_than_the_circle_does_not_pair_the_same_bases_twice() {
        //                1234567890123456789012345678901234567890
        let template = b"ACGGTTACCAGTTGCATCGA"; // 20 bp
        let n = template.len();
        // Reverse-strand: the reverse complement of the whole circle plus six
        // bases of a second turn. Only one turn can physically pair; the extra
        // six bases are a 5' tail like any other.
        let primer = b"AACCGTTCGATGCAACTGGTAACCGT"; // 26 nt

        let circ = find_bindings(primer, template, true, &Default::default());
        assert!(!circ.is_empty(), "the site is real, only over-long");
        for b in &circ {
            assert!(
                b.footprint.len() <= n,
                "a {} nt footprint on a {n} bp molecule pairs {} bases twice: {}",
                b.footprint.len(),
                b.footprint.len() - n,
                b.footprint_str()
            );
            // The span the coordinates describe must be the number of bases
            // that pair. This is the cheaper half of the same defect: the
            // 26 nt footprint was reported at 1..6, six bases.
            let span = (b.end as i64 - b.start as i64).rem_euclid(n as i64) as usize + 1;
            assert_eq!(
                span,
                b.footprint.len(),
                "span {}..{} is {span} bases against a {} nt footprint",
                b.start,
                b.end,
                b.footprint.len()
            );
        }

        let rev = circ
            .iter()
            .find(|b| b.strand == Strand::Reverse)
            .unwrap_or_else(|| panic!("{circ:?}"));
        assert_eq!(rev.footprint_str(), "TCGATGCAACTGGTAACCGT");
        assert_eq!(rev.tail_str(), "AACCGT", "the second turn is tail");

        // The control that makes it undeniable: closing the molecule cannot
        // lengthen a footprint past the molecule, and cannot delete a tail the
        // linear answer reports. Every field must agree.
        let lin = find_bindings(primer, template, false, &Default::default());
        assert_eq!(
            circ, lin,
            "circular and linear must agree when the site does not wrap"
        );
    }

    #[test]
    fn a_primer_that_binds_twice_reports_both() {
        // The case that ruins a PCR and is invisible if only the best site is
        // reported.
        let mut template = b"TGCACGAATGAGAACAGAACCACAAATGGTG".to_vec();
        template.extend_from_slice(b"CCTTAGGTCTTAGG");
        template.extend_from_slice(b"GAATGAGAACAGAACCA");
        let b = find_bindings(b"GAATGAGAACAGAACCA", &template, false, &p());
        assert_eq!(b.len(), 2, "{b:?}");
        assert!(b[0].start < b[1].start);
    }
}
