//! Auto-annotation — the §7.7 pipeline.
//!
//! Open a plasmid, get its features back. This is the "magic moment" the plan
//! identifies as the product's centre of gravity, and its most important
//! scoping decision is what it *does not* do: no BLAST+, no DIAMOND, no
//! Infernal, no gigabyte database. SnapGene's behaviour is documented as
//! approximate matching against a curated library at ≥96% identity, tolerant of
//! occasional mismatches and indels, plus perfect-protein matching so
//! codon-optimised genes still resolve. That is string matching over a few
//! megabytes, and it belongs in-process.
//!
//! # Coordinates
//!
//! Output is **1-based inclusive**, matching `pl_core` and both file formats we
//! read (`docs/PLAN.md` §5.3.1 as amended). Internally everything is 0-based
//! half-open, because that is what alignment wants; the conversion happens once,
//! at the boundary, in [`Annotation::from_span`].
//!
//! # Circularity
//!
//! A circular plasmid has no beginning, so a feature may straddle whatever base
//! the file happens to number 1. The query is therefore doubled before matching
//! and hits are folded back, which is the same trick the digester uses. The
//! cost is that every feature is found twice; [`dedupe`] resolves that by
//! position modulo length rather than by identity, so the *origin-spanning* copy
//! is the one that survives.

use pl_core::iupac;
use pl_core::translate::{self, Code, Frame};
use pl_core::{Molecule, Strand};

use crate::align;
use crate::index::{Index, K_DNA, K_PROTEIN};
use crate::{Db, Record};

/// Knobs, all with the plan's defaults.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// §7.7 step 4. User-adjustable by design: a cloning scar or a silent
    /// mutation should not hide a marker, but neither should 80% identity
    /// invent one.
    pub min_identity: f64,
    /// Seeds a chain needs before it is worth aligning.
    pub min_seeds: usize,
    /// §7.7 step 8. Below this fraction of the database feature's length, a hit
    /// is a **fragment** — a real thing to report, drawn as an unfilled arrow,
    /// not a whole feature and not nothing.
    pub fragment_coverage: f64,
    /// Below this fraction of the feature's length, a hit is not a fragment but
    /// a coincidence, and is discarded.
    ///
    /// §7.7 specifies no such floor and needs one. Three 12-mers are enough to
    /// nominate a chain, so without it the annotator reports a "15 bp KanR
    /// fragment" — an 816 bp gene claimed on the evidence of fifteen bases.
    /// Measured against 108 real molecules that produced **10,146 fragments and
    /// 5 whole features**: not a coverage result, a wall of noise deep enough
    /// to bury the real answers and make every map untrustworthy.
    pub min_coverage: f64,
    /// An absolute floor in symbols as well, so a short feature must be found
    /// nearly whole rather than on a lucky seed. Clamped to the feature's own
    /// length, so a 34 bp lox site stays findable — at 34 bp.
    pub min_match_len: usize,
    /// §7.7 step 7's overlap rule: shrink each hit by this fraction at each end
    /// before asking whether two hits collide, so features that merely abut are
    /// both kept.
    ///
    /// The rule is applied **across records**, per the plan, with one
    /// exception: a hit lying strictly inside a better-scoring one is kept.
    /// See [`resolve_overlaps`].
    pub overlap_trim: f64,
    /// Match codon-optimised CDSs by six-frame translation.
    pub protein: bool,
    pub code: Code,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            min_identity: 0.96,
            min_seeds: 3,
            fragment_coverage: 0.95,
            min_coverage: 0.30,
            min_match_len: 50,
            overlap_trim: 0.15,
            protein: true,
            code: translate::TABLE1,
        }
    }
}

/// One found feature.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    /// Index into [`Db::records`].
    pub record: usize,
    /// 1-based inclusive.
    pub start: u64,
    /// 1-based inclusive.
    pub end: u64,
    pub strand: Strand,
    /// Fraction of the *aligned region* that matched — a **local** identity.
    ///
    /// Not the fraction of the database feature reproduced; that is
    /// [`coverage`](Self::coverage), and the two only mean anything together.
    /// The denominator is the seed-supported core plus whatever the outward
    /// `walk` added, so a plasmid carrying the first 300 bp of a 600 bp
    /// marker exactly reports `identity = 1.0` alongside `coverage = 0.50`:
    /// 300 bases of the feature, reproduced perfectly. A caller that prints
    /// one of these without the other is not saying anything.
    ///
    /// This line used to read "Fraction of the database feature reproduced.
    /// See [`Hit::identity`](crate::align::Hit::identity)", which is the opposite convention and describes
    /// a function nothing on this path calls — [`Hit::identity`](crate::align::Hit::identity) divides by the
    /// feature length *precisely so* a half-deleted feature scores 0.5. Under
    /// that sentence the shipped fixture
    /// `a_truncated_feature_is_reported_as_a_fragment_not_dropped` reads as a
    /// bug rather than the intended result, and a UI trusting it would label a
    /// half-length fragment 100%.
    pub identity: f64,
    /// How much of the database feature this hit spans: aligned symbols over
    /// the record's length. This is the number that falls when a feature is
    /// truncated; [`identity`](Self::identity) does not.
    pub coverage: f64,
    /// `match_length × identity × coverage`, per §7.7 step 7.
    pub score: f64,
    /// Coverage below [`Config::fragment_coverage`].
    pub is_fragment: bool,
    /// Found by translation rather than by nucleotide identity — i.e. this is
    /// probably a codon-optimised or otherwise recoded copy.
    pub via_protein: bool,
    /// Runs across the origin of a circular molecule, so `start > end`.
    pub wraps_origin: bool,
}

impl Annotation {
    /// Build from a 0-based half-open span in the *unwrapped* molecule.
    fn from_span(record: usize, span: (usize, usize), len: usize, strand: Strand) -> Annotation {
        let wraps = span.1 > len;
        let start = (span.0 % len) as u64 + 1;
        let end = if wraps {
            ((span.1 - 1) % len) as u64 + 1
        } else {
            span.1 as u64
        };
        Annotation {
            record,
            start,
            end,
            strand,
            identity: 0.0,
            coverage: 0.0,
            score: 0.0,
            is_fragment: false,
            via_protein: false,
            wraps_origin: wraps,
        }
    }

    /// Length in bases, correct across the origin.
    pub fn len(&self, molecule_len: u64) -> u64 {
        if self.wraps_origin {
            molecule_len - self.start + 1 + self.end
        } else {
            self.end.saturating_sub(self.start) + 1
        }
    }

    pub fn is_empty(&self, molecule_len: u64) -> bool {
        self.len(molecule_len) == 0
    }
}

/// An annotator with its indexes already built.
///
/// Building the index is the expensive part and does not depend on the
/// molecule, so it is done once and reused across every file opened.
pub struct Annotator<'a> {
    db: &'a Db,
    dna: Index,
    protein: Index,
    config: Config,
}

impl<'a> Annotator<'a> {
    pub fn new(db: &'a Db, config: Config) -> Annotator<'a> {
        Annotator {
            dna: Index::build(db, false, K_DNA),
            protein: Index::build(db, true, K_PROTEIN),
            db,
            config,
        }
    }

    pub fn db(&self) -> &Db {
        self.db
    }

    /// Database entries that neither index can reach.
    ///
    /// Reported rather than swallowed: a caller that believes it searched the
    /// whole database when it did not will report a confident empty result.
    pub fn unseedable(&self) -> Vec<&Record> {
        // The **intersection**, not the union. A record only one index can
        // reach is still reachable, and listing it here was worse than saying
        // nothing: a well-formed 5-codon CDS was reported unsearchable and then
        // found at coverage 1.0 in the same run.
        //
        // Gated on `config.protein`, because with translated matching switched
        // off the protein index is never consulted and a record the DNA index
        // cannot seed really is unreachable. Under-reporting is the worse of
        // the two failures.
        let dna: std::collections::BTreeSet<u32> = self.dna.short().iter().copied().collect();
        let kept: Vec<u32> = if self.config.protein {
            let protein: std::collections::BTreeSet<u32> =
                self.protein.short().iter().copied().collect();
            dna.into_iter()
                .filter(|i| {
                    // A record with no protein reference was never in the
                    // protein index, so that index cannot rescue it.
                    !self.db.records[*i as usize].has_protein() || protein.contains(i)
                })
                .collect()
        } else {
            dna.into_iter().collect()
        };
        // BTreeSet throughout, so this is record order rather than hash order.
        kept.into_iter()
            .map(|i| &self.db.records[i as usize])
            .collect()
    }

    /// Annotate a molecule.
    pub fn annotate(&self, mol: &Molecule) -> Vec<Annotation> {
        let len = mol.seq.len();
        if len == 0 {
            return Vec::new();
        }
        // §7.7 step 2 — duplicate for circularity so origin-spanning features
        // are contiguous in the text being searched.
        let doubled: Vec<u8> = if mol.topology.is_circular() {
            let mut v = mol.seq.clone();
            v.extend_from_slice(&mol.seq);
            v
        } else {
            mol.seq.clone()
        };

        let mut hits = Vec::new();
        self.scan_dna(&doubled, len, &mut hits);
        if self.config.protein {
            self.scan_protein(&doubled, len, &mut hits);
        }

        let hits = dedupe(hits, len as u64);
        resolve_overlaps(hits, self.config.overlap_trim, len as u64)
    }

    fn scan_dna(&self, doubled: &[u8], len: usize, out: &mut Vec<Annotation>) {
        let rc = iupac::reverse_complement(doubled);
        for reverse in [false, true] {
            let text: &[u8] = if reverse { &rc } else { doubled };
            let seeds = self.dna.seeds(text);
            let slack = 40;
            for chain in self
                .dna
                .chain(&seeds, text.len(), slack, self.config.min_seeds)
            {
                let rec = &self.db.records[chain.record as usize];
                let Some(m) = self.verify(&rec.reference_nt, text, &chain) else {
                    continue;
                };
                // Map back through the reverse complement if we searched it.
                let span = if reverse {
                    (text.len() - m.span.1, text.len() - m.span.0)
                } else {
                    m.span
                };
                if let Some(a) = self.make(
                    Candidate {
                        record: chain.record as usize,
                        span,
                        strand: if reverse {
                            Strand::Reverse
                        } else {
                            Strand::Forward
                        },
                        m,
                        db_units: rec.reference_nt.len(),
                        protein: false,
                    },
                    len,
                ) {
                    out.push(a);
                }
            }
        }
    }

    /// §7.7 step 5 — six-frame translation, which is what finds a marker whose
    /// nucleotides were rewritten for expression in another organism.
    fn scan_protein(&self, doubled: &[u8], len: usize, out: &mut Vec<Annotation>) {
        if self.protein.words() == 0 {
            return;
        }
        for frame in translate::six_frames(doubled, self.config.code) {
            let seeds = self.protein.seeds(&frame.protein);
            if seeds.is_empty() {
                continue;
            }
            for chain in self
                .protein
                .chain(&seeds, frame.protein.len(), 12, self.config.min_seeds)
            {
                let rec = &self.db.records[chain.record as usize];
                let Some(aa) = rec.reference_aa.as_deref() else {
                    continue;
                };
                let Some(m) = self.verify(aa, &frame.protein, &chain) else {
                    continue;
                };
                let span = residues_to_bases(&frame, m.span.0, m.span.1, doubled.len());
                let strand = if frame.reverse {
                    Strand::Reverse
                } else {
                    Strand::Forward
                };
                if let Some(mut a) = self.make(
                    Candidate {
                        record: chain.record as usize,
                        span,
                        strand,
                        m,
                        db_units: aa.len(),
                        protein: true,
                    },
                    len,
                ) {
                    a.via_protein = true;
                    out.push(a);
                }
            }
        }
    }

    /// Edit distance budget implied by the identity threshold.
    fn budget(&self, aligned_len: usize) -> u32 {
        ((1.0 - self.config.min_identity) * aligned_len as f64).floor() as u32
    }

    /// Align the seed-supported part of a record, then push the boundaries
    /// outward as far as the identity threshold allows.
    ///
    /// The two halves do different jobs. Alignment adjudicates whether this is
    /// the feature at all, over a region seeding says is present. The outward
    /// walk then recovers the true edges, because a mismatch near an end
    /// suppresses every seed overlapping it and would otherwise leave the
    /// feature reported a dozen bases short at each end — a wrong coordinate
    /// dressed as a right one.
    fn verify(&self, record: &[u8], text: &[u8], chain: &crate::index::Chain) -> Option<Match> {
        let (rlo, rhi) = chain.record_span;
        let (wlo, whi) = chain.window;
        let core = &record[rlo..rhi];
        let hit = align::infix(core, &text[wlo..whi], self.budget(core.len()))?;

        let mut aligned = core.len();
        let mut dist = hit.dist as usize;
        let mut tstart = wlo + hit.start;
        let mut tend = wlo + hit.end;

        // Walk left, then right, taking the furthest point that still satisfies
        // the identity threshold over the whole extended alignment.
        let (add, mism) = walk(record[..rlo].iter().rev(), text[..tstart].iter().rev());
        tstart -= add;
        aligned += add;
        dist += mism;

        let (add, mism) = walk(record[rhi..].iter(), text[tend..].iter());
        tend += add;
        aligned += add;
        dist += mism;

        let identity = 1.0 - dist as f64 / aligned as f64;
        if identity < self.config.min_identity {
            return None;
        }
        Some(Match {
            span: (tstart, tend),
            aligned,
            identity,
        })
    }

    /// Turn a verified match into an annotation, or reject it as too thin.
    fn make(&self, c: Candidate, len: usize) -> Option<Annotation> {
        let Candidate {
            record,
            span,
            strand,
            m,
            db_units,
            protein,
        } = c;
        // A hit lying wholly in the second copy is the same feature as one in
        // the first; keep only those that start in the real molecule.
        if span.0 >= len || span.1 <= span.0 || db_units == 0 {
            return None;
        }
        // A database record longer than a circular molecule can match more
        // bases than the molecule has: a 40 bp terminal repeat matched 1240
        // bases of a 1200 bp plasmid, which came back flagged `wraps_origin`
        // while `start <= end`, with `len()` bigger than the molecule and
        // coverage 1.000. Clamp the span so those three agree.
        let span = (span.0, span.1.min(span.0 + len));
        let coverage = (m.aligned.min(db_units) as f64 / db_units as f64).min(1.0);
        // Too little of the feature to be a claim about the feature at all.
        if coverage < self.config.min_coverage {
            return None;
        }
        let floor = if protein {
            self.config.min_match_len / 3
        } else {
            self.config.min_match_len
        };
        if m.aligned < floor.min(db_units) {
            return None;
        }

        let mut a = Annotation::from_span(record, span, len, strand);
        debug_assert!(
            !a.wraps_origin || a.start > a.end,
            "wraps_origin must mean start > end, got {}..{}",
            a.start,
            a.end
        );
        a.identity = m.identity;
        a.coverage = coverage;
        // §7.7 step 7.
        a.score = (span.1 - span.0) as f64 * a.identity * a.coverage;
        a.is_fragment = a.coverage < self.config.fragment_coverage;
        Some(a)
    }
}

/// Everything needed to decide whether a verified match becomes an annotation.
///
/// A struct rather than seven positional arguments, because two of them are
/// lengths in different units and one is a flag saying which — exactly the
/// shape that produces a silent unit bug. `db_units` is the reference length
/// **in the same units as `Match::aligned`**: residues for a translated hit,
/// bases for a nucleotide one. Mixing them understates a protein hit's coverage
/// threefold, which marks every full-length translated match as a fragment.
struct Candidate {
    record: usize,
    span: (usize, usize),
    strand: Strand,
    m: Match,
    db_units: usize,
    protein: bool,
}

/// A verified match: where in the text, how much of the record, how well.
#[derive(Debug, Clone, Copy)]
pub struct Match {
    span: (usize, usize),
    /// Symbols of the record actually accounted for — the numerator of coverage.
    aligned: usize,
    identity: f64,
}

/// Extend an alignment outward one symbol at a time, stopping when the
/// extension stops looking like sequence and starts looking like coincidence.
///
/// Ungapped on purpose: this runs *outside* an already-verified core, where the
/// only question is where the feature ends, and allowing gaps there lets an
/// alignment wander into unrelated sequence to buy a few more matches.
///
/// # Why a local rule and not the identity threshold
///
/// The obvious criterion — "extend while overall identity stays ≥96%" — is
/// wrong, and wrong in a way that looks fine in a unit test. A 300 bp core
/// matching perfectly can absorb twelve consecutive mismatches and still report
/// 96%, so the walk marches a dozen bases into the random flank and reports a
/// feature that is measurably longer than the feature. The budget must not be
/// spendable on the edges.
///
/// So this is BLAST's X-drop instead: score `+1` per match, `-3` per mismatch,
/// keep the best-scoring point, and give up once the score falls `X` below it.
/// Unrelated DNA matches a quarter of the time, worth `-2` per base on average,
/// so it stops within a few bases; a genuine 96% extension gains `+0.84` per
/// base and runs as far as the feature does.
fn walk<'a>(
    record: impl Iterator<Item = &'a u8>,
    text: impl Iterator<Item = &'a u8>,
) -> (usize, usize) {
    const MISMATCH: i32 = -3;
    const X_DROP: i32 = 10;

    let mut best = (0usize, 0usize);
    let mut best_score = 0i32;
    let mut score = 0i32;
    let mut mismatches = 0usize;

    for (i, (r, t)) in record.zip(text).enumerate() {
        if r.eq_ignore_ascii_case(t) {
            score += 1;
        } else {
            score += MISMATCH;
            mismatches += 1;
        }
        if score > best_score {
            best_score = score;
            best = (i + 1, mismatches);
        }
        if best_score - score > X_DROP {
            break;
        }
    }
    best
}

/// Map a residue range in a frame back to base coordinates.
///
/// The reverse frames run *backwards* along the sequence, so the first residue
/// of a protein hit is at the high end. Getting this the wrong way round places
/// every translated hit at a mirrored coordinate — plausible-looking and wrong.
fn residues_to_bases(frame: &Frame, aa_start: usize, aa_end: usize, len: usize) -> (usize, usize) {
    if aa_end <= aa_start {
        return (0, 0);
    }
    let first = frame.to_source(aa_start, len);
    let last = frame.to_source(aa_end - 1, len);
    if frame.reverse {
        (last.0, first.1)
    } else {
        (first.0, last.1)
    }
}

/// §7.7 step 9 — one feature at one place, once.
///
/// Keyed by `(record, start mod L)` so the two copies produced by doubling a
/// circular molecule collapse. Where they differ, the longer hit wins, which is
/// what keeps the origin-spanning version rather than the truncated one.
fn dedupe(mut hits: Vec<Annotation>, len: u64) -> Vec<Annotation> {
    hits.sort_by(|a, b| {
        (a.record, a.start)
            .cmp(&(b.record, b.start))
            .then(b.len(len).cmp(&a.len(len)))
            .then(
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    let mut out: Vec<Annotation> = Vec::new();
    for h in hits {
        let dup = out
            .iter()
            .any(|k| k.record == h.record && k.start == h.start && k.strand == h.strand);
        if !dup {
            out.push(h);
        }
    }
    out
}

/// §7.7 step 7's overlap rule, applied across records.
///
/// Each hit is shrunk by `trim` of its length at both ends before overlap is
/// tested, so features that merely abut survive together while two competing
/// calls for the same span do not.
///
/// Two hits nest rather than compete when one lies strictly inside the other —
/// an operator within a promoter, an M13 site within lacZ-alpha — and both are
/// kept. That is a deliberate departure from pLannotate, which drops the inner
/// one; SnapGene shows both, and so do we.
fn resolve_overlaps(mut hits: Vec<Annotation>, trim: f64, len: u64) -> Vec<Annotation> {
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then((a.record, a.start).cmp(&(b.record, b.start)))
    });

    let core = |a: &Annotation| -> (i64, i64) {
        let l = a.len(len) as i64;
        let cut = (l as f64 * trim).round() as i64;
        let s = a.start as i64 + cut;
        (s, a.start as i64 + l - cut)
    };

    // On a circle, "overlapping" cannot be decided by comparing two intervals
    // as written: an origin-spanning feature runs past `len` in unwrapped
    // coordinates and so never numerically meets the copy of itself sitting at
    // base 1. Comparing against the ±L translates is what closes the circle.
    let overlaps = |a: (i64, i64), b: (i64, i64)| -> bool {
        let l = len as i64;
        [-l, 0, l]
            .iter()
            .any(|shift| a.0 < b.1 + shift && b.0 + shift < a.1)
    };

    // Is `a` *strictly* inside `b`, on any of the circle's translates?
    //
    // Strictly: two hits with the identical span are competing calls, not a
    // nesting, and treating equality as containment let three near-identical
    // records stack three annotations on one locus — the very thing the
    // cross-record rule exists to prevent.
    let contained_in = |a: (i64, i64), b: (i64, i64)| -> bool {
        let l = len as i64;
        (a.1 - a.0) < (b.1 - b.0)
            && [-l, 0, l]
                .iter()
                .any(|shift| b.0 + shift <= a.0 && a.1 <= b.1 + shift)
    };

    let mut kept: Vec<Annotation> = Vec::new();
    for h in hits {
        let hc = core(&h);
        let clash = kept.iter().any(|k| {
            if !overlaps(hc, core(k)) {
                return false;
            }
            // Containment is not competition.
            //
            // §7.7 step 7 is a cross-hit rule -- "drop lower-scoring hits that
            // still overlap" -- and this was gated on `k.record == h.record`,
            // so it never fired between different database records and three
            // near-identical records at one locus produced three stacked
            // annotations of the same span.
            //
            // Applying it across records unmodified goes too far the other
            // way: a *nested* record, a lac operator inside a lac promoter or
            // an M13 site inside lacZ-alpha, would silently vanish. Those are
            // real annotations rather than duplicate calls, SnapGene shows
            // them, and PLAN 8.3 plans to add exactly that shape. So a hit
            // wholly inside a better-scoring one of a *different* record is
            // kept; only partial overlap -- two calls competing for one span --
            // resolves to the higher score.
            if k.record != h.record && contained_in(hc, core(k)) {
                return false;
            }
            true
        });
        if !clash {
            kept.push(h);
        }
    }
    kept.sort_by_key(|a| (a.start, a.end, a.record));
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoundaryRule, Class, Record, ReviewStatus};
    use pl_core::Topology;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn dna(&mut self, n: usize) -> String {
            (0..n)
                .map(|_| b"ACGT"[(self.next() % 4) as usize] as char)
                .collect()
        }
    }

    fn rec(id: &str, seq: &str, protein: bool) -> Record {
        Record {
            id: id.into(),
            name: id.into(),
            aliases: vec![],
            class: Class::Cds,
            genbank_key: "CDS".into(),
            // A protein-only test record still needs a nucleotide reference,
            // because the schema requires every row to have one; it is made
            // deliberately unmatchable so it cannot satisfy a test by accident.
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

    fn db_of(r: Vec<Record>) -> Db {
        Db {
            records: r,
            provenance: vec![],
            version: "test".into(),
        }
    }

    fn mol(seq: &str, circular: bool) -> Molecule {
        Molecule {
            seq: seq.as_bytes().to_vec(),
            topology: if circular {
                Topology::Circular
            } else {
                Topology::Linear
            },
            ..Default::default()
        }
    }

    #[test]
    fn a_planted_feature_is_found_at_the_right_coordinates() {
        let mut rng = Rng(0x1234_0000_0000_0001);
        let feature = rng.dna(400);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        let left = rng.dna(1000);
        let m = mol(&format!("{left}{feature}{}", rng.dna(1000)), false);
        let found = ann.annotate(&m);

        assert_eq!(found.len(), 1, "{found:?}");
        let f = &found[0];
        // 1-based inclusive: the feature occupies bases 1001..=1400.
        assert_eq!((f.start, f.end), (1001, 1400));
        assert_eq!(f.strand, Strand::Forward);
        assert!((f.identity - 1.0).abs() < 1e-9);
        assert!(!f.is_fragment);
        // ...and the coordinates really do point at the feature.
        assert_eq!(
            &m.seq[(f.start - 1) as usize..f.end as usize],
            feature.as_bytes()
        );
    }

    #[test]
    fn a_feature_on_the_reverse_strand_is_found_and_labelled_reverse() {
        let mut rng = Rng(0x2222_0000_0000_0002);
        let feature = rng.dna(300);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        let rc = String::from_utf8(iupac::reverse_complement(feature.as_bytes())).unwrap();
        let m = mol(&format!("{}{rc}{}", rng.dna(500), rng.dna(500)), false);
        let found = ann.annotate(&m);

        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].strand, Strand::Reverse);
        assert_eq!((found[0].start, found[0].end), (501, 800));
        assert_eq!(
            iupac::reverse_complement(&m.seq[500..800]),
            feature.as_bytes().to_vec()
        );
    }

    #[test]
    fn a_feature_spanning_the_origin_of_a_circle_is_found_whole() {
        // The case that justifies doubling the query, and the one every naive
        // annotator reports as two fragments or misses entirely.
        let mut rng = Rng(0x3333_0000_0000_0003);
        let feature = rng.dna(200);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        // Put the last 80 bases of the feature at the start of the file and the
        // first 120 at the end, so it runs across base 1.
        let middle = rng.dna(600);
        let m = mol(
            &format!("{}{middle}{}", &feature[120..], &feature[..120]),
            true,
        );
        let found = ann.annotate(&m);

        assert_eq!(found.len(), 1, "{found:?}");
        let f = &found[0];
        assert!(f.wraps_origin, "{f:?}");
        assert_eq!(f.len(m.seq.len() as u64), 200);
        assert!(!f.is_fragment);
        assert_eq!(f.start, 681);
        assert_eq!(f.end, 80);
    }

    #[test]
    fn a_linear_molecule_does_not_wrap() {
        let mut rng = Rng(0x4444_0000_0000_0004);
        let feature = rng.dna(200);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());
        let middle = rng.dna(600);
        let m = mol(
            &format!("{}{middle}{}", &feature[120..], &feature[..120]),
            false,
        );
        assert!(ann.annotate(&m).iter().all(|a| !a.wraps_origin));
    }

    #[test]
    fn a_codon_optimised_gene_is_found_by_its_protein() {
        // The whole reason step 5 exists. Recode a CDS with synonymous codons
        // until nucleotide identity is hopeless; the protein is untouched.
        let protein = "MKWVTFISLLFLFSSAYSRGVFRRDAHKSEVAHRFKDLGEENFKALVLIAFAQYLQQCPFEDHVKLVNEVTEFAK";
        let db = db_of(vec![rec("pf:prot", protein, true)]);
        let ann = Annotator::new(&db, Config::default());

        // Build a DNA sequence encoding exactly that protein, choosing the last
        // synonymous codon available for each residue so it looks nothing like
        // any "usual" coding sequence.
        let code = translate::TABLE1;
        let mut cds = String::new();
        for aa in protein.bytes() {
            let mut chosen = None;
            for b1 in b"TCAG" {
                for b2 in b"TCAG" {
                    for b3 in b"TCAG" {
                        let c = [*b1, *b2, *b3];
                        if code.codon(&c) == aa {
                            chosen = Some(c);
                        }
                    }
                }
            }
            let c = chosen.expect("every residue has a codon");
            cds.push_str(std::str::from_utf8(&c).unwrap());
        }
        assert_eq!(code.translate(cds.as_bytes()), protein.as_bytes().to_vec());

        let mut rng = Rng(0x5555_0000_0000_0005);
        let m = mol(&format!("{}{cds}{}", rng.dna(400), rng.dna(400)), false);
        let found = ann.annotate(&m);

        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].via_protein);
        assert_eq!(found[0].strand, Strand::Forward);
        assert_eq!(
            (found[0].start, found[0].end),
            (401, 400 + cds.len() as u64)
        );
    }

    #[test]
    fn a_protein_on_the_reverse_strand_maps_back_to_the_right_bases() {
        let protein = "MKWVTFISLLFLFSSAYSRGVFRRDAHKSEVAHRFKDLGEENFKALVLIAF";
        let db = db_of(vec![rec("pf:prot", protein, true)]);
        let ann = Annotator::new(&db, Config::default());
        let code = translate::TABLE1;

        let mut cds = String::new();
        for aa in protein.bytes() {
            'outer: for b1 in b"TCAG" {
                for b2 in b"TCAG" {
                    for b3 in b"TCAG" {
                        let c = [*b1, *b2, *b3];
                        if code.codon(&c) == aa {
                            cds.push_str(std::str::from_utf8(&c).unwrap());
                            break 'outer;
                        }
                    }
                }
            }
        }
        let rc = String::from_utf8(iupac::reverse_complement(cds.as_bytes())).unwrap();
        let mut rng = Rng(0x6666_0000_0000_0006);
        let m = mol(&format!("{}{rc}{}", rng.dna(300), rng.dna(300)), false);

        let found = ann.annotate(&m);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].strand, Strand::Reverse);
        assert_eq!(
            (found[0].start, found[0].end),
            (301, 300 + cds.len() as u64)
        );
        // The bases named really do translate to the protein.
        let region = &m.seq[(found[0].start - 1) as usize..found[0].end as usize];
        assert_eq!(
            code.translate(&iupac::reverse_complement(region)),
            protein.as_bytes().to_vec()
        );
    }

    #[test]
    fn a_truncated_feature_is_reported_as_a_fragment_not_dropped() {
        // §7.7 step 8. A user who cloned half a marker should see half a
        // marker, drawn differently — not silence.
        let mut rng = Rng(0x7777_0000_0000_0007);
        let feature = rng.dna(600);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(
            &format!("{}{}{}", rng.dna(300), &feature[..300], rng.dna(300)),
            false,
        );

        let found = ann.annotate(&m);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].is_fragment, "{:?}", found[0]);
        assert!(found[0].coverage < 0.95);
        assert!((found[0].coverage - 0.5).abs() < 0.02);
    }

    #[test]
    fn a_fragments_identity_is_local_and_its_coverage_carries_the_truncation() {
        // Pins which of the two numbers means what, because the doc on
        // `Annotation::identity` used to describe `coverage` instead and point
        // at `Hit::identity`, which divides by the feature length — the
        // opposite convention, and one nothing on this path calls. Half a
        // 600 bp marker reproduced perfectly is identity 1.0 with coverage
        // 0.50, and a reader who believed the old sentence would have called
        // that a bug or, worse, printed "100% identity" under the label
        // "fraction of the feature reproduced".
        let mut rng = Rng(0x7777_0000_0000_0107);
        let feature = rng.dna(600);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        let half = mol(
            &format!("{}{}{}", rng.dna(300), &feature[..300], rng.dna(300)),
            false,
        );
        let found = ann.annotate(&half);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            (found[0].identity - 1.0).abs() < 1e-9,
            "identity is local: the 300 bases present matched exactly, so it is \
             1.0 and not 0.5: {:?}",
            found[0]
        );
        assert!((found[0].coverage - 0.5).abs() < 0.02, "{:?}", found[0]);
        assert!(found[0].is_fragment);

        // The control: whole feature, same identity, coverage now 1.0. So the
        // truncation moves `coverage` and only `coverage`.
        let whole = mol(&format!("{}{feature}{}", rng.dna(300), rng.dna(300)), false);
        let found = ann.annotate(&whole);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!((found[0].identity - 1.0).abs() < 1e-9, "{:?}", found[0]);
        assert!((found[0].coverage - 1.0).abs() < 1e-9, "{:?}", found[0]);
        assert!(!found[0].is_fragment);
    }

    #[test]
    fn a_feature_below_the_identity_threshold_is_not_reported() {
        let mut rng = Rng(0x8888_0000_0000_0008);
        let feature = rng.dna(400);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        // 10% of positions changed — far below 96% identity.
        let mut damaged: Vec<u8> = feature.bytes().collect();
        for i in (0..damaged.len()).step_by(10) {
            damaged[i] = if damaged[i] == b'A' { b'C' } else { b'A' };
        }
        let m = mol(
            &format!(
                "{}{}{}",
                rng.dna(200),
                String::from_utf8(damaged).unwrap(),
                rng.dna(200)
            ),
            false,
        );
        assert!(ann.annotate(&m).is_empty());
    }

    #[test]
    fn a_feature_just_inside_the_threshold_is_reported() {
        let mut rng = Rng(0x9999_0000_0000_0009);
        let feature = rng.dna(500);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        // Ten substitutions in 500 bases is 98% identity.
        let mut damaged: Vec<u8> = feature.bytes().collect();
        for i in (0..500).step_by(50) {
            damaged[i] = if damaged[i] == b'G' { b'T' } else { b'G' };
        }
        let m = mol(
            &format!(
                "{}{}{}",
                rng.dna(200),
                String::from_utf8(damaged).unwrap(),
                rng.dna(200)
            ),
            false,
        );
        let found = ann.annotate(&m);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].identity >= 0.96);
    }

    #[test]
    fn unrelated_sequence_yields_nothing() {
        let mut rng = Rng(0xaaaa_0000_0000_000a);
        let db = db_of(vec![
            rec("pf:a", &rng.dna(500), false),
            rec("pf:b", &rng.dna(300), false),
        ]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(&rng.dna(8000), true);
        assert!(ann.annotate(&m).is_empty(), "false positives on random DNA");
    }

    #[test]
    fn two_features_at_the_same_place_do_not_both_survive() {
        // Overlap resolution: near-identical database entries must not produce
        // two stacked annotations of the same span.
        let mut rng = Rng(0xbbbb_0000_0000_000b);
        let feature = rng.dna(400);
        let mut variant: Vec<u8> = feature.bytes().collect();
        variant[10] = if variant[10] == b'A' { b'C' } else { b'A' };
        let db = db_of(vec![
            rec("pf:a", &feature, false),
            rec("pf:b", std::str::from_utf8(&variant).unwrap(), false),
        ]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(&format!("{}{feature}{}", rng.dna(300), rng.dna(300)), false);

        let found = ann.annotate(&m);
        // Both records legitimately match; the rule is that the *span* is not
        // reported twice for the same record, and the better call wins overall.
        assert!(found.len() <= 2, "{found:?}");
        assert!(found.iter().any(|f| f.record == 0));
    }

    #[test]
    fn two_records_competing_for_one_span_do_not_both_survive() {
        // PLAN 7.7 step 7 is a cross-hit rule, and it was gated on
        // `k.record == h.record` so it never fired between different database
        // records. Three near-identical records at one locus produced three
        // stacked annotations of the identical span.
        let mut rng = Rng(0xe3e3_0000_0000_0001);
        let feature = rng.dna(400);
        let mut a: Vec<u8> = feature.bytes().collect();
        let mut b: Vec<u8> = feature.bytes().collect();
        a[7] = if a[7] == b'A' { b'C' } else { b'A' };
        b[9] = if b[9] == b'A' { b'C' } else { b'A' };
        let db = db_of(vec![
            rec("pf:a", &feature, false),
            rec("pf:b", std::str::from_utf8(&a).unwrap(), false),
            rec("pf:c", std::str::from_utf8(&b).unwrap(), false),
        ]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(&format!("{}{feature}{}", rng.dna(300), rng.dna(300)), false);

        let found = ann.annotate(&m);
        assert_eq!(
            found.len(),
            1,
            "three records calling the same span should resolve to one: {found:?}"
        );
    }

    #[test]
    fn a_nested_record_is_kept_rather_than_swallowed() {
        // The reason the cross-record rule is not applied unmodified. An
        // operator inside a promoter, or an M13 site inside lacZ-alpha, is a
        // real annotation rather than a duplicate call -- SnapGene shows both,
        // and PLAN 8.3 plans to add exactly that shape.
        let mut rng = Rng(0xe3e3_0000_0000_0002);
        let big = rng.dna(600);
        let inner = big[200..260].to_string(); // wholly inside `big`
        let db = db_of(vec![
            rec("pf:big", &big, false),
            rec("pf:inner", &inner, false),
        ]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(&format!("{}{big}{}", rng.dna(200), rng.dna(200)), false);

        let found = ann.annotate(&m);
        let names: Vec<&str> = found
            .iter()
            .map(|f| db.records[f.record].id.as_str())
            .collect();
        assert!(names.contains(&"pf:big"), "{names:?}");
        assert!(
            names.contains(&"pf:inner"),
            "the nested feature was swallowed: {names:?}"
        );
    }

    #[test]
    fn adjacent_features_are_both_kept() {
        // The 15% trim exists so that abutting elements are not treated as a
        // collision — an MCS immediately after a promoter is the normal case.
        let mut rng = Rng(0xcccc_0000_0000_000c);
        let a = rng.dna(300);
        let b = rng.dna(300);
        let db = db_of(vec![rec("pf:a", &a, false), rec("pf:b", &b, false)]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(&format!("{}{a}{b}{}", rng.dna(200), rng.dna(200)), false);

        let found = ann.annotate(&m);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!((found[0].start, found[0].end), (201, 500));
        assert_eq!((found[1].start, found[1].end), (501, 800));

        // ...and the trim is what saves them: with no trim the two abutting
        // cores still must not collide, so this asserts the rule rather than
        // the default. Previously this test passed even at trim 0.0, which
        // means it was validating nothing.
        let strict = Config {
            overlap_trim: 0.0,
            ..Config::default()
        };
        let found = Annotator::new(&db, strict).annotate(&m);
        assert_eq!(
            found.len(),
            2,
            "abutting features must not be treated as overlapping: {found:?}"
        );
    }

    #[test]
    fn the_same_feature_twice_in_one_plasmid_is_found_twice() {
        let mut rng = Rng(0xdddd_0000_0000_000d);
        let feature = rng.dna(250);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(
            &format!(
                "{}{feature}{}{feature}{}",
                rng.dna(300),
                rng.dna(500),
                rng.dna(300)
            ),
            true,
        );
        let found = ann.annotate(&m);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].start, 301);
        assert_eq!(found[1].start, 1051);
    }

    #[test]
    fn a_tandem_repeat_is_counted_the_same_from_every_origin_of_a_circle() {
        // A circle has no first base, so the feature count must not depend on
        // which base the file numbers 1. It did: seeds were grouped by
        // `diagonal / bucket_width`, a fixed grid, and two copies of a 60 bp
        // feature sit 60 apart on the diagonal — inside one 80-wide cell for
        // some rotations and across two for others. When they shared a cell the
        // group was reduced to its median diagonal and one copy was dropped
        // with no diagnostic. Measured on this molecule before the fix: 160 of
        // the 580 rotations reported one copy, the other 420 reported two.
        let mut rng = Rng(0x7a4d_0000_0000_0011);
        let feature = rng.dna(60);
        let filler = rng.dna(460);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        let base = format!("{feature}{feature}{filler}");
        assert_eq!(base.len(), 580);
        let mut counts: std::collections::BTreeMap<usize, usize> =
            std::collections::BTreeMap::new();
        for r in 0..base.len() {
            let rotated = format!("{}{}", &base[r..], &base[..r]);
            *counts
                .entry(ann.annotate(&mol(&rotated, true)).len())
                .or_insert(0) += 1;
        }
        assert_eq!(
            counts,
            [(2usize, 580usize)].into_iter().collect(),
            "feature count changed with the origin: {counts:?}"
        );
    }

    #[test]
    fn a_single_copy_is_rotation_invariant_too() {
        // The control: one copy of the same feature on the same size of circle.
        // If the count above had wobbled for some unrelated reason — a seeding
        // artefact at the origin, say — this would wobble with it.
        let mut rng = Rng(0x7a4d_0000_0000_0012);
        let feature = rng.dna(60);
        let filler = rng.dna(520);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());

        let base = format!("{feature}{filler}");
        for r in 0..base.len() {
            let rotated = format!("{}{}", &base[r..], &base[..r]);
            assert_eq!(
                ann.annotate(&mol(&rotated, true)).len(),
                1,
                "rotation {r} of a single-copy molecule"
            );
        }
    }

    #[test]
    fn a_circular_molecule_does_not_report_each_feature_twice() {
        // The doubling in step 2 must not leak into the output.
        let mut rng = Rng(0xeeee_0000_0000_000e);
        let feature = rng.dna(300);
        let db = db_of(vec![rec("pf:a", &feature, false)]);
        let ann = Annotator::new(&db, Config::default());
        let m = mol(
            &format!("{}{feature}{}", rng.dna(1000), rng.dna(1000)),
            true,
        );
        let found = ann.annotate(&m);
        assert_eq!(found.len(), 1, "doubling leaked: {found:?}");
    }

    #[test]
    fn results_are_stable_between_runs() {
        let mut rng = Rng(0xffff_0000_0000_000f);
        let db = db_of(vec![
            rec("pf:a", &rng.dna(300), false),
            rec("pf:b", &rng.dna(200), false),
            rec("pf:c", &rng.dna(400), false),
        ]);
        let ann = Annotator::new(&db, Config::default());
        let seq: String = db
            .records
            .iter()
            .map(|r| String::from_utf8(r.reference_nt.clone()).unwrap())
            .collect::<Vec<_>>()
            .join(&rng.dna(200));
        let m = mol(&seq, true);
        let first = ann.annotate(&m);
        assert_eq!(first.len(), 3);
        for _ in 0..10 {
            assert_eq!(ann.annotate(&m), first);
        }
    }

    #[test]
    fn an_empty_molecule_and_an_empty_database_are_not_errors() {
        let empty = db_of(vec![]);
        let ann = Annotator::new(&empty, Config::default());
        assert!(ann.annotate(&mol("ACGTACGT", false)).is_empty());

        let db = db_of(vec![rec("pf:a", "ACGTACGTACGTACGT", false)]);
        let ann = Annotator::new(&db, Config::default());
        assert!(ann.annotate(&mol("", false)).is_empty());
    }

    #[test]
    fn unseedable_entries_are_reported_rather_than_silently_unsearched() {
        let db = db_of(vec![
            rec("pf:minus35", "TTGACA", false),
            rec("pf:long", "ACGTACGTACGTACGTACGTACGT", false),
        ]);
        let ann = Annotator::new(&db, Config::default());
        let un = ann.unseedable();
        assert_eq!(un.len(), 1);
        assert_eq!(un[0].id, "pf:minus35");
    }
}
