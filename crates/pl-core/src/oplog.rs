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
    /// exactly the class of loss ADR-2 exists to prevent.
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
            OpError::IdCollision { id } => write!(
                f,
                "operation id {id} is already used by a different operation;                  refusing rather than losing the edit"
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
            remap_annotations(mol, *start, *len, 0);
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
            mol.seq = crate::reverse_complement(&mol.seq);
            // A coordinate already past the end cannot be reflected onto a
            // real position. Saturating rather than wrapping keeps the
            // *existing* problem visible to `validate()` instead of turning it
            // into a fresh one at 18446744073709550634 — and the operation-log
            // gate only refuses newly introduced problems, so a wrapped value
            // would have committed.
            let flip = |p: u64| -> u64 { n.saturating_sub(p).saturating_add(1) };
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
fn remap_annotations(mol: &mut Molecule, start: u64, old_len: u64, new_len: u64) {
    let old_end = start + old_len; // one past the edited region
    let delta = new_len as i64 - old_len as i64;

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

    let map_start = |p: u64| -> Option<u64> {
        if p < start {
            Some(p)
        } else if p >= old_end {
            Some((p as i64 + delta) as u64)
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
            Some((p as i64 + delta) as u64)
        } else if interior_survives {
            Some(p)
        } else {
            None
        }
    };

    let remap = |s_start: &mut u64, s_end: &mut u64| -> bool {
        // A segment straddling the edit keeps whichever ends survive.
        // A segment whose far end survives the edit keeps its near end pinned
        // to where the replaced region begins.
        let a = map_start(*s_start).or_else(|| (*s_end >= old_end).then_some(start));
        let b = map_end(*s_end).or_else(|| (*s_start < start).then_some(start - 1));
        match (a, b) {
            (Some(a), Some(b)) if b >= a && a >= 1 => {
                *s_start = a;
                *s_end = b;
                true
            }
            _ => false,
        }
    };

    for f in &mut mol.features {
        f.segments.retain_mut(|s| remap(&mut s.start, &mut s.end));
    }
    // A feature with nothing left to point at is not a feature.
    mol.features.retain(|f| !f.segments.is_empty());

    for p in &mut mol.primers {
        p.sites.retain_mut(|s| remap(&mut s.start, &mut s.end));
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

        // ...and refuse it if the result would not describe itself correctly.
        // Only *new* problems count: a file that arrived with a bad coordinate
        // should still be editable, and blaming the user's edit for the
        // importer's mess would be both wrong and infuriating.
        let before = self.current.validate();
        let after = next.validate();
        // Compare problems by KIND, counting them.
        //
        // Two things were wrong with `if after.len() > before.len()`. It let an
        // edit that *swapped* one problem for another straight through, because
        // one is not greater than one — which is how a reverse-complement could
        // trade a `PastEnd` for a wrapped `Inverted` and commit it. And the
        // `!before.contains(x)` inside compares `Invalid` values, which cannot
        // work: every variant embeds data that moves under ordinary editing
        // (`PastEnd` carries the molecule length; `what` carries the feature
        // index and name), so deleting that guard refuses a length-changing
        // insert, a feature removal, and even a rename, on any file that
        // arrived with a bad coordinate.
        //
        // Counts per kind are stable under all of those and still catch a
        // genuinely new problem. See [`crate::Invalid::kind`].
        let tally = |v: &[crate::Invalid]| {
            let mut m: std::collections::BTreeMap<&'static str, usize> = Default::default();
            for x in v {
                *m.entry(x.kind()).or_default() += 1;
            }
            m
        };
        let (was, now) = (tally(&before), tally(&after));
        let worsened: Vec<&'static str> = now
            .iter()
            .filter(|(k, n)| **n > was.get(*k).copied().unwrap_or(0))
            .map(|(k, _)| *k)
            .collect();
        if !worsened.is_empty() {
            let fresh: Vec<_> = after
                .into_iter()
                .filter(|x| worsened.contains(&x.kind()))
                .collect();
            return Err(OpError::WouldCorrupt(fresh));
        }

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
}
