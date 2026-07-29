//! What the sequence view draws besides the letters: which features cover a
//! row, which lane each one sits in, and where the enzymes cut.
//!
//! Nothing here touches egui, so the arithmetic that decides whether a ribbon
//! covers exactly its bases is exercised by ordinary unit tests rather than by
//! driving a window. `main.rs` owns the painting and calls in.
//!
//! # One coordinate convention, converted once
//!
//! Everything stored here is **0-based half-open** `[lo, hi)`, the same space
//! the caret lives in: `lo` is the gap before the first base of the piece and
//! `hi` the gap after its last. `pl_core::Segment` is 1-based inclusive. That
//! conversion happens in exactly one place, [`AnnotIndex::build`], because the
//! failure it prevents is invisible: a ribbon one cell out says EcoRI cuts
//! inside AmpR when it cuts immediately after, and one cell in sixty is not
//! something anyone catches in a screenshot.
//!
//! # Origin-crossing segments are split at build, not at draw
//!
//! A segment written `7900..120` on an 8,117 bp circle has `end < start`, which
//! `Molecule::validate` says is legal. Stored unsplit, `lo > hi` makes the
//! interval empty and the feature vanishes from the sequence view while still
//! drawing on the map — a plasmid missing one annotation looks entirely normal.
//! So every stored interval is non-wrapping and the query has no wrap branch.

use pl_core::oplog::OpId;
use pl_core::Molecule;

/// Which document, and where in its history.
///
/// The cursor alone is **not** enough, and that is the single most dangerous
/// mistake available here. Every `Document` starts at cursor `None`, so opening
/// plasmid A and then plasmid B compares equal, the index is not rebuilt, and
/// A's features are painted onto B. Every ribbon lands somewhere plausible and
/// nothing errors. The generation counter is what separates two documents that
/// happen to sit at the same point in their own histories.
pub type Version = (u64, Option<OpId>);

/// How many overlapping features get a ribbon of their own.
///
/// Three, because the strip is under every row and a fourth lane costs three
/// pixels on all 203 of them. Anything past it is placed into whatever lanes
/// the row leaves empty — see [`compact_row`] — and only counted when the row
/// genuinely has no room. `docs/PLAN.md` item 33 records a hidden site costing
/// a user a month of bench time; a hidden feature is the same failure in the
/// drawing layer.
pub const MAX_LANES: u8 = 3;

/// One drawable piece of one feature: a maximal run of its segments that is
/// contiguous on the molecule.
///
/// # Abutting segments are one piece, not two
///
/// pKoV spells its SacB `complement(join(1976..3310,3311..3397))` — two
/// segments that touch. Drawn as two pieces it grew a seam, a boundary tick and
/// a second direction arrowhead in the middle of what every biologist reads as
/// one contiguous CDS. So the segments of a feature are unioned here, and only
/// a real gap between them (an intron) survives as two pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Iv {
    /// 0-based, inclusive.
    pub lo: u64,
    /// 0-based, exclusive.
    pub hi: u64,
    /// Index into `Molecule::features`.
    pub feat: u32,
    /// Index into that feature's `segments` of the FIRST segment in the run.
    pub seg: u16,
    /// Which ribbon row. `>= MAX_LANES` means "not drawn, counted instead".
    pub lane: u8,
    /// This piece carries a real 5' boundary: `lo` is where a segment genuinely
    /// begins, not an origin split and not a coordinate clamped into range.
    pub starts: bool,
    /// The same for `hi`, in coordinate order.
    pub ends: bool,
    /// `lo` is the whole FEATURE's extreme terminus in coordinate order.
    ///
    /// Separate from `starts` because a joined CDS has as many segment
    /// boundaries as it has exons and exactly one 5' and one 3' end. The
    /// direction arrowhead belongs to the feature; the smaller intron tick
    /// belongs to the segment.
    pub feat_lo: bool,
    /// The same for `hi`.
    pub feat_hi: bool,
}

/// One feature's segment, flattened, before its neighbours are unioned into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Piece {
    lo: u64,
    hi: u64,
    seg: u16,
    real_lo: bool,
    real_hi: bool,
}

/// Union the pieces of ONE feature, in place, keeping them sorted by `lo`.
///
/// Touching counts as overlapping: `join(1976..3310,3311..3397)` is contiguous
/// DNA and drawing it as two ribbons put a seam and a spurious arrowhead in the
/// middle of pKoV's SacB. Two segments of one feature that genuinely overlap
/// are unioned for a second reason — left apart they are two intervals over the
/// same bases, so the greedy lane colouring spends two lanes drawing one
/// feature twice.
fn union_in_place(pieces: &mut Vec<Piece>) {
    if pieces.len() < 2 {
        return;
    }
    pieces.sort_unstable_by_key(|p| (p.lo, p.hi));
    let mut w = 0usize;
    for r in 1..pieces.len() {
        let cur = pieces[r];
        if cur.lo <= pieces[w].hi {
            if cur.hi > pieces[w].hi {
                pieces[w].hi = cur.hi;
                pieces[w].real_hi = cur.real_hi;
            }
        } else {
            w += 1;
            pieces[w] = cur;
        }
    }
    pieces.truncate(w + 1);
}

/// Clear the terminus flags on a pair of pieces that meet across the origin.
///
/// `7900..120` arrives as one segment with `end < start` and is split above,
/// which already sets the flags asymmetrically. The same feature spelled the
/// GenBank way — `join(7900..8117,1..120)` — arrives as two ordinary segments
/// that happen to end at the last base and start at the first, and nothing
/// downstream can tell that from a feature that really stops at the origin. Left
/// alone it drew a 3' arrowhead at base 8,117 as well as at base 120.
fn mark_termini(pieces: &mut [Piece], n: u64, circular: bool) {
    if !circular || pieces.len() < 2 {
        return;
    }
    let head = pieces.iter().position(|p| p.lo == 0);
    let tail = pieces.iter().position(|p| p.hi == n);
    if let (Some(h), Some(t)) = (head, tail) {
        // Distinct pieces only: a feature covering the whole circle is one
        // piece whose two ends really are the origin.
        if h != t {
            pieces[h].real_lo = false;
            pieces[t].real_hi = false;
        }
    }
}

/// One cut, and the recognition site that produced it.
///
/// Both, because they are different objects and drawing one as the other is a
/// real off-by-one: EcoRI is `G^AATTC`, so the cut is at `site_lo + 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cut {
    /// 0-based **gap** the top strand is nicked at. A cut is a bond, not a
    /// base, which is why this is in the caret's coordinate space.
    pub at: u64,
    /// 0-based first base of the recognition match.
    pub site_lo: u64,
    pub site_len: u32,
    /// Index into `pl_enzymes::ENZYMES`.
    pub enzyme: u32,
}

/// A pending typing run, in the two numbers the remap needs.
///
/// A copy rather than a borrow of `seqedit::Run` so this module stays free of
/// everything except `pl_core`, and so the preview can be tested against a real
/// commit without a `SeqEdit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunSpan {
    /// Gap in the **committed** molecule where the replaced region begins.
    pub start: u64,
    pub removed: u64,
    pub inserted: u64,
}

/// Features by position, enzyme cuts by position, and the lane assignment.
pub struct AnnotIndex {
    /// Sorted by `lo`, then by length descending, then by feature index — the
    /// order the lane colouring is defined in, so the array and the lanes
    /// cannot disagree.
    ivs: Vec<Iv>,
    /// `max_hi[m]` is the largest `hi` in the subtree whose midpoint is `m`.
    ///
    /// The recursion that halves `0..n` visits each index as a midpoint exactly
    /// once for a fixed `n`, so one array indexed by midpoint is a complete
    /// augmentation. Pruning on it is what makes the query `O(log n + k)`
    /// rather than the "binary search then walk backwards" shape, which
    /// degenerates to a linear scan the moment a full-length `backbone`
    /// misc_feature precedes the query — which is exactly what a plasmid has.
    max_hi: Vec<u64>,
    /// Lanes actually drawn: `min(depth, MAX_LANES)`. Drives the row height, so
    /// it is a property of the document and never of the visible rows.
    pub lanes: u8,
    /// The real maximum overlap depth, uncapped (saturating at 255).
    pub depth: u8,
    /// Segments whose coordinates named nothing. Counted, not silenced: a
    /// hostile `.dna` can write `range="0-18446744073709551615"` and pl-gui's
    /// document never calls `Molecule::validate` on the way in.
    pub dropped: u32,
    /// Sorted by `at`.
    cuts: Vec<Cut>,
    /// The widest distance any cut sits from the far edge of its own site, so
    /// [`sites_touching`](Self::sites_touching) can widen its window by exactly
    /// enough and no more.
    site_span: u64,
    pub version: Version,
}

impl Default for AnnotIndex {
    fn default() -> Self {
        AnnotIndex {
            ivs: Vec::new(),
            max_hi: Vec::new(),
            lanes: 0,
            depth: 0,
            dropped: 0,
            cuts: Vec::new(),
            site_span: 0,
            // A version no document can hold: `u64::MAX` generations never
            // happen, so the first frame always rebuilds rather than trusting
            // an empty index that happens to compare equal.
            version: (u64::MAX, None),
        }
    }
}

impl AnnotIndex {
    /// Flatten, split, sort, colour, augment. Once per document version.
    pub fn build(mol: &Molecule, version: Version) -> Self {
        let n = mol.len();
        let circular = mol.topology.is_circular();
        let mut ivs: Vec<Iv> = Vec::new();
        let mut dropped = 0u32;

        let mut pieces: Vec<Piece> = Vec::new();
        for (fi, f) in mol.features.iter().enumerate() {
            pieces.clear();
            for (si, s) in f.segments.iter().enumerate() {
                let si = si as u16;
                // The SnapGene reader parses `<Segment range="0-4"/>` with a
                // bare `parse()` and carries the zero through rather than
                // guessing, so `start` really can be 0 here.
                let lo = s.start.saturating_sub(1);
                if s.end < s.start {
                    // Legal on a circle, and only there. Two pieces, each
                    // non-wrapping, each remembering which real terminus it
                    // carries so the direction cap and the boundary ticks land
                    // on the right ends.
                    let head_ok = circular && lo < n;
                    let tail_ok = circular && s.end >= 1;
                    if head_ok {
                        pieces.push(Piece {
                            lo,
                            hi: n,
                            seg: si,
                            real_lo: true,
                            real_hi: false,
                        });
                    }
                    if tail_ok {
                        pieces.push(Piece {
                            lo: 0,
                            hi: s.end.min(n),
                            seg: si,
                            real_lo: false,
                            real_hi: true,
                        });
                    }
                    if !head_ok && !tail_ok {
                        dropped += 1;
                    }
                    continue;
                }
                let hi = s.end.min(n);
                if hi <= lo {
                    dropped += 1;
                    continue;
                }
                pieces.push(Piece {
                    lo,
                    hi,
                    seg: si,
                    real_lo: true,
                    // Clamped rather than ended: `hi` is the edge of the
                    // molecule, not the edge of the feature.
                    real_hi: s.end <= n,
                });
            }
            union_in_place(&mut pieces);
            mark_termini(&mut pieces, n, circular);
            let feat_lo_x = pieces.iter().filter(|p| p.real_lo).map(|p| p.lo).min();
            let feat_hi_x = pieces.iter().filter(|p| p.real_hi).map(|p| p.hi).max();
            for p in &pieces {
                ivs.push(Iv {
                    lo: p.lo,
                    hi: p.hi,
                    feat: fi as u32,
                    seg: p.seg,
                    lane: 0,
                    starts: p.real_lo,
                    ends: p.real_hi,
                    feat_lo: p.real_lo && feat_lo_x == Some(p.lo),
                    feat_hi: p.real_hi && feat_hi_x == Some(p.hi),
                });
            }
        }

        // Longest first at equal starts, so the feature a reader thinks of as
        // the container gets lane 0 and its passengers stack under it. Feature
        // index last, so the order is total and the picture is reproducible.
        ivs.sort_by(|a, b| {
            a.lo.cmp(&b.lo)
                .then((b.hi - b.lo).cmp(&(a.hi - a.lo)))
                .then(a.feat.cmp(&b.feat))
                .then(a.seg.cmp(&b.seg))
        });

        // Greedy interval-graph colouring, over the WHOLE document. Assigning
        // lanes from the visible rows instead would make a feature hop lanes
        // while scrolling, and a screenshot would show one assignment and look
        // perfect. Processing left-to-right, the number of lanes greedy uses is
        // the maximum overlap depth exactly.
        let mut lane_max_hi: Vec<u64> = Vec::new();
        for iv in &mut ivs {
            let lane = lane_max_hi
                .iter()
                .position(|&end| end <= iv.lo)
                .unwrap_or(lane_max_hi.len());
            if lane == lane_max_hi.len() {
                lane_max_hi.push(iv.hi);
            } else {
                lane_max_hi[lane] = lane_max_hi[lane].max(iv.hi);
            }
            iv.lane = lane.min(254) as u8;
        }
        let depth = lane_max_hi.len().min(255) as u8;

        let mut max_hi = vec![0u64; ivs.len()];
        augment(&ivs, &mut max_hi, 0, ivs.len());

        AnnotIndex {
            lanes: depth.min(MAX_LANES),
            depth,
            dropped,
            ivs,
            max_hi,
            cuts: Vec::new(),
            site_span: 0,
            version,
        }
    }

    /// Replace the cut list. Sorted here so callers cannot forget.
    pub fn set_cuts(&mut self, mut cuts: Vec<Cut>) {
        cuts.sort_unstable_by_key(|c| (c.at, c.enzyme, c.site_lo));
        // How far a cut can be from the far edge of its own site. Measured off
        // the data rather than assumed, because a Type IIS enzyme cuts OUTSIDE
        // its recognition sequence — BsaI is GGTCTC(1/5) — so no constant
        // derived from site length alone is safe.
        self.site_span = cuts
            .iter()
            .map(|c| {
                let hi = c.at.max(c.site_lo + c.site_len as u64);
                hi - c.at.min(c.site_lo)
            })
            .max()
            .unwrap_or(0);
        self.cuts = cuts;
    }

    pub fn cut_count(&self) -> usize {
        self.cuts.len()
    }

    /// Every interval overlapping `[a, b)`, appended to `out`.
    pub fn query(&self, a: u64, b: u64, out: &mut Vec<Iv>) {
        if a >= b {
            return;
        }
        self.descend(0, self.ivs.len(), a, b, out);
    }

    fn descend(&self, l: usize, r: usize, a: u64, b: u64, out: &mut Vec<Iv>) {
        if l >= r {
            return;
        }
        let m = l + (r - l) / 2;
        // Nothing in this subtree reaches `a`.
        if self.max_hi[m] <= a {
            return;
        }
        self.descend(l, m, a, b, out);
        // Sorted by `lo`, so this node and everything right of it start past
        // the query.
        if self.ivs[m].lo >= b {
            return;
        }
        if self.ivs[m].hi > a {
            out.push(self.ivs[m]);
        }
        self.descend(m + 1, r, a, b, out);
    }

    /// The cuts in `[a, b)`, as a slice of the sorted vector.
    ///
    /// One `partition_point` and a forward walk: the positions inside a row are
    /// contiguous in the merged vector, so no tree is needed on this side.
    pub fn cuts_in(&self, a: u64, b: u64) -> &[Cut] {
        if a >= b {
            return &[];
        }
        let s = self.cuts.partition_point(|c| c.at < a);
        let e = self.cuts.partition_point(|c| c.at < b);
        &self.cuts[s..e]
    }

    /// Every cut whose recognition SITE touches `[a, b)`, even when the cut
    /// itself is on another row.
    ///
    /// A row bracket drawn from `cuts_in` shows only the part of a site that
    /// shares a row with its nick: NcoI's CCATGG at pKoV 6,119..6,124 cuts at
    /// 6,120, so the row starting at 6,061 drew a two-cell stub at its right
    /// edge and the row starting at 6,121 — whose first four bases ARE the rest
    /// of that site — drew nothing at all. On a circle a site can also run off
    /// the end of the molecule, which `wrapped` reports so the caller can draw
    /// the far half on row 0.
    pub fn sites_touching(&self, a: u64, b: u64, n: u64) -> impl Iterator<Item = &Cut> {
        // Widened by the largest cut-to-site distance in this document, which
        // is a handful of bases — never a scan of the whole cut list.
        let w = self.site_span;
        let window = |lo: u64, hi: u64| -> &[Cut] {
            if a >= b || lo >= hi {
                return &[];
            }
            let s = self.cuts.partition_point(|c| c.at < lo);
            let e = self.cuts.partition_point(|c| c.at < hi);
            &self.cuts[s..e]
        };
        let main = window(a.saturating_sub(w), b.saturating_add(w));
        // A site running off the end of a circular molecule belongs to row 0 as
        // well, and its cut sits at the far end of the list. Only asked for by
        // the first row, and only when that window is disjoint from the main
        // one, so no cut is offered twice.
        let wrap = if a < w && n.saturating_sub(w) >= b.saturating_add(w) {
            window(n - w, u64::MAX)
        } else {
            &[]
        };
        main.iter().chain(wrap.iter()).filter(move |c| {
            let end = c.site_lo + c.site_len as u64;
            // The second span is the part past the origin, which exists only on
            // a circle and only when the match ran off the end.
            (c.site_lo < b && end > a) || (end > n && end - n > a)
        })
    }

    /// Every interval overlapping the **effective** row `[a, b)`, in effective
    /// coordinates, with the pending run applied.
    ///
    /// The index is in committed coordinates because a keystroke does not go
    /// into the log — it goes into the run, and rebuilding per keystroke would
    /// undo the amortisation `Run` exists for (measured: 4.4 ms of main-thread
    /// work per keystroke on a 4.6 Mb molecule, 442 ms for 100 keystrokes,
    /// against 13 ms for the same keystrokes as one operation). So the QUERY is
    /// translated instead of the index rebuilt.
    pub fn query_run(&self, a: u64, b: u64, run: Option<RunSpan>, out: &mut Vec<Iv>) {
        let Some(r) = run else {
            self.query(a, b, out);
            return;
        };
        // Deliberately generous: a committed coordinate is within `inserted` or
        // `removed` of its effective one, both bounded by `Run::MAX_CHARS`. The
        // clip below throws away whatever this over-collects, and an
        // under-collected range would silently drop a ribbon.
        let lo_c = a.saturating_sub(r.inserted);
        let hi_c = b.saturating_sub(r.inserted) + r.removed + 1;
        let mut raw = Vec::new();
        self.query(lo_c, hi_c, &mut raw);
        for iv in raw {
            let Some((lo, hi)) = remap_for_run(iv.lo, iv.hi, r) else {
                continue;
            };
            let (clo, chi) = (lo.max(a), hi.min(b));
            if chi > clo {
                out.push(Iv { lo, hi, ..iv });
            }
        }
    }
}

/// A lane number that means "there was nowhere on this row to draw it".
///
/// Distinct from any real lane, so the row's `+N` badge and the ribbon loop
/// agree on what was lost without a second flag to keep in step.
pub const NO_LANE: u8 = u8::MAX;

/// Give the overflow whatever lanes this row leaves empty, and report what
/// still did not fit.
///
/// The lanes themselves are assigned once over the whole document so a feature
/// cannot hop while the user scrolls. That is right for everything that HAS a
/// lane and wrong for everything past the cap: a file whose maximum depth
/// anywhere is six gives lanes 3, 4 and 5 to three features and then hides them
/// over their entire length — including on rows where lanes 1 and 2 are empty
/// and there was room all along. Measured on a six-deep file: row 421 drew one
/// ribbon over two empty lanes and still showed an orange "+3".
///
/// So the global lane is kept for every feature that fits in one, and only the
/// overflow is placed here, into the holes this particular row leaves. The
/// invariant that buys: a row showing `+N` really has no room, and a row with an
/// empty lane never shows one.
pub fn compact_row(ivs: &mut [Iv], lanes: u8) -> usize {
    if lanes == 0 {
        let n = ivs.len();
        for iv in ivs.iter_mut() {
            iv.lane = NO_LANE;
        }
        return n;
    }
    // What each lane already holds on this row. Small by construction: a lane
    // is conflict-free document-wide, so it can only hold pieces that are
    // disjoint, and a row is sixty bases wide.
    let mut taken: Vec<(u8, u64, u64)> = Vec::new();
    for iv in ivs.iter() {
        if iv.lane < lanes {
            taken.push((iv.lane, iv.lo, iv.hi));
        }
    }
    let mut hidden = 0usize;
    for iv in ivs.iter_mut() {
        if iv.lane < lanes {
            continue;
        }
        let free = (0..lanes).find(|&l| {
            !taken
                .iter()
                .any(|&(tl, lo, hi)| tl == l && lo < iv.hi && hi > iv.lo)
        });
        match free {
            Some(l) => {
                iv.lane = l;
                taken.push((l, iv.lo, iv.hi));
            }
            None => {
                iv.lane = NO_LANE;
                hidden += 1;
            }
        }
    }
    hidden
}

/// Fill `max_hi` for the subtree covering `ivs[l..r]`, returning its maximum.
fn augment(ivs: &[Iv], max_hi: &mut [u64], l: usize, r: usize) -> u64 {
    if l >= r {
        return 0;
    }
    let m = l + (r - l) / 2;
    let left = augment(ivs, max_hi, l, m);
    let right = augment(ivs, max_hi, m + 1, r);
    let v = ivs[m].hi.max(left).max(right);
    max_hi[m] = v;
    v
}

/// Where a committed span lands while a run is open, or `None` if nothing of it
/// survives.
///
/// This is `pl_core::oplog::remap_annotations` read in 0-based half-open terms.
/// It is not a paraphrase for its own sake: if the preview disagrees with the
/// commit, the ribbon visibly snaps a second after the user stops typing, and
/// `the_pending_preview_matches_what_the_commit_actually_produces` compares the
/// two directly rather than comparing new code against new code.
///
/// The consequence worth stating: an insertion strictly inside a segment keeps
/// the segment's start and moves its end, so the feature GROWS over the typed
/// bases. Previewing them as outside it would draw a gap in the ribbon that is
/// not there a second later.
pub fn remap_for_run(lo: u64, hi: u64, r: RunSpan) -> Option<(u64, u64)> {
    let (s, rem, ins) = (r.start, r.removed, r.inserted);
    let old_end = s + rem;
    let shift = |p: u64| -> u64 {
        if ins >= rem {
            p.saturating_add(ins - rem)
        } else {
            p.saturating_sub(rem - ins)
        }
    };
    // An equal-length replacement leaves every base in place, so a coordinate
    // inside it still means what it meant. A length change does not.
    let interior_survives = ins > 0 && ins == rem;

    let a = if lo < s {
        Some(lo)
    } else if lo >= old_end {
        Some(shift(lo))
    } else if interior_survives {
        Some(lo)
    } else {
        // The far end survives, so the near end pins to where the replacement
        // ends rather than where it begins.
        (hi > old_end).then_some(s + ins)
    };
    let b = if hi <= s {
        Some(hi)
    } else if hi > old_end {
        Some(shift(hi))
    } else if interior_survives {
        Some(hi)
    } else {
        (lo < s).then_some(s)
    };
    match (a, b) {
        (Some(a), Some(b)) if b > a => Some((a, b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{Feature, Segment, Topology};

    fn mol(n: usize, circular: bool, segs: &[(u64, u64)]) -> Molecule {
        let mut m = Molecule {
            seq: vec![b'a'; n],
            topology: if circular {
                Topology::Circular
            } else {
                Topology::Linear
            },
            ..Default::default()
        };
        for (i, (s, e)) in segs.iter().enumerate() {
            let mut f = Feature::new(format!("f{i}"), "misc_feature");
            f.segments.push(Segment::new(*s, *e));
            m.features.push(f);
        }
        m
    }

    fn v() -> Version {
        (0, None)
    }

    /// What a linear scan says, in the same shape the index answers in.
    ///
    /// The union step is here too, written out longhand rather than reusing
    /// [`union_in_place`], because it is part of the SPEC the index answers to
    /// and not an implementation detail: a joined CDS whose exons touch is one
    /// piece. What stays genuinely independent — and is what this oracle is for
    /// — is which pieces a range query returns, i.e. the augmented tree.
    fn naive(mol: &Molecule, a: u64, b: u64) -> Vec<(u32, u16, u64, u64)> {
        // An empty range overlaps nothing. Stated rather than falling out of
        // the loop, because `plo < b && phi > a` with `a == b` is a non-empty
        // test on an empty range and answers "everything spanning that point".
        if a >= b {
            return Vec::new();
        }
        let n = mol.len();
        let circular = mol.topology.is_circular();
        let mut out = Vec::new();
        for (fi, f) in mol.features.iter().enumerate() {
            let mut flat: Vec<(u64, u64, u16)> = Vec::new();
            for (si, s) in f.segments.iter().enumerate() {
                let lo = s.start.saturating_sub(1);
                let pieces: Vec<(u64, u64)> = if s.end < s.start {
                    let mut p = Vec::new();
                    if circular && lo < n {
                        p.push((lo, n));
                    }
                    if circular && s.end >= 1 {
                        p.push((0, s.end.min(n)));
                    }
                    p
                } else {
                    vec![(lo, s.end.min(n))]
                };
                for (plo, phi) in pieces {
                    if phi > plo {
                        flat.push((plo, phi, si as u16));
                    }
                }
            }
            flat.sort_unstable();
            let mut merged: Vec<(u64, u64, u16)> = Vec::new();
            for (plo, phi, si) in flat {
                match merged.last_mut() {
                    Some(last) if plo <= last.1 => last.1 = last.1.max(phi),
                    _ => merged.push((plo, phi, si)),
                }
            }
            for (plo, phi, si) in merged {
                if plo < b && phi > a {
                    out.push((fi as u32, si, plo, phi));
                }
            }
        }
        out.sort_unstable();
        out
    }

    fn asked(ix: &AnnotIndex, a: u64, b: u64) -> Vec<(u32, u16, u64, u64)> {
        let mut got = Vec::new();
        ix.query(a, b, &mut got);
        let mut got: Vec<_> = got.iter().map(|i| (i.feat, i.seg, i.lo, i.hi)).collect();
        got.sort_unstable();
        got
    }

    #[test]
    fn one_based_inclusive_becomes_zero_based_half_open_exactly_once() {
        // Segment 1..10 is bases 1 through 10, which is caret gaps 0 through 10
        // — columns 0..10 half-open. One cell either way is the difference
        // between "EcoRI cuts inside AmpR" and "immediately after it".
        let ix = AnnotIndex::build(&mol(100, false, &[(1, 10)]), v());
        assert_eq!(asked(&ix, 0, 100), vec![(0, 0, 0, 10)]);
        // And the boundaries: base 10 is in, base 11 is not.
        assert_eq!(asked(&ix, 9, 10).len(), 1, "the last base is covered");
        assert_eq!(asked(&ix, 10, 11).len(), 0, "and the next one is not");
    }

    #[test]
    fn an_origin_crossing_segment_is_two_pieces_and_neither_is_empty() {
        // 7900..120 on 8,117 bp: `end < start`, legal on a circle. Stored
        // unsplit this is `lo > hi`, i.e. empty, and the feature disappears
        // from the sequence view while still drawing on the map.
        let ix = AnnotIndex::build(&mol(8117, true, &[(7900, 120)]), v());
        assert_eq!(
            asked(&ix, 0, 8117),
            vec![(0, 0, 0, 120), (0, 0, 7899, 8117)]
        );
        assert_eq!(asked(&ix, 0, 1).len(), 1, "base 1 is inside it");
        assert_eq!(asked(&ix, 7899, 7900).len(), 1, "and so is base 7,900");
        assert_eq!(asked(&ix, 200, 7899).len(), 0, "and nothing between");
    }

    #[test]
    fn the_two_pieces_of_a_wrap_carry_one_terminus_each() {
        let ix = AnnotIndex::build(&mol(8117, true, &[(7900, 120)]), v());
        let mut got = Vec::new();
        ix.query(0, 8117, &mut got);
        got.sort_unstable_by_key(|i| i.lo);
        assert!(!got[0].starts && got[0].ends, "the tail owns the 3' end");
        assert!(got[1].starts && !got[1].ends, "the head owns the 5' end");
    }

    #[test]
    fn a_wrap_on_a_linear_molecule_is_dropped_and_counted() {
        let ix = AnnotIndex::build(&mol(100, false, &[(90, 10)]), v());
        assert_eq!(asked(&ix, 0, 100), vec![]);
        assert_eq!(ix.dropped, 1, "counted, never silent");
    }

    #[test]
    fn absurd_coordinates_from_a_hostile_file_do_not_panic() {
        let ix = AnnotIndex::build(&mol(100, true, &[(1, u64::MAX), (0, 0)]), v());
        // The first is clamped to the molecule; the second names nothing.
        assert_eq!(asked(&ix, 0, 100), vec![(0, 0, 0, 100)]);
        assert_eq!(ix.dropped, 1);
    }

    #[test]
    fn the_index_answers_exactly_what_a_linear_scan_answers() {
        // The oracle. New code against an independent naive implementation, on
        // a corpus that includes every shape a real file arrives in.
        let mut s = 0x2545_F491_4F6C_DD1Du64;
        let mut rnd = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let n = 8117u64;
        let mut m = Molecule {
            seq: vec![b'a'; n as usize],
            topology: Topology::Circular,
            ..Default::default()
        };
        // The named shapes, first.
        let fixed: &[&[(u64, u64)]] = &[
            &[(7900, 120)],            // origin-crossing single segment
            &[(7900, 8117), (1, 120)], // the GenBank join spelling of it
            &[(100, 200), (400, 500)], // multi-exon join
            &[(50, 50)],               // one base
            &[(60, 59)],               // inverted
            &[(1, 1)],                 // the very first base
            &[(n, n)],                 // the very last
            &[(1, n)],                 // the whole molecule
            &[(1, u64::MAX)],          // hostile
        ];
        for segs in fixed {
            let mut f = Feature::new("fixed", "misc_feature");
            for (a, b) in segs.iter() {
                f.segments.push(Segment::new(*a, *b));
            }
            m.features.push(f);
        }
        // And 400 random ones.
        for _ in 0..400 {
            let a = rnd() % (n + 40);
            let len = rnd() % 900;
            let b = a + len;
            let mut f = Feature::new("r", "CDS");
            f.segments.push(Segment::new(a, b));
            m.features.push(f);
        }
        let ix = AnnotIndex::build(&m, v());

        let per_row = 60u64;
        let rows = n.div_ceil(per_row);
        for row in 0..rows {
            let (a, b) = (row * per_row, ((row + 1) * per_row).min(n));
            assert_eq!(asked(&ix, a, b), naive(&m, a, b), "row {row} [{a}, {b})");
        }
        // Including the last partial row stated on its own, and a few awkward
        // ranges the row loop cannot produce.
        let last = (rows - 1) * per_row;
        assert_eq!(asked(&ix, last, n), naive(&m, last, n));
        for (a, b) in [(0, 1), (n - 1, n), (0, n), (n, n + 10), (4000, 4000)] {
            assert_eq!(asked(&ix, a, b), naive(&m, a, b), "[{a}, {b})");
        }
    }

    #[test]
    fn lanes_are_assigned_once_over_the_whole_document() {
        // Three mutually overlapping features get three lanes; a fourth that
        // does not overlap them reuses lane 0. The point is that the answer
        // does not depend on which rows happen to be visible.
        let ix = AnnotIndex::build(
            &mol(1000, false, &[(1, 100), (50, 150), (60, 160), (500, 600)]),
            v(),
        );
        let mut got = Vec::new();
        ix.query(0, 1000, &mut got);
        let lane = |f: u32| got.iter().find(|i| i.feat == f).unwrap().lane;
        assert_eq!(lane(0), 0);
        assert_eq!(lane(1), 1);
        assert_eq!(lane(2), 2);
        assert_eq!(lane(3), 0, "no overlap, so lane 0 is free again");
        assert_eq!(ix.depth, 3);
        assert_eq!(ix.lanes, 3);
    }

    #[test]
    fn depth_past_the_cap_is_counted_rather_than_dropped() {
        let segs: Vec<(u64, u64)> = (0..6).map(|i| (1 + i, 500)).collect();
        let ix = AnnotIndex::build(&mol(1000, false, &segs), v());
        assert_eq!(ix.depth, 6, "the file really has six");
        assert_eq!(ix.lanes, MAX_LANES, "three are drawn");
        let mut got = Vec::new();
        ix.query(0, 60, &mut got);
        let hidden = got.iter().filter(|i| i.lane >= ix.lanes).count();
        assert_eq!(hidden, 3, "and three are counted, not dropped");
    }

    /// A helper for the shapes below: one feature, several segments.
    fn joined(n: usize, circular: bool, segs: &[(u64, u64)]) -> Molecule {
        let mut m = Molecule {
            seq: vec![b'a'; n],
            topology: if circular {
                Topology::Circular
            } else {
                Topology::Linear
            },
            ..Default::default()
        };
        let mut f = Feature::new("j", "CDS");
        for (s, e) in segs {
            f.segments.push(Segment::new(*s, *e));
        }
        m.features.push(f);
        m
    }

    fn all(ix: &AnnotIndex, n: u64) -> Vec<Iv> {
        let mut got = Vec::new();
        ix.query(0, n, &mut got);
        got.sort_unstable_by_key(|i| i.lo);
        got
    }

    /// pKoV's own SacB: `join(1976..3310,3311..3397)`, two segments that touch.
    ///
    /// COMPILE-ONLY at bd96e5b (no `AnnotIndex`). The MUTATION it is proof
    /// against — deleting the `union_in_place` call — was run and fails on the
    /// first assertion with two pieces, which is what put a spurious arrowhead
    /// and a boundary tick through the middle of a contiguous CDS.
    #[test]
    fn segments_that_touch_are_one_piece_with_one_pair_of_ends() {
        let ix = AnnotIndex::build(&joined(8117, true, &[(1976, 3310), (3311, 3397)]), v());
        let got = all(&ix, 8117);
        assert_eq!(got.len(), 1, "one contiguous CDS, not two exons");
        assert_eq!((got[0].lo, got[0].hi), (1975, 3397));
        assert!(
            got[0].feat_lo && got[0].feat_hi,
            "and one 5' and one 3' end"
        );
    }

    /// A real intron stays two pieces, and only the outer ends are the
    /// feature's ends — the inner two are segment boundaries, which the view
    /// draws with a smaller mark and no arrowhead.
    #[test]
    fn a_gap_between_segments_survives_but_only_the_outer_ends_are_the_features() {
        let ix = AnnotIndex::build(&joined(1000, false, &[(100, 200), (400, 500)]), v());
        let got = all(&ix, 1000);
        assert_eq!(got.len(), 2);
        assert!(
            got[0].starts && got[0].ends,
            "both are real exon boundaries"
        );
        assert!(got[1].starts && got[1].ends);
        assert!(got[0].feat_lo && !got[0].feat_hi, "5' end only");
        assert!(!got[1].feat_lo && got[1].feat_hi, "3' end only");
    }

    /// The GenBank spelling of an origin-crossing feature.
    ///
    /// `join(7900..8117,1..120)` is two ordinary segments; nothing in the
    /// coordinates says they are adjacent across the origin. Left alone, base
    /// 8,117 looked like a 3' terminus and the view drew an arrowhead there as
    /// well as at base 120.
    #[test]
    fn the_join_spelling_of_a_wrap_has_one_3_prime_end_and_it_is_past_the_origin() {
        let ix = AnnotIndex::build(&joined(8117, true, &[(7900, 8117), (1, 120)]), v());
        let got = all(&ix, 8117);
        assert_eq!(got.len(), 2, "two arcs");
        assert_eq!((got[0].lo, got[0].hi), (0, 120));
        assert_eq!((got[1].lo, got[1].hi), (7899, 8117));
        assert!(!got[0].starts, "base 1 is the origin, not a 5' end");
        assert!(!got[1].ends, "and base 8,117 is not a 3' end");
        assert!(got[1].feat_lo, "the 5' end is at 7,900");
        assert!(got[0].feat_hi, "and the 3' end at 120");
        assert_eq!(
            got.iter().filter(|i| i.feat_hi).count(),
            1,
            "exactly one arrowhead"
        );
        // And the same file written the other way agrees.
        let one = AnnotIndex::build(&joined(8117, true, &[(7900, 120)]), v());
        let b = all(&one, 8117);
        assert_eq!(
            b.iter()
                .map(|i| (i.lo, i.hi, i.feat_lo, i.feat_hi))
                .collect::<Vec<_>>(),
            got.iter()
                .map(|i| (i.lo, i.hi, i.feat_lo, i.feat_hi))
                .collect::<Vec<_>>(),
        );
    }

    /// A feature covering the whole circle has one piece whose two ends really
    /// are the origin, so the wrap rule must not fire on it.
    #[test]
    fn a_whole_molecule_feature_keeps_both_of_its_ends() {
        let ix = AnnotIndex::build(&joined(500, true, &[(1, 500)]), v());
        let got = all(&ix, 500);
        assert_eq!(got.len(), 1);
        assert!(got[0].feat_lo && got[0].feat_hi);
    }

    /// The defect: three features hidden on a row with two empty lanes.
    ///
    /// COMPILE-ONLY at bd96e5b. The MUTATION — returning `ivs.len()` without
    /// placing anything, i.e. the old behaviour — was run and fails on the
    /// second assertion.
    #[test]
    fn the_overflow_is_drawn_in_the_lanes_a_row_leaves_empty() {
        let mut ivs: Vec<Iv> = [(0u64, 500u64, 0u8), (300, 430, 3), (300, 440, 4)]
            .iter()
            .enumerate()
            .map(|(i, &(lo, hi, lane))| Iv {
                lo,
                hi,
                feat: i as u32,
                seg: 0,
                lane,
                starts: true,
                ends: true,
                feat_lo: true,
                feat_hi: true,
            })
            .collect();
        let hidden = compact_row(&mut ivs, MAX_LANES);
        assert_eq!(hidden, 0, "there were two empty lanes and two to place");
        assert_eq!(ivs[0].lane, 0, "a feature that has a lane never moves");
        assert_eq!(ivs[1].lane, 1);
        assert_eq!(ivs[2].lane, 2);
        // Nothing overlapping shares a lane.
        for i in 0..ivs.len() {
            for j in i + 1..ivs.len() {
                assert!(
                    ivs[i].lane != ivs[j].lane || ivs[i].hi <= ivs[j].lo || ivs[j].hi <= ivs[i].lo,
                    "{i} and {j} overlap in lane {}",
                    ivs[i].lane
                );
            }
        }
    }

    /// And the promise that makes the badge mean something: a row can only say
    /// `+N` when every lane really is occupied where the hidden thing sits.
    #[test]
    fn a_row_that_reports_a_hidden_feature_has_no_empty_lane_under_it() {
        let mk = |i: u32, lane: u8| Iv {
            lo: 300,
            hi: 450,
            feat: i,
            seg: 0,
            lane,
            starts: true,
            ends: true,
            feat_lo: true,
            feat_hi: true,
        };
        let mut ivs: Vec<Iv> = vec![mk(0, 0), mk(1, 1), mk(2, 2), mk(3, 3), mk(4, 4)];
        let hidden = compact_row(&mut ivs, MAX_LANES);
        assert_eq!(hidden, 2, "five mutually overlapping, three lanes");
        assert_eq!(ivs[3].lane, NO_LANE);
        assert_eq!(ivs[4].lane, NO_LANE);
        let drawn = ivs.iter().filter(|i| i.lane < MAX_LANES).count();
        assert_eq!(
            drawn, MAX_LANES as usize,
            "every lane is in use, which is what makes the +N honest"
        );
    }

    /// NcoI at pKoV 6,119..6,124 cutting at 6,120: the site straddles the row
    /// boundary at 6,120, so the row that owns the cut is not the only row it
    /// covers.
    #[test]
    fn a_site_that_straddles_a_row_boundary_is_offered_to_both_rows() {
        let mut ix = AnnotIndex::build(&mol(8117, true, &[]), v());
        ix.set_cuts(vec![Cut {
            at: 6119,
            site_lo: 6118,
            site_len: 6,
            enzyme: 0,
        }]);
        let n = 8117;
        let row = |a: u64| ix.sites_touching(a, a + 60, n).count();
        assert_eq!(
            ix.cuts_in(6120, 6180).len(),
            0,
            "the cut is on the row before"
        );
        assert_eq!(row(6060), 1, "the row that owns the cut");
        assert_eq!(row(6120), 1, "and the row holding the rest of the site");
        assert_eq!(row(6180), 0, "and no further");
        assert_eq!(row(6000), 0);
    }

    /// A Type IIS enzyme cuts outside its own site, so the window cannot be
    /// derived from site length: BsaI is GGTCTC(1/5).
    #[test]
    fn a_cut_far_from_its_own_site_still_finds_every_row_the_site_touches() {
        let mut ix = AnnotIndex::build(&mol(1000, false, &[]), v());
        ix.set_cuts(vec![Cut {
            at: 131,
            site_lo: 124,
            site_len: 6,
            enzyme: 0,
        }]);
        assert_eq!(ix.sites_touching(120, 180, 1000).count(), 1);
        assert_eq!(ix.sites_touching(60, 120, 1000).count(), 0);
        assert_eq!(ix.sites_touching(180, 240, 1000).count(), 0);
    }

    /// On a circle a match can run off the end, and the bases past the origin
    /// are as much inside the site as the ones before it.
    #[test]
    fn a_site_running_past_the_origin_is_offered_to_the_first_row_too() {
        let mut ix = AnnotIndex::build(&mol(8117, true, &[]), v());
        ix.set_cuts(vec![Cut {
            at: 8115,
            site_lo: 8114,
            site_len: 6,
            enzyme: 0,
        }]);
        assert_eq!(ix.sites_touching(8100, 8117, 8117).count(), 1);
        assert_eq!(ix.sites_touching(0, 60, 8117).count(), 1, "bases 1..3");
        assert_eq!(ix.sites_touching(60, 120, 8117).count(), 0);
    }

    #[test]
    fn a_cut_and_its_site_are_different_objects() {
        let mut ix = AnnotIndex::build(&mol(100, false, &[]), v());
        // EcoRI G^AATTC matched at 0-based 10: the site is [10, 16), the cut is
        // the gap at 11. Drawing the cut at the site start is one base out.
        ix.set_cuts(vec![Cut {
            at: 11,
            site_lo: 10,
            site_len: 6,
            enzyme: 0,
        }]);
        assert_eq!(
            ix.cuts_in(0, 11).len(),
            0,
            "the cut is not at the site start"
        );
        assert_eq!(ix.cuts_in(11, 12).len(), 1);
        assert_eq!(ix.cuts_in(0, 60).len(), 1);
    }

    #[test]
    fn the_run_preview_grows_a_feature_over_bases_typed_inside_it() {
        // feature 10..20 (0-based [9, 20)); type three bases at gap 14.
        let r = RunSpan {
            start: 14,
            removed: 0,
            inserted: 3,
        };
        assert_eq!(
            remap_for_run(9, 20, r),
            Some((9, 23)),
            "start pinned, end moved"
        );
        // Typed before it: the whole thing shifts.
        assert_eq!(
            remap_for_run(
                9,
                20,
                RunSpan {
                    start: 2,
                    removed: 0,
                    inserted: 3
                }
            ),
            Some((12, 23))
        );
        // Typed after it: untouched.
        assert_eq!(
            remap_for_run(
                9,
                20,
                RunSpan {
                    start: 40,
                    removed: 0,
                    inserted: 3
                }
            ),
            Some((9, 20))
        );
        // Deleted out from under it entirely.
        assert_eq!(
            remap_for_run(
                9,
                20,
                RunSpan {
                    start: 0,
                    removed: 60,
                    inserted: 0
                }
            ),
            None
        );
    }
}
