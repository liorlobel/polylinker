//! The SnapGene `.dna` container.
//!
//! Written from the empirical specification in `docs/DNA-FORMAT.md`, which was
//! derived from black-box observation of byte layout. No vendor code was
//! disassembled and no vendor database is reproduced. See `PROVENANCE.md`.
//!
//! # Layout
//!
//! A flat, ordered stream of blocks:
//!
//! ```text
//! block := type:u8  length:u32be  payload[length]
//! ```
//!
//! The first block must be type 9, whose payload begins `"SnapGene"` followed
//! by three big-endian u16: file type, export version, import version.
//!
//! Blocks 2 and 3 are caches of (sequence x enzyme set) and are pure functions
//! of data held elsewhere in the file. They are ~78% of a typical file and a
//! writer may regenerate or omit them.

use pl_core::{BindingSite, Feature, Methylation, Molecule, Primer, Segment, Strand, Topology};

use crate::xml::{self, Event};

pub const MAGIC: &[u8; 8] = b"SnapGene";

pub mod block {
    pub const SEQUENCE: u8 = 0;
    pub const CUTSITE_CACHE: u8 = 2;
    pub const ENZYME_TABLE: u8 = 3;
    pub const PRIMERS: u8 = 5;
    pub const NOTES: u8 = 6;
    pub const HISTORY_TREE: u8 = 7;
    pub const EXTRA_PROPS: u8 = 8;
    pub const HEADER: u8 = 9;
    pub const FEATURES: u8 = 10;
    pub const HISTORY_NODE: u8 = 11;
}

/// Blocks that are pure caches and can be regenerated from the rest of the file.
pub const DERIVED: &[u8] = &[block::CUTSITE_CACHE, block::ENZYME_TABLE];

pub mod flag {
    pub const CIRCULAR: u8 = 0x01;
    pub const DOUBLE_STRANDED: u8 = 0x02;
    pub const DAM: u8 = 0x04;
    pub const DCM: u8 = 0x08;
    pub const ECOKI: u8 = 0x10;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Empty,
    TruncatedHeader {
        offset: usize,
    },
    ShortBlock {
        kind: u8,
        offset: usize,
        claimed: u32,
        available: usize,
    },
    FirstBlockNotHeader {
        kind: u8,
    },
    MissingMagic,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Empty => write!(f, "empty file"),
            Error::TruncatedHeader { offset } => {
                write!(f, "truncated block header at offset {offset}")
            }
            Error::ShortBlock { kind, offset, claimed, available } => write!(
                f,
                "block type {kind} at offset {offset} claims {claimed} bytes, only {available} remain"
            ),
            Error::FirstBlockNotHeader { kind } => {
                write!(f, "first block is type {kind}, expected 9")
            }
            Error::MissingMagic => write!(f, "missing 'SnapGene' magic in header block"),
        }
    }
}

impl std::error::Error for Error {}

/// A raw block, retained verbatim so unknown types survive a rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: u8,
    pub payload: Vec<u8>,
}

impl Block {
    pub fn size_on_disk(&self) -> usize {
        5 + self.payload.len()
    }
    pub fn is_derived(&self) -> bool {
        DERIVED.contains(&self.kind)
    }
}

/// A parsed document: the molecule plus everything needed to write the file back.
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub molecule: Molecule,
    pub file_type: u16,
    pub export_version: u16,
    pub import_version: u16,
    /// Every block as read, in order. Keeping these is what makes a byte-exact
    /// rewrite possible without having to understand all of them.
    pub blocks: Vec<Block>,
    /// Present and xz-compressed in modern files; we detect it but do not
    /// inflate it, and say so rather than pretending the provenance is gone.
    pub history_present: bool,
    pub history_compressed: bool,
}

impl Document {
    pub fn total_bytes(&self) -> usize {
        self.blocks.iter().map(Block::size_on_disk).sum()
    }
    pub fn derived_bytes(&self) -> usize {
        self.blocks
            .iter()
            .filter(|b| b.is_derived())
            .map(Block::size_on_disk)
            .sum()
    }
}

/// Split the byte stream into blocks, validating framing as we go.
pub fn read_blocks(data: &[u8]) -> Result<Vec<Block>, Error> {
    let n = data.len();
    let mut pos = 0usize;
    let mut blocks = Vec::new();
    let mut first = true;

    while pos < n {
        if pos + 5 > n {
            return Err(Error::TruncatedHeader { offset: pos });
        }
        let kind = data[pos];
        let len = u32::from_be_bytes([data[pos + 1], data[pos + 2], data[pos + 3], data[pos + 4]]);
        let body = pos + 5;
        let len_us = len as usize;
        if body + len_us > n {
            return Err(Error::ShortBlock {
                kind,
                offset: pos,
                claimed: len,
                available: n - body,
            });
        }
        if first {
            if kind != block::HEADER {
                return Err(Error::FirstBlockNotHeader { kind });
            }
            if !data[body..].starts_with(MAGIC) {
                return Err(Error::MissingMagic);
            }
            first = false;
        }
        blocks.push(Block {
            kind,
            payload: data[body..body + len_us].to_vec(),
        });
        pos = body + len_us;
    }

    if first {
        return Err(Error::Empty);
    }
    Ok(blocks)
}

pub fn write_blocks(blocks: &[Block]) -> Vec<u8> {
    let mut out = Vec::with_capacity(blocks.iter().map(Block::size_on_disk).sum());
    for b in blocks {
        out.push(b.kind);
        out.extend_from_slice(&(b.payload.len() as u32).to_be_bytes());
        out.extend_from_slice(&b.payload);
    }
    out
}

pub fn parse(data: &[u8]) -> Result<Document, Error> {
    let blocks = read_blocks(data)?;
    let mut doc = Document {
        blocks,
        ..Default::default()
    };
    doc.molecule.topology = Topology::Linear;

    for i in 0..doc.blocks.len() {
        let kind = doc.blocks[i].kind;
        let payload = doc.blocks[i].payload.clone();
        match kind {
            block::HEADER if payload.len() >= 14 => {
                doc.file_type = u16::from_be_bytes([payload[8], payload[9]]);
                doc.export_version = u16::from_be_bytes([payload[10], payload[11]]);
                doc.import_version = u16::from_be_bytes([payload[12], payload[13]]);
            }
            block::SEQUENCE if !payload.is_empty() => {
                let flags = payload[0];
                doc.molecule.topology = if flags & flag::CIRCULAR != 0 {
                    Topology::Circular
                } else {
                    Topology::Linear
                };
                doc.molecule.double_stranded = flags & flag::DOUBLE_STRANDED != 0;
                doc.molecule.methylation = Methylation {
                    dam: flags & flag::DAM != 0,
                    dcm: flags & flag::DCM != 0,
                    ecoki: flags & flag::ECOKI != 0,
                };
                // Bases are stored as ASCII. Case is preserved deliberately.
                doc.molecule.seq = payload[1..].to_vec();
            }
            block::FEATURES => {
                doc.molecule.features = parse_features(&String::from_utf8_lossy(&payload));
            }
            block::PRIMERS => {
                doc.molecule.primers = parse_primers(&String::from_utf8_lossy(&payload));
            }
            block::NOTES => {
                doc.molecule.notes = parse_notes(&String::from_utf8_lossy(&payload));
            }
            block::HISTORY_TREE => {
                doc.history_present = true;
                doc.history_compressed = payload.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]);
            }
            _ => {}
        }
    }

    if let Some(name) = doc.molecule.notes.iter().find(|(k, _)| k == "Description") {
        doc.molecule.description = name.1.clone();
    }
    Ok(doc)
}

fn parse_features(x: &str) -> Vec<Feature> {
    let mut out: Vec<Feature> = Vec::new();
    let mut cur: Option<Feature> = None;
    let mut q_name: Option<String> = None;

    for ev in xml::scan(x) {
        match ev {
            Event::Open {
                name,
                attrs,
                self_closing,
            } => match name.as_str() {
                "Feature" => {
                    if let Some(f) = cur.take() {
                        out.push(f);
                    }
                    let dir = Event::attr(&attrs, "directionality").and_then(|d| d.parse().ok());
                    cur = Some(Feature {
                        name: Event::attr(&attrs, "name").unwrap_or_default().to_string(),
                        kind: Event::attr(&attrs, "type")
                            .unwrap_or("misc_feature")
                            .to_string(),
                        strand: Strand::from_directionality(dir),
                        segments: Vec::new(),
                        qualifiers: Vec::new(),
                    });
                    if self_closing {
                        if let Some(f) = cur.take() {
                            out.push(f);
                        }
                    }
                }
                "Segment" => {
                    if let (Some(f), Some(range)) = (cur.as_mut(), Event::attr(&attrs, "range")) {
                        if let Some((a, b)) = range.split_once('-') {
                            if let (Ok(start), Ok(end)) = (a.trim().parse(), b.trim().parse()) {
                                f.segments.push(Segment {
                                    start,
                                    end,
                                    color: Event::attr(&attrs, "color").map(str::to_string),
                                    translated: Event::attr(&attrs, "translated") == Some("1"),
                                    kind: Event::attr(&attrs, "type")
                                        .unwrap_or("standard")
                                        .to_string(),
                                });
                            }
                        }
                    }
                }
                "Q" => q_name = Event::attr(&attrs, "name").map(str::to_string),
                "V" => {
                    if let (Some(f), Some(k)) = (cur.as_mut(), q_name.as_ref()) {
                        // A qualifier value arrives as whichever typed attribute fits.
                        let v = Event::attr(&attrs, "text")
                            .or_else(|| Event::attr(&attrs, "int"))
                            .or_else(|| Event::attr(&attrs, "predef"))
                            .unwrap_or_default();
                        f.qualifiers.push((k.clone(), v.to_string()));
                    }
                }
                _ => {}
            },
            Event::Close { name } => {
                if name == "Feature" {
                    if let Some(f) = cur.take() {
                        out.push(f);
                    }
                } else if name == "Q" {
                    q_name = None;
                }
            }
            Event::Text(_) => {}
        }
    }
    if let Some(f) = cur.take() {
        out.push(f);
    }
    out
}

fn parse_primers(x: &str) -> Vec<Primer> {
    let mut out: Vec<Primer> = Vec::new();
    let mut cur: Option<Primer> = None;

    for ev in xml::scan(x) {
        match ev {
            Event::Open {
                name,
                attrs,
                self_closing,
            } => match name.as_str() {
                "Primer" => {
                    if let Some(p) = cur.take() {
                        out.push(p);
                    }
                    cur = Some(Primer {
                        name: Event::attr(&attrs, "name").unwrap_or_default().to_string(),
                        seq: Event::attr(&attrs, "sequence")
                            .unwrap_or_default()
                            .to_string(),
                        description: Event::attr(&attrs, "description")
                            .unwrap_or_default()
                            .to_string(),
                        sites: Vec::new(),
                    });
                    if self_closing {
                        if let Some(p) = cur.take() {
                            out.push(p);
                        }
                    }
                }
                "BindingSite" => {
                    // The "simplified" entry duplicates the detailed one.
                    if Event::attr(&attrs, "simplified") == Some("1") {
                        continue;
                    }
                    if let (Some(p), Some(loc)) = (cur.as_mut(), Event::attr(&attrs, "location")) {
                        if let Some((a, b)) = loc.split_once('-') {
                            if let (Ok(start), Ok(end)) = (a.trim().parse(), b.trim().parse()) {
                                let rev = Event::attr(&attrs, "boundStrand") == Some("1");
                                p.sites.push(BindingSite {
                                    start,
                                    end,
                                    strand: if rev {
                                        Strand::Reverse
                                    } else {
                                        Strand::Forward
                                    },
                                    tm: Event::attr(&attrs, "meltingTemperature")
                                        .and_then(|t| t.parse().ok()),
                                });
                            }
                        }
                    }
                }
                _ => {}
            },
            Event::Close { name } if name == "Primer" => {
                if let Some(p) = cur.take() {
                    out.push(p);
                }
            }
            _ => {}
        }
    }
    if let Some(p) = cur.take() {
        out.push(p);
    }
    out
}

fn parse_notes(x: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    for ev in xml::scan(x) {
        match ev {
            Event::Open {
                name, self_closing, ..
            } => {
                if !self_closing {
                    stack.push(name);
                }
            }
            Event::Close { .. } => {
                stack.pop();
            }
            Event::Text(t) => {
                // Direct children of <Notes> only; depth 2 is <Notes><Key>.
                if stack.len() == 2 {
                    out.push((stack[1].clone(), t.trim().to_string()));
                }
            }
        }
    }
    out
}

/// Serialize back to `.dna`.
///
/// With the original blocks retained this is byte-exact. `drop_derived` omits
/// the regenerable caches, which is what a writer that has not yet implemented
/// cut-site computation should do rather than emitting a stale cache.
pub fn write(doc: &Document, drop_derived: bool) -> Vec<u8> {
    if drop_derived {
        let kept: Vec<Block> = doc
            .blocks
            .iter()
            .filter(|b| !b.is_derived())
            .cloned()
            .collect();
        write_blocks(&kept)
    } else {
        write_blocks(&doc.blocks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_block() -> Vec<u8> {
        let mut p = MAGIC.to_vec();
        p.extend_from_slice(&1u16.to_be_bytes()); // DNA
        p.extend_from_slice(&15u16.to_be_bytes());
        p.extend_from_slice(&19u16.to_be_bytes());
        p
    }

    fn build(blocks: &[(u8, Vec<u8>)]) -> Vec<u8> {
        write_blocks(
            &blocks
                .iter()
                .map(|(k, p)| Block {
                    kind: *k,
                    payload: p.clone(),
                })
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn framing_round_trips_byte_exactly() {
        let raw = build(&[
            (block::HEADER, header_block()),
            (block::SEQUENCE, {
                let mut v = vec![flag::CIRCULAR | flag::DOUBLE_STRANDED];
                v.extend_from_slice(b"ACGTacgt");
                v
            }),
        ]);
        let doc = parse(&raw).unwrap();
        assert_eq!(write(&doc, false), raw);
    }

    #[test]
    fn sequence_case_is_preserved() {
        let raw = build(&[
            (block::HEADER, header_block()),
            (block::SEQUENCE, {
                let mut v = vec![flag::DOUBLE_STRANDED];
                v.extend_from_slice(b"ACGTacgtNn");
                v
            }),
        ]);
        let doc = parse(&raw).unwrap();
        assert_eq!(doc.molecule.seq, b"ACGTacgtNn".to_vec());
    }

    #[test]
    fn topology_and_methylation_come_from_the_flag_byte() {
        let raw = build(&[
            (block::HEADER, header_block()),
            (
                block::SEQUENCE,
                vec![flag::CIRCULAR | flag::DAM | flag::ECOKI, b'A'],
            ),
        ]);
        let doc = parse(&raw).unwrap();
        assert_eq!(doc.molecule.topology, Topology::Circular);
        assert!(doc.molecule.methylation.dam);
        assert!(!doc.molecule.methylation.dcm);
        assert!(doc.molecule.methylation.ecoki);
        assert!(!doc.molecule.double_stranded);
    }

    #[test]
    fn rejects_files_that_are_not_dna() {
        assert_eq!(parse(b"").unwrap_err(), Error::Empty);
        let bogus = build(&[(block::SEQUENCE, vec![0])]);
        assert_eq!(
            parse(&bogus).unwrap_err(),
            Error::FirstBlockNotHeader { kind: 0 }
        );
        let bad_magic = build(&[(block::HEADER, b"NotSnap!......".to_vec())]);
        assert_eq!(parse(&bad_magic).unwrap_err(), Error::MissingMagic);
    }

    #[test]
    fn rejects_a_block_that_overruns_the_file() {
        // Claims 999 bytes, supplies none.
        let mut raw = vec![block::HEADER];
        raw.extend_from_slice(&999u32.to_be_bytes());
        assert!(matches!(parse(&raw), Err(Error::ShortBlock { .. })));
    }

    #[test]
    fn parses_features_with_joins_colours_and_qualifiers() {
        let xml = r##"<Features>
          <Feature name="AmpR" type="CDS" directionality="2">
            <Segment range="100-200" color="#9a5b8c" translated="1"/>
            <Segment range="300-400" color="#9a5b8c"/>
            <Q name="gene"><V text="bla"/></Q>
            <Q name="codon_start"><V int="1"/></Q>
          </Feature>
          <Feature name="oriT" type="misc_feature"><Segment range="10-20"/></Feature>
        </Features>"##;
        let f = parse_features(xml);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].name, "AmpR");
        assert_eq!(f[0].strand, Strand::Reverse);
        assert_eq!(f[0].segments.len(), 2);
        assert_eq!(f[0].start(), 100);
        assert_eq!(f[0].end(), 400);
        assert_eq!(f[0].color(), Some("#9a5b8c"));
        assert!(f[0].segments[0].translated);
        assert_eq!(f[0].qualifier("gene"), Some("bla"));
        assert_eq!(f[0].qualifier("codon_start"), Some("1"));
        // No directionality attribute means unoriented, not forward-by-default.
        assert_eq!(f[1].strand, Strand::Unoriented);
    }

    #[test]
    fn feature_labels_containing_entities_survive() {
        let f = parse_features(
            r#"<Features><Feature name="P&amp;S &lt;δ&gt;" type="CDS"><Segment range="1-9"/></Feature></Features>"#,
        );
        assert_eq!(f[0].name, "P&S <δ>");
    }

    #[test]
    fn simplified_binding_sites_are_not_double_counted() {
        let xml = r#"<Primers>
          <Primer name="M13F" sequence="GTAAAACGACGGCCAGT">
            <BindingSite location="100-116" boundStrand="0" meltingTemperature="55.3"/>
            <BindingSite location="100-116" boundStrand="0" simplified="1"/>
          </Primer>
        </Primers>"#;
        let p = parse_primers(xml);
        assert_eq!(p.len(), 1);
        assert_eq!(
            p[0].sites.len(),
            1,
            "the simplified duplicate must be dropped"
        );
        assert_eq!(p[0].sites[0].tm, Some(55.3));
        assert_eq!(p[0].sites[0].strand, Strand::Forward);
    }

    #[test]
    fn notes_read_as_ordered_key_value_pairs() {
        let n =
            parse_notes("<Notes><UUID>abc-123</UUID><Created>2026.07.26</Created><Empty/></Notes>");
        assert_eq!(
            n,
            vec![
                ("UUID".to_string(), "abc-123".to_string()),
                ("Created".to_string(), "2026.07.26".to_string()),
            ]
        );
    }

    #[test]
    fn dropping_derived_blocks_removes_only_caches() {
        let raw = build(&[
            (block::HEADER, header_block()),
            (block::SEQUENCE, vec![flag::CIRCULAR, b'A']),
            (block::CUTSITE_CACHE, vec![0; 64]),
            (block::ENZYME_TABLE, vec![0; 32]),
            (block::FEATURES, b"<Features/>".to_vec()),
        ]);
        let doc = parse(&raw).unwrap();
        assert_eq!(doc.derived_bytes(), (5 + 64) + (5 + 32));

        let slim = write(&doc, true);
        let reparsed = parse(&slim).unwrap();
        assert_eq!(reparsed.blocks.len(), 3);
        assert_eq!(reparsed.derived_bytes(), 0);
        assert_eq!(reparsed.molecule.topology, Topology::Circular);
        assert!(slim.len() < raw.len());
    }

    #[test]
    fn unknown_block_types_survive_a_rewrite() {
        let raw = build(&[
            (block::HEADER, header_block()),
            (block::SEQUENCE, vec![0, b'A']),
            (200, vec![1, 2, 3, 4]), // never seen in the wild
        ]);
        let doc = parse(&raw).unwrap();
        assert_eq!(
            write(&doc, false),
            raw,
            "unknown blocks must pass through verbatim"
        );
    }
}
