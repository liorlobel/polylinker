//! The amino-acid track: which features are translated, in which frame, and
//! which residue sits over which base.
//!
//! Nothing here touches egui and nothing here touches `SeqEdit`, so the
//! arithmetic that decides whether a glyph sits over its own codon is exercised
//! by ordinary unit tests rather than by driving a window. `main.rs` owns the
//! painting and calls in. Bases arrive through a `read` closure rather than off
//! `Molecule::seq` — see [`Path::residue`] for why that is not a matter of
//! taste.
//!
//! # A track is a VIEW
//!
//! Nothing in this module constructs an [`OpKind`](pl_core::oplog::OpKind) or
//! touches a [`Document`](crate::doc::Document). Showing a translation must not
//! edit anything, and the only way to keep that true is for the code that draws
//! it to have no way to say otherwise.
//!
//! # `Segment::translated` finally has a reader
//!
//! `crates/pl-core/src/lib.rs` has carried `Segment::translated` since the
//! SnapGene reader was written, `crates/pl-fileio/src/snapgene.rs:453` parses
//! it, `featedit.rs` offers a checkbox for it, and until this module existed
//! **nothing in this program read it** — the checkbox's own hover text said so.
//! [`Translations::build`] is that reader: rule 1 below is the flag, honoured
//! even on a feature whose kind is not `CDS`, because that is the question
//! SnapGene asks the bit and the only thing it can mean.
//!
//! # One coordinate convention
//!
//! Everything here is **0-based half-open**, the caret's own space, the same as
//! `annot.rs`. `pl_core::Segment` is 1-based inclusive and the conversion
//! happens in exactly one place, [`Translations::build`].

use pl_core::iupac;
use pl_core::translate::{self, Code};
use pl_core::{Molecule, Strand};

use crate::annot::{remap_for_run, RunSpan};

/// How many simultaneously-translated features get a residue lane per strand.
///
/// Two, and the number is bought with vertical space rather than guessed:
/// measured at the default panel on pKoV, one forward lane plus the complement
/// row plus one reverse lane takes the row pitch from 39.94 pt to 84.76 and the
/// viewport from 15 rows to 7 — 53% of the sequence on screen. A third lane per
/// strand costs another 14.94 pt and buys a case no plasmid in the corpus has.
/// Overflow past the cap goes into the row's existing `+N` badge, which is the
/// channel that already means "N things on this row I could not show you".
///
/// The ad-hoc translation of the selection is NOT counted against this. It gets
/// a lane of its own, above the cap, because the alternative is the one failure
/// with no honest reading: sharing a lane with a file translation draws two
/// proteins interleaved a column apart, and a reader has no way to tell which
/// letters are which. It costs one more `text_h` of row pitch, and only while
/// `+ selection` is switched on.
pub const MAX_AA_LANES: u8 = 2;

/// Which translations the sequence view is showing.
///
/// An enum and not three booleans: three independent flags give eight states,
/// several of which draw the same picture, and the reserved row height has to
/// be ONE number per document — see the row-height note in `main.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackMode {
    Off,
    /// The file's own translations: `Segment::translated`, else `kind == "CDS"`.
    #[default]
    File,
    /// The file's own, plus an ad-hoc translation of the selection.
    ///
    /// The selection's translation is a *view* of bases the user pointed at. It
    /// is the front door for "is my His tag in frame?" on a feature the file
    /// never marked translated — which is exactly what pKoV's `decR his` is:
    /// `misc_feature`, `strand: none`, no `translated` flag, and 483 bases this
    /// plasmid exists to carry.
    Selection,
}

impl TrackMode {
    pub fn label(self) -> &'static str {
        match self {
            TrackMode::Off => "off",
            TrackMode::File => "from file",
            TrackMode::Selection => "+ selection",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            TrackMode::Off => "off",
            TrackMode::File => "file",
            TrackMode::Selection => "selection",
        }
    }
    pub fn from_key(s: &str) -> Option<Self> {
        match s {
            "off" => Some(TrackMode::Off),
            "file" => Some(TrackMode::File),
            "selection" => Some(TrackMode::Selection),
            _ => None,
        }
    }
    pub fn is_on(self) -> bool {
        !matches!(self, TrackMode::Off)
    }
}

/// Why a residue is not simply a letter.
///
/// Colour is never the only channel in this application, so every one of these
/// carries a shape as well as a hue where it is drawn, and a sentence in the
/// hover line where it is explained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Plain,
    /// The last codon of the reading, and a stop. Expected.
    StopEnd,
    /// A stop with more reading after it. The single most valuable thing this
    /// track can tell a cloner, and the loudest thing it draws.
    StopInside,
    /// A codon that both terminates and encodes, which is true of exactly six
    /// codons across tables 27, 28 and 31. `pl_core::translate` says the choice
    /// between them depends on context it does not have; neither does this.
    AmbiguousStop,
    /// The first codon, read as `M` although it does not spell one.
    ///
    /// `Code::translate_cds` substitutes the initiator, which is the convention
    /// a GenBank `/translation` uses — and it CHANGES A LETTER, so somebody
    /// checking a primer against the track has to be told, or they see `M` and
    /// order `ATG` for a codon that reads `GTG`.
    Initiator,
    /// `Code::codon` could not resolve the codon to one residue: an ambiguity
    /// code with more than one answer, or a byte that is not a nucleotide code
    /// at all. The hover line separates the two; the glyph cannot.
    Ambiguous,
}

/// One residue, placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Residue {
    /// 0-based index along the reading.
    pub k: usize,
    /// The letter to draw. Already `M`-substituted for an initiator.
    pub aa: u8,
    pub mark: Mark,
    /// The three bases, in reading order, complemented for a reverse reading.
    pub codon: [u8; 3],
    /// Their molecule coordinates, 0-based, in reading order.
    pub coords: [u64; 3],
    /// Are the three coordinates three adjacent cells on one row?
    ///
    /// False across a join and across the origin, where the three cells under
    /// the glyph are NOT the codon's three bases. Unmarked, a reader takes the
    /// cells for the codon and reads a triplet that is not there.
    pub contiguous: bool,
}

impl Residue {
    /// The cell the glyph goes in: the codon's MIDDLE base.
    ///
    /// Every codon has exactly one middle base, so every codon is drawn exactly
    /// once, on exactly the row that holds it, and always entirely inside the
    /// band. Placing the glyph on the codon's FIRST base instead puts a codon
    /// starting at column 59 one and a half cells past the right edge of the
    /// row, into the gutter or the clip.
    pub fn mid(&self) -> u64 {
        self.coords[1]
    }
    /// The lowest and highest coordinate the codon covers.
    ///
    /// The OUTER BOUND, which for a contiguous codon is the codon and for one
    /// that straddles a join or the origin is not — it is everything between the
    /// two arcs. Use [`run_containing`](Self::run_containing) to select; this is
    /// only for asking whether a codon reaches a range at all.
    pub fn span(&self) -> (u64, u64) {
        let lo = self.coords.iter().copied().min().unwrap_or(0);
        let hi = self.coords.iter().copied().max().unwrap_or(0);
        (lo, hi + 1)
    }

    /// The maximal run of adjacent coordinates of this codon that contains
    /// `coord`, half-open.
    ///
    /// For an ordinary codon that is all three bases. For one split across a
    /// `join()` or the origin it is the one to two bases on the arc the pointer
    /// is actually on, and this is the distinction [`span`](Self::span) cannot
    /// make: on a 22 bp circle a codon at coordinates 20, 21, 0 has a span of
    /// `(0, 22)`, so selecting it by span selects THE WHOLE MOLECULE — and a
    /// `join(101..150, 501..551)` seam codon selects 353 bases, under a sentence
    /// that read "353 of 3 bases selected". One Backspace on either is the loss
    /// of everything between the arcs.
    ///
    /// Returns `None` when `coord` is not one of the codon's three coordinates,
    /// which is a caller that asked about a base it was not pointing at.
    pub fn run_containing(&self, coord: u64) -> Option<(u64, u64)> {
        if !self.coords.contains(&coord) {
            return None;
        }
        let (mut lo, mut hi) = (coord, coord + 1);
        // Three coordinates, so two passes settle it whatever the order they
        // are in — a reverse reading walks them downwards.
        for _ in 0..2 {
            if self.coords.contains(&hi) {
                hi += 1;
            }
            if lo > 0 && self.coords.contains(&(lo - 1)) {
                lo -= 1;
            }
        }
        Some((lo, hi))
    }
}

/// One translation, as a path through the molecule.
///
/// Not a materialised protein. MG1655 carries about 4,400 CDS features and
/// 1.3 million residues; the coordinates of residue `k` are two additions away
/// from the path and there is no reason to store them.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    /// Index into `Molecule::features`, or [`SELECTION`] for the ad-hoc one.
    pub feat: u32,
    pub name: String,
    pub reverse: bool,
    pub code: Code,
    /// The translated segments in **reading order**, 0-based half-open and
    /// never wrapping. See [`Translations::build`] for why the order is not the
    /// coordinate order on a reverse strand.
    pub parts: Vec<(u64, u64)>,
    /// Bases of `parts` skipped before the first codon: `/codon_start - 1`.
    pub skip: u64,
    /// Which residue lane, within this path's strand.
    pub lane: u8,
    /// Did `Segment::translated` say so, or was this inferred from `kind`?
    pub from_flag: bool,
    /// A `/codon_start` that is present and is not 1, 2 or 3. Said, not
    /// silently clamped.
    pub bad_codon_start: Option<String>,
}

/// The feature index of the ad-hoc translation of the selection.
///
/// `u32::MAX` cannot collide with a real index: a `Molecule` holding four
/// billion features would have exhausted memory long before.
pub const SELECTION: u32 = u32::MAX;

impl Path {
    /// Bases in the reading, after `skip`.
    pub fn bases(&self) -> u64 {
        self.parts
            .iter()
            .map(|&(lo, hi)| hi - lo)
            .sum::<u64>()
            .saturating_sub(self.skip)
    }

    /// Whole codons. A trailing partial codon has no residue — `Code::translate`
    /// drops it, and so does this.
    pub fn aa_len(&self) -> usize {
        (self.bases() / 3) as usize
    }

    /// Bases left over after the last whole codon: 0, 1 or 2.
    ///
    /// Reported rather than ignored. A CDS clamped into range by the index, or
    /// one running off the end of a linear molecule, otherwise simply stops one
    /// or two cells early, which reads as a shorter protein rather than as
    /// missing data.
    pub fn ragged(&self) -> u64 {
        self.bases() % 3
    }

    /// The molecule coordinate `off` bases along the reading, before `skip`.
    fn raw_at(parts: &[(u64, u64)], reverse: bool, off: u64) -> Option<u64> {
        let mut o = off;
        for &(lo, hi) in parts {
            let len = hi - lo;
            if o < len {
                // Reverse readings walk each part from its HIGH end. The parts
                // themselves are already in reading order, so this is the only
                // place the direction appears.
                return Some(if reverse { hi - 1 - o } else { lo + o });
            }
            o -= len;
        }
        None
    }

    /// The molecule coordinate of the `off`-th base of the reading.
    pub fn at(&self, off: u64) -> Option<u64> {
        Self::raw_at(&self.parts, self.reverse, off + self.skip)
    }

    /// The three coordinates of residue `k` and the bases they hold, in reading
    /// order and already complemented for a reverse reading.
    ///
    /// `read` rather than `&Molecule::seq`, and this is the highest-probability
    /// defect in the whole feature because it looks perfect in a screenshot:
    /// `SeqEdit::byte_at` applies the pending typing run, `Molecule::seq` does
    /// not. A track fed the committed sequence draws the PRE-edit frame under
    /// POST-edit letters, so typing one base into a CDS and watching the
    /// downstream residues fail to change is the exact inverse of what this
    /// track exists for.
    ///
    /// Split out so the internal-stop count can ask `Code::is_stop` alone
    /// without paying for the rest of [`residue`](Self::residue) -- and split
    /// out rather than duplicated, so the two cannot disagree about which three
    /// bases a residue is.
    pub fn codon_at(&self, k: usize, read: &dyn Fn(u64) -> u8) -> Option<([u64; 3], [u8; 3])> {
        let base = (k as u64).checked_mul(3)?;
        let coords = [self.at(base)?, self.at(base + 1)?, self.at(base + 2)?];
        let mut codon = [0u8; 3];
        for (i, &c) in coords.iter().enumerate() {
            let b = read(c);
            codon[i] = if self.reverse {
                iupac::complement(b)
            } else {
                b
            };
        }
        Some((coords, codon))
    }

    /// Residue `k`: its letter, its mark, its codon and where that codon sits.
    pub fn residue(&self, k: usize, read: &dyn Fn(u64) -> u8) -> Option<Residue> {
        let (coords, codon) = self.codon_at(k, read)?;
        let [c0, c1, c2] = coords;
        let mut aa = self.code.codon(&codon);
        let mut mark = Mark::Plain;
        if k == 0 && self.code.is_start(&codon) {
            // `translate_cds`' rule, and the reason it needs a mark: the letter
            // on screen is not the letter the codon spells. `tet(A)` starts
            // GTG, which table 11 initiates and table 1 does not, so the SAME
            // three bases are `M` under one number and `V` under another.
            if aa != b'M' {
                mark = Mark::Initiator;
            }
            aa = b'M';
        } else if self.code.is_stop(&codon) {
            mark = if self.code.is_ambiguous_stop(&codon) {
                Mark::AmbiguousStop
            } else if k + 1 >= self.aa_len() {
                Mark::StopEnd
            } else {
                Mark::StopInside
            };
        } else if aa == b'X' {
            mark = Mark::Ambiguous;
        }
        let contiguous = if self.reverse {
            c0 == c1 + 1 && c1 == c2 + 1
        } else {
            c1 == c0 + 1 && c2 == c1 + 1
        };
        Some(Residue {
            k,
            aa,
            mark,
            codon,
            coords,
            contiguous,
        })
    }

    /// Which residue covers `coord` at all, in any of its three positions.
    pub fn residue_covering(&self, coord: u64) -> Option<usize> {
        let off = self.offset_of(coord)?;
        (off >= self.skip).then(|| ((off - self.skip) / 3) as usize)
    }

    /// How far along the reading `coord` sits, before `skip`.
    fn offset_of(&self, coord: u64) -> Option<u64> {
        let mut base = 0u64;
        for &(lo, hi) in &self.parts {
            if coord >= lo && coord < hi {
                return Some(
                    base + if self.reverse {
                        hi - 1 - coord
                    } else {
                        coord - lo
                    },
                );
            }
            base += hi - lo;
        }
        None
    }

    /// Every residue whose MIDDLE base falls in `[start, end)`, appended.
    ///
    /// The middle-base rule is what makes this exactly-once: a codon split 2|1
    /// across a row edge draws at the last column of the earlier row, one split
    /// 1|2 draws at column 0 of the later row, and neither is drawn twice or
    /// clipped. The alternative — widening the window to `[start-2, end+2)` and
    /// drawing a straddling codon on both rows — has to then decide which half
    /// of a glyph to clip, and puts the same residue on screen twice.
    pub fn residues_in_row(
        &self,
        start: u64,
        end: u64,
        read: &dyn Fn(u64) -> u8,
        out: &mut Vec<Residue>,
    ) {
        if start >= end {
            return;
        }
        let mut base = 0u64;
        for &(lo, hi) in &self.parts {
            let a = lo.max(start);
            let b = hi.min(end);
            // Bounded by the row, never by the part: a 4.6 Mb CDS contributes
            // at most `per_row` iterations here, the same as a 30 bp one.
            for c in a..b {
                let off = base + if self.reverse { hi - 1 - c } else { c - lo };
                if off < self.skip {
                    continue;
                }
                let t = off - self.skip;
                if t % 3 != 1 {
                    continue;
                }
                let k = (t / 3) as usize;
                if k >= self.aa_len() {
                    continue;
                }
                if let Some(r) = self.residue(k, read) {
                    out.push(r);
                }
            }
            base += hi - lo;
        }
    }

    /// This path in the coordinates the user is looking at, with a typing run
    /// applied.
    ///
    /// `remap_for_run` and not a second formula: it is `pl_core`'s own
    /// `remap_annotations` read in half-open terms, and the consequence worth
    /// stating is that an insertion strictly inside a segment GROWS it. That is
    /// exactly right here — typing three bases into a CDS shifts every
    /// downstream residue by one, which is the frame check this track is for.
    pub fn effective(&self, run: Option<RunSpan>) -> Path {
        let Some(r) = run else {
            return self.clone();
        };
        let parts = self
            .parts
            .iter()
            .filter_map(|&(lo, hi)| remap_for_run(lo, hi, r))
            .collect();
        Path {
            parts,
            ..self.clone()
        }
    }
}

/// Every translation this document offers, and how many lanes they need.
#[derive(Debug, Default)]
pub struct Translations {
    paths: Vec<Path>,
    /// `by_feat[i]` is the index into `paths` of the path for feature `i`.
    by_feat: Vec<Option<u32>>,
    /// Residue lanes reserved on the forward strand. A property of the
    /// DOCUMENT, never of the visible rows — see `annot::AnnotIndex::lanes`.
    pub fwd_lanes: u8,
    pub rev_lanes: u8,
    /// Translations past the cap, over the whole document.
    pub over_cap: usize,
    /// Features with `translated` set, or `kind == "CDS"`, whose strand is
    /// neither forward nor reverse. A feature with no direction has no reading
    /// direction, and guessing forward is right by luck on one file and wrong
    /// on the next.
    pub unoriented: Vec<String>,
    /// Features that asked for a reading and produced no bases to read.
    ///
    /// One shape reaches this today: a segment written `end < start` — "round
    /// the origin" — on a LINEAR molecule, which has no origin to go round. It
    /// is the same shape `annot::AnnotIndex::of_spans` already counts as
    /// `dropped`, and it is counted rather than skipped because a CDS missing
    /// from the track looks exactly like a molecule that never had one.
    pub dropped: Vec<String>,
    /// `(table id, how many features)` for the features translated with a
    /// `/transl_table` of their own rather than with the document's default.
    ///
    /// The header's "translated with table N" sentence exists to say which code
    /// produced the letters, so it must not be able to name a table nothing
    /// used. Every one of pKoV's three CDSs carries `/transl_table=1`, so on
    /// that file the document default reaches no residue at all.
    pub own_tables: Vec<(u8, usize)>,
    /// `(name, 1-based coordinate)` of every stop codon with more reading after
    /// it, over the committed sequence.
    ///
    /// Counted here rather than only drawn, because an internal stop means the
    /// annotation is wrong or an insert is out of frame and a reader who has to
    /// notice one red asterisk among 470 residues will not.
    pub internal_stops: Vec<(String, u64)>,
    /// True when [`STOP_SCAN_CAP`] was reached and the count above is partial.
    ///
    /// A cap rather than an unbounded scan because MG1655 carries about 4,400
    /// CDS features and 1.3 million residues, and `Code::codon` allocates once
    /// per codon — a scan nobody asked for, on every document version. Said
    /// rather than silently truncated, which would be a count that looks
    /// complete and is not.
    pub stops_capped: bool,
    /// How many readings the internal-stop scan got through before its budget
    /// ran out. Only meaningful when [`stops_capped`](Self::stops_capped).
    ///
    /// Recorded so the disclosure can say what was NOT examined rather than
    /// only what was. "counted over the first 100,000 residues" reads like a
    /// footnote on a whole-document count; on MG1655 that budget is spent
    /// inside the first ~330 of 4,400 readings, and a frameshift past that
    /// point produces no warning and no statement that its region was skipped.
    pub readings_scanned: usize,
    /// `/codon_start` values that are present and are not 1, 2 or 3.
    pub bad_codon_starts: Vec<String>,
}

/// Codons the internal-stop count will look at before it gives up. See
/// [`Translations::stops_capped`].
pub const STOP_SCAN_CAP: usize = 100_000;

impl Translations {
    /// Which segments of which features are translated, and in which frame.
    ///
    /// Rule 1 — **any segment carries `Segment::translated`** — uses exactly
    /// those segments, whatever the feature's kind. That is SnapGene's explicit
    /// per-segment answer and it is what gives the bit meaning: pKoV's `.dna`
    /// sets it on exactly the three CDS features' segments and on nothing else.
    ///
    /// Rule 2 — **no segment carries it and `kind == "CDS"`** — uses all of
    /// them. This exists because GenBank has no spelling for the flag at all:
    /// `pl convert --to genbank` re-emits pKoV's three CDSs with
    /// `/translation` and no per-segment flag, so without rule 2 the same
    /// plasmid shows a track as `.dna` and none as `.gb` out of one binary.
    pub fn build(mol: &Molecule, default_code: Code) -> Self {
        let n = mol.len();
        let circular = mol.topology.is_circular();
        let mut paths: Vec<Path> = Vec::new();
        let mut by_feat: Vec<Option<u32>> = vec![None; mol.features.len()];
        let mut unoriented: Vec<String> = Vec::new();
        let mut dropped: Vec<String> = Vec::new();
        let mut own_tables: Vec<(u8, usize)> = Vec::new();

        for (fi, f) in mol.features.iter().enumerate() {
            let flagged = f.segments.iter().any(|s| s.translated);
            if !flagged && f.kind != "CDS" {
                continue;
            }
            let reverse = match f.strand {
                Strand::Forward => false,
                Strand::Reverse => true,
                // Said, not guessed. This arm is reached only by something that
                // has already ASKED for a reading — a `CDS`, or any feature
                // whose segment carries `translated` — and has no direction to
                // read it in. pKoV does NOT reach it: its `decR` and `decR his`
                // are unoriented, but they are `misc_feature` with no flag, so
                // they are skipped by the kind test above and are never named
                // here. A comment claiming otherwise stood in this place until a
                // reviewer dumped the file; the disclosure it justified had
                // never once appeared on the plasmid it was written for.
                Strand::Unoriented | Strand::Both => {
                    unoriented.push(f.name.clone());
                    continue;
                }
            };

            // 1-based inclusive -> 0-based half-open, splitting an
            // origin-crossing segment the way `annot::AnnotIndex::build` does.
            // Order is preserved: GenBank's `join(a,b)` means a then b.
            let mut parts: Vec<(u64, u64)> = Vec::new();
            for s in f.segments.iter().filter(|s| !flagged || s.translated) {
                let lo = s.start.saturating_sub(1);
                if s.end < s.start {
                    // `end < start` says "round the origin", and a LINE has no
                    // origin to go round. Nothing can be read from it, so
                    // nothing is pushed — and it is counted, because a CDS that
                    // simply vanishes from the track is indistinguishable from a
                    // molecule that never had one. `annot::AnnotIndex::of_spans`
                    // counts the identical shape as `dropped`; this is that
                    // channel, not a second one.
                    if !circular {
                        continue;
                    }
                    if lo < n {
                        parts.push((lo, n));
                    }
                    if s.end >= 1 {
                        parts.push((0, s.end.min(n)));
                    }
                    continue;
                }
                let hi = s.end.min(n);
                if hi > lo {
                    parts.push((lo, hi));
                }
            }
            if parts.is_empty() {
                dropped.push(f.name.clone());
                continue;
            }
            if reverse {
                // Transcription order. `complement(join(a,b))` reads rc(b) then
                // rc(a) — verified against the file's own answer: pKoV's SacB
                // is `complement(join(1976..3310,3311..3397))` and the `.dna`'s
                // stored /translation begins MNIKKFAKQATVLTFTTALLAGGATQAFA,
                // which is the reverse complement of the LAST-listed segment.
                // Reading the segments in coordinate order gives the protein
                // back to front with its signal peptide at the C terminus.
                parts.reverse();
            }

            // The header exists to say which code produced the letters, so it
            // cannot be allowed to name a table nothing used. `feature_code`'s
            // second return was discarded here, and the consequence was
            // asymmetric in the dangerous direction: switching the combo to
            // table 4 for a mycoplasma insert changed the sentence and not one
            // residue, so a terminal TGA stayed drawn as `*` and read as an
            // internal stop in a code that was never applied.
            let (code, from_file) = feature_code(f, default_code);
            if from_file {
                match own_tables.iter_mut().find(|(id, _)| *id == code.id) {
                    Some(e) => e.1 += 1,
                    None => own_tables.push((code.id, 1)),
                }
            }
            let (skip, bad_codon_start) = feature_codon_start(f);
            paths.push(Path {
                feat: fi as u32,
                name: f.name.clone(),
                reverse,
                code,
                parts,
                skip,
                lane: 0,
                from_flag: flagged,
                bad_codon_start,
            });
            by_feat[fi] = Some(paths.len() as u32 - 1);
        }

        let mut out = Translations {
            bad_codon_starts: paths
                .iter()
                .filter_map(|p| p.bad_codon_start.clone())
                .collect(),
            paths,
            by_feat,
            fwd_lanes: 0,
            rev_lanes: 0,
            over_cap: 0,
            unoriented,
            dropped,
            own_tables,
            internal_stops: Vec::new(),
            stops_capped: false,
            readings_scanned: 0,
        };
        out.assign_lanes();
        out.count_internal_stops(mol);
        out
    }

    /// Walk every reading once and note the stops that are not the last codon.
    ///
    /// Over `Molecule::seq` and not through the typing run: this is a statement
    /// about the DOCUMENT, and it is recomputed when the document version
    /// changes. The marks in the track itself are run-aware; this count is one
    /// version behind during a run and the header says the view is mid-edit.
    fn count_internal_stops(&mut self, mol: &Molecule) {
        let read = |i: u64| mol.seq.get(i as usize).copied().unwrap_or(b' ');
        let mut budget = STOP_SCAN_CAP;
        for (i, p) in self.paths.iter().enumerate() {
            for k in 0..p.aa_len() {
                if budget == 0 {
                    self.stops_capped = true;
                    // Readings COMPLETED, so the disclosure can subtract and
                    // name what it did not look at. This one is partial and is
                    // not counted, which is the conservative direction.
                    self.readings_scanned = i;
                    return;
                }
                budget -= 1;
                // Only a stop codon can be an internal stop, and asking that
                // costs one `codon_resolutions` against `residue`'s four --
                // measured at 10.5 ms -> 6.0 ms for the first frame of a 4.6 Mb
                // genome with 4,641 CDS features. The classification itself
                // still goes through `residue`, so there is one rule for what a
                // mark means and not two.
                let Some((_, codon)) = p.codon_at(k, &read) else {
                    continue;
                };
                if !p.code.is_stop(&codon) {
                    continue;
                }
                let Some(r) = p.residue(k, &read) else {
                    continue;
                };
                if r.mark == Mark::StopInside {
                    self.internal_stops.push((p.name.clone(), r.mid() + 1));
                }
            }
        }
    }

    /// Greedy interval colouring, per strand, over the WHOLE document.
    ///
    /// Per strand because forward residues sit above the strand they are read
    /// from and reverse residues below theirs, so the two never compete for a
    /// lane. Over the whole document for the reason `annot.rs` gives: a lane
    /// assigned from the visible rows makes a translation hop lanes while the
    /// user scrolls, and a screenshot shows one assignment and looks perfect.
    fn assign_lanes(&mut self) {
        let mut over = 0usize;
        for reverse in [false, true] {
            let mut idx: Vec<usize> = (0..self.paths.len())
                .filter(|&i| self.paths[i].reverse == reverse)
                .collect();
            idx.sort_by_key(|&i| extent(&self.paths[i]));
            let mut lane_max_hi: Vec<u64> = Vec::new();
            for i in idx {
                let (lo, hi) = extent(&self.paths[i]);
                let lane = lane_max_hi
                    .iter()
                    .position(|&end| end <= lo)
                    .unwrap_or(lane_max_hi.len());
                if lane == lane_max_hi.len() {
                    lane_max_hi.push(hi);
                } else {
                    lane_max_hi[lane] = lane_max_hi[lane].max(hi);
                }
                self.paths[i].lane = lane.min(255) as u8;
                if lane >= MAX_AA_LANES as usize {
                    over += 1;
                }
            }
            let used = lane_max_hi.len().min(MAX_AA_LANES as usize) as u8;
            if reverse {
                self.rev_lanes = used;
            } else {
                self.fwd_lanes = used;
            }
        }
        self.over_cap = over;
    }

    pub fn paths(&self) -> &[Path] {
        &self.paths
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// The translation of feature `fi`, if it has one.
    ///
    /// Keyed by feature index so the sequence view needs NO second interval
    /// query: the row loop already holds `annot::AnnotIndex::query_run`'s answer
    /// for its ribbons, and every translated feature is one of those features.
    pub fn for_feature(&self, fi: u32) -> Option<&Path> {
        self.by_feat.get(fi as usize).copied().flatten().map(|p| {
            self.paths
                .get(p as usize)
                .expect("by_feat only ever holds indices into paths")
        })
    }
}

/// The coordinate extent of a path, for lane colouring.
///
/// The outer bound over its parts, so a translation crossing the origin is
/// conservatively treated as covering everything between. That can reserve a
/// lane it does not need; drawing two translations in one lane would overprint
/// residues, which is worse and silent.
fn extent(p: &Path) -> (u64, u64) {
    let lo = p.parts.iter().map(|&(l, _)| l).min().unwrap_or(0);
    let hi = p.parts.iter().map(|&(_, h)| h).max().unwrap_or(0);
    (lo, hi)
}

/// Which genetic code translates this feature, and where the number came from.
///
/// Per feature, because a mitochondrial CDS carried in a shuttle vector really
/// is a different code from the plasmid around it and the file already says so.
/// `/transl_table` has been parsed by both readers and echoed by the feature
/// editor since they were written and read by nothing at all.
pub fn feature_code(f: &pl_core::Feature, default_code: Code) -> (Code, bool) {
    match f
        .qualifier("transl_table")
        .and_then(|v| v.trim().parse::<u8>().ok())
        .and_then(translate::table)
    {
        Some(c) => (c, true),
        None => (default_code, false),
    }
}

/// `/codon_start` as a number of bases to drop, and a complaint if it is not
/// one of the three legal values.
///
/// A CDS with `/codon_start=2` translated from its first base is one base out
/// and EVERY residue is wrong while the picture looks perfect. Nothing in this
/// program read it before.
pub fn feature_codon_start(f: &pl_core::Feature) -> (u64, Option<String>) {
    let Some(v) = f.qualifier("codon_start") else {
        return (0, None);
    };
    match v.trim().parse::<u64>() {
        Ok(k @ 1..=3) => (k - 1, None),
        // Used as 1 and SAID, not silently clamped, in an application whose CLI
        // names the number when it refuses one.
        _ => (
            0,
            Some(format!(
                "{}: /codon_start={} is not 1, 2 or 3 — read in frame 1",
                f.name,
                v.trim()
            )),
        ),
    }
}

/// The document's default table: the modal `/transl_table` its CDS features
/// carry, or `None` if none of them says.
///
/// Reading is not editing. This is derived from the file on open and held as
/// view state; it is never written to the molecule, never entered in the
/// append-only log, and never makes the document dirty.
pub fn modal_table(mol: &Molecule) -> Option<Code> {
    let mut counts: Vec<(u8, usize)> = Vec::new();
    for f in &mol.features {
        let Some(id) = f
            .qualifier("transl_table")
            .and_then(|v| v.trim().parse::<u8>().ok())
        else {
            continue;
        };
        if translate::table(id).is_none() {
            continue;
        }
        match counts.iter_mut().find(|(i, _)| *i == id) {
            Some(e) => e.1 += 1,
            None => counts.push((id, 1)),
        }
    }
    counts
        .iter()
        .max_by_key(|(_, c)| *c)
        .and_then(|&(id, _)| translate::table(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{Feature, Segment, Topology};

    fn code() -> Code {
        translate::table(11).expect("table 11 exists")
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

    fn cds(name: &str, strand: Strand, segs: &[(u64, u64)]) -> Feature {
        let mut f = Feature::new(name, "CDS");
        f.strand = strand;
        for &(s, e) in segs {
            f.segments.push(Segment::new(s, e));
        }
        f
    }

    fn protein(p: &Path, seq: &[u8]) -> String {
        let read = |i: u64| seq.get(i as usize).copied().unwrap_or(b' ');
        (0..p.aa_len())
            .map(|k| p.residue(k, &read).expect("in range").aa as char)
            .collect()
    }

    /// The oracle: what `pl_core::translate` says about the same bases.
    fn oracle(mol: &Molecule, p: &Path) -> String {
        let mut bases: Vec<u8> = Vec::new();
        for &(lo, hi) in &p.parts {
            let piece = &mol.seq[lo as usize..hi as usize];
            if p.reverse {
                bases.extend(iupac::reverse_complement(piece));
            } else {
                bases.extend_from_slice(piece);
            }
        }
        let bases = &bases[p.skip as usize..];
        String::from_utf8(p.code.translate_cds(bases)).expect("ascii")
    }

    #[test]
    fn a_single_segment_cds_translates_to_what_pl_core_says() {
        let m = {
            let mut m = mol("TTATGAAACGCGGTTGCTAAGG", false);
            m.features.push(cds("p", Strand::Forward, &[(3, 20)]));
            m
        };
        let t = Translations::build(&m, code());
        let p = &t.paths()[0];
        assert_eq!(protein(p, &m.seq), oracle(&m, p));
        assert_eq!(protein(p, &m.seq), "MKRGC*");
    }

    #[test]
    fn a_two_segment_cds_reads_its_segments_in_transcription_order() {
        // complement(join(4..12, 13..21)): the LAST-listed segment translates
        // FIRST. This is the shape pKoV's SacB has, and reading the segments in
        // coordinate order gives the protein back to front.
        let seq = "GGTTTAGCAACCGCGTTTCATAGG";
        let m = {
            let mut m = mol(seq, false);
            m.features
                .push(cds("s", Strand::Reverse, &[(4, 12), (13, 21)]));
            m
        };
        let t = Translations::build(&m, code());
        let p = &t.paths()[0];
        assert_eq!(p.parts, vec![(12, 21), (3, 12)]);
        assert_eq!(protein(p, &m.seq), oracle(&m, p));
        assert_eq!(protein(p, &m.seq), "MKRGC*");
    }

    #[test]
    fn a_cds_crossing_the_origin_translates_as_one_reading() {
        // 22 bp circle; the CDS runs 18..11, i.e. off the end and back.
        let seq = "GAAACGCGGTTGCTAATTTTAT";
        let m = {
            let mut m = mol(seq, true);
            m.features.push(cds("o", Strand::Forward, &[(21, 16)]));
            m
        };
        let t = Translations::build(&m, code());
        let p = &t.paths()[0];
        assert_eq!(p.parts, vec![(20, 22), (0, 16)]);
        assert_eq!(protein(p, &m.seq), oracle(&m, p));
        assert_eq!(protein(p, &m.seq), "MKRGC*");
    }

    #[test]
    fn the_segment_translated_flag_selects_the_segments_and_beats_the_kind() {
        // A misc_feature — NOT a CDS — with the flag on one of two segments.
        // Nothing in this program read that bit before this module existed.
        let mut f = Feature::new("tagged", "misc_feature");
        f.strand = Strand::Forward;
        let mut a = Segment::new(3, 20);
        a.translated = true;
        f.segments.push(a);
        f.segments.push(Segment::new(21, 22));
        let m = {
            let mut m = mol("TTATGAAACGCGGTTGCTAAGG", false);
            m.features.push(f);
            m
        };
        let t = Translations::build(&m, code());
        assert_eq!(t.paths().len(), 1, "the flag must produce a translation");
        let p = &t.paths()[0];
        assert!(p.from_flag);
        assert_eq!(p.parts, vec![(2, 20)], "only the flagged segment");
        assert_eq!(protein(p, &m.seq), "MKRGC*");
    }

    #[test]
    fn a_feature_with_no_strand_gets_no_track_and_is_named() {
        // pKoV's `decR his` exactly: no direction, so no reading direction.
        let mut f = Feature::new("decR his", "misc_feature");
        f.strand = Strand::Unoriented;
        let mut s = Segment::new(3, 20);
        s.translated = true;
        f.segments.push(s);
        let m = {
            let mut m = mol("TTATGAAACGCGGTTGCTAAGG", false);
            m.features.push(f);
            m
        };
        let t = Translations::build(&m, code());
        assert!(t.is_empty());
        assert_eq!(t.unoriented, vec!["decR his".to_string()]);
    }

    #[test]
    fn codon_start_two_shifts_every_residue() {
        let mut f = cds("p", Strand::Forward, &[(2, 20)]);
        f.qualifiers.push(("codon_start".into(), Some("2".into())));
        let m = {
            let mut m = mol("TTATGAAACGCGGTTGCTAAGG", false);
            m.features.push(f);
            m
        };
        let t = Translations::build(&m, code());
        let p = &t.paths()[0];
        assert_eq!(p.skip, 1);
        assert_eq!(protein(p, &m.seq), oracle(&m, p));
        assert_eq!(protein(p, &m.seq), "MKRGC*");
    }

    #[test]
    fn every_codon_has_exactly_one_middle_base_and_is_drawn_once() {
        let seq: Vec<u8> = "ATG".bytes().cycle().take(300).collect();
        let m = {
            let mut m = Molecule {
                seq: seq.clone(),
                topology: Topology::Linear,
                ..Default::default()
            };
            m.features.push(cds("p", Strand::Forward, &[(1, 300)]));
            m
        };
        let t = Translations::build(&m, code());
        let p = &t.paths()[0];
        let read = |i: u64| seq.get(i as usize).copied().unwrap_or(b' ');
        let mut seen: Vec<usize> = Vec::new();
        let per_row = 60u64;
        for r in 0..5 {
            let mut got = Vec::new();
            p.residues_in_row(r * per_row, (r + 1) * per_row, &read, &mut got);
            for res in got {
                assert!(
                    res.mid() >= r * per_row && res.mid() < (r + 1) * per_row,
                    "residue {} placed outside its row",
                    res.k
                );
                seen.push(res.k);
            }
        }
        seen.sort_unstable();
        let expect: Vec<usize> = (0..100).collect();
        assert_eq!(seen, expect, "every codon exactly once across the rows");
    }

    #[test]
    fn a_codon_split_across_a_row_boundary_draws_on_the_row_of_its_middle_base() {
        // Frame 1: codons at 1..4, 4..7, ... so the codon spanning 58,59,60 has
        // its middle at 59 (row 0) and the one at 59,60,61 has its middle at 60
        // (row 1). Both are drawn, on different rows, and neither twice.
        let seq: Vec<u8> = "ATGCCC".bytes().cycle().take(240).collect();
        let m = {
            let mut m = Molecule {
                seq: seq.clone(),
                topology: Topology::Linear,
                ..Default::default()
            };
            m.features.push(cds("p", Strand::Forward, &[(2, 240)]));
            m
        };
        let t = Translations::build(&m, code());
        let p = &t.paths()[0];
        let read = |i: u64| seq.get(i as usize).copied().unwrap_or(b' ');
        let mut row0 = Vec::new();
        p.residues_in_row(0, 60, &read, &mut row0);
        let mut row1 = Vec::new();
        p.residues_in_row(60, 120, &read, &mut row1);
        // Path starts at coordinate 1, so residue k has coords 1+3k..3+3k.
        // Middle base of residue 19 is 1 + 57 + 1 = 59 -> row 0.
        // Residue 20's coords are 61,62,63 -> row 1. Residue 19's codon covers
        // 58,59,60 and is entirely inside row 0.
        assert!(row0.iter().any(|r| r.k == 19));
        assert!(!row1.iter().any(|r| r.k == 19), "not drawn twice");
        assert_eq!(row0.last().expect("non-empty").k, 19);
        assert_eq!(row1.first().expect("non-empty").k, 20);
        // And no residue is dropped at the seam.
        let ks: Vec<usize> = row0.iter().chain(row1.iter()).map(|r| r.k).collect();
        assert_eq!(ks, (0..40).collect::<Vec<_>>());
    }

    #[test]
    fn a_codon_across_the_origin_is_marked_not_contiguous() {
        let seq = "GAAACGCGGTTGCTAATTTTAT";
        let m = {
            let mut m = mol(seq, true);
            m.features.push(cds("o", Strand::Forward, &[(21, 16)]));
            m
        };
        let t = Translations::build(&m, code());
        let p = &t.paths()[0];
        let read = |i: u64| seq.as_bytes().get(i as usize).copied().unwrap_or(b' ');
        // Residue 0 is at coordinates 20, 21, 0 — the origin sits inside it.
        let r0 = p.residue(0, &read).expect("residue 0");
        assert_eq!(r0.coords, [20, 21, 0]);
        assert!(!r0.contiguous, "the three cells under it are not its bases");
        let r1 = p.residue(1, &read).expect("residue 1");
        assert!(r1.contiguous);
    }

    /// PROVEN TO FAIL before the fix: `span()` was what the click handler fed
    /// to `Selection`, and for a codon that straddles the origin `span()` is the
    /// OUTER BOUND. Reverting `run_containing` to `span()` turns this red.
    ///
    /// Both numbers below are what the running application actually did: on this
    /// 22 bp circle clicking residue 0 selected ALL 22 BASES, and on a
    /// `join(101..150, 501..551)` the seam codon selected 353 — under a sentence
    /// that read "353 of 3 bases selected". Either one is a Backspace away from
    /// deleting everything between the two arcs, from a click meant for three
    /// bases.
    #[test]
    fn a_codon_across_the_origin_selects_only_the_arc_the_pointer_is_on() {
        let seq = "GAAACGCGGTTGCTAATTTTAT";
        let m = {
            let mut m = mol(seq, true);
            m.features.push(cds("o", Strand::Forward, &[(21, 16)]));
            m
        };
        let t = Translations::build(&m, code());
        let p = &t.paths()[0];
        let read = |i: u64| seq.as_bytes().get(i as usize).copied().unwrap_or(b' ');
        let r0 = p.residue(0, &read).expect("residue 0");
        assert_eq!(r0.coords, [20, 21, 0]);
        assert!(!r0.contiguous);
        // The whole molecule, which is what `span()` says and what was selected.
        assert_eq!(r0.span(), (0, 22));
        // The arc under the pointer, which is what is selected now.
        assert_eq!(r0.run_containing(20), Some((20, 22)), "two bases, not 22");
        assert_eq!(r0.run_containing(21), Some((20, 22)));
        assert_eq!(r0.run_containing(0), Some((0, 1)), "one base, not 22");
        assert_eq!(r0.run_containing(5), None, "not one of its three");
        // An ordinary codon is unaffected: its run IS its span.
        let r1 = p.residue(1, &read).expect("residue 1");
        assert!(r1.contiguous);
        assert_eq!(r1.run_containing(r1.mid()), Some(r1.span()));
    }

    /// The `join()` half of the same defect, on a molecule with a real intron.
    ///
    /// No file in the corpus has one — pKoV does not — so this is the only thing
    /// that pins it, and it is synthetic and said to be.
    #[test]
    fn a_codon_across_a_join_selects_only_the_exon_the_pointer_is_on() {
        let seq: Vec<u8> = "ACGT".bytes().cycle().take(800).collect();
        let m = {
            let mut m = Molecule {
                seq,
                topology: Topology::Linear,
                ..Default::default()
            };
            m.features
                .push(cds("j", Strand::Forward, &[(101, 150), (501, 551)]));
            m
        };
        let t = Translations::build(&m, code());
        let p = &t.paths()[0];
        let read = |i: u64| m.seq.get(i as usize).copied().unwrap_or(b' ');
        // Exon 1 is 50 bases, so residue 16 takes 148, 149 and then jumps.
        let r = p.residue(16, &read).expect("the seam residue");
        assert_eq!(r.coords, [148, 149, 500]);
        assert!(!r.contiguous);
        assert_eq!(r.span(), (148, 501), "353 bases: the outer bound");
        assert_eq!(r.run_containing(149), Some((148, 150)), "the exon, not 353");
        assert_eq!(r.run_containing(500), Some((500, 501)));
    }

    /// PROVEN TO FAIL before the fix: the feature simply was not there, and
    /// nothing anywhere said so. Removing the `dropped.push` turns this red.
    #[test]
    fn a_backwards_segment_on_a_linear_molecule_is_counted_rather_than_vanishing() {
        let m = {
            let mut m = mol("TTATGAAACGCGGTTGCTAAGG", false);
            // `end < start` means "round the origin", and a line has none.
            m.features.push(cds("nowhere", Strand::Forward, &[(15, 4)]));
            m
        };
        let t = Translations::build(&m, code());
        assert!(t.is_empty(), "there is nothing to read");
        assert_eq!(t.dropped, vec!["nowhere".to_string()]);
        assert!(t.unoriented.is_empty(), "it has a strand; that is not why");
        // The same segment on a CIRCLE is a real reading and is not counted.
        let c = {
            let mut c = mol("TTATGAAACGCGGTTGCTAAGG", true);
            c.features.push(cds("wraps", Strand::Forward, &[(15, 4)]));
            c
        };
        let t = Translations::build(&c, code());
        assert_eq!(t.paths().len(), 1);
        assert!(t.dropped.is_empty());
    }

    /// PROVEN TO FAIL before the fix: `feature_code`'s "did the file say?"
    /// return was discarded, so `Translations` could not tell the header that
    /// the document default reached no residue. Discarding it again empties
    /// `own_tables` and turns this red.
    #[test]
    fn a_feature_translated_with_its_own_table_is_recorded_for_the_header() {
        let mut own = cds("mito", Strand::Forward, &[(1, 12)]);
        own.qualifiers
            .push(("transl_table".into(), Some("4".into())));
        let m = {
            let mut m = mol("ATGTGAAAATAAATGAAACGCTAA", false);
            m.features.push(own);
            m.features.push(cds("plain", Strand::Forward, &[(13, 24)]));
            m
        };
        let t = Translations::build(&m, code());
        assert_eq!(t.own_tables, vec![(4u8, 1usize)], "one feature, table 4");
        // And the code really did reach the residues, which is the thing the
        // header would otherwise be lying about.
        assert_eq!(t.paths()[0].code.id, 4);
        assert_eq!(t.paths()[1].code.id, 11, "the document default");
    }

    #[test]
    fn a_stop_inside_and_a_stop_at_the_end_are_different_marks() {
        let seq = "ATGTAAAAATGCTAA";
        let m = {
            let mut m = mol(seq, false);
            m.features.push(cds("p", Strand::Forward, &[(1, 15)]));
            m
        };
        let t = Translations::build(&m, code());
        let p = &t.paths()[0];
        let read = |i: u64| seq.as_bytes().get(i as usize).copied().unwrap_or(b' ');
        assert_eq!(p.residue(1, &read).expect("k1").mark, Mark::StopInside);
        assert_eq!(p.residue(4, &read).expect("k4").mark, Mark::StopEnd);
        assert_eq!(p.residue(1, &read).expect("k1").aa, b'*');
    }

    #[test]
    fn an_alternative_initiator_reads_m_and_says_it_did() {
        // GTG is an initiator in table 11 and is not one in table 1. Same three
        // bases, different letter, decided by a number that must be on screen.
        let seq = "GTGAAACGCTAA";
        let m = {
            let mut m = mol(seq, false);
            m.features.push(cds("p", Strand::Forward, &[(1, 12)]));
            m
        };
        let read = |i: u64| seq.as_bytes().get(i as usize).copied().unwrap_or(b' ');
        let t11 = Translations::build(&m, code());
        let r = t11.paths()[0].residue(0, &read).expect("k0");
        assert_eq!(r.aa, b'M');
        assert_eq!(r.mark, Mark::Initiator);

        let t1 = Translations::build(&m, translate::table(1).expect("table 1"));
        let r = t1.paths()[0].residue(0, &read).expect("k0");
        assert_eq!(r.aa, b'V', "table 1 does not initiate at GTG");
        assert_eq!(r.mark, Mark::Plain);
    }

    #[test]
    fn a_codon_containing_n_renders_as_x_and_is_marked() {
        let seq = "ATGGNCAAATAA";
        let m = {
            let mut m = mol(seq, false);
            m.features.push(cds("p", Strand::Forward, &[(1, 12)]));
            m
        };
        let t = Translations::build(&m, code());
        let p = &t.paths()[0];
        let read = |i: u64| seq.as_bytes().get(i as usize).copied().unwrap_or(b' ');
        let r = p.residue(1, &read).expect("k1");
        assert_eq!(r.aa, b'X');
        assert_eq!(r.mark, Mark::Ambiguous);
        // GGN is glycine under every resolution, so it is NOT X.
        let seq2 = "ATGGGNAAATAA";
        let read2 = |i: u64| seq2.as_bytes().get(i as usize).copied().unwrap_or(b' ');
        let r = p.residue(1, &read2).expect("k1");
        assert_eq!(r.aa, b'G');
        assert_eq!(r.mark, Mark::Plain);
    }

    #[test]
    fn a_trailing_partial_codon_has_no_residue_and_is_counted() {
        let m = {
            let mut m = mol("ATGAAACG", false);
            m.features.push(cds("p", Strand::Forward, &[(1, 8)]));
            m
        };
        let t = Translations::build(&m, code());
        let p = &t.paths()[0];
        assert_eq!(p.aa_len(), 2);
        assert_eq!(p.ragged(), 2);
    }

    #[test]
    fn a_reverse_residue_reads_the_complement_of_the_letters_above_it() {
        // The letters on screen are the top strand. A reverse residue's codon
        // is their complement, read right to left.
        let seq = "TTAGCAACCGCGTTTCAT";
        let m = {
            let mut m = mol(seq, false);
            m.features.push(cds("r", Strand::Reverse, &[(1, 18)]));
            m
        };
        let t = Translations::build(&m, code());
        let p = &t.paths()[0];
        let read = |i: u64| seq.as_bytes().get(i as usize).copied().unwrap_or(b' ');
        let r = p.residue(0, &read).expect("k0");
        assert_eq!(&r.codon, b"ATG");
        assert_eq!(r.coords, [17, 16, 15]);
        assert!(r.contiguous, "a reverse codon on one arc is still adjacent");
        assert_eq!(protein(p, &m.seq), "MKRGC*");
    }

    #[test]
    fn the_lane_count_is_a_property_of_the_document() {
        let m = {
            let mut m = mol(&"ATGAAACGCGGTTGCTAA".repeat(10), false);
            m.features.push(cds("a", Strand::Forward, &[(1, 60)]));
            m.features.push(cds("b", Strand::Forward, &[(30, 90)]));
            m.features.push(cds("c", Strand::Reverse, &[(1, 60)]));
            m
        };
        let t = Translations::build(&m, code());
        assert_eq!(t.fwd_lanes, 2);
        assert_eq!(t.rev_lanes, 1);
        assert_eq!(t.over_cap, 0);
    }

    #[test]
    fn a_typed_base_moves_every_downstream_residue() {
        // The whole point of the track. An insert of one base at coordinate 3
        // inside the CDS grows it and shifts the frame.
        let seq = "ATGAAACGCGGTTGCTAA";
        let m = {
            let mut m = mol(seq, false);
            m.features.push(cds("p", Strand::Forward, &[(1, 18)]));
            m
        };
        let t = Translations::build(&m, code());
        let p = t.paths()[0].effective(Some(RunSpan {
            start: 3,
            removed: 0,
            inserted: 1,
        }));
        assert_eq!(p.parts, vec![(0, 19)], "the CDS grew over the typed base");
        // The effective bases, as `SeqEdit::byte_at` would report them.
        let eff = "ATGTAAACGCGGTTGCTAA";
        let read = |i: u64| eff.as_bytes().get(i as usize).copied().unwrap_or(b' ');
        assert_eq!(p.residue(1, &read).expect("k1").aa, b'*');
        assert_eq!(p.aa_len(), 6);
    }

    #[test]
    fn a_features_own_transl_table_beats_the_document_default() {
        let mut f = cds("mito", Strand::Forward, &[(1, 12)]);
        f.qualifiers.push(("transl_table".into(), Some("4".into())));
        let m = {
            let mut m = mol("ATGTGAAAATAA", false);
            m.features.push(f);
            m
        };
        let t = Translations::build(&m, code());
        let p = &t.paths()[0];
        assert_eq!(p.code.id, 4);
        let read = |i: u64| m.seq.get(i as usize).copied().unwrap_or(b' ');
        // TGA is tryptophan in table 4 and a stop in table 11.
        assert_eq!(p.residue(1, &read).expect("k1").aa, b'W');
    }

    #[test]
    fn a_nonsense_codon_start_is_said_rather_than_clamped_silently() {
        let mut f = cds("p", Strand::Forward, &[(1, 12)]);
        f.qualifiers.push(("codon_start".into(), Some("7".into())));
        let (skip, why) = feature_codon_start(&f);
        assert_eq!(skip, 0);
        assert!(why.expect("said").contains("not 1, 2 or 3"));
    }
}
