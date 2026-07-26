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

use std::collections::HashMap;

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
    fn content(&self) -> Vec<u8> {
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
                v.extend_from_slice(feature.name.as_bytes());
                v.push(0);
                v.extend_from_slice(feature.kind.as_bytes());
                v.push(0);
                v.extend_from_slice(
                    &(feature.strand.to_directionality().unwrap_or(0)).to_be_bytes(),
                );
                for s in &feature.segments {
                    v.extend_from_slice(&s.start.to_be_bytes());
                    v.extend_from_slice(&s.end.to_be_bytes());
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
            shift_features(mol, *at, k as i64);
        }
        OpKind::DeleteRange { start, len } => {
            if *start < 1 || *len == 0 || start + len - 1 > n {
                return Err(OpError::OutOfRange {
                    what: "deletion",
                    at: *start,
                    len: n,
                });
            }
            let a = (*start - 1) as usize;
            let b = a + *len as usize;
            mol.seq.drain(a..b);
            shift_features(mol, *start, -(*len as i64));
        }
        OpKind::ReplaceRange { start, len, seq } => {
            if *start < 1 || start + len - 1 > n {
                return Err(OpError::OutOfRange {
                    what: "replacement",
                    at: *start,
                    len: n,
                });
            }
            let a = (*start - 1) as usize;
            let b = a + *len as usize;
            mol.seq.splice(a..b, seq.bytes());
            shift_features(mol, *start, seq.len() as i64 - *len as i64);
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
            let n = mol.len();
            mol.seq = crate::reverse_complement(&mol.seq);
            // Everything flips end for end, and each feature changes strand.
            for f in &mut mol.features {
                for s in &mut f.segments {
                    let (a, b) = (s.start, s.end);
                    s.start = n - b + 1;
                    s.end = n - a + 1;
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
                    s.start = n - b + 1;
                    s.end = n - a + 1;
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
    Ok(())
}

/// Move every feature coordinate at or after `from` by `delta`.
///
/// A segment that spanned the edited region grows or shrinks with it; one
/// entirely before it is untouched.
fn shift_features(mol: &mut Molecule, from: u64, delta: i64) {
    let adjust = |p: u64| -> u64 {
        if p < from {
            p
        } else {
            (p as i64 + delta).max(from as i64 - 1).max(0) as u64
        }
    };
    for f in &mut mol.features {
        for s in &mut f.segments {
            s.start = adjust(s.start);
            s.end = adjust(s.end);
        }
    }
    for p in &mut mol.primers {
        for s in &mut p.sites {
            s.start = adjust(s.start);
            s.end = adjust(s.end);
        }
    }
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

    /// Perform an operation, recording it.
    ///
    /// If the cursor is not at the tip — that is, something was undone — this
    /// creates a *branch*. The undone operations remain in the log and can
    /// still be reached. Nothing is discarded, ever.
    pub fn apply(&mut self, kind: OpKind, actor: &str) -> Result<&Molecule, OpError> {
        // Try it on a copy first, so a rejected operation leaves no trace.
        let mut next = self.current.clone();
        apply(&mut next, &kind)?;

        let id = derive_id(self.cursor, &kind);
        if !self.by_id.contains_key(&id) {
            self.by_id.insert(id, self.ops.len());
            self.ops.push(Op {
                id,
                parent: self.cursor,
                kind,
                actor: actor.to_string(),
            });
            self.children.entry(self.cursor).or_default().push(id);
        }

        self.cursor = Some(id);
        self.current = next;
        if self.depth() % SNAPSHOT_EVERY == 0 {
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
        self.children
            .iter()
            .filter(|(k, v)| v.len() > 1 && k.is_some())
            .filter_map(|(k, _)| *k)
            .collect()
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
}
