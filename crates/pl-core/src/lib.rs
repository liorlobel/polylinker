//! Polylinker's sequence model.
//!
//! No I/O and no dependencies: everything that could be wrong *about a
//! molecule* lives here, so it can be tested without a GUI, a file, or a
//! network. Formats are somebody else's crate's problem.
//!
//! # Coordinates
//!
//! The model is **1-based inclusive**, matching both GenBank and the SnapGene
//! container, so neither reader has to shift on the way in. Conversion to
//! 0-based half-open happens only where an external convention demands it,
//! and is always spelled out at that site.

pub mod base64;
pub mod iupac;
pub mod oplog;
pub mod seguid;
pub mod sha1;
pub mod translate;

pub use iupac::{complement, matches, reverse_complement, Composition};
pub use oplog::{OpKind, OpLog};
pub use seguid::{cdseguid, csseguid, ldseguid, lsseguid};

/// Whether the molecule's ends join.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Topology {
    #[default]
    Linear,
    Circular,
}

impl Topology {
    pub fn is_circular(self) -> bool {
        matches!(self, Topology::Circular)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Topology::Linear => "linear",
            Topology::Circular => "circular",
        }
    }
}

/// Which strand a feature is annotated on.
///
/// `Unoriented` is a real state that SnapGene stores and GenBank cannot
/// express — a GenBank feature is either plain or wrapped in `complement()`.
/// Modelling it honestly means the loss is visible at the boundary where it
/// happens instead of being silently invented earlier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strand {
    #[default]
    Forward,
    Reverse,
    Unoriented,
    Both,
}

impl Strand {
    /// SnapGene's `directionality` attribute.
    pub fn from_directionality(d: Option<u32>) -> Self {
        match d {
            Some(1) => Strand::Forward,
            Some(2) => Strand::Reverse,
            Some(3) => Strand::Both,
            _ => Strand::Unoriented,
        }
    }
    pub fn to_directionality(self) -> Option<u32> {
        match self {
            Strand::Forward => Some(1),
            Strand::Reverse => Some(2),
            Strand::Both => Some(3),
            Strand::Unoriented => None,
        }
    }
    pub fn is_reverse(self) -> bool {
        matches!(self, Strand::Reverse)
    }
}

/// One contiguous span of a feature. 1-based inclusive.
///
/// A feature with more than one segment is a join: an intron-split CDS, or a
/// span that crosses the origin of a circular molecule.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start: u64,
    pub end: u64,
    pub color: Option<String>,
    pub translated: bool,
    pub kind: String,
}

impl Segment {
    pub fn new(start: u64, end: u64) -> Self {
        Segment {
            start,
            end,
            color: None,
            translated: false,
            kind: "standard".into(),
        }
    }
    /// Number of bases covered. Zero if the span is inverted.
    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start).saturating_add(1)
    }
    pub fn is_empty(&self) -> bool {
        self.end < self.start
    }
}

/// An annotation on the molecule.
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    pub name: String,
    /// GenBank feature key: `CDS`, `promoter`, `misc_feature`, ...
    pub kind: String,
    pub strand: Strand,
    pub segments: Vec<Segment>,
    /// Ordered and allowed to repeat, because GenBank qualifiers are.
    pub qualifiers: Vec<(String, String)>,
}

impl Feature {
    pub fn new(name: impl Into<String>, kind: impl Into<String>) -> Self {
        Feature {
            name: name.into(),
            kind: kind.into(),
            strand: Strand::Forward,
            segments: Vec::new(),
            qualifiers: Vec::new(),
        }
    }

    /// Lowest coordinate across all segments.
    pub fn start(&self) -> u64 {
        self.segments.iter().map(|s| s.start).min().unwrap_or(0)
    }
    /// Highest coordinate across all segments.
    pub fn end(&self) -> u64 {
        self.segments.iter().map(|s| s.end).max().unwrap_or(0)
    }
    /// First colour any segment carries.
    pub fn color(&self) -> Option<&str> {
        self.segments.iter().find_map(|s| s.color.as_deref())
    }
    pub fn qualifier(&self, key: &str) -> Option<&str> {
        self.qualifiers
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
    pub fn set_qualifier(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.qualifiers.push((key.into(), value.into()));
    }
}

/// Where a primer anneals.
#[derive(Debug, Clone, PartialEq)]
pub struct BindingSite {
    pub start: u64,
    pub end: u64,
    pub strand: Strand,
    /// As recorded by the source file. Recomputing it is `pl-thermo`'s job,
    /// and the two are deliberately kept apart so a disagreement is visible.
    pub tm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Primer {
    pub name: String,
    pub seq: String,
    pub description: String,
    pub sites: Vec<BindingSite>,
}

/// Methylation state, which blocks some restriction enzymes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Methylation {
    pub dam: bool,
    pub dcm: bool,
    pub ecoki: bool,
}

/// A coordinate that does not describe anything real.
///
/// See [`Molecule::validate`] for why these are possible at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    /// `end < start`. In this model a span that crosses the origin is written
    /// as two segments, so an inverted one is a mistake rather than a wrap.
    Inverted { what: String, start: u64, end: u64 },
    /// Coordinates are 1-based, so there is no position 0.
    ZeroStart { what: String },
    /// The coordinate is past the end of the molecule.
    PastEnd { what: String, end: u64, len: u64 },
    /// A feature that annotates nowhere.
    FeatureWithoutSegments { index: usize, name: String },
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Invalid::Inverted { what, start, end } => {
                write!(f, "{what}: {start}..{end} ends before it starts")
            }
            Invalid::ZeroStart { what } => {
                write!(f, "{what}: starts at 0, but coordinates are 1-based")
            }
            Invalid::PastEnd { what, end, len } => {
                write!(f, "{what}: ends at {end}, past the {len} bp molecule")
            }
            Invalid::FeatureWithoutSegments { index, name } => {
                write!(f, "feature {index} '{name}': has no segments")
            }
        }
    }
}

/// A nucleic-acid molecule with its annotations.
#[derive(Debug, Clone, Default)]
pub struct Molecule {
    pub name: String,
    pub description: String,
    /// Raw bases, **case preserved**. Not guaranteed to be valid IUPAC.
    pub seq: Vec<u8>,
    pub topology: Topology,
    /// `None` when the source does not record it — which is the normal case for
    /// GenBank and always the case for FASTA.
    ///
    /// Defaulting this to `false` made the viewer assert "single-stranded" about
    /// ordinary plasmids, and defaulting to `true` would invent information just
    /// as confidently. Unknown is a real third state and callers should say so.
    pub double_stranded: Option<bool>,
    pub methylation: Methylation,
    pub features: Vec<Feature>,
    pub primers: Vec<Primer>,
    pub notes: Vec<(String, String)>,
    /// Length claimed by the source when the bases themselves are absent.
    ///
    /// Annotation-only GenBank is real and common: `ORIGIN` immediately
    /// followed by `//`, with the length declared only on the LOCUS line.
    /// Recording it separately keeps "we have 2.9 Mb of bases" distinct from
    /// "the file says 2.9 Mb but shipped none", which is a distinction other
    /// tools blur.
    pub declared_len: Option<u64>,
}

impl Molecule {
    /// Number of bases actually present.
    pub fn len(&self) -> u64 {
        self.seq.len() as u64
    }
    pub fn is_empty(&self) -> bool {
        self.seq.is_empty()
    }
    /// The span annotations are drawn against: real bases if we have them,
    /// otherwise the length the file declared.
    pub fn span(&self) -> u64 {
        if self.seq.is_empty() {
            self.declared_len.unwrap_or(0)
        } else {
            self.len()
        }
    }
    /// True when the file described a molecule but carried no bases.
    pub fn sequence_absent(&self) -> bool {
        self.seq.is_empty() && self.declared_len.unwrap_or(0) > 0
    }

    /// True for a standalone annotation track: features, but neither bases nor
    /// a declared length. UGENE and SnapGene both export these — a set of
    /// coordinates meant to be applied to a sequence held somewhere else.
    pub fn is_annotation_track(&self) -> bool {
        self.seq.is_empty() && self.declared_len.unwrap_or(0) == 0 && !self.features.is_empty()
    }

    /// A span to draw annotations against, inferring one from the features
    /// when the file supplied neither bases nor a length.
    ///
    /// Separate from [`span`](Self::span) because this one *infers*. Anything
    /// writing a file header must use `span`, which never invents a number.
    pub fn annotation_span(&self) -> u64 {
        let s = self.span();
        if s > 0 {
            return s;
        }
        self.features.iter().map(Feature::end).max().unwrap_or(0)
    }
    pub fn composition(&self) -> Composition {
        Composition::of(&self.seq)
    }
    pub fn gc_percent(&self) -> Option<f64> {
        self.composition().gc_percent()
    }

    /// Bases in `[start, end]`, 1-based inclusive, wrapping the origin when
    /// the molecule is circular. Returns `None` if the range is unusable.
    pub fn subseq(&self, start: u64, end: u64) -> Option<Vec<u8>> {
        let n = self.len();
        if n == 0 || start == 0 || start > n {
            return None;
        }
        if end >= start {
            if end > n {
                return None;
            }
            return Some(self.seq[(start - 1) as usize..end as usize].to_vec());
        }
        // end < start: only meaningful as an origin-crossing span.
        if !self.topology.is_circular() || end == 0 {
            return None;
        }
        let mut v = self.seq[(start - 1) as usize..].to_vec();
        v.extend_from_slice(&self.seq[..end as usize]);
        Some(v)
    }

    /// Check every coordinate against the molecule it describes.
    ///
    /// This exists because of a deliberate trade recorded in `docs/PLAN.md`
    /// §5.3.1. Coordinates here are 1-based inclusive `{start, end}`, chosen so
    /// that conversion to and from GenBank and the SnapGene container is the
    /// identity. The cost is that an invalid interval is *constructible* —
    /// `end < start`, `start == 0`, `end` past the sequence — where the
    /// alternative `{start, length}` representation would have made it
    /// unrepresentable. That safety has to be bought back explicitly, and this
    /// is where it is bought.
    ///
    /// Note what is *not* an error: a feature made of several segments. That is
    /// how both source formats express an intron, a fusion, and a span crossing
    /// the origin.
    pub fn validate(&self) -> Vec<Invalid> {
        let mut out = Vec::new();
        // Annotations are checked against the span they annotate, which for a
        // file carrying no bases is the length it declares.
        let n = self.annotation_span();

        for (i, f) in self.features.iter().enumerate() {
            if f.segments.is_empty() {
                out.push(Invalid::FeatureWithoutSegments {
                    index: i,
                    name: f.name.clone(),
                });
                continue;
            }
            for (j, s) in f.segments.iter().enumerate() {
                if s.start == 0 {
                    out.push(Invalid::ZeroStart {
                        what: format!("feature {i} '{}' segment {j}", f.name),
                    });
                }
                if s.end < s.start {
                    out.push(Invalid::Inverted {
                        what: format!("feature {i} '{}' segment {j}", f.name),
                        start: s.start,
                        end: s.end,
                    });
                }
                if n > 0 && s.end > n {
                    out.push(Invalid::PastEnd {
                        what: format!("feature {i} '{}' segment {j}", f.name),
                        end: s.end,
                        len: n,
                    });
                }
            }
        }

        for (i, p) in self.primers.iter().enumerate() {
            for (j, s) in p.sites.iter().enumerate() {
                if s.start == 0 {
                    out.push(Invalid::ZeroStart {
                        what: format!("primer {i} '{}' site {j}", p.name),
                    });
                }
                if s.end < s.start {
                    out.push(Invalid::Inverted {
                        what: format!("primer {i} '{}' site {j}", p.name),
                        start: s.start,
                        end: s.end,
                    });
                }
                if n > 0 && s.end > n {
                    out.push(Invalid::PastEnd {
                        what: format!("primer {i} '{}' site {j}", p.name),
                        end: s.end,
                        len: n,
                    });
                }
            }
        }
        out
    }

    /// True when nothing in [`validate`](Self::validate) objects.
    pub fn is_valid(&self) -> bool {
        self.validate().is_empty()
    }

    /// Rotate a circular molecule so that 1-based position `origin` becomes 1,
    /// moving every annotation with it. No-op on a linear molecule.
    pub fn rotate(&mut self, origin: u64) -> bool {
        let n = self.len();
        if !self.topology.is_circular() || n == 0 || origin == 0 || origin > n {
            return false;
        }
        let shift = origin - 1;
        if shift == 0 {
            return true;
        }
        self.seq.rotate_left(shift as usize);
        let remap = |p: u64| -> u64 { ((p - 1 + n - shift) % n) + 1 };
        for f in &mut self.features {
            for s in &mut f.segments {
                s.start = remap(s.start);
                s.end = remap(s.end);
            }
        }
        for p in &mut self.primers {
            for s in &mut p.sites {
                s.start = remap(s.start);
                s.end = remap(s.end);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circular(seq: &[u8]) -> Molecule {
        Molecule {
            seq: seq.to_vec(),
            topology: Topology::Circular,
            ..Default::default()
        }
    }

    #[test]
    fn subseq_is_one_based_inclusive() {
        let m = circular(b"ACGTACGT");
        assert_eq!(m.subseq(1, 4).unwrap(), b"ACGT".to_vec());
        assert_eq!(m.subseq(4, 4).unwrap(), b"T".to_vec());
        assert!(m.subseq(0, 4).is_none(), "position 0 does not exist");
        assert!(m.subseq(1, 9).is_none(), "runs off the end");
    }

    #[test]
    fn subseq_wraps_only_when_circular() {
        let c = circular(b"ACGTACGT");
        assert_eq!(c.subseq(7, 2).unwrap(), b"GTAC".to_vec());
        let mut l = c.clone();
        l.topology = Topology::Linear;
        assert!(l.subseq(7, 2).is_none());
    }

    #[test]
    fn rotate_moves_sequence_and_annotations_together() {
        let mut m = circular(b"AAAACCCCGGGGTTTT");
        let mut f = Feature::new("gg", "misc_feature");
        f.segments.push(Segment::new(9, 12)); // the GGGG
        m.features.push(f);

        assert_eq!(m.subseq(9, 12).unwrap(), b"GGGG".to_vec());
        assert!(m.rotate(9));
        assert_eq!(&m.seq[..4], b"GGGG");
        // the annotation followed the bases it describes
        let s = &m.features[0].segments[0];
        assert_eq!((s.start, s.end), (1, 4));
        assert_eq!(m.subseq(s.start, s.end).unwrap(), b"GGGG".to_vec());
    }

    #[test]
    fn rotation_that_crosses_the_origin_stays_consistent() {
        let mut m = circular(b"AAAACCCCGGGGTTTT");
        let mut f = Feature::new("wrap", "misc_feature");
        f.segments.push(Segment::new(15, 16));
        f.segments.push(Segment::new(1, 2));
        m.features.push(f);
        assert!(m.rotate(5));
        let segs = &m.features[0].segments;
        assert_eq!((segs[0].start, segs[0].end), (11, 12));
        assert_eq!((segs[1].start, segs[1].end), (13, 14));
    }

    #[test]
    fn validate_accepts_a_sound_molecule() {
        let mut m = Molecule {
            seq: b"ACGTACGTACGT".to_vec(),
            ..Default::default()
        };
        let mut f = Feature::new("ok", "CDS");
        f.segments.push(Segment::new(1, 6));
        f.segments.push(Segment::new(9, 12)); // a join, which is fine
        m.features.push(f);
        assert!(m.is_valid(), "{:?}", m.validate());
    }

    #[test]
    fn validate_catches_what_the_other_representation_would_have_prevented() {
        // Each of these is impossible to express as {start, length} mod L, and
        // constructible here. That trade is recorded in PLAN 5.3.1.
        let base = Molecule {
            seq: b"ACGTACGTACGT".to_vec(),
            ..Default::default()
        };

        let mut inverted = base.clone();
        let mut f = Feature::new("backwards", "CDS");
        f.segments.push(Segment::new(9, 4));
        inverted.features.push(f);
        assert!(matches!(
            inverted.validate().as_slice(),
            [Invalid::Inverted {
                start: 9,
                end: 4,
                ..
            }]
        ));

        let mut zero = base.clone();
        let mut f = Feature::new("zero", "CDS");
        f.segments.push(Segment::new(0, 4));
        zero.features.push(f);
        assert!(matches!(
            zero.validate().as_slice(),
            [Invalid::ZeroStart { .. }]
        ));

        let mut past = base.clone();
        let mut f = Feature::new("past", "CDS");
        f.segments.push(Segment::new(4, 999));
        past.features.push(f);
        assert!(matches!(
            past.validate().as_slice(),
            [Invalid::PastEnd {
                end: 999,
                len: 12,
                ..
            }]
        ));

        let mut empty = base.clone();
        empty.features.push(Feature::new("nowhere", "CDS"));
        assert!(matches!(
            empty.validate().as_slice(),
            [Invalid::FeatureWithoutSegments { .. }]
        ));
    }

    #[test]
    fn an_annotation_only_file_is_checked_against_its_declared_span() {
        // No bases, so there is nothing to compare against except the length
        // the file claims. A feature inside that span is fine.
        let mut m = Molecule {
            declared_len: Some(1000),
            ..Default::default()
        };
        let mut f = Feature::new("gene", "CDS");
        f.segments.push(Segment::new(100, 400));
        m.features.push(f);
        assert!(m.is_valid(), "{:?}", m.validate());

        let mut past = m.clone();
        past.features[0].segments[0].end = 2000;
        assert!(!past.is_valid());
    }

    #[test]
    fn primer_sites_are_checked_too() {
        let mut m = Molecule {
            seq: b"ACGTACGTACGT".to_vec(),
            ..Default::default()
        };
        m.primers.push(Primer {
            name: "p".into(),
            seq: "ACGT".into(),
            description: String::new(),
            sites: vec![BindingSite {
                start: 10,
                end: 3,
                strand: Strand::Forward,
                tm: None,
            }],
        });
        assert!(matches!(
            m.validate().as_slice(),
            [Invalid::Inverted { .. }]
        ));
    }

    #[test]
    fn declared_length_is_not_confused_with_real_bases() {
        let m = Molecule {
            declared_len: Some(2_944_528),
            ..Default::default()
        };
        assert_eq!(m.len(), 0);
        assert_eq!(m.span(), 2_944_528);
        assert!(m.sequence_absent());
        assert_eq!(m.gc_percent(), None);
    }

    #[test]
    fn unoriented_strand_survives_the_model() {
        assert_eq!(Strand::from_directionality(None), Strand::Unoriented);
        assert_eq!(Strand::from_directionality(Some(2)), Strand::Reverse);
        assert_eq!(Strand::Unoriented.to_directionality(), None);
    }
}
