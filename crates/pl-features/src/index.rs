//! K-mer seeding — deciding which of thousands of features are worth aligning.
//!
//! `docs/PLAN.md` §7.7 steps 1–2. Aligning every database entry against every
//! plasmid would be correct and far too slow; seeding reduces it to the handful
//! of (feature, position) pairs that share exact short words with the query, and
//! alignment then adjudicates those.
//!
//! # Where the constants come from, and where they bite
//!
//! `k = 12` is the plan's. At ≥96% identity a mismatch every ~25 bases still
//! leaves long exact stretches, so a 12-mer survives comfortably; and 4^12 is
//! 16.7 M, specific enough that random 20 kb of plasmid produces few spurious
//! hits.
//!
//! The consequence nobody mentions: **a feature shorter than `k` cannot be
//! seeded at all**, and a feature containing ambiguity codes cannot be seeded
//! across them. Short degenerate consensus sites — a −35 box, a ribosome
//! binding site — are exactly that case.
//!
//! [`Index::short`] **reports** those records; it does not scan for them. Its
//! only caller is `Annotator::unseedable`, and `annotate` runs `scan_dna` and
//! `scan_protein` and nothing else — so an unseedable feature is currently
//! *named as unsupported*, not found by another route.
//!
//! This paragraph used to say they were "routed to a direct IUPAC-aware scan
//! instead ([`Index::short`])". They were not. Describing a fallback that had
//! never been written is worse than describing the gap, because a reader
//! checking whether short degenerate sites are handled would have stopped
//! reading there and believed they were. The scan now exists as
//! [`pl_core::iupac::find_all`]; wiring it in is a separate change with its own
//! tests. Indels stay out of scope for these either way — a 10 bp element with
//! an indel is not identifiable.
//!
//! # A peptide-only record is invisible to the DNA index by construction
//!
//! Since 2026-07-28 a `synthetic_part` row may carry a peptide and no
//! nucleotides. `Index::build` gives such a record a length of 0 in the DNA
//! index, seeds nothing from it, and therefore lists it in [`Index::short`] —
//! **always, and for every one of them**. That is correct and is not a defect:
//! there are no bases to seed. A reader who finds fourteen shipped rows sitting
//! in the DNA index's short list and concludes something is broken is reading a
//! true statement about the wrong index.
//!
//! `Annotator::unseedable` intersects the two short lists rather than uniting
//! them, so those records are not reported as unreachable while translated
//! matching is on. With `Config::protein` off they are, and that is also
//! correct — with the protein index never consulted they really cannot be
//! found by anything.
//!
//! The failure this arrangement does **not** catch, and which is why the
//! builder carries a peptide length floor rather than trusting the index to
//! complain: `short` reports records with *no* seedable word, not records with
//! *too few*. A 6-residue peptide yields two 5-mer windows, never reaches
//! `Config::min_seeds = 3`, and is therefore seeded, unchainable, absent from
//! `short`, absent from `unseedable`, and never found. Silent. See
//! `MIN_PEPTIDE_AA` in `features/build/stage_curated.py`.

use std::collections::HashMap;

use crate::Db;

/// Word length for nucleotide seeding. `docs/PLAN.md` §7.7 step 1.
pub const K_DNA: usize = 12;

/// Word length for protein seeding. Shorter because the alphabet is 20 letters
/// rather than 4, so a 5-mer is already more specific than a DNA 12-mer.
pub const K_PROTEIN: usize = 5;

/// A shared exact word between the query and a database record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seed {
    pub record: u32,
    /// Offset in the query.
    pub query_pos: u32,
    /// Offset in the database record.
    pub record_pos: u32,
}

/// A run of collinear seeds: the evidence that one record occupies one place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    pub record: u32,
    /// `query_pos - record_pos`, i.e. where the record's first base would fall.
    pub diagonal: i64,
    pub seeds: usize,
    /// The query window worth aligning against, already widened for indels.
    pub window: (usize, usize),
    /// The half-open span of the **record** that has seed support.
    ///
    /// This is what makes fragments findable. Alignment consumes its pattern
    /// entirely, so aligning a whole 600 bp marker against a plasmid holding
    /// only its first half scores 300 deletions and is rejected — a truncated
    /// feature would be invisible rather than reported, which is exactly the
    /// case §7.7 step 8 says to draw as an unfilled arrow. Seeds already know
    /// which part of the record is present, so that part is what gets aligned.
    pub record_span: (usize, usize),
}

/// FNV-1a over the uppercased word.
///
/// A hash rather than the tighter 2-bit packing so that nucleotide and protein
/// alphabets share one code path. Collisions cost an extra alignment and change
/// no result, because alignment is the arbiter — so the cheap choice is also
/// the safe one here.
fn hash(word: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in word {
        h ^= b.to_ascii_uppercase() as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Is every byte an unambiguous letter we can seed on?
fn seedable(word: &[u8], protein: bool) -> bool {
    word.iter().all(|&b| {
        if protein {
            b.is_ascii_alphabetic() && !b.eq_ignore_ascii_case(&b'X')
        } else {
            matches!(b.to_ascii_uppercase(), b'A' | b'C' | b'G' | b'T')
        }
    })
}

/// Split one diagonal bucket's seeds into runs that are genuinely collinear.
///
/// # The failure this exists to prevent
///
/// Bucketing a diagonal as `d / bucket_width` is a fixed grid, and a grid
/// cannot separate two copies of a record that happen to land in one cell. A
/// 60 bp feature in direct tandem puts its copies on diagonals 60 apart; the
/// DNA scan's `slack = 40` makes the cell 80 wide, so diagonals 160/220 share
/// cell 2 while 140/200 straddle cells 1 and 2. The trigger was therefore grid
/// alignment, not distance. When two copies did share a cell, the group was
/// reduced to a single median diagonal, so only the copy nearest that diagonal
/// was aligned and the other was dropped with no diagnostic at all: measured,
/// 160 bp of leading filler reported one annotation and 140 bp reported two, on
/// the same molecule. Worse on a circle, where absolute position depends on the
/// file's origin — a 580 bp circle carrying one 60 bp tandem repeat reported 1
/// annotation for 160 of its 580 rotations and 2 for the other 420, so the
/// feature count changed with nothing but which base the file numbers 1. The
/// records this bites are exactly the repeat-bearing ones (tetO7, lacO, 5xUAS,
/// TALE, ITR) that PLAN §8.3 plans to add.
///
/// # Why the split is on collinearity and not on distance
///
/// Widening or narrowing the bucket cannot fix it, because distance does not
/// carry the answer: one copy carrying a 60 bp insertion spreads its diagonals
/// exactly as far as two copies 60 bp apart, and holding the insertion case
/// together is the entire reason the bucketing exists
/// (`an_indel_does_not_split_a_chain_into_unsupported_halves`).
///
/// What separates them is collinearity. A second copy walks record positions
/// the first copy has already walked; a single copy with an indel never walks
/// backwards through the record, however far its diagonal drifts. So a run
/// accepts a seed only while *both* coordinates advance, and a seed that would
/// step backwards in the record opens a new run rather than being averaged into
/// the median of the old one.
///
/// Support is then counted per run, not per bucket, which is the honest count:
/// a chain's evidence is the seeds that actually line up with it.
fn collinear_runs<'a>(group: &[&'a Seed]) -> Vec<Vec<&'a Seed>> {
    let mut seeds: Vec<&Seed> = group.to_vec();
    // Deterministic, and the greedy assignment below depends on it: seeds are
    // distinct `(query_pos, record_pos)` pairs, so this order is total.
    seeds.sort_unstable_by_key(|s| (s.query_pos, s.record_pos));

    let mut runs: Vec<Vec<&Seed>> = Vec::new();
    for s in seeds {
        // Extend the run whose tail sits *closest below* this seed. Taking the
        // first run that merely could take it is what re-creates the bug on the
        // small scale: the leading copy would swallow the trailing copy's first
        // seeds and drag its median diagonal back onto itself.
        let best = runs
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                let tail = r[r.len() - 1];
                tail.query_pos < s.query_pos && tail.record_pos < s.record_pos
            })
            .max_by_key(|(_, r)| r[r.len() - 1].record_pos)
            .map(|(i, _)| i);
        match best {
            Some(i) => runs[i].push(s),
            None => runs.push(vec![s]),
        }
    }
    runs
}

/// A seed index over one alphabet's worth of database records.
#[derive(Debug, Clone)]
pub struct Index {
    k: usize,
    protein: bool,
    map: HashMap<u64, Vec<(u32, u32)>>,
    /// Records too short or too degenerate to seed, with their lengths.
    short: Vec<u32>,
    lengths: Vec<usize>,
}

impl Index {
    /// Build an index over the records of `db` matching `protein`.
    ///
    /// Records are addressed by their position in `db.records`, so a caller can
    /// always get back to the full row and its provenance.
    pub fn build(db: &Db, protein: bool, k: usize) -> Index {
        let mut map: HashMap<u64, Vec<(u32, u32)>> = HashMap::new();
        let mut short = Vec::new();
        let lengths: Vec<usize> = db
            .records
            .iter()
            .map(|r| {
                if protein {
                    r.reference_aa.as_ref().map_or(0, |p| p.len())
                } else {
                    r.reference_nt.len()
                }
            })
            .collect();

        for (i, rec) in db.records.iter().enumerate() {
            let seq: &[u8] = if protein {
                match rec.reference_aa.as_deref() {
                    Some(p) if !p.is_empty() => p,
                    _ => continue,
                }
            } else {
                &rec.reference_nt
            };
            let mut indexed = 0usize;
            if seq.len() >= k {
                for (off, w) in seq.windows(k).enumerate() {
                    if seedable(w, protein) {
                        map.entry(hash(w)).or_default().push((i as u32, off as u32));
                        indexed += 1;
                    }
                }
            }
            // No seedable word anywhere means seeding cannot find it at all.
            if indexed == 0 {
                short.push(i as u32);
            }
        }

        Index {
            k,
            protein,
            map,
            short,
            lengths,
        }
    }

    pub fn k(&self) -> usize {
        self.k
    }

    /// Records this index cannot seed, which a caller must handle another way.
    pub fn short(&self) -> &[u32] {
        &self.short
    }

    /// Distinct words held.
    pub fn words(&self) -> usize {
        self.map.len()
    }

    /// Every shared word between `query` and the indexed records.
    pub fn seeds(&self, query: &[u8]) -> Vec<Seed> {
        let mut out = Vec::new();
        if query.len() < self.k {
            return out;
        }
        for (qpos, w) in query.windows(self.k).enumerate() {
            if !seedable(w, self.protein) {
                continue;
            }
            if let Some(hits) = self.map.get(&hash(w)) {
                for &(record, record_pos) in hits {
                    out.push(Seed {
                        record,
                        query_pos: qpos as u32,
                        record_pos,
                    });
                }
            }
        }
        out
    }

    /// Group seeds into collinear chains, one per (record, place).
    ///
    /// `slack` widens each window so an alignment with indels still fits;
    /// `min_seeds` is the support a chain needs before it is worth aligning.
    ///
    /// Diagonals are bucketed rather than required to be equal, because an
    /// indel shifts every subsequent seed onto a neighbouring diagonal — the
    /// exact case a purely-collinear chainer would split in two and then
    /// under-support.
    ///
    /// Bucketing alone is not enough to say "one place", though, because a
    /// bucket is a fixed grid cell and two copies of a short feature can share
    /// one. Each bucket is therefore split into collinear runs before a chain
    /// is emitted; `collinear_runs` below carries the measurements.
    pub fn chain(
        &self,
        seeds: &[Seed],
        query_len: usize,
        slack: usize,
        min_seeds: usize,
    ) -> Vec<Chain> {
        // (record, diagonal bucket) -> seeds
        let mut buckets: HashMap<(u32, i64), Vec<&Seed>> = HashMap::new();
        let bucket_width = (slack.max(1) * 2) as i64;
        for s in seeds {
            let d = s.query_pos as i64 - s.record_pos as i64;
            buckets
                .entry((s.record, d / bucket_width))
                .or_default()
                .push(s);
        }

        // A chain landing exactly on a bucket boundary is split across two
        // buckets and may fail `min_seeds` twice. Merge neighbours back.
        let mut merged: HashMap<(u32, i64), Vec<&Seed>> = HashMap::new();
        let mut keys: Vec<_> = buckets.keys().copied().collect();
        keys.sort_unstable();
        for key in keys {
            let (rec, b) = key;
            let entry = merged.entry(key).or_default();
            for probe in [b, b - 1] {
                if let Some(v) = buckets.get(&(rec, probe)) {
                    entry.extend(v.iter().copied());
                }
            }
        }

        let mut out = Vec::new();
        for ((record, _), group) in merged {
            // One bucket is not one place. See [`collinear_runs`]: a group that
            // holds two copies of the record must yield two chains, or the
            // median below picks one copy's diagonal and the other copy is
            // dropped without a word.
            for run in collinear_runs(&group) {
                if run.len() < min_seeds {
                    continue;
                }
                let mut diags: Vec<i64> = run
                    .iter()
                    .map(|s| s.query_pos as i64 - s.record_pos as i64)
                    .collect();
                diags.sort_unstable();
                let diagonal = diags[diags.len() / 2];

                let rec_len = self.lengths[record as usize];
                let rlo = run.iter().map(|s| s.record_pos as usize).min().unwrap_or(0);
                let rhi = run
                    .iter()
                    .map(|s| s.record_pos as usize + self.k)
                    .max()
                    .unwrap_or(rec_len)
                    .min(rec_len);

                let lo = (diagonal - slack as i64).max(0) as usize;
                let hi =
                    ((diagonal + rec_len as i64 + slack as i64).max(0) as usize).min(query_len);
                if lo >= hi || rlo >= rhi {
                    continue;
                }
                out.push(Chain {
                    record,
                    diagonal,
                    seeds: run.len(),
                    window: (lo, hi),
                    record_span: (rlo, rhi),
                });
            }
        }

        // Deterministic order, so annotation output is stable between runs.
        //
        // The sort key must decide the *survivor*, not merely the order. It
        // used to be `(record, window)` — exactly the key `dedup_by_key` then
        // compares — so among chains sharing a window the winner was whichever
        // the (randomly seeded) `HashMap` above happened to yield first. Those
        // chains carry different `record_span`s, which selects a different slice
        // of the reference to align, so the annotation could differ run to run
        // on identical input. Two diagonals collide like this whenever both
        // clamp at both ends, i.e. for any feature nearly as long as the
        // molecule.
        //
        // The tie-break is a preference, not just a tidy-up: most seed support
        // first, then the widest span of the reference, so the surviving chain
        // is the best-evidenced one rather than an arbitrary one.
        out.sort_by_key(|c| {
            (
                c.record,
                c.window.0,
                c.window.1,
                std::cmp::Reverse(c.seeds),
                std::cmp::Reverse(c.record_span.1 - c.record_span.0),
                c.record_span.0,
                c.diagonal,
            )
        });
        out.dedup_by_key(|c| (c.record, c.window));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoundaryRule, Class, Record, ReviewStatus};

    fn rec(id: &str, seq: &str, protein: bool) -> Record {
        Record {
            id: id.into(),
            name: id.into(),
            aliases: vec![],
            class: if protein { Class::Cds } else { Class::Misc },
            genbank_key: "misc_feature".into(),
            reference_nt: if protein {
                b"ATG".to_vec()
            } else {
                seq.as_bytes().to_ascii_uppercase()
            },
            reference_aa: if protein {
                Some(seq.as_bytes().to_ascii_uppercase())
            } else {
                None
            },
            boundary_rule: BoundaryRule::OrfAtgToStop,
            boundary_evidence: "test".into(),
            description: String::new(),
            review_status: ReviewStatus::Proposed,
            curator: String::new(),
            date_added: "2026-07-27".into(),
            patent_flag: false,
            notes: String::new(),
        }
    }

    fn db_of(records: Vec<Record>) -> Db {
        Db {
            records,
            provenance: vec![],
            version: "test".into(),
        }
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn seq(&mut self, n: usize) -> String {
            (0..n)
                .map(|_| b"ACGT"[(self.next() % 4) as usize] as char)
                .collect()
        }
    }

    #[test]
    fn a_planted_feature_seeds_on_its_own_diagonal() {
        let mut rng = Rng(0x1111_2222_3333_4444);
        let feature = rng.seq(200);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let idx = Index::build(&db, false, K_DNA);

        let left = rng.seq(500);
        let query = format!("{left}{feature}{}", rng.seq(500));
        let seeds = idx.seeds(query.as_bytes());
        assert!(!seeds.is_empty());

        let chains = idx.chain(&seeds, query.len(), 40, 3);
        assert_eq!(chains.len(), 1, "one feature, one place");
        assert_eq!(chains[0].diagonal, 500);
        let (lo, hi) = chains[0].window;
        assert!(
            lo <= 500 && hi >= 700,
            "window {lo}..{hi} must contain the feature"
        );
    }

    #[test]
    fn two_copies_of_a_feature_give_two_chains() {
        let mut rng = Rng(0xaaaa_bbbb_cccc_dddd);
        let feature = rng.seq(150);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let idx = Index::build(&db, false, K_DNA);
        let query = format!(
            "{}{feature}{}{feature}{}",
            rng.seq(300),
            rng.seq(400),
            rng.seq(100)
        );
        let chains = idx.chain(&idx.seeds(query.as_bytes()), query.len(), 40, 3);
        assert_eq!(chains.len(), 2);
        let mut ds: Vec<i64> = chains.iter().map(|c| c.diagonal).collect();
        ds.sort_unstable();
        assert_eq!(ds, vec![300, 850]);
    }

    #[test]
    fn a_tandem_repeat_landing_in_one_diagonal_bucket_still_gives_two_chains() {
        // The bucket grid, not the distance, used to decide this. A 60 bp
        // feature in direct tandem has diagonals 60 apart; with slack 40 the
        // bucket is 80 wide, so `pad = 160` puts diagonals 160 and 220 both in
        // bucket 2 and the group collapsed to its median diagonal 220 — one
        // chain, and the copy at 161..220 gone with no diagnostic. `pad = 140`
        // (diagonals 140 and 200, Δ = 60, *smaller* than the bucket) straddled
        // buckets 1 and 2 and reported both. Sweeping the pad is what shows it
        // is grid alignment rather than proximity, so the test sweeps it.
        let mut rng = Rng(0x7a4d_0000_1111_2222);
        let feature = rng.seq(60);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let idx = Index::build(&db, false, K_DNA);

        for pad in [140usize, 150, 160, 170, 180, 190, 200, 210, 220] {
            let query = format!("{}{feature}{feature}{}", rng.seq(pad), rng.seq(160));
            let chains = idx.chain(&idx.seeds(query.as_bytes()), query.len(), 40, 3);
            let mut ds: Vec<i64> = chains.iter().map(|c| c.diagonal).collect();
            ds.sort_unstable();
            ds.dedup();
            assert_eq!(
                ds,
                vec![pad as i64, pad as i64 + 60],
                "pad={pad}: a tandem repeat is two placements, not one: {chains:?}"
            );
        }
    }

    #[test]
    fn an_indel_does_not_split_a_chain_into_unsupported_halves() {
        // The bucketing exists for this case: after an insertion every later
        // seed sits on a different diagonal.
        let mut rng = Rng(0x9999_8888_7777_6666);
        let feature = rng.seq(300);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let idx = Index::build(&db, false, K_DNA);

        let mut planted = feature.clone();
        planted.insert(150, 'A');
        let query = format!("{}{planted}{}", rng.seq(200), rng.seq(200));

        let chains = idx.chain(&idx.seeds(query.as_bytes()), query.len(), 40, 3);
        assert_eq!(
            chains.len(),
            1,
            "an indel is one feature, not two: {chains:?}"
        );
        let (lo, hi) = chains[0].window;
        assert!(lo <= 200 && hi >= 501);
    }

    #[test]
    fn a_single_copy_with_a_large_insertion_keeps_one_whole_record_chain() {
        // The control for the tandem split above. Sixty bases inserted into the
        // middle of a 200 bp feature spread its diagonals by exactly the 60 the
        // tandem pair spreads by, so any rule that split on *distance* would cut
        // this copy into two half-supported chains and report a feature twice at
        // half coverage. The record coordinate is what tells them apart: here it
        // never walks backwards, so all 179 seeds stay in one chain spanning the
        // whole record.
        let mut rng = Rng(0x51de_0000_3333_4444);
        let feature = rng.seq(200);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let idx = Index::build(&db, false, K_DNA);

        let mut planted = feature.clone();
        planted.insert_str(100, &rng.seq(60));
        let query = format!("{}{planted}{}", rng.seq(200), rng.seq(200));

        let chains = idx.chain(&idx.seeds(query.as_bytes()), query.len(), 40, 3);
        assert_eq!(chains.len(), 1, "one copy, one chain: {chains:?}");
        assert_eq!(chains[0].record_span, (0, 200));
        assert_eq!(chains[0].seeds, 179, "no seed was split off: {chains:?}");
    }

    #[test]
    fn unrelated_sequence_produces_no_supported_chain() {
        let mut rng = Rng(0x4242_4242_4242_4242);
        let db = db_of(vec![rec("pf:a", &rng.seq(200), false)]);
        let idx = Index::build(&db, false, K_DNA);
        let query = rng.seq(5000);
        let chains = idx.chain(&idx.seeds(query.as_bytes()), query.len(), 40, 3);
        assert!(
            chains.is_empty(),
            "random sequence should not chain: {chains:?}"
        );
    }

    #[test]
    fn a_feature_shorter_than_k_is_reported_as_unseedable_not_missing() {
        // The failure this module most needs to not hide.
        let db = db_of(vec![
            rec("pf:short", "TTGACA", false),
            rec("pf:long", "ACGTACGTACGTACGTACGT", false),
        ]);
        let idx = Index::build(&db, false, K_DNA);
        assert_eq!(idx.short(), &[0], "the 6 bp feature cannot be seeded");
    }

    #[test]
    fn a_fully_degenerate_feature_is_unseedable_too() {
        let db = db_of(vec![rec(
            "pf:consensus",
            "TTGACANNNNNNNNNNNNNNNNNTATAAT",
            false,
        )]);
        let idx = Index::build(&db, false, K_DNA);
        assert_eq!(
            idx.short(),
            &[0],
            "no 12-mer of unambiguous bases exists in it"
        );
    }

    #[test]
    fn a_partly_degenerate_feature_seeds_on_its_definite_stretch() {
        let db = db_of(vec![rec("pf:mixed", "NNNNACGTACGTACGTACGTACGTNNNN", false)]);
        let idx = Index::build(&db, false, K_DNA);
        assert!(idx.short().is_empty(), "it has definite 12-mers");
        assert!(!idx.seeds(b"TTTTACGTACGTACGTACGTACGTTTTT").is_empty());
    }

    #[test]
    fn protein_records_and_dna_records_do_not_contaminate_each_other() {
        let db = db_of(vec![
            rec("pf:dna", "ACGTACGTACGTACGTACGT", false),
            rec("pf:prot", "MKWVTFISLLFLFSSAYS", true),
        ]);
        let dna = Index::build(&db, false, K_DNA);
        let prot = Index::build(&db, true, K_PROTEIN);
        assert!(dna.seeds(b"MKWVTFISLLFLFSSAYS").is_empty());
        assert!(!prot.seeds(b"AAAMKWVTFISLLFLFSSAYSAAA").is_empty());
        // The protein index must not have indexed the DNA record.
        let chains = prot.chain(&prot.seeds(b"ACGTACGTACGTACGTACGT"), 20, 10, 2);
        assert!(chains.iter().all(|c| c.record == 1), "{chains:?}");
    }

    #[test]
    fn chains_sharing_a_window_resolve_to_the_same_one_every_time() {
        // A feature nearly as long as the molecule makes several diagonals
        // clamp to the same window. The sort key used to be identical to the
        // dedup key, so which chain survived was decided by HashMap iteration
        // order — and the survivors carry different `record_span`s, which
        // changes the slice of the reference that gets aligned.
        let mut rng = Rng(0x5150_1234_9999_0001);
        let feature = rng.seq(600);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let idx = Index::build(&db, false, K_DNA);

        // Two near-identical copies, a few bases apart, in a molecule barely
        // longer than the feature: both diagonals clamp at both ends.
        let query = format!("{}{feature}", rng.seq(6));
        let seeds = idx.seeds(query.as_bytes());
        let first = idx.chain(&seeds, query.len(), 40, 3);

        // Fresh `Index` values each round, so each gets its own HashMap seed.
        for _ in 0..40 {
            let idx2 = Index::build(&db, false, K_DNA);
            let again = idx2.chain(&idx2.seeds(query.as_bytes()), query.len(), 40, 3);
            assert_eq!(
                again, first,
                "chain() is not deterministic across instances"
            );
        }
    }

    #[test]
    fn chaining_is_deterministic() {
        let mut rng = Rng(0xfeed_beef_dead_c0de);
        let feature = rng.seq(180);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let idx = Index::build(&db, false, K_DNA);
        let query = format!("{}{feature}{}", rng.seq(100), rng.seq(100));
        let seeds = idx.seeds(query.as_bytes());
        let first = idx.chain(&seeds, query.len(), 40, 3);
        for _ in 0..20 {
            assert_eq!(idx.chain(&seeds, query.len(), 40, 3), first);
        }
    }
}
