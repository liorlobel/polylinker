//! History as an append-only, content-addressed operation log.
//!
//! `docs/PLAN.md` ADR-2: **history and undo are the same mechanism**. There is
//! no separate "history feature" bolted on later — undo is a cursor into the
//! log, and the log is the provenance record.
//!
//! # Three properties, and why each one is load-bearing
//!
//! **Append-only.** Nothing is ever mutated or discarded. Undo moves a cursor
//! backwards; a *new edit after an undo* forks the log rather than truncating
//! it. Every other editor throws the redo branch away at that moment, and that
//! is where people lose an afternoon's work without ever seeing a warning.
//!
//! **Content-addressed.** An operation's identity is the hash of what it does
//! and what it was done to. Two labs performing the same digest on the same
//! plasmid derive the same id, which turns history into something *comparable
//! across machines* — a provenance record rather than a UI affordance.
//! Timestamps and author names are therefore deliberately **excluded** from the
//! hash: they are metadata about who did it, not about what was done.
//!
//! **Lazily materialised.** Only the current document and a snapshot every
//! [`SNAPSHOT_EVERY`] operations are kept. Seeking replays forward from the
//! nearest snapshot. Snapshot-per-edit on a 200 kb sequence is untenable, and
//! is the documented cause of SnapGene's history memory bloat.
//!
//! # What is deliberately absent
//!
//! No CRDT. A naive JSON CRDT merging two edits to a sequence will happily
//! produce a construct whose feature coordinates no longer match its bases —
//! a silent corruption rather than a conflict. The log is *shaped* so it could
//! replay into one later, and that is as far as it goes for now.

use std::collections::{BTreeMap, HashMap};

use crate::sha1::sha1;
use crate::{Feature, Methylation, Molecule, Topology};

/// Documents are snapshotted this often so seeking does not replay from zero.
pub const SNAPSHOT_EVERY: usize = 50;

/// An operation's identity: the hash of its content and its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OpId([u8; 20]);

impl OpId {
    /// Short form for display, like a git prefix.
    pub fn short(&self) -> String {
        self.0[..4].iter().map(|b| format!("{b:02x}")).collect()
    }
    pub fn hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl std::fmt::Display for OpId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.short())
    }
}

/// What an operation does.
///
/// Coordinates are 1-based inclusive, matching the rest of the model.
#[derive(Debug, Clone, PartialEq)]
pub enum OpKind {
    InsertAt {
        at: u64,
        seq: String,
    },
    DeleteRange {
        start: u64,
        len: u64,
    },
    ReplaceRange {
        start: u64,
        len: u64,
        seq: String,
    },
    SetTopology(Topology),
    Rotate {
        origin: u64,
    },
    ReverseComplement,
    /// Add a feature, or replace the one at `index`.
    SetFeature {
        index: Option<usize>,
        feature: Box<Feature>,
    },
    RemoveFeature {
        index: usize,
    },
    SetMethylation(Methylation),
}

impl OpKind {
    /// A short human-readable description, for a history view.
    pub fn describe(&self) -> String {
        match self {
            OpKind::InsertAt { at, seq } => format!("insert {} bp at {at}", seq.len()),
            OpKind::DeleteRange { start, len } => format!("delete {len} bp at {start}"),
            OpKind::ReplaceRange { start, len, seq } => {
                format!("replace {len} bp at {start} with {} bp", seq.len())
            }
            OpKind::SetTopology(t) => format!("make {}", t.as_str()),
            OpKind::Rotate { origin } => format!("rotate origin to {origin}"),
            OpKind::ReverseComplement => "reverse complement".into(),
            OpKind::SetFeature { index, feature } => match index {
                Some(i) => format!("edit feature {i} ({})", feature.name),
                None => format!("add feature {}", feature.name),
            },
            OpKind::RemoveFeature { index } => format!("remove feature {index}"),
            OpKind::SetMethylation(_) => "set methylation".into(),
        }
    }

    /// The bytes that define this operation for hashing.
    ///
    /// Every field that changes the result must appear here, and nothing else
    /// may. Adding a timestamp would make two identical digests hash
    /// differently and destroy the cross-machine comparability that is the
    /// whole point of content addressing.
    ///
    /// # The encoding must be injective
    ///
    /// Two operations that differ in *any* way `apply` acts on must produce
    /// different bytes, and this got it wrong twice.
    ///
    /// Fields were omitted: `SetFeature` hashed only a feature's name, kind,
    /// strand and segment bounds, while `apply` clones the whole feature. So
    /// editing a qualifier or a segment colour derived the *same* `OpId` as the
    /// edit before it, `apply` saw the id already present and declined to record
    /// the operation — while still advancing to the new document. The log then
    /// said one thing and the document another, and undo/redo silently restored
    /// the older text. A qualifier edit disappearing without an error is
    /// exactly the class of loss ADR-2 exists to prevent. `SetMethylation` had
    /// the same hole for `Methylation::cpg`, a field added to the struct after
    /// this encoding was written: two states differing only in CpG hashed alike,
    /// so toggling CpG after an undo was refused outright.
    ///
    /// And the framing was ambiguous: variable-length fields were separated by
    /// NUL bytes, but a Rust `String` may contain NUL, so `(name "a\0b", kind
    /// "c")` and `(name "a", kind "b\0c")` encoded identically. Every
    /// variable-length field is now length-prefixed instead.
    fn content(&self) -> Vec<u8> {
        /// Append a length-prefixed field. Unambiguous for any byte content.
        fn field(v: &mut Vec<u8>, bytes: &[u8]) {
            v.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
            v.extend_from_slice(bytes);
        }

        let mut v = Vec::new();
        match self {
            OpKind::InsertAt { at, seq } => {
                v.push(1);
                v.extend_from_slice(&at.to_be_bytes());
                v.extend_from_slice(seq.as_bytes());
            }
            OpKind::DeleteRange { start, len } => {
                v.push(2);
                v.extend_from_slice(&start.to_be_bytes());
                v.extend_from_slice(&len.to_be_bytes());
            }
            OpKind::ReplaceRange { start, len, seq } => {
                v.push(3);
                v.extend_from_slice(&start.to_be_bytes());
                v.extend_from_slice(&len.to_be_bytes());
                v.extend_from_slice(seq.as_bytes());
            }
            OpKind::SetTopology(t) => {
                v.push(4);
                v.push(if t.is_circular() { 1 } else { 0 });
            }
            OpKind::Rotate { origin } => {
                v.push(5);
                v.extend_from_slice(&origin.to_be_bytes());
            }
            OpKind::ReverseComplement => v.push(6),
            OpKind::SetFeature { index, feature } => {
                v.push(7);
                v.extend_from_slice(&(index.map(|i| i as u64).unwrap_or(u64::MAX)).to_be_bytes());
                field(&mut v, feature.name.as_bytes());
                field(&mut v, feature.kind.as_bytes());
                // `Unoriented` has no directionality; distinguish it from a
                // recorded 0 rather than folding the two together.
                match feature.strand.to_directionality() {
                    Some(d) => {
                        v.push(1);
                        v.extend_from_slice(&d.to_be_bytes());
                    }
                    None => v.push(0),
                }
                v.extend_from_slice(&(feature.segments.len() as u64).to_be_bytes());
                for s in &feature.segments {
                    v.extend_from_slice(&s.start.to_be_bytes());
                    v.extend_from_slice(&s.end.to_be_bytes());
                    v.push(s.translated as u8);
                    field(&mut v, s.kind.as_bytes());
                    match &s.color {
                        Some(c) => {
                            v.push(1);
                            field(&mut v, c.as_bytes());
                        }
                        None => v.push(0),
                    }
                }
                v.extend_from_slice(&(feature.qualifiers.len() as u64).to_be_bytes());
                for (k, val) in &feature.qualifiers {
                    field(&mut v, k.as_bytes());
                    match val {
                        Some(x) => {
                            v.push(1);
                            field(&mut v, x.as_bytes());
                        }
                        None => v.push(0),
                    }
                }
            }
            OpKind::RemoveFeature { index } => {
                v.push(8);
                v.extend_from_slice(&(*index as u64).to_be_bytes());
            }
            OpKind::SetMethylation(m) => {
                v.push(9);
                v.push(m.dam as u8);
                v.push(m.dcm as u8);
                v.push(m.ecoki as u8);
                // `cpg` was added to `Methylation` after this encoding was
                // written and never reached it, while `apply` assigns the whole
                // struct. Two states differing only in `cpg` therefore derived
                // one `OpId`: toggle CpG, undo, toggle it the other way, and the
                // second edit was refused with `IdCollision` — the error two
                // comments in this file call unreachable.
                v.push(m.cpg as u8);
            }
        }
        v
    }
}

/// One entry in the log.
#[derive(Debug, Clone)]
pub struct Op {
    pub id: OpId,
    pub parent: Option<OpId>,
    pub kind: OpKind,
    /// Metadata only. Not hashed — see [`OpKind::content`].
    pub actor: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpError {
    /// The operation refers to coordinates the molecule does not have.
    OutOfRange {
        what: &'static str,
        at: u64,
        len: u64,
    },
    /// Rotation only means something on a circle.
    NotCircular,
    NoSuchFeature {
        index: usize,
    },
    /// The operation would have left the molecule describing itself
    /// incorrectly.
    ///
    /// Because coordinates here are `{start, end}` rather than
    /// `{start, length}`, an edit *can* produce a feature that points outside
    /// the sequence or ends before it starts. `docs/PLAN.md` §5.3.1 makes that
    /// an accepted cost, paid back here: an operation that would corrupt the
    /// annotations is refused rather than recorded.
    WouldCorrupt(Vec<crate::Invalid>),
    /// A reverse complement was asked to reflect a coordinate that has no
    /// reflection.
    ///
    /// Reversing is reflection across the molecule's length: position `p`
    /// becomes `n + 1 - p`. A coordinate outside `1..=n` has no image under
    /// that map, and `n.saturating_sub(p).saturating_add(1)` answered 1 for
    /// every one of them — so a segment `4..9000` on 8 bases became `1..5`,
    /// `validate()` went from one `PastEnd` to clean, and `is_valid()` flipped
    /// from false to true. The log's own gate cannot catch that: it compares
    /// per-kind problem counts and only refuses *increases*, and an edit that
    /// erases a problem by inventing an in-range coordinate is precisely what
    /// it waves through. The `n == 0` case is worse still — a standalone
    /// annotation track (features, no bases, no declared length; UGENE and
    /// SnapGene both export them) collapsed every feature and every primer site
    /// to `1..1`, destroying a 774 bp and an 834 bp annotation with `apply`
    /// returning `Ok`, and reverse-complementing again did not bring them back.
    /// Refusing is a loss the user can see and undo; the alternative was a
    /// fabrication they could not.
    CannotReflect {
        what: String,
        at: u64,
        len: u64,
    },
    /// Two different operations derived the same identity.
    ///
    /// Should be unreachable: `OpKind::content` is length-prefixed and covers
    /// every field `apply` acts on. It exists because the alternative to
    /// noticing a collision was silently discarding an edit, and a refused
    /// operation is recoverable where a lost one is not.
    IdCollision {
        id: OpId,
    },
    /// Nothing to undo or redo.
    AtEnd,
}

impl std::fmt::Display for OpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpError::OutOfRange { what, at, len } => {
                write!(f, "{what} at {at} is outside a {len} bp molecule")
            }
            OpError::NotCircular => write!(f, "only a circular molecule can be rotated"),
            OpError::NoSuchFeature { index } => write!(f, "there is no feature {index}"),
            OpError::WouldCorrupt(v) => {
                write!(f, "that would leave the molecule inconsistent: ")?;
                for (i, x) in v.iter().take(3).enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{x}")?;
                }
                if v.len() > 3 {
                    write!(f, "; and {} more", v.len() - 3)?;
                }
                Ok(())
            }
            OpError::CannotReflect { what, at, len } => {
                if *len == 0 {
                    write!(
                        f,
                        "there is nothing to reverse complement against: this file carries annotations but neither bases nor a declared length, so {what} at {at} has no reflection"
                    )
                } else {
                    write!(
                        f,
                        "{what} at {at} lies outside the {len} bp being reverse complemented, so it has no reflection"
                    )
                }
            }
            OpError::IdCollision { id } => write!(
                f,
                "operation id {id} is already used by a different operation; refusing rather than losing the edit"
            ),
            OpError::AtEnd => write!(f, "nothing further in that direction"),
        }
    }
}

impl std::error::Error for OpError {}

/// Apply one operation to a molecule, in place.
///
/// Feature coordinates move with the bases: an insertion of *k* at *p* shifts
/// every coordinate at or after *p* by exactly *k*. Getting this wrong is the
/// classic silent corruption — the sequence is right, the annotations point at
/// the wrong bases, and nothing looks broken.
pub fn apply(mol: &mut Molecule, kind: &OpKind) -> Result<(), OpError> {
    let n = mol.len();
    // Whether the source's declared length is still a claim about *these*
    // bases. See the `declared_len` handling at the foot of this function.
    let had_bases = !mol.seq.is_empty();
    match kind {
        OpKind::InsertAt { at, seq } => {
            if *at < 1 || *at > n + 1 {
                return Err(OpError::OutOfRange {
                    what: "insertion",
                    at: *at,
                    len: n,
                });
            }
            let idx = (*at - 1) as usize;
            let k = seq.len() as u64;
            mol.seq.splice(idx..idx, seq.bytes());
            remap_annotations(mol, *at, 0, k);
        }
        OpKind::DeleteRange { start, len } => {
            // `start + len - 1 > n` overflowed on exactly the inputs it exists
            // to reject. `DeleteRange { start: u64::MAX, len: 1 }` panicked
            // "attempt to add with overflow" here in every checked build, which
            // is every `cargo test` and every dev GUI; with checks off the sum
            // wrapped and the guard answered *wrongly* for a whole family —
            // `{ start: 2, len: u64::MAX }` computed 0, passed, and panicked in
            // `Vec::drain(1..0)` inside libcore, naming no polylinker code at
            // all. `len > n` first makes `n - len` safe, and `start >= 1` is
            // already established by the short circuit.
            if *start < 1 || *len == 0 || *len > n || *start > n - *len + 1 {
                return Err(OpError::OutOfRange {
                    what: "deletion",
                    at: *start,
                    len: n,
                });
            }
            let a = (*start - 1) as usize;
            let b = a + *len as usize;
            mol.seq.drain(a..b);
            remap_annotations(mol, *start, *len, 0);
        }
        OpKind::ReplaceRange { start, len, seq } => {
            // Same overflow as `DeleteRange` above, spelled without the sum.
            // `len == 0` stays deliberately *legal* here where `DeleteRange`
            // refuses it — a zero-length replacement is an insertion, and
            // `seqedit.rs` relies on the asymmetry — so this cannot simply
            // borrow the guard above. `start - 1 > n - len` is `start + len - 1
            // > n` with both subtractions already proven non-negative.
            if *start < 1 || *len > n || *start - 1 > n - *len {
                return Err(OpError::OutOfRange {
                    what: "replacement",
                    at: *start,
                    len: n,
                });
            }
            let a = (*start - 1) as usize;
            let b = a + *len as usize;
            mol.seq.splice(a..b, seq.bytes());
            remap_annotations(mol, *start, *len, seq.len() as u64);
        }
        OpKind::SetTopology(t) => mol.topology = *t,
        OpKind::Rotate { origin } => {
            if !mol.topology.is_circular() {
                return Err(OpError::NotCircular);
            }
            if !mol.rotate(*origin) {
                return Err(OpError::OutOfRange {
                    what: "origin",
                    at: *origin,
                    len: n,
                });
            }
        }
        OpKind::ReverseComplement => {
            // `span()`, not `len()`: annotation-only GenBank is a supported,
            // common class (no ORIGIN block, length declared on the LOCUS
            // line), and measuring against `len()` made every coordinate in
            // such a file underflow. `validate()` already measures against
            // `annotation_span()`, so the two disagreed and a molecule
            // `is_valid()` accepted could still panic here.
            let n = mol.span();
            // Every coordinate is checked BEFORE a single base moves, because
            // saturating arithmetic cannot keep an out-of-range coordinate
            // out of range and pretending otherwise destroyed data.
            //
            // `flip(p) = n + 1 - p` has no answer for `p` outside `1..=n`, and
            // `n.saturating_sub(p).saturating_add(1)` gave 1 for every such p.
            // Two measured consequences. A segment `4..9000` on 8 bases became
            // `1..5`: `validate()` went from `[PastEnd]` to `[]` and
            // `is_valid()` from false to true, so the comment that used to sit
            // here — claiming saturation "keeps the existing problem visible" —
            // was not merely inaccurate but unachievable, since `flip` maps
            // every input into `[1, n]` by construction. And on a standalone
            // annotation track (`span() == 0`; the UGENE file pinned by
            // pl-fileio's `standalone_annotation_tracks_are_read`, features at
            // 242..1015 and 1118..1951) EVERY segment and EVERY primer site
            // collapsed to `1..1`, `annotation_span()` fell from 1951 to 1,
            // `validate()` returned clean, and `apply` returned `Ok`. Neither
            // was catchable downstream: the log's gate compares per-kind problem
            // counts and only refuses increases.
            for (i, f) in mol.features.iter().enumerate() {
                for (j, s) in f.segments.iter().enumerate() {
                    for p in [s.start, s.end] {
                        if p < 1 || p > n {
                            return Err(OpError::CannotReflect {
                                what: format!("feature {i} '{}' segment {j}", f.name),
                                at: p,
                                len: n,
                            });
                        }
                    }
                }
            }
            for (i, pr) in mol.primers.iter().enumerate() {
                for (j, s) in pr.sites.iter().enumerate() {
                    for p in [s.start, s.end] {
                        if p < 1 || p > n {
                            return Err(OpError::CannotReflect {
                                what: format!("primer {i} '{}' site {j}", pr.name),
                                at: p,
                                len: n,
                            });
                        }
                    }
                }
            }
            mol.seq = crate::reverse_complement(&mol.seq);
            let flip = |p: u64| -> u64 { n + 1 - p };
            // Everything flips end for end, and each feature changes strand.
            for f in &mut mol.features {
                for s in &mut f.segments {
                    let (a, b) = (s.start, s.end);
                    s.start = flip(b);
                    s.end = flip(a);
                }
                f.strand = match f.strand {
                    crate::Strand::Forward => crate::Strand::Reverse,
                    crate::Strand::Reverse => crate::Strand::Forward,
                    other => other,
                };
            }
            for p in &mut mol.primers {
                for s in &mut p.sites {
                    let (a, b) = (s.start, s.end);
                    s.start = flip(b);
                    s.end = flip(a);
                    s.strand = match s.strand {
                        crate::Strand::Forward => crate::Strand::Reverse,
                        crate::Strand::Reverse => crate::Strand::Forward,
                        other => other,
                    };
                }
            }
        }
        OpKind::SetFeature { index, feature } => match index {
            None => mol.features.push((**feature).clone()),
            Some(i) => {
                if *i >= mol.features.len() {
                    return Err(OpError::NoSuchFeature { index: *i });
                }
                mol.features[*i] = (**feature).clone();
            }
        },
        OpKind::RemoveFeature { index } => {
            if *index >= mol.features.len() {
                return Err(OpError::NoSuchFeature { index: *index });
            }
            mol.features.remove(*index);
        }
        OpKind::SetMethylation(m) => mol.methylation = *m,
    }

    // A length-changing edit retires the source file's declared length.
    //
    // `genbank::parse` sets `declared_len` from the LOCUS line *unconditionally*,
    // including when the ORIGIN block supplied the bases, so an ordinary .gb
    // opens with `seq.len() == 12` and `declared_len == Some(12)`.
    // `Molecule::validate` then reports `LengthMismatch` the moment those
    // disagree, and `OpLog::apply`'s gate refuses any operation that raises a
    // problem kind's count from zero. Measured on a 12 bp GenBank record before
    // this line existed: every insertion and every deletion was refused with
    // "that would leave the molecule inconsistent: the file declares 12 bases
    // but carries 13", while an equal-length replacement was accepted. In other
    // words the editor could do point mutations and nothing else, on the
    // project's own default save format, and the failure looked exactly like
    // the corruption gate working correctly. FASTA and `.dna` were unaffected
    // because neither reader sets the field, which is why nothing caught it.
    //
    // Cleared rather than refreshed to `Some(new_len)`: the field means "the
    // length the source declared for a file that shipped no bases", and a copy
    // of `seq.len()` would be a field that can only ever agree with itself.
    // `genbank::write` already takes its LOCUS length from `span()`, which is
    // `seq.len()` whenever bases are present, so nothing downstream loses an
    // answer.
    //
    // Guarded on bases having been present BEFORE the edit, which is what keeps
    // annotation-only GenBank (no bases, a meaningful declaration, features
    // measured against it) untouched. Clearing there would collapse `span()`
    // from 2.9 Mb to 1 and turn every feature into `PastEnd`. A keystroke on
    // such a file is still refused, and should be: the bases those coordinates
    // describe are not in the file.
    if had_bases && mol.len() != n {
        debug_assert!(matches!(
            kind,
            OpKind::InsertAt { .. } | OpKind::DeleteRange { .. } | OpKind::ReplaceRange { .. }
        ));
        mol.declared_len = None;
    }
    Ok(())
}

/// Remap annotations across an edit that replaced `old_len` bases at `start`
/// with `new_len` bases.
///
/// Insertion is `old_len == 0`, deletion is `new_len == 0`.
///
/// Three cases, and the third is the one that matters:
///
/// - entirely before the edit: unchanged
/// - entirely after: shifted by the size difference
/// - **overlapping**: truncated to what survives — and if *nothing* survives,
///   the segment is dropped, and a feature left with no segments is removed.
///
/// That last rule is deliberate. Clamping a wiped-out feature to the edit site
/// instead would leave a one-base `AmpR` sitting in the file: valid, plausible
/// at a glance, and false. Deleting the bases a feature describes deletes the
/// feature.
///
/// It applies only to a feature this function *emptied*. A feature that arrived
/// from the file already carrying no segments is left alone — see the retain at
/// the foot of this function.
fn remap_annotations(mol: &mut Molecule, start: u64, old_len: u64, new_len: u64) {
    let old_end = start + old_len; // one past the edited region
    let delta = new_len as i64 - old_len as i64;

    // The bases were already spliced before this ran, so `mol.len()` is the
    // length *after* the edit. Taking an origin-crossing segment apart into its
    // two linear pieces needs the length it was crossing.
    let old_n = if delta >= 0 {
        mol.len().saturating_sub(delta as u64)
    } else {
        mol.len().saturating_add(delta.unsigned_abs())
    };

    // Returns None when the coordinate fell inside a region that no longer
    // exists and there is nothing to anchor it to.
    // Whether an *interior* coordinate — one inside the replaced region —
    // still means anything afterwards.
    //
    // It does when the replacement is the same length: a 1 bp point mutation
    // under a 1 bp `snp` feature, or an equal-length codon-optimisation swap
    // across exactly a gene's span, both leave every base in place and the
    // annotation still describes what it describes.
    //
    // It does not when the length changed. Collapsing interior coordinates onto
    // the edges regardless meant no segment was ever dropped by a replacement,
    // so pasting a 20 bp linker over AmpR left "AmpR" behind as a 20 bp feature
    // sitting on the linker — the same "valid, plausible at a glance, and
    // false" annotation this function's own docstring says it exists to
    // prevent. It also moved features that nothing had touched: replacing the
    // whole sequence with byte-identical bases dragged a feature at 5..12 out
    // to 1..16.
    let interior_survives = new_len > 0 && new_len == old_len;

    // `(p as i64 + delta) as u64` was two failures in one expression, and `p`
    // comes straight off disk: `snapgene.rs` parses `<Segment range="a-b"/>`
    // with an unbounded `parse::<u64>()` and `pl-gui`'s document never calls
    // `Molecule::validate()` on the way in.
    //
    // With `range="1-9223372036854775807"` and a 12 bp insertion, `i64::MAX + 12`
    // panicked "attempt to add with overflow" in any checked build. With
    // `range="1-18446744073709551615"` and checks off — the workspace release
    // profile sets none — `u64::MAX as i64` is -1, so the END came back as 11
    // while the start moved to 13, `b >= a` failed, and the whole feature was
    // deleted with `apply` returning `Ok`: measured, features after = 0, no
    // error and no report.
    //
    // Saturating in u64 keeps an absurd coordinate absurd, which is exactly
    // what `validate()` needs in order to go on reporting it as `PastEnd`. The
    // only values that saturate are ones already within `delta` of `u64::MAX`,
    // i.e. ones that named no base to begin with. `Segment::len` already
    // defends the identical threat the identical way.
    let shift = |p: u64| -> u64 {
        if delta >= 0 {
            p.saturating_add(delta as u64)
        } else {
            p.saturating_sub(delta.unsigned_abs())
        }
    };

    let map_start = |p: u64| -> Option<u64> {
        if p < start {
            Some(p)
        } else if p >= old_end {
            Some(shift(p))
        } else if interior_survives {
            Some(p)
        } else {
            None
        }
    };
    let map_end = |p: u64| -> Option<u64> {
        if p < start {
            Some(p)
        } else if p >= old_end {
            Some(shift(p))
        } else if interior_survives {
            Some(p)
        } else {
            None
        }
    };

    // A segment crossing the origin is written `end < start`, and on a circular
    // molecule that is legal — `Molecule::validate` says so explicitly.
    let circular = mol.topology.is_circular();

    // Remap one span that reads *forwards*, returning `None` when nothing of it
    // survives. `allow_inverted` exists only for the leftover shapes the wrap
    // decomposition below refuses to take apart.
    let remap_span = |s_start: u64, s_end: u64, allow_inverted: bool| -> Option<(u64, u64)> {
        // A segment straddling the edit keeps whichever ends survive.
        //
        // A segment whose far end survives keeps its near end pinned to where
        // the replaced text *ends*, not where it begins. `start + new_len`
        // collapses to `start` for a pure deletion, so deletions behave exactly
        // as before; for a replacement it is the difference between the feature
        // claiming all of the new text and claiming only what survived of
        // itself. Replacing 5 bases at 10 with 20 used to move a feature that
        // began at 12 back to 10, swallowing twenty bases it has no
        // relationship to.
        //
        // `s_end <= old_n` is what stops that rescue running on a coordinate
        // that named no base. The fallback reads "the far end lies past the
        // edit, so the span survives it" — a conclusion only a real base can
        // support. `gene 5..20` on a 12 bp molecule is `PastEnd`, and deleting
        // every base used to rescue the dead start against that fictitious end
        // and hand back `gene 1..8` on a molecule with NO bases. Worse than the
        // fabrication, it laundered the report: `validate()` went 1 problem ->
        // 0, `refuse_new_problems` read that as an improvement and committed,
        // and `genbank::write` then emitted `gene 1..8` under a `0 bp` LOCUS.
        // The guard is on this fallback alone, deliberately: an out-of-range
        // end whose *start* survives is still shifted, absurd value and all, so
        // `validate()` goes on reporting it instead of being quietly truncated.
        let a = map_start(s_start)
            .or_else(|| (s_end >= old_end && s_end <= old_n).then_some(start + new_len));
        let b = map_end(s_end).or_else(|| (s_start < start).then_some(start - 1));
        match (a, b) {
            // A segment the remap did not move is handed back exactly as it
            // arrived, whatever coordinate it carries. This is the docstring's
            // first case — "entirely before the edit: unchanged" — spelled out,
            // and it has to come first because the arm below judges the result
            // rather than the movement: `0..0`, and an inverted span on a
            // *linear* molecule, both fail it. Neither is a coordinate this
            // function produced, and an edit that did not touch a segment has no
            // business deleting the `ZeroStart` or `Inverted` that `validate()`
            // is reporting for it.
            (Some(a), Some(b)) if a == s_start && b == s_end => Some((a, b)),
            // `a >= 1` used to stand where `a >= 1 || b >= 1` stands now, and it
            // deleted the wrong thing. Trace the branches that produce `a` and
            // none can compute 0 on its own: `InsertAt`, `DeleteRange` and
            // `ReplaceRange` all validate `start >= 1` first, so `a == 0` only
            // ever meant a start of 0 the SnapGene reader carried through from
            // `<Segment range="0-4"/>` — a state `Molecule::rotate` goes out of
            // its way to preserve. Testing `a` alone therefore threw away a
            // feature whose bases had merely been *trimmed*: `0-4` under
            // `DeleteRange { start: 3, len: 2 }` keeps bases 1 and 2 and should
            // become `0..2`, and instead the whole feature vanished with `apply`
            // returning `Ok`, because the vanished `ZeroStart` made the gate see
            // an improvement rather than a loss. `b` is the honest survivor
            // test: `b == 0` here can only come from the `start - 1` fallback
            // with `start == 1`, i.e. nothing of the span is left.
            (Some(a), Some(b)) if (a >= 1 || b >= 1) && (b >= a || allow_inverted) => Some((a, b)),
            _ => None,
        }
    };

    let remap = |s_start: &mut u64, s_end: &mut u64| -> bool {
        // Did this segment cross the origin *before* the edit? Then its
        // remapped ends are still expected to read backwards.
        let wrapped = circular && *s_end < *s_start;

        // An origin-crossing segment is two linear runs, `[s_start, old_n]` and
        // `[1, s_end]`, and the fallbacks above encode "the remnant lies on the
        // far side of the cut" — true only in numeric order. For a wrap the two
        // ends sit on opposite sides of the origin, which makes both fallback
        // guards unsatisfiable: `a`'s needs `s_end >= old_end` but is only
        // reached when `s_start < old_end`, and `b`'s needs `s_start < start`
        // but is only reached when `s_end >= start`. So ANY length-changing edit
        // whose replaced region contained either endpoint deleted the whole
        // segment however many of its bases survived: a 401 bp `AmpR` written
        // `3900..300` lost all 401 to a 21 bp deletion at 3890, while the same
        // biological span written as the two-segment join `3900..4000, 1..300`
        // truncated correctly. Remapping the two runs separately and rejoining
        // makes the two encodings agree, and drops the segment only when both
        // runs are gone.
        if wrapped && *s_end >= 1 && *s_start <= old_n {
            let head = remap_span(*s_start, old_n, false);
            let tail = remap_span(1, *s_end, false);
            return match (head, tail) {
                // Still crosses the origin: take the far end of each run.
                (Some((hs, _)), Some((_, te))) => {
                    *s_start = hs;
                    *s_end = te;
                    true
                }
                // Only one run left, so it is an ordinary forward span now.
                (Some(a), None) | (None, Some(a)) => {
                    *s_start = a.0;
                    *s_end = a.1;
                    true
                }
                (None, None) => false,
            };
        }

        // What is left is a forward span, or a "wrap" whose own coordinates say
        // it names nothing — `s_end == 0`, or a start past the end of the
        // molecule. Those keep the pre-existing treatment, which preserves the
        // absurd coordinate so `validate()` goes on reporting it.
        match remap_span(*s_start, *s_end, wrapped) {
            Some((a, b)) => {
                *s_start = a;
                *s_end = b;
                true
            }
            None => false,
        }
    };

    // Which features had no segments *on arrival*. See the retain below.
    let arrived_empty: Vec<bool> = mol.features.iter().map(|f| f.segments.is_empty()).collect();

    for f in &mut mol.features {
        f.segments.retain_mut(|s| remap(&mut s.start, &mut s.end));
    }
    // A feature this edit emptied is not a feature. One that arrived empty is
    // the importer's problem, not the user's, and deleting it here erased the
    // `FeatureWithoutSegments` that `validate()` was reporting — which
    // `refuse_new_problems` then read as an improvement, so an insertion
    // nowhere near it removed a named `CDS` and its qualifiers with `apply`
    // returning `Ok`. `snapgene::parse_features` builds every feature with
    // `segments: Vec::new()` and pushes it whether or not a `<Segment>` follows,
    // so `<Feature name="AmpR" type="CDS"/>` reaches this point routinely.
    let mut i = 0;
    mol.features.retain(|f| {
        let keep = arrived_empty[i] || !f.segments.is_empty();
        i += 1;
        keep
    });

    for p in &mut mol.primers {
        p.sites.retain_mut(|s| remap(&mut s.start, &mut s.end));
    }
}

/// How many coordinate problems of each kind this molecule has.
///
/// Half of the corruption gate, and public because it is the only half that
/// has to be measured *before* an operation runs. A caller that wants to ask
/// "would this be refused?" without committing it — the GUI pre-flights a
/// gesture before opening a typing run — otherwise has to build a whole
/// throwaway [`OpLog`] to borrow the gate, which clones the molecule three
/// more times than the question needs. At 4.6 Mb that was a measured 7.1 ms
/// per call on the ordinary typing path.
pub fn problem_tally(mol: &Molecule) -> BTreeMap<&'static str, usize> {
    let mut m: BTreeMap<&'static str, usize> = BTreeMap::new();
    for x in mol.validate() {
        *m.entry(x.kind()).or_default() += 1;
    }
    m
}

/// Refuse `after` if it has a problem `was` did not, counting by kind.
///
/// Only *new* problems count: a file that arrived with a bad coordinate should
/// still be editable, and blaming the user's edit for the importer's mess would
/// be both wrong and infuriating.
///
/// Two things were wrong with the `if after.len() > before.len()` this replaced.
/// It let an edit that *swapped* one problem for another straight through,
/// because one is not greater than one — which is how a reverse-complement could
/// trade a `PastEnd` for a wrapped `Inverted` and commit it. And the
/// `!before.contains(x)` inside compares `Invalid` values, which cannot work:
/// every variant embeds data that moves under ordinary editing (`PastEnd`
/// carries the molecule length; `what` carries the feature index and name), so
/// deleting that guard refuses a length-changing insert, a feature removal, and
/// even a rename, on any file that arrived with a bad coordinate.
///
/// Counts per kind are stable under all of those and still catch a genuinely
/// new problem. See [`crate::Invalid::kind`].
pub fn refuse_new_problems(
    was: &BTreeMap<&'static str, usize>,
    after: &Molecule,
) -> Result<(), OpError> {
    let found = after.validate();
    let mut now: BTreeMap<&'static str, usize> = BTreeMap::new();
    for x in &found {
        *now.entry(x.kind()).or_default() += 1;
    }
    let worsened: Vec<&'static str> = now
        .iter()
        .filter(|(k, n)| **n > was.get(*k).copied().unwrap_or(0))
        .map(|(k, _)| *k)
        .collect();
    if worsened.is_empty() {
        return Ok(());
    }
    let fresh: Vec<_> = found
        .into_iter()
        .filter(|x| worsened.contains(&x.kind()))
        .collect();
    Err(OpError::WouldCorrupt(fresh))
}

/// The log: a molecule, and every operation ever performed on it.
pub struct OpLog {
    base: Molecule,
    ops: Vec<Op>,
    by_id: HashMap<OpId, usize>,
    /// Children in creation order, so redo can prefer the most recent branch.
    children: HashMap<Option<OpId>, Vec<OpId>>,
    cursor: Option<OpId>,
    current: Molecule,
    snapshots: HashMap<Option<OpId>, Molecule>,
}

impl OpLog {
    pub fn new(base: Molecule) -> Self {
        let mut snapshots = HashMap::new();
        snapshots.insert(None, base.clone());
        OpLog {
            current: base.clone(),
            base,
            ops: Vec::new(),
            by_id: HashMap::new(),
            children: HashMap::new(),
            cursor: None,
            snapshots,
        }
    }

    /// The document as it stands at the cursor.
    pub fn current(&self) -> &Molecule {
        &self.current
    }

    pub fn cursor(&self) -> Option<OpId> {
        self.cursor
    }

    /// Every operation ever recorded, including branches no longer on the path.
    pub fn all_ops(&self) -> &[Op] {
        &self.ops
    }

    /// The operations from the base to the cursor, oldest first.
    pub fn path(&self) -> Vec<&Op> {
        let mut out = Vec::new();
        let mut at = self.cursor;
        while let Some(id) = at {
            let op = &self.ops[self.by_id[&id]];
            out.push(op);
            at = op.parent;
        }
        out.reverse();
        out
    }

    /// How many operations separate `ancestor` from the cursor, or `None` when
    /// `ancestor` is not on the path back from it.
    ///
    /// A walk, not a subtraction of two [`OpLog::path`] lengths: the log is a
    /// DAG and two points at the same depth can be on different branches
    /// holding different molecules. `None` is a real answer and callers must
    /// render it as "changes" rather than a count — save, then seek onto
    /// another branch from the History tab, and the number genuinely does not
    /// exist.
    pub fn distance_from(&self, ancestor: Option<OpId>) -> Option<usize> {
        let mut n = 0usize;
        let mut at = self.cursor;
        loop {
            if at == ancestor {
                return Some(n);
            }
            let id = at?;
            let &i = self.by_id.get(&id)?;
            at = self.ops[i].parent;
            n += 1;
        }
    }

    /// Perform an operation, recording it.
    ///
    /// If the cursor is not at the tip — that is, something was undone — this
    /// creates a *branch*. The undone operations remain in the log and can
    /// still be reached. Nothing is discarded, ever.
    pub fn apply(&mut self, kind: OpKind, actor: &str) -> Result<&Molecule, OpError> {
        // Try it on a copy first, so a rejected operation leaves no trace.
        let was = problem_tally(&self.current);
        let mut next = self.current.clone();
        apply(&mut next, &kind)?;
        refuse_new_problems(&was, &next)?;

        let id = derive_id(self.cursor, &kind);
        match self.by_id.get(&id) {
            None => {
                self.by_id.insert(id, self.ops.len());
                self.ops.push(Op {
                    id,
                    parent: self.cursor,
                    kind,
                    actor: actor.to_string(),
                });
                self.children.entry(self.cursor).or_default().push(id);
            }
            // Re-doing the identical operation from the identical parent is
            // legitimate and idempotent — that is what content addressing is
            // for. But an id collision between two *different* operations is
            // not, and used to pass silently: the op was not recorded while
            // `current` still advanced, so the log and the document disagreed
            // and the next undo/redo quietly reinstated the older version.
            //
            // Belt and braces alongside the injective encoding above. If this
            // ever fires, the encoding has a hole, and losing an edit is far
            // worse than refusing one.
            Some(&existing) if self.ops[existing].kind != kind => {
                return Err(OpError::IdCollision { id });
            }
            Some(_) => {}
        }

        self.cursor = Some(id);
        self.current = next;
        if self.depth().is_multiple_of(SNAPSHOT_EVERY) {
            self.snapshots.insert(self.cursor, self.current.clone());
        }
        Ok(&self.current)
    }

    /// Step the cursor back one operation.
    pub fn undo(&mut self) -> Result<&Molecule, OpError> {
        let Some(id) = self.cursor else {
            return Err(OpError::AtEnd);
        };
        let parent = self.ops[self.by_id[&id]].parent;
        self.cursor = parent;
        self.current = self.materialise(parent);
        Ok(&self.current)
    }

    /// Is there anything to undo?
    ///
    /// Cheap enough to call every frame, which is what a toolbar needs in
    /// order to grey a button out rather than let the user press it and be
    /// told no.
    pub fn can_undo(&self) -> bool {
        self.cursor.is_some()
    }

    /// Is there anything to redo?
    ///
    /// True when this point in the log has a child — including after a fork,
    /// because a branch abandoned by a later edit is still reachable. Nothing
    /// is ever discarded, so "redo" here can mean "go back down the branch you
    /// left", which is exactly the afternoon's work every other editor throws
    /// away without warning.
    pub fn can_redo(&self) -> bool {
        self.children
            .get(&self.cursor)
            .is_some_and(|c| !c.is_empty())
    }

    /// Step the cursor forward, along the most recently created branch.
    pub fn redo(&mut self) -> Result<&Molecule, OpError> {
        let next = self
            .children
            .get(&self.cursor)
            .and_then(|c| c.last())
            .copied();
        let Some(id) = next else {
            return Err(OpError::AtEnd);
        };
        self.cursor = Some(id);
        self.current = self.materialise(Some(id));
        Ok(&self.current)
    }

    /// Move the cursor to any operation in the log, on any branch.
    pub fn seek(&mut self, to: Option<OpId>) -> Result<&Molecule, OpError> {
        if let Some(id) = to {
            if !self.by_id.contains_key(&id) {
                return Err(OpError::AtEnd);
            }
        }
        self.cursor = to;
        self.current = self.materialise(to);
        Ok(&self.current)
    }

    /// How many operations lie between the base and the cursor.
    pub fn depth(&self) -> usize {
        let mut d = 0;
        let mut at = self.cursor;
        while let Some(id) = at {
            d += 1;
            at = self.ops[self.by_id[&id]].parent;
        }
        d
    }

    /// The branch points: operations with more than one child.
    pub fn forks(&self) -> Vec<OpId> {
        let mut out: Vec<OpId> = self
            .children
            .iter()
            .filter(|(k, v)| v.len() > 1 && k.is_some())
            .filter_map(|(k, _)| *k)
            .collect();
        // `children` is a HashMap, so this came back in a different order on
        // every call — four builds of the identical log in one process gave
        // four distinct orders. Sorted by position in `ops`, i.e. **creation
        // order**, which is what a history panel wants; sorting by the id would
        // be stable but would order by SHA-1 bytes, which means nothing to a
        // reader.
        //
        // The `children` *values* are deliberately left alone: `redo()` reads
        // `.last()` for its documented "most recent branch" semantics.
        out.sort_by_key(|id| self.by_id.get(id).copied().unwrap_or(usize::MAX));
        out
    }

    /// Rebuild the document at `target` by replaying from the nearest snapshot.
    fn materialise(&self, target: Option<OpId>) -> Molecule {
        // Walk back to a snapshot, collecting what has to be replayed.
        let mut chain = Vec::new();
        let mut at = target;
        loop {
            if let Some(snap) = self.snapshots.get(&at) {
                let mut mol = snap.clone();
                for id in chain.iter().rev() {
                    let op = &self.ops[self.by_id[id]];
                    // Replaying an op that applied cleanly once cannot fail.
                    let _ = apply(&mut mol, &op.kind);
                }
                return mol;
            }
            match at {
                None => break,
                Some(id) => {
                    chain.push(id);
                    at = self.ops[self.by_id[&id]].parent;
                }
            }
        }
        // No snapshot found at all: replay everything from the base.
        let mut mol = self.base.clone();
        for id in chain.iter().rev() {
            let op = &self.ops[self.by_id[id]];
            let _ = apply(&mut mol, &op.kind);
        }
        mol
    }
}

/// An operation's id: the hash of its parent and its content.
///
/// Deriving the id rather than generating one is what makes the log
/// content-addressed. Two people who start from the same molecule and do the
/// same things get the same ids, so their histories can be compared directly.
fn derive_id(parent: Option<OpId>, kind: &OpKind) -> OpId {
    let mut buf = Vec::with_capacity(64);
    match parent {
        Some(p) => buf.extend_from_slice(&p.0),
        None => buf.extend_from_slice(&[0u8; 20]),
    }
    buf.extend_from_slice(&kind.content());
    OpId(sha1(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Feature, Segment};

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

    fn with_feature(seq: &str, start: u64, end: u64) -> Molecule {
        let mut m = mol(seq, false);
        let mut f = Feature::new("f", "misc_feature");
        f.segments.push(Segment::new(start, end));
        m.features.push(f);
        m
    }

    /// A molecule as `genbank::parse` hands one over: bases present, and the
    /// LOCUS line's length recorded alongside them.
    fn as_genbank_reads_it(seq: &str) -> Molecule {
        let mut m = mol(seq, false);
        m.declared_len = Some(seq.len() as u64);
        m
    }

    /// PROVEN TO FAIL at 528dcd9, where `distance_from` did not exist and the
    /// only thing available was `path().len()`.
    ///
    /// The fork is the point. Two positions at the same depth can be on
    /// different branches holding different molecules, so subtracting two path
    /// lengths would report 0 here and the caller would render "0 edits that are
    /// not in any file" about a document that differs from the file. An
    /// assertion that only ever sees a straight line proves nothing.
    #[test]
    fn the_distance_to_a_point_on_another_branch_is_unknowable() {
        let mut log = OpLog::new(mol("AAAACCCCGGGGTTTT", true));
        log.apply(OpKind::SetTopology(Topology::Linear), "t")
            .unwrap();
        let base = log.cursor();
        assert_eq!(log.distance_from(None), Some(1), "a straight line counts");

        log.apply(OpKind::ReverseComplement, "t").unwrap();
        let branch_a = log.cursor();
        assert_eq!(log.distance_from(base), Some(1));
        assert_eq!(log.distance_from(None), Some(2));

        // Step back and fork. Both tips are two ops deep and neither is an
        // ancestor of the other.
        log.undo().unwrap();
        log.apply(OpKind::SetTopology(Topology::Circular), "t")
            .unwrap();
        assert_eq!(log.path().len(), 2, "the premise: the same depth");
        assert_eq!(
            log.distance_from(branch_a),
            None,
            "and the other branch is not on the way back from here"
        );
        assert_eq!(log.distance_from(base), Some(1), "but its parent is");
    }

    #[test]
    fn a_genbank_document_can_have_a_base_inserted_into_it() {
        // This was refused at every size, on every GenBank file, before
        // `apply` learned to retire a stale declaration:
        //
        //   WouldCorrupt([LengthMismatch { declared: 12, actual: 13 }])
        //   "the file declares 12 bases but carries 13"
        //
        // The editor could overwrite a base and never insert or delete one, on
        // the project's own default save format, and the refusal read like the
        // corruption gate doing its job.
        let mut log = OpLog::new(as_genbank_reads_it("ACGTACGTACGT"));
        log.apply(
            OpKind::InsertAt {
                at: 5,
                seq: "TTT".into(),
            },
            "test",
        )
        .expect("a GenBank document must accept an insertion");
        assert_eq!(log.current().seq, b"ACGTTTTACGTACGT");
        assert_eq!(
            log.current().declared_len,
            None,
            "the LOCUS length described the bases the file shipped, not these"
        );
        assert!(log.current().is_valid());
    }

    #[test]
    fn a_genbank_document_can_have_a_base_deleted_from_it() {
        let mut log = OpLog::new(as_genbank_reads_it("ACGTACGTACGT"));
        log.apply(OpKind::DeleteRange { start: 1, len: 4 }, "test")
            .expect("a GenBank document must accept a deletion");
        assert_eq!(log.current().seq, b"ACGTACGT");
        assert_eq!(log.current().declared_len, None);
    }

    #[test]
    fn an_equal_length_replacement_leaves_the_declaration_alone() {
        // Nothing about the number of bases changed, so the file's claim is
        // still a claim about this molecule. Clearing it here would be a
        // gratuitous loss of what the source said.
        let mut log = OpLog::new(as_genbank_reads_it("ACGTACGTACGT"));
        log.apply(
            OpKind::ReplaceRange {
                start: 1,
                len: 4,
                seq: "TTTT".into(),
            },
            "test",
        )
        .unwrap();
        assert_eq!(log.current().declared_len, Some(12));
    }

    #[test]
    fn a_file_that_declares_bases_it_does_not_carry_keeps_its_declaration() {
        // Annotation-only GenBank: `ORIGIN` immediately followed by `//`, the
        // length on the LOCUS line, features measured against it. Clearing
        // `declared_len` on the first inserted base would collapse `span()`
        // from 2,900,000 to 1 and turn every feature into `PastEnd` — so the
        // guard is "did this molecule carry bases *before* the edit", not
        // "does it carry them now".
        let mut m = mol("", false);
        m.declared_len = Some(2_900_000);
        let mut f = Feature::new("orphan", "misc_feature");
        f.segments.push(Segment::new(100, 400));
        m.features.push(f);

        let mut trial = m.clone();
        // Refused, as it must be: the bases those coordinates describe are not
        // in this file. What matters here is that the declaration survives.
        let _ = apply(
            &mut trial,
            &OpKind::InsertAt {
                at: 1,
                seq: "A".into(),
            },
        );
        assert_eq!(trial.declared_len, Some(2_900_000));
    }

    #[test]
    fn an_edit_moves_the_annotations_with_the_bases() {
        // The classic silent corruption: right sequence, wrong coordinates.
        let mut log = OpLog::new(with_feature("AAAACCCCGGGGTTTT", 9, 12)); // the GGGG
        assert_eq!(&log.current().seq[8..12], b"GGGG");

        log.apply(
            OpKind::InsertAt {
                at: 1,
                seq: "TTT".into(),
            },
            "test",
        )
        .unwrap();

        let f = &log.current().features[0];
        assert_eq!(
            (f.start(), f.end()),
            (12, 15),
            "feature must follow its bases"
        );
        let s = &log.current().seq;
        assert_eq!(&s[(f.start() - 1) as usize..f.end() as usize], b"GGGG");
    }

    #[test]
    fn a_deletion_before_a_feature_pulls_it_back() {
        let mut log = OpLog::new(with_feature("AAAACCCCGGGGTTTT", 9, 12));
        log.apply(OpKind::DeleteRange { start: 1, len: 4 }, "test")
            .unwrap();
        let f = &log.current().features[0];
        assert_eq!((f.start(), f.end()), (5, 8));
        let s = &log.current().seq;
        assert_eq!(&s[(f.start() - 1) as usize..f.end() as usize], b"GGGG");
    }

    #[test]
    fn a_feature_entirely_before_an_edit_does_not_move() {
        let mut log = OpLog::new(with_feature("AAAACCCCGGGGTTTT", 1, 4));
        log.apply(
            OpKind::InsertAt {
                at: 13,
                seq: "GG".into(),
            },
            "test",
        )
        .unwrap();
        let f = &log.current().features[0];
        assert_eq!((f.start(), f.end()), (1, 4));
    }

    #[test]
    fn undo_and_redo_return_the_same_document() {
        let mut log = OpLog::new(mol("AAAACCCC", false));
        let before = log.current().clone();
        log.apply(
            OpKind::InsertAt {
                at: 5,
                seq: "GGGG".into(),
            },
            "t",
        )
        .unwrap();
        let after = log.current().clone();
        assert_eq!(after.seq, b"AAAAGGGGCCCC".to_vec());

        log.undo().unwrap();
        assert_eq!(log.current().seq, before.seq);
        log.redo().unwrap();
        assert_eq!(log.current().seq, after.seq);
    }

    #[test]
    fn editing_after_an_undo_forks_instead_of_truncating() {
        // The property the plan singles out: every other editor throws the
        // redo branch away here, and that is where people lose work.
        let mut log = OpLog::new(mol("AAAA", false));
        log.apply(
            OpKind::InsertAt {
                at: 5,
                seq: "CCCC".into(),
            },
            "t",
        )
        .unwrap();
        let branch_a = log.cursor().unwrap();
        assert_eq!(log.current().seq, b"AAAACCCC".to_vec());

        log.undo().unwrap();
        log.apply(
            OpKind::InsertAt {
                at: 5,
                seq: "GGGG".into(),
            },
            "t",
        )
        .unwrap();
        let branch_b = log.cursor().unwrap();
        assert_eq!(log.current().seq, b"AAAAGGGG".to_vec());
        assert_ne!(branch_a, branch_b);

        // The abandoned branch is still there, and still reachable.
        assert_eq!(log.all_ops().len(), 2);
        log.seek(Some(branch_a)).unwrap();
        assert_eq!(
            log.current().seq,
            b"AAAACCCC".to_vec(),
            "the first branch survived"
        );
        assert_eq!(
            log.forks().len(),
            0,
            "the fork is at the base, not at an op"
        );
    }

    #[test]
    fn identical_work_from_an_identical_start_gets_identical_ids() {
        // What makes the log a provenance record rather than a UI affordance.
        let build = || {
            let mut log = OpLog::new(mol("AAAACCCC", false));
            log.apply(
                OpKind::InsertAt {
                    at: 5,
                    seq: "GG".into(),
                },
                "alice",
            )
            .unwrap();
            log.apply(OpKind::SetTopology(Topology::Circular), "alice")
                .unwrap();
            log.apply(OpKind::Rotate { origin: 3 }, "alice").unwrap();
            log
        };
        let a = build();
        let mut b = OpLog::new(mol("AAAACCCC", false));
        // A different person, same operations.
        b.apply(
            OpKind::InsertAt {
                at: 5,
                seq: "GG".into(),
            },
            "bob",
        )
        .unwrap();
        b.apply(OpKind::SetTopology(Topology::Circular), "bob")
            .unwrap();
        b.apply(OpKind::Rotate { origin: 3 }, "bob").unwrap();

        assert_eq!(a.cursor(), b.cursor(), "the actor must not change the id");
        let ids_a: Vec<_> = a.path().iter().map(|o| o.id).collect();
        let ids_b: Vec<_> = b.path().iter().map(|o| o.id).collect();
        assert_eq!(ids_a, ids_b);
    }

    #[test]
    fn a_different_order_gives_different_ids() {
        let mut a = OpLog::new(mol("AAAACCCC", false));
        a.apply(
            OpKind::InsertAt {
                at: 1,
                seq: "T".into(),
            },
            "t",
        )
        .unwrap();
        a.apply(
            OpKind::InsertAt {
                at: 9,
                seq: "G".into(),
            },
            "t",
        )
        .unwrap();

        let mut b = OpLog::new(mol("AAAACCCC", false));
        b.apply(
            OpKind::InsertAt {
                at: 9,
                seq: "G".into(),
            },
            "t",
        )
        .unwrap();
        b.apply(
            OpKind::InsertAt {
                at: 1,
                seq: "T".into(),
            },
            "t",
        )
        .unwrap();

        assert_ne!(a.cursor(), b.cursor(), "history is a sequence, not a set");
    }

    #[test]
    fn a_rejected_operation_leaves_no_trace() {
        let mut log = OpLog::new(mol("AAAA", false));
        let before = log.current().seq.clone();
        let e = log
            .apply(OpKind::DeleteRange { start: 99, len: 1 }, "t")
            .unwrap_err();
        assert!(matches!(e, OpError::OutOfRange { .. }));
        assert_eq!(log.current().seq, before);
        assert_eq!(log.all_ops().len(), 0, "a refused edit is not history");
        assert_eq!(log.depth(), 0);
    }

    #[test]
    fn deleting_the_bases_a_feature_describes_deletes_the_feature() {
        // Not "collapse it to a one-base stub". A 1 bp AmpR is valid, looks
        // plausible, and is false — worse than no feature at all.
        let mut log = OpLog::new(with_feature("AAAACCCCGGGGTTTT", 13, 16));
        assert_eq!(log.current().features.len(), 1);
        log.apply(OpKind::DeleteRange { start: 13, len: 4 }, "t")
            .unwrap();
        assert_eq!(log.current().seq, b"AAAACCCCGGGG".to_vec());
        assert!(
            log.current().features.is_empty(),
            "the feature had nothing left to point at"
        );
        assert!(log.current().is_valid());

        // ...and undo brings it back, because nothing is ever discarded.
        log.undo().unwrap();
        assert_eq!(log.current().features.len(), 1);
        assert_eq!(
            (
                log.current().features[0].start(),
                log.current().features[0].end()
            ),
            (13, 16)
        );
    }

    #[test]
    fn reverse_complement_survives_a_molecule_with_no_bases() {
        // Annotation-only GenBank is a real, supported class: no ORIGIN block,
        // length declared only on the LOCUS line. `n = mol.len()` was 0 there,
        // so every coordinate underflowed — a panic in debug, and in release a
        // wrapped coordinate that the operation-log gate then let commit,
        // because it swapped one problem for another rather than adding one.
        let mut mol = Molecule {
            declared_len: Some(1000),
            ..Default::default()
        };
        let mut f = Feature::new("gene", "CDS");
        f.segments.push(Segment::new(100, 200));
        mol.features.push(f);
        assert!(mol.seq.is_empty());
        assert!(mol.is_valid());

        let mut log = OpLog::new(mol);
        log.apply(OpKind::ReverseComplement, "t").unwrap();
        let s = &log.current().features[0].segments[0];
        assert_eq!((s.start, s.end), (801, 901));
        assert!(log.current().is_valid(), "{:?}", log.current().validate());

        // ...and it is an involution, which is the property that matters.
        log.apply(OpKind::ReverseComplement, "t").unwrap();
        let s = &log.current().features[0].segments[0];
        assert_eq!((s.start, s.end), (100, 200));
    }

    #[test]
    fn reverse_complement_does_not_invent_a_coordinate_from_a_broken_one() {
        // A file that arrived with a coordinate past the end stays broken in
        // the same way rather than acquiring a fresh, larger absurdity.
        let mut mol = Molecule {
            seq: b"ACGTACGT".to_vec(),
            ..Default::default()
        };
        let mut f = Feature::new("bad", "misc_feature");
        f.segments.push(Segment::new(4, 9_000));
        mol.features.push(f);
        assert!(!mol.is_valid());

        let mut log = OpLog::new(mol);
        // Refused or applied, it must not panic and must not produce a
        // wrapped u64.
        let _ = log.apply(OpKind::ReverseComplement, "t");
        for s in &log.current().features[0].segments {
            assert!(s.start < 1_000_000, "coordinate wrapped: {}", s.start);
            assert!(s.end < 1_000_000, "coordinate wrapped: {}", s.end);
        }
    }

    #[test]
    fn replacing_a_region_keeps_annotations_only_where_they_still_mean_something() {
        // `ReplaceRange` had no test that could tell a preserved feature from a
        // mangled one, so every one of these was free to be wrong.
        let base = || OpLog::new(with_feature("AAAACCCCGGGGTTTT", 9, 12)); // GGGG

        // Interior, shorter: the bases the feature described are gone.
        let mut log = base();
        log.apply(
            OpKind::ReplaceRange {
                start: 5,
                len: 8,
                seq: "NN".into(),
            },
            "t",
        )
        .unwrap();
        assert!(
            log.current().features.is_empty(),
            "a feature whose bases were all replaced must not be re-anchored onto              the replacement: {:?}",
            log.current().features
        );

        // Interior, longer: same reasoning. Pasting a linker over a gene does
        // not leave the gene behind sitting on the linker.
        let mut log = base();
        log.apply(
            OpKind::ReplaceRange {
                start: 5,
                len: 8,
                seq: "NNNNNNNNNNNNNNNNNNNN".into(),
            },
            "t",
        )
        .unwrap();
        assert!(log.current().features.is_empty());

        // Interior, equal length: every base stayed put, so the annotation
        // still describes what it describes. A codon-optimisation swap.
        let mut log = base();
        log.apply(
            OpKind::ReplaceRange {
                start: 5,
                len: 8,
                seq: "TTTTAAAA".into(),
            },
            "t",
        )
        .unwrap();
        assert_eq!(log.current().features.len(), 1);
        let f = &log.current().features[0];
        assert_eq!(
            (f.start(), f.end()),
            (9, 12),
            "an equal-length swap moves nothing"
        );

        // A 1 bp point mutation under a 1 bp feature is the commonest real
        // replacement there is, and must be preserved.
        let mut log = OpLog::new(with_feature("AAAACCCCGGGGTTTT", 9, 9));
        log.apply(
            OpKind::ReplaceRange {
                start: 9,
                len: 1,
                seq: "T".into(),
            },
            "t",
        )
        .unwrap();
        assert_eq!(log.current().features.len(), 1);
        assert_eq!(
            (
                log.current().features[0].start(),
                log.current().features[0].end()
            ),
            (9, 9)
        );

        // An identity replacement changes nothing, so it must move nothing.
        let mut log = OpLog::new(with_feature("AAAACCCCGGGGTTTT", 5, 12));
        log.apply(
            OpKind::ReplaceRange {
                start: 1,
                len: 16,
                seq: "AAAACCCCGGGGTTTT".into(),
            },
            "t",
        )
        .unwrap();
        assert_eq!(
            (
                log.current().features[0].start(),
                log.current().features[0].end()
            ),
            (5, 12),
            "replacing bases with themselves must not drag the annotation"
        );

        // A straddling segment keeps the part that survived.
        let mut log = OpLog::new(with_feature("AAAACCCCGGGGTTTT", 5, 16));
        log.apply(
            OpKind::ReplaceRange {
                start: 9,
                len: 8,
                seq: "N".into(),
            },
            "t",
        )
        .unwrap();
        assert_eq!(log.current().features.len(), 1);
        let f = &log.current().features[0];
        assert_eq!((f.start(), f.end()), (5, 8), "the surviving prefix is kept");
        assert!(log.current().is_valid());
    }

    #[test]
    fn a_partly_deleted_feature_is_truncated_to_what_survives() {
        // The feature covers CCCCGGGG; remove its last four bases.
        let mut log = OpLog::new(with_feature("AAAACCCCGGGGTTTT", 5, 12));
        log.apply(OpKind::DeleteRange { start: 9, len: 4 }, "t")
            .unwrap();
        let f = &log.current().features[0];
        assert_eq!((f.start(), f.end()), (5, 8), "the surviving half is kept");
        let s = &log.current().seq;
        assert_eq!(&s[(f.start() - 1) as usize..f.end() as usize], b"CCCC");
        assert!(log.current().is_valid());
    }

    #[test]
    fn two_different_feature_edits_do_not_share_an_identity() {
        // `content()` hashed only name/kind/strand/segment-bounds while `apply`
        // clones the whole feature, so editing a qualifier or a colour derived
        // the SAME id as the previous edit. `apply` then saw the id already
        // present, declined to record the operation, and advanced `current`
        // anyway — so the log and the document disagreed and the next
        // undo/redo silently reinstated the older version.
        let mut a = Feature::new("gene", "misc_feature");
        a.segments.push(Segment::new(1, 4));
        a.set_qualifier("note", "version A");

        let mut b = a.clone();
        b.qualifiers.clear();
        b.set_qualifier("note", "version B");
        b.segments[0].color = Some("#ff0000".into());

        assert_ne!(
            derive_id(
                None,
                &OpKind::SetFeature {
                    index: Some(0),
                    feature: Box::new(a.clone())
                }
            ),
            derive_id(
                None,
                &OpKind::SetFeature {
                    index: Some(0),
                    feature: Box::new(b.clone())
                }
            ),
            "two different features must not derive the same id"
        );

        // End to end: edit, undo, edit differently, undo, redo.
        let mut log = OpLog::new(with_feature("AAAACCCCGGGGTTTT", 1, 4));
        log.apply(
            OpKind::SetFeature {
                index: Some(0),
                feature: Box::new(a),
            },
            "t",
        )
        .unwrap();
        log.undo().unwrap();
        log.apply(
            OpKind::SetFeature {
                index: Some(0),
                feature: Box::new(b),
            },
            "t",
        )
        .unwrap();
        assert_eq!(
            log.current().features[0].qualifier("note"),
            Some("version B")
        );

        log.undo().unwrap();
        log.redo().unwrap();
        assert_eq!(
            log.current().features[0].qualifier("note"),
            Some("version B"),
            "the second edit was lost through undo/redo"
        );
        assert_eq!(
            log.current().features[0].segments[0].color.as_deref(),
            Some("#ff0000")
        );
    }

    #[test]
    fn a_nul_byte_in_a_name_cannot_forge_another_operation() {
        // NUL used to delimit the variable-length fields, but a Rust String may
        // contain NUL, so these two encoded identically.
        let mk = |name: &str, kind: &str| {
            let mut f = Feature::new(name, kind);
            f.segments.push(Segment::new(1, 4));
            OpKind::SetFeature {
                index: Some(0),
                feature: Box::new(f),
            }
        };
        assert_ne!(
            derive_id(None, &mk("a\0b", "c")),
            derive_id(None, &mk("a", "b\0c"))
        );
    }

    #[test]
    fn identical_work_still_derives_an_identical_id() {
        // The property the whole scheme exists for must survive the fix:
        // the same edit from the same parent is the same operation, on any
        // machine, at any time.
        let mk = || {
            let mut f = Feature::new("AmpR", "CDS");
            f.segments.push(Segment::new(10, 900));
            f.set_qualifier("gene", "bla");
            OpKind::SetFeature {
                index: None,
                feature: Box::new(f),
            }
        };
        assert_eq!(derive_id(None, &mk()), derive_id(None, &mk()));
    }

    #[test]
    fn every_edit_leaves_a_self_consistent_molecule() {
        // The guarantee that replaces "invalid states are unrepresentable".
        let mut log = OpLog::new(with_feature("AAAACCCCGGGGTTTT", 5, 12));
        for kind in [
            OpKind::InsertAt {
                at: 1,
                seq: "TT".into(),
            },
            OpKind::DeleteRange { start: 3, len: 6 },
            OpKind::ReplaceRange {
                start: 2,
                len: 4,
                seq: "GG".into(),
            },
            OpKind::SetTopology(Topology::Circular),
            OpKind::ReverseComplement,
        ] {
            if log.apply(kind.clone(), "t").is_ok() {
                assert!(
                    log.current().is_valid(),
                    "{} left {:?}",
                    kind.describe(),
                    log.current().validate()
                );
            }
        }
    }

    #[test]
    fn a_file_that_arrived_broken_can_still_be_edited() {
        // Only *new* problems block an edit. Refusing to let someone touch a
        // file because its importer left a bad coordinate would blame the user
        // for someone else's bug.
        // The fixture matters. This used to plant an inverted segment at 4..2,
        // which `remap_annotations` then wiped, so `after.validate()` was empty
        // and the test passed whatever the gate did. A coordinate PAST THE END
        // survives every edit below, so the gate is actually exercised.
        let mut m = mol("AAAACCCC", false);
        let mut f = Feature::new("already wrong", "misc_feature");
        f.segments.push(Segment::new(2, 4000)); // past the end on arrival
        m.features.push(f);
        let mut ok = Feature::new("fine", "misc_feature");
        ok.segments.push(Segment::new(2, 5));
        m.features.push(ok);
        assert_eq!(m.validate().len(), 1, "{:?}", m.validate());

        let mut log = OpLog::new(m);

        // A length-changing insert. `PastEnd` embeds the molecule length, so a
        // value-comparing gate reads the same problem as brand new here.
        log.apply(
            OpKind::InsertAt {
                at: 1,
                seq: "TT".into(),
            },
            "t",
        )
        .expect("a pre-existing problem must not block an unrelated edit");
        assert_eq!(log.current().seq, b"TTAAAACCCC".to_vec());

        // Removing a feature renumbers the rest, and `what` embeds the index.
        log.apply(OpKind::RemoveFeature { index: 1 }, "t")
            .expect("removing an unrelated feature must be allowed");

        // Renaming: `what` embeds the name too.
        let mut renamed = log.current().features[0].clone();
        renamed.name = "renamed".into();
        log.apply(
            OpKind::SetFeature {
                index: Some(0),
                feature: Box::new(renamed),
            },
            "t",
        )
        .expect("renaming a feature the importer broke must be allowed");
        assert_eq!(log.current().features[0].name, "renamed");
    }

    #[test]
    fn an_edit_that_trades_one_problem_for_another_is_refused() {
        // The old gate asked only whether the *total* went up, so swapping a
        // `PastEnd` for an `Inverted` slipped through: one is not greater than
        // one. That is how a reverse-complement could commit a wrapped
        // coordinate of 18446744073709550634.
        let mut m = mol("AAAACCCCGGGGTTTT", false);
        let mut broken = Feature::new("broken", "misc_feature");
        broken.segments.push(Segment::new(2, 9000)); // past the end
        m.features.push(broken);
        assert_eq!(m.validate().len(), 1);

        let mut log = OpLog::new(m);
        // Replace it with a feature that is inverted instead: still one
        // problem, but a different kind, so it must not be waved through.
        let mut inverted = Feature::new("broken", "misc_feature");
        inverted.segments.push(Segment::new(9, 3));
        let e = log
            .apply(
                OpKind::SetFeature {
                    index: Some(0),
                    feature: Box::new(inverted),
                },
                "t",
            )
            .unwrap_err();
        match e {
            OpError::WouldCorrupt(v) => {
                assert!(v.iter().any(|x| x.kind() == "inverted"), "{v:?}");
            }
            other => panic!("expected WouldCorrupt, got {other:?}"),
        }
    }

    #[test]
    fn rotation_is_refused_on_a_linear_molecule() {
        let mut log = OpLog::new(mol("AAAACCCC", false));
        assert_eq!(
            log.apply(OpKind::Rotate { origin: 3 }, "t").unwrap_err(),
            OpError::NotCircular
        );
    }

    #[test]
    fn reverse_complement_flips_features_as_well_as_bases() {
        let mut log = OpLog::new(with_feature("AAAACCCCGGGGTTTT", 1, 4));
        log.apply(OpKind::ReverseComplement, "t").unwrap();
        assert_eq!(log.current().seq, b"AAAACCCCGGGGTTTT".to_vec());
        let f = &log.current().features[0];
        // The AAAA at 1..4 is now the TTTT at 13..16.
        assert_eq!((f.start(), f.end()), (13, 16));
        assert_eq!(f.strand, crate::Strand::Reverse);
    }

    #[test]
    fn replay_over_a_long_history_matches_direct_application() {
        // Crosses several snapshot boundaries, so this exercises the lazy
        // materialisation path rather than a straight replay.
        let mut log = OpLog::new(mol("ACGT", false));
        let mut direct = mol("ACGT", false);
        let mut marks = Vec::new();
        for i in 0..(SNAPSHOT_EVERY * 3 + 7) {
            let k = OpKind::InsertAt {
                at: 1,
                seq: if i % 2 == 0 { "A".into() } else { "GC".into() },
            };
            log.apply(k.clone(), "t").unwrap();
            apply(&mut direct, &k).unwrap();
            if i % 37 == 0 {
                marks.push((log.cursor(), log.current().seq.clone()));
            }
        }
        assert_eq!(log.current().seq, direct.seq);
        assert_eq!(log.depth(), SNAPSHOT_EVERY * 3 + 7);

        // Seeking back to arbitrary points reproduces exactly what was there.
        for (id, seq) in marks {
            log.seek(id).unwrap();
            assert_eq!(log.current().seq, seq, "replay diverged at {id:?}");
        }
    }

    #[test]
    fn undoing_everything_returns_the_original() {
        let mut log = OpLog::new(with_feature("AAAACCCCGGGG", 5, 8));
        let original = log.current().clone();
        log.apply(
            OpKind::InsertAt {
                at: 1,
                seq: "TTT".into(),
            },
            "t",
        )
        .unwrap();
        log.apply(OpKind::SetTopology(Topology::Circular), "t")
            .unwrap();
        log.apply(OpKind::DeleteRange { start: 2, len: 2 }, "t")
            .unwrap();
        while log.undo().is_ok() {}
        assert_eq!(log.current().seq, original.seq);
        assert_eq!(log.current().topology, original.topology);
        assert_eq!(
            log.current().features[0].start(),
            original.features[0].start()
        );
    }

    #[test]
    fn an_edit_elsewhere_does_not_delete_a_feature_that_crosses_the_origin() {
        // A segment crossing the origin is written `end < start`, which
        // `validate` calls legal on a circle. `remap` tested `b >= a`, which is
        // right for an ordinary segment and false by construction for a wrapped
        // one, so EVERY edit anywhere on a circular molecule silently deleted
        // every origin-crossing feature -- including edits nowhere near them --
        // and `apply` still returned Ok.
        let mut mol = Molecule {
            seq: b"AAAACCCCGGGGTTTT".to_vec(),
            topology: Topology::Circular,
            ..Default::default()
        };
        let mut wrapped = Feature::new("crosses the origin", "misc_feature");
        wrapped.segments.push(Segment::new(15, 2));
        let mut control = Feature::new("ordinary", "misc_feature");
        control.segments.push(Segment::new(9, 12));
        mol.features.push(wrapped);
        mol.features.push(control);

        let mut log = OpLog::new(mol);
        log.apply(
            OpKind::InsertAt {
                at: 5,
                seq: "TTT".into(),
            },
            "test",
        )
        .expect("an insert at 5 touches neither feature");

        let m = log.current();
        assert_eq!(
            m.features.len(),
            2,
            "neither feature may vanish: {:?}",
            m.features
        );
        let w = &m.features[0];
        assert_eq!(w.name, "crosses the origin");
        // Three bases went in before it, so its start moves and its end does
        // not -- it still crosses the origin.
        assert_eq!((w.segments[0].start, w.segments[0].end), (18, 2));
        let c = &m.features[1];
        assert_eq!((c.segments[0].start, c.segments[0].end), (12, 15));
    }

    #[test]
    fn a_replacement_does_not_hand_a_feature_the_text_that_replaced_it() {
        // The near end of a straddling feature was pinned to where the replaced
        // region BEGINS, so a feature starting inside the replacement was moved
        // to the front of it and claimed every new base. The pin belongs at the
        // end of the new text: `start + new_len`, which collapses to `start`
        // for a pure deletion.
        let mut mol = Molecule {
            seq: b"ACGTACGTACGTACGTACGTACGTACGTACGTACGTACGT".to_vec(),
            ..Default::default()
        };
        let mut f = Feature::new("straddles", "misc_feature");
        f.segments.push(Segment::new(12, 30));
        mol.features.push(f);

        let mut log = OpLog::new(mol);
        log.apply(
            OpKind::ReplaceRange {
                start: 10,
                len: 5,
                seq: "G".repeat(20),
            },
            "test",
        )
        .expect("a replacement overlapping the feature's start");

        let seg = &log.current().features[0].segments[0];
        // The replacement occupies 10..29. The feature's surviving part begins
        // where that text ends, not where it starts.
        assert_eq!(
            seg.start, 30,
            "pinned to the end of the new text, not its beginning"
        );
        assert_eq!(seg.end, 45, "its far end shifts by the length difference");
    }

    #[test]
    fn a_pure_deletion_still_pins_a_straddling_feature_to_the_cut() {
        // The guard against over-correcting: with new_len == 0 the new pin is
        // identical to the old one, so deletions behave exactly as before.
        let mut mol = Molecule {
            seq: b"ACGTACGTACGTACGTACGTACGTACGTACGTACGTACGT".to_vec(),
            ..Default::default()
        };
        let mut f = Feature::new("straddles", "misc_feature");
        f.segments.push(Segment::new(12, 30));
        mol.features.push(f);
        let mut log = OpLog::new(mol);
        log.apply(OpKind::DeleteRange { start: 10, len: 5 }, "test")
            .expect("delete");
        let seg = &log.current().features[0].segments[0];
        assert_eq!((seg.start, seg.end), (10, 25));
    }

    #[test]
    fn reverse_complementing_a_standalone_annotation_track_is_refused_not_collapsed() {
        // Features, no bases, no declared length — UGENE and SnapGene both
        // export this, pl-fileio pins the exact file with
        // `standalone_annotation_tracks_are_read`, and pl-gui offers
        // Edit -> "Reverse complement" on it with no further predicate.
        // `span()` is 0 there, so `n.saturating_sub(p).saturating_add(1)` gave 1
        // for every coordinate: a 774 bp and an 834 bp annotation were both
        // replaced by 1 bp annotations at base 1, `validate()` stayed clean
        // because it measures against `annotation_span()` and only reports
        // `PastEnd` when `n > 0`, `apply` returned `Ok`, and doing it twice did
        // not undo it.
        let mut m = Molecule::default();
        for (name, a, b) in [("orf1", 242u64, 1015u64), ("orf2", 1118, 1951)] {
            let mut f = Feature::new(name, "CDS");
            f.segments.push(Segment::new(a, b));
            m.features.push(f);
        }
        assert!(m.is_annotation_track());
        assert_eq!(m.span(), 0);
        assert_eq!(m.annotation_span(), 1951);

        let mut log = OpLog::new(m);
        let e = log.apply(OpKind::ReverseComplement, "t").unwrap_err();
        assert!(
            matches!(e, OpError::CannotReflect { len: 0, .. }),
            "expected a refusal naming the missing length, got {e:?}"
        );
        let f = log.current();
        assert_eq!(
            (
                f.features[0].segments[0].start,
                f.features[0].segments[0].end
            ),
            (242, 1015),
            "a refused operation must leave the document untouched"
        );
        assert_eq!(
            (
                f.features[1].segments[0].start,
                f.features[1].segments[0].end
            ),
            (1118, 1951)
        );
        assert_eq!(log.all_ops().len(), 0, "a refused edit is not history");
    }

    #[test]
    fn a_coordinate_past_the_end_survives_a_reverse_complement_instead_of_being_erased() {
        // `flip` maps EVERY input into `[1, n]`, so a `PastEnd` could never
        // survive the operation for any molecule and any out-of-range value.
        // On these 8 bases the segment `4..9000` came back as `1..5`,
        // `validate()` went from one `PastEnd` to none, and `is_valid()` flipped
        // from false to true — the feature then made a confident, wrong claim
        // about bases 1..5. The log's gate cannot see it: it compares per-kind
        // problem counts and refuses only increases.
        let mut m = Molecule {
            seq: b"ACGTACGT".to_vec(),
            ..Default::default()
        };
        let mut f = Feature::new("bad", "misc_feature");
        f.segments.push(Segment::new(4, 9_000));
        m.features.push(f);
        assert_eq!(m.validate().len(), 1, "{:?}", m.validate());

        let mut log = OpLog::new(m);
        let e = log.apply(OpKind::ReverseComplement, "t").unwrap_err();
        assert!(
            matches!(
                e,
                OpError::CannotReflect {
                    at: 9_000,
                    len: 8,
                    ..
                }
            ),
            "expected a refusal naming the coordinate, got {e:?}"
        );
        let s = &log.current().features[0].segments[0];
        assert_eq!((s.start, s.end), (4, 9_000));
        assert!(
            !log.current().is_valid(),
            "the problem the file arrived with must still be reported"
        );
        assert_eq!(log.current().seq, b"ACGTACGT".to_vec(), "no bases moved");
    }

    #[test]
    fn a_reverse_complement_of_coordinates_that_all_exist_is_unchanged() {
        // The guard against over-correcting: refusing must apply only where
        // there is genuinely nothing to reflect onto. Both endpoints of a legal
        // origin-crossing wrap are real positions, as is every primer site here,
        // so this must still commit — and still be an involution.
        let mut m = Molecule {
            seq: b"AAAACCCCGGGGTTTT".to_vec(),
            topology: Topology::Circular,
            ..Default::default()
        };
        let mut wrapped = Feature::new("crosses the origin", "misc_feature");
        wrapped.segments.push(Segment::new(15, 2));
        m.features.push(wrapped);
        m.primers.push(crate::Primer {
            name: "p".into(),
            seq: "AAAA".into(),
            description: String::new(),
            sites: vec![crate::BindingSite {
                start: 1,
                end: 4,
                strand: crate::Strand::Forward,
                tm: None,
            }],
        });

        let mut log = OpLog::new(m);
        log.apply(OpKind::ReverseComplement, "t")
            .expect("every coordinate here names a real base");
        let s = &log.current().features[0].segments[0];
        assert_eq!((s.start, s.end), (15, 2), "the wrap reflects onto itself");
        let site = &log.current().primers[0].sites[0];
        assert_eq!((site.start, site.end), (13, 16));
        assert_eq!(site.strand, crate::Strand::Reverse);

        log.apply(OpKind::ReverseComplement, "t").unwrap();
        let site = &log.current().primers[0].sites[0];
        assert_eq!((site.start, site.end), (1, 4), "an involution");
        assert_eq!(site.strand, crate::Strand::Forward);
    }

    #[test]
    fn an_edit_elsewhere_does_not_delete_a_feature_the_importer_gave_a_zero_start() {
        // `snapgene.rs` parses `<Segment range="0-4"/>` verbatim, and
        // `Molecule::rotate` deliberately preserves such a start rather than
        // dropping it. `remap`'s `a >= 1` conjunct did the opposite: it deleted
        // the whole feature on the first length-changing edit anywhere in the
        // molecule — here an insertion six bases away — and `apply` returned
        // `Ok`, because removing the sole `ZeroStart` made the gate see an
        // improvement rather than a loss.
        let mut m = mol("AAAACCCCGGGGTTTT", false);
        let mut zero = Feature::new("zero start", "misc_feature");
        zero.segments.push(Segment::new(0, 4));
        let mut ordinary = Feature::new("ordinary", "misc_feature");
        ordinary.segments.push(Segment::new(12, 14));
        m.features.push(zero);
        m.features.push(ordinary);
        assert_eq!(m.validate().len(), 1, "{:?}", m.validate());

        let mut log = OpLog::new(m);
        log.apply(
            OpKind::InsertAt {
                at: 10,
                seq: "GG".into(),
            },
            "t",
        )
        .expect("an insert at 10 touches neither feature");

        let after = log.current();
        assert_eq!(
            after.features.len(),
            2,
            "the zero-start feature must not vanish: {:?}",
            after.features
        );
        assert_eq!(after.features[0].name, "zero start");
        assert_eq!(
            (
                after.features[0].segments[0].start,
                after.features[0].segments[0].end
            ),
            (0, 4),
            "entirely before the edit means unchanged, exactly as documented"
        );
        assert_eq!(
            (
                after.features[1].segments[0].start,
                after.features[1].segments[0].end
            ),
            (14, 16),
            "the control feature still follows its bases"
        );
        assert!(
            after
                .validate()
                .iter()
                .any(|p| matches!(p, crate::Invalid::ZeroStart { .. })),
            "the importer's problem is reported, not quietly repaired: {:?}",
            after.validate()
        );
    }

    #[test]
    fn a_zero_start_feature_whose_bases_are_deleted_is_still_deleted() {
        // The guard against over-correcting the above. `0-4` really describes
        // bases 1..4; delete those four bases and there is nothing left to point
        // at, so the feature goes — a 1 bp stub would be valid, plausible and
        // false.
        let mut m = mol("AAAACCCCGGGGTTTT", false);
        let mut zero = Feature::new("zero start", "misc_feature");
        zero.segments.push(Segment::new(0, 4));
        m.features.push(zero);

        let mut log = OpLog::new(m);
        log.apply(OpKind::DeleteRange { start: 1, len: 4 }, "t")
            .expect("delete");
        assert!(
            log.current().features.is_empty(),
            "{:?}",
            log.current().features
        );
    }

    #[test]
    fn a_zero_start_segment_is_truncated_when_only_some_of_its_bases_go() {
        // Between the two tests above. `0-4` describes bases 1..4; delete bases
        // 3 and 4 and bases 1 and 2 survive, so the segment should become
        // `0..2`. The `a >= 1` conjunct tested the wrong endpoint and threw the
        // whole feature away instead, with `apply` returning `Ok` and the sole
        // `ZeroStart` disappearing with it, so the gate saw an improvement.
        let mut m = mol("AAAACCCCGGGGTTTT", false);
        let mut zero = Feature::new("zero start", "misc_feature");
        zero.segments.push(Segment::new(0, 4));
        m.features.push(zero);
        assert_eq!(m.validate().len(), 1, "{:?}", m.validate());

        let mut log = OpLog::new(m);
        log.apply(OpKind::DeleteRange { start: 3, len: 2 }, "t")
            .expect("two of the four bases survive");

        let after = log.current();
        assert_eq!(
            after.features.len(),
            1,
            "two bases survived, so the feature must: {:?}",
            after.features
        );
        assert_eq!(
            (
                after.features[0].segments[0].start,
                after.features[0].segments[0].end
            ),
            (0, 2)
        );
        assert!(
            after
                .validate()
                .iter()
                .any(|p| matches!(p, crate::Invalid::ZeroStart { .. })),
            "the importer's problem is reported, not repaired by deletion: {:?}",
            after.validate()
        );

        // The same, with the deletion strictly *inside* the segment, which is
        // not an overlap at all: `0-8` less bases 2 and 3 is `0..6`.
        let mut m = mol("AAAACCCCGGGGTTTT", false);
        let mut zero = Feature::new("zero start", "misc_feature");
        zero.segments.push(Segment::new(0, 8));
        m.features.push(zero);
        let mut log = OpLog::new(m);
        log.apply(OpKind::DeleteRange { start: 2, len: 2 }, "t")
            .expect("six of the eight bases survive");
        assert_eq!(
            (
                log.current().features[0].segments[0].start,
                log.current().features[0].segments[0].end
            ),
            (0, 6)
        );
    }

    #[test]
    fn an_edit_overlapping_a_wrapped_feature_truncates_it_instead_of_deleting_it() {
        // The two `or_else` fallbacks encode "the remnant lies on the far side
        // of the cut", which is only true in numeric order. A wrapped segment
        // has its two ends on opposite sides of the origin, so both guards are
        // unsatisfiable and any length-changing edit touching either endpoint
        // deleted the segment outright — however many of its bases survived.
        //
        // `AmpR` at 15..2 covers bases 15, 16, 1, 2. Delete bases 1 and 2 and
        // two of its four bases survive, so it must become 13..14.
        let mut m = mol("AAAACCCCGGGGTTTT", true);
        let mut ampr = Feature::new("AmpR", "CDS");
        ampr.segments.push(Segment::new(15, 2));
        let mut ori = Feature::new("ori", "rep_origin");
        ori.segments.push(Segment::new(5, 8));
        m.features.push(ampr);
        m.features.push(ori);
        assert!(
            m.is_valid(),
            "a wrap is legal on a circle: {:?}",
            m.validate()
        );

        let mut log = OpLog::new(m);
        log.apply(OpKind::DeleteRange { start: 1, len: 2 }, "t")
            .expect("delete the two bases at the front");

        let after = log.current();
        assert_eq!(
            after.features.len(),
            2,
            "half of AmpR survived the cut: {:?}",
            after.features
        );
        assert_eq!(after.features[0].name, "AmpR");
        assert_eq!(
            (
                after.features[0].segments[0].start,
                after.features[0].segments[0].end
            ),
            (13, 14),
            "the head of the wrap moved with its bases; the tail is gone"
        );
        assert_eq!(
            (
                after.features[1].segments[0].start,
                after.features[1].segments[0].end
            ),
            (3, 6),
            "the control follows its bases"
        );
    }

    #[test]
    fn the_two_spellings_of_one_origin_crossing_span_survive_an_edit_alike() {
        // The decisive A/B. The same biological span written two ways — one
        // wrapped segment, and the two-segment join `genbank::write` emits for
        // it — must come out of the same edit describing the same bases. The
        // join truncated correctly while the wrap was deleted whole, so the
        // answer depended on which file format the molecule had been through.
        //
        // 40 bp circle, span covers 37..40 + 1..8 (12 bases). Delete bases 6-8,
        // three of which the span names. The nine survivors are 37..40 and
        // 1..5, which the deletion renumbers to 34..37 and 1..5.
        let build = |segs: &[(u64, u64)]| {
            let mut m = mol("ACGTACGTACGTACGTACGTACGTACGTACGTACGTACGT", true);
            let mut f = Feature::new("wrapper", "misc_feature");
            for &(a, b) in segs {
                f.segments.push(Segment::new(a, b));
            }
            m.features.push(f);
            m
        };

        let mut wrap = OpLog::new(build(&[(37, 8)]));
        wrap.apply(OpKind::DeleteRange { start: 6, len: 3 }, "t")
            .expect("nine of the twelve bases survive");
        let w = &wrap.current().features[0].segments;
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!((w[0].start, w[0].end), (34, 5));

        let mut join = OpLog::new(build(&[(37, 40), (1, 8)]));
        join.apply(OpKind::DeleteRange { start: 6, len: 3 }, "t")
            .expect("nine of the twelve bases survive");
        let j = &join.current().features[0].segments;
        assert_eq!(j.len(), 2, "{j:?}");
        assert_eq!((j[0].start, j[0].end), (34, 37));
        assert_eq!((j[1].start, j[1].end), (1, 5));
    }

    #[test]
    fn a_feature_that_arrived_with_no_segments_survives_an_unrelated_edit() {
        // `snapgene::parse_features` builds every feature with
        // `segments: Vec::new()` and pushes it on `</Feature>` whether or not a
        // `<Segment>` ever arrived, so `<Feature name="AmpR" type="CDS"/>` — or
        // any feature whose `range` failed to parse — reaches the operation log
        // carrying its name, its type and its qualifiers and nothing else.
        //
        // The unconditional retain deleted it on the first length-changing edit
        // anywhere in the molecule. That erased the `FeatureWithoutSegments`
        // report along with it, so `refuse_new_problems` counted 1 -> 0 and read
        // the loss as an improvement: `apply` returned `Ok` and the feature and
        // its qualifiers were gone from the document and from the next save.
        let mut m = mol("AAAACCCCGGGGTTTT", false);
        let mut orphan = Feature::new("AmpR", "CDS");
        orphan.set_qualifier("gene", "bla");
        orphan.set_qualifier("note", "confers ampicillin resistance");
        let mut ordinary = Feature::new("prom", "promoter");
        ordinary.segments.push(Segment::new(1, 4));
        m.features.push(orphan);
        m.features.push(ordinary);
        assert!(matches!(
            m.validate().as_slice(),
            [crate::Invalid::FeatureWithoutSegments { .. }]
        ));

        let mut log = OpLog::new(m);
        log.apply(
            OpKind::InsertAt {
                at: 9,
                seq: "T".into(),
            },
            "t",
        )
        .expect("an insert at 9 touches neither feature");

        let after = log.current();
        assert_eq!(
            after.features.len(),
            2,
            "a feature this edit did not empty must not be deleted: {:?}",
            after.features
        );
        assert_eq!(after.features[0].name, "AmpR");
        assert_eq!(after.features[0].qualifier("gene"), Some("bla"));
        assert!(
            after
                .validate()
                .iter()
                .any(|p| matches!(p, crate::Invalid::FeatureWithoutSegments { .. })),
            "the importer's mess stays visible: {:?}",
            after.validate()
        );

        // ...and the deliberate rule is untouched: a feature THIS edit emptied
        // still goes.
        let mut log = OpLog::new(with_feature("AAAACCCCGGGGTTTT", 13, 16));
        log.apply(OpKind::DeleteRange { start: 13, len: 4 }, "t")
            .expect("delete");
        assert!(log.current().features.is_empty());
    }

    #[test]
    fn a_coordinate_that_was_already_past_the_end_is_no_anchor_for_a_dead_start() {
        // `gene 5..20` on a 12 bp molecule names eight bases that do not exist,
        // and `validate()` reports it as `PastEnd`. Deleting every base then
        // produced `gene 1..8` on a molecule with NO bases: the `a` fallback
        // rescued a start that had died inside the deleted region, on the
        // strength of `s_end >= old_end` — a test that reads as "the far end is
        // past the edit and therefore survived", which is true only of a
        // coordinate that named a base to begin with. 20 named none.
        //
        // Two harms, and the second is what let it travel. It fabricated a span
        // on a molecule with nothing to put it on, and it LAUNDERED the
        // `PastEnd`: `validate()` went 1 problem -> 0, which
        // `refuse_new_problems` reads as an improvement, so the edit committed.
        // `genbank::write` then emits `gene 1..8` under a `0 bp` LOCUS, which
        // reopens as a real annotation track, and `is_annotation_track()`
        // answered true for a document the user had merely emptied — which is
        // what left `pl-gui`'s sequence editor refusing to edit it.
        let mut m = mol("ACGTACGTACGT", false);
        let mut gene = Feature::new("gene", "CDS");
        gene.segments.push(Segment::new(5, 20));
        m.features.push(gene);
        assert!(
            matches!(m.validate().as_slice(), [crate::Invalid::PastEnd { .. }]),
            "the premise: 5..20 on a 12 bp molecule is past the end"
        );

        let mut log = OpLog::new(m);
        log.apply(OpKind::DeleteRange { start: 1, len: 12 }, "t")
            .expect("deleting every base is a legal edit");
        let after = log.current();
        assert_eq!(after.len(), 0, "every base was deleted");
        assert!(
            after.features.is_empty(),
            "the only bases this feature really had went with the rest, so the \
             feature goes too — it must not come back as a span of a molecule \
             that has no bases: {:?}",
            after.features.first().map(|f| f.segments.clone())
        );
        assert!(
            !after.is_annotation_track(),
            "a molecule the user emptied is not an annotation track"
        );

        // The control, and the reason the fix is a guard on that one fallback
        // rather than a blanket "out of range is not an anchor": an edit that
        // does NOT kill the start must still carry the absurd end through, so
        // `validate()` goes on reporting it. Dropping or truncating it here
        // would launder the same `PastEnd` in the other direction.
        let mut m = mol("ACGTACGTACGT", false);
        let mut gene = Feature::new("gene", "CDS");
        gene.segments.push(Segment::new(5, 20));
        m.features.push(gene);
        let mut log = OpLog::new(m);
        log.apply(
            OpKind::InsertAt {
                at: 9,
                seq: "TT".into(),
            },
            "t",
        )
        .expect("an insert at 9 leaves base 5 where it is");
        let after = log.current();
        let s = &after.features[0].segments[0];
        assert_eq!((s.start, s.end), (5, 22), "{s:?}");
        assert!(
            matches!(
                after.validate().as_slice(),
                [crate::Invalid::PastEnd { .. }]
            ),
            "the coordinate that named no base stays absurd: {:?}",
            after.validate()
        );
    }

    #[test]
    fn two_methylation_states_that_differ_only_in_cpg_are_two_operations() {
        // `OpKind::content` hashed dam, dcm and ecoki while `apply` assigns the
        // whole struct, so the two states below derived one `OpId`. From the
        // same parent — which is what an undo leaves behind — the second one hit
        // the collision guard and was refused, and CpG is the field that matters
        // most: 26 of 34 blocking pairs are CpG.
        let off = crate::Methylation {
            dam: true,
            dcm: false,
            ecoki: false,
            cpg: false,
        };
        let on = crate::Methylation { cpg: true, ..off };
        assert_ne!(
            derive_id(None, &OpKind::SetMethylation(off)),
            derive_id(None, &OpKind::SetMethylation(on)),
            "the encoding has to be injective over every field `apply` acts on"
        );

        let mut log = OpLog::new(mol("AAAACCCCGGGG", false));
        log.apply(OpKind::SetMethylation(off), "t").unwrap();
        log.undo().unwrap();
        log.apply(OpKind::SetMethylation(on), "t")
            .expect("toggling CpG after an undo is an ordinary edit");
        assert!(log.current().methylation.cpg);
        assert_eq!(log.all_ops().len(), 2);
    }

    #[test]
    fn a_range_that_names_no_base_is_refused_rather_than_overflowing() {
        // `start + len - 1 > n` overflowed on exactly the inputs it exists to
        // reject. In a checked build — every `cargo test`, every dev GUI — it
        // panicked at the guard itself; with checks off the sum wrapped, the
        // guard passed, and the panic moved into `Vec::drain`/`Vec::splice`
        // inside libcore where it names no polylinker code at all.
        let base = mol("ACGT", false);
        for (kind, what) in [
            (
                OpKind::DeleteRange {
                    start: u64::MAX,
                    len: 1,
                },
                "deletion",
            ),
            (
                OpKind::DeleteRange {
                    start: 2,
                    len: u64::MAX,
                },
                "deletion",
            ),
            (
                OpKind::ReplaceRange {
                    start: 1,
                    len: u64::MAX,
                    seq: "A".into(),
                },
                "replacement",
            ),
            (
                OpKind::ReplaceRange {
                    start: 5,
                    len: u64::MAX,
                    seq: "A".into(),
                },
                "replacement",
            ),
        ] {
            let mut m = base.clone();
            let e = apply(&mut m, &kind).unwrap_err();
            assert!(
                matches!(e, OpError::OutOfRange { what: w, .. } if w == what),
                "{kind:?} -> {e:?}"
            );
            assert_eq!(m.seq, base.seq, "a refused op leaves the bases alone");
        }

        // The asymmetry the guards deliberately keep: `DeleteRange` refuses a
        // zero length, `ReplaceRange` accepts it as an insertion, and appending
        // at `n + 1` is still legal.
        let mut m = base.clone();
        assert!(matches!(
            apply(&mut m, &OpKind::DeleteRange { start: 1, len: 0 }).unwrap_err(),
            OpError::OutOfRange { .. }
        ));
        let mut m = base.clone();
        apply(
            &mut m,
            &OpKind::ReplaceRange {
                start: 1,
                len: 0,
                seq: "TT".into(),
            },
        )
        .expect("a zero-length replacement is an insertion");
        assert_eq!(m.seq, b"TTACGT".to_vec());
        let mut m = base.clone();
        apply(
            &mut m,
            &OpKind::ReplaceRange {
                start: 5,
                len: 0,
                seq: "TT".into(),
            },
        )
        .expect("appending one past the end is in range");
        assert_eq!(m.seq, b"ACGTTT".to_vec());
        let mut m = base.clone();
        apply(&mut m, &OpKind::DeleteRange { start: 4, len: 1 })
            .expect("the last base is in range");
        assert_eq!(m.seq, b"ACG".to_vec());
    }

    #[test]
    fn a_hostile_coordinate_neither_overflows_nor_vanishes_when_the_length_changes() {
        // `(p as i64 + delta) as u64` on a value straight off disk.
        // `snapgene.rs` parses `<Segment range="a-b"/>` with an unbounded
        // `parse::<u64>()` and pl-gui never validates on the way in.
        //
        // At `i64::MAX` a 12 bp insertion panicked "attempt to add with
        // overflow" in any checked build. At `u64::MAX`, with checks off,
        // `u64::MAX as i64` is -1 so the end came back as 11 while the start
        // moved to 13; `b >= a` failed and the entire feature was deleted with
        // `apply` returning `Ok` and reporting nothing.
        for end in [i64::MAX as u64, u64::MAX] {
            let mut m = mol("AAAACCCCGGGGTTTT", false);
            let mut f = Feature::new("hostile", "misc_feature");
            f.segments.push(Segment::new(1, end));
            m.features.push(f);
            assert_eq!(m.validate().len(), 1, "one PastEnd on arrival");

            let mut log = OpLog::new(m);
            log.apply(
                OpKind::InsertAt {
                    at: 1,
                    seq: "GAATTCGGATCC".into(),
                },
                "t",
            )
            .expect("an insertion elsewhere must not be blamed on the importer");

            let after = log.current();
            assert_eq!(
                after.features.len(),
                1,
                "a coordinate the file supplied must not delete the feature"
            );
            let s = &after.features[0].segments[0];
            assert_eq!(s.start, 13, "the real end of the segment moved by 12");
            assert!(
                s.end > after.len(),
                "an absurd coordinate stays absurd so `validate` goes on reporting it: {}",
                s.end
            );
            assert!(
                after
                    .validate()
                    .iter()
                    .any(|p| matches!(p, crate::Invalid::PastEnd { .. })),
                "{:?}",
                after.validate()
            );
        }
    }

    #[test]
    fn redos_own_documentation_sits_on_redo_and_not_on_can_undo() {
        // A doc comment is a claim the code has to keep. This sentence sat at
        // the head of `can_undo`'s `///` block, so rustdoc printed "Step the
        // cursor forward, along the most recently created branch." as the
        // summary for a read-only `&self` predicate that moves no cursor and
        // answers the opposite question, while `redo` — the only way forward
        // through the log — was listed with no description at all. Verified in
        // the generated `struct.OpLog.html`, and asserted here rather than
        // eyeballed because nothing else in the build reads doc comments. The
        // sibling assertion for `rotate` lives in `lib.rs`.
        let src = include_str!("oplog.rs");
        let sentence = "/// Step the cursor forward, along the most recently created branch.";
        let can_undo = src
            .find("pub fn can_undo(&self)")
            .expect("the undo predicate is still here");
        let redo = src
            .find("pub fn redo(&mut self)")
            .expect("redo is still here");
        let doc = src.find(sentence).expect("redo still describes itself");
        assert!(
            doc > can_undo && doc < redo,
            "redo's summary must attach to redo, not to the predicate above it"
        );
        // ...and the predicate keeps its own.
        let can_undo_doc = src
            .find("/// Is there anything to undo?")
            .expect("the undo predicate still describes itself");
        assert!(can_undo_doc < can_undo);
    }
}
