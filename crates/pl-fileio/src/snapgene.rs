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

/// Does a block body of `len` bytes starting at `body` run past the end of an
/// `n`-byte file?
///
/// A subtraction, never `body + len > n`. `usize` is 32 bits on wasm32, where a
/// declared length of 0xFFFFFFFB wraps the sum back below `n`, the guard passes,
/// and the payload slice becomes `data[5..4]` — a trap that killed the shipped
/// module on a 19-byte file. `body <= n` is guaranteed by the `TruncatedHeader`
/// check at the call site, so the subtraction cannot underflow.
///
/// Generic purely so a test can run it at **32-bit width on a 64-bit machine**.
/// Every CI runner is 64-bit and the wasm32 job runs `cargo build` rather than
/// `cargo test`, so an integer bug that only exists at 32 bits had nowhere to be
/// executed: the file-level test that claims to cover it feeds a 28-byte fixture
/// whose 64-bit sums top out at 4,294,967,319 — no wrap, no overflow, and the
/// pre-fix code returns the identical error. Only calling this at `u32` can tell
/// the two implementations apart. See
/// `the_block_length_guard_is_a_subtraction_and_cannot_wrap_at_32_bit_width`.
fn overruns<T>(n: T, body: T, len: T) -> bool
where
    T: Copy + PartialOrd + core::ops::Sub<Output = T>,
{
    len > n - body
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
        // See `overruns`: a subtraction, because the addition wrapped on wasm32.
        if overruns(n, body, len_us) {
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
                // The .dna format does record this, so it is known either way.
                doc.molecule.double_stranded = Some(flags & flag::DOUBLE_STRANDED != 0);
                doc.molecule.methylation = Methylation {
                    dam: flags & flag::DAM != 0,
                    dcm: flags & flag::DCM != 0,
                    ecoki: flags & flag::ECOKI != 0,
                    // The container has no CpG bit. False is "not recorded
                    // here", not a claim that the plasmid is unmethylated.
                    cpg: false,
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
    let mut q_had_value = false;

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
                "Q" => {
                    q_name = Event::attr(&attrs, "name").map(str::to_string);
                    q_had_value = false;
                    // A `<Q/>` that closes immediately can never carry a `<V>`,
                    // so it is a valueless qualifier and is recorded here.
                    if self_closing {
                        if let (Some(f), Some(k)) = (cur.as_mut(), q_name.as_ref()) {
                            f.qualifiers.push((k.clone(), None));
                        }
                        q_name = None;
                    }
                }
                "V" => {
                    if let (Some(f), Some(k)) = (cur.as_mut(), q_name.as_ref()) {
                        // A qualifier value arrives as whichever typed attribute fits.
                        let v = Event::attr(&attrs, "text")
                            .or_else(|| Event::attr(&attrs, "int"))
                            .or_else(|| Event::attr(&attrs, "predef"))
                            .unwrap_or_default();
                        f.qualifiers.push((k.clone(), Some(v.to_string())));
                        q_had_value = true;
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
                    // `<Q name="pseudo"></Q>` — no `<V>` ever came. That is a
                    // **valueless** qualifier, GenBank's `/pseudo`, and it is a
                    // different thing from `/replace=""`, an empty value.
                    // Dropping it turns a pseudogene into an ordinary
                    // protein-coding gene, which is exactly the bug this
                    // project already fixed once on the GenBank side; the
                    // SnapGene reader had the same hole, and writing a writer
                    // is what surfaced it.
                    if let (Some(f), Some(k)) = (cur.as_mut(), q_name.as_ref()) {
                        if !q_had_value {
                            f.qualifiers.push((k.clone(), None));
                        }
                    }
                    q_name = None;
                    q_had_value = false;
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
                            // `checked_add`: these are numbers from an untrusted
                            // file, and `location="18446744073709551615-1"`
                            // panicked here in debug and wrapped to `{start: 0,
                            // end: 2}` in release — a fabricated coordinate that
                            // then entered the model unflagged.
                            if let (Some(start), Some(end)) = (
                                a.trim().parse::<u64>().ok().and_then(|v| v.checked_add(1)),
                                b.trim().parse::<u64>().ok().and_then(|v| v.checked_add(1)),
                            ) {
                                let rev = Event::attr(&attrs, "boundStrand") == Some("1");
                                p.sites.push(BindingSite {
                                    // NOT a typo, and not the same as Segment
                                    // above: `location` is 0-based inclusive
                                    // while `range` is 1-based inclusive. See
                                    // `binding_sites_are_zero_based_unlike_segments`.
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

/// Read block 6 as ordered key/value pairs.
///
/// # What this loses, and why it is not fixed here
///
/// Element **attributes are discarded**: `<Created UTC="22:0:0">2022.12.13`
/// reads as the date alone, so the recorded time of day is gone, and
/// `from_molecule` then writes `<Created>2022.12.13</Created>`. `xml::scan`
/// parses the attributes perfectly well — the `..` in the pattern below throws
/// them away — but `Molecule::notes` is `Vec<(String, String)>` and `notes_xml`
/// has no attribute channel, so there is nowhere to put them and nothing that
/// could re-emit them. Carrying them needs `Molecule::notes` to hold a note with
/// attributes, which is a `pl-core` change plus the three places that render
/// notes (`pl-scan`, `pl-wasm`, `pl-gui`); encoding them into the key here would
/// invent a syntax those three would display raw.
///
/// Anything nested deeper than a direct child of `<Notes>` is dropped for the
/// same reason. No corpus file has one.
///
/// `snapgene::write` is unaffected: it re-emits the original block verbatim, so
/// the loss appears only on the `from_molecule` path.
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

/// Build a `.dna` container from a molecule alone.
///
/// [`write`] re-emits the blocks a file was read from, which is what makes a
/// byte-exact rewrite possible and is useless for a molecule that never came
/// from a `.dna` — a GenBank file being converted, or anything this program
/// built. This synthesises the container instead.
///
/// # What is written, and what is deliberately not
///
/// Written: the header, the sequence block with its topology and methylation
/// flags, the feature XML, the primer XML, and the notes — the last of these
/// without element attributes, see "Known gap" below.
///
/// **Not written: blocks 2 and 3.** Measured across a 41-file corpus, they are
/// 78% of a typical file and up to 96%, and both are pure caches — block 3 is
/// the list of recognition sites active when the file was saved, block 2 the
/// cut-position index over them. Neither can hold anything a user authored. A
/// writer that emitted a *stale* cache would be worse than one that emits none,
/// because a reader trusting it would show cut sites that are not there.
///
/// **Not written: block 7,** the history tree. It is an xz-compressed recursive
/// provenance graph with each parent construct embedded whole. Polylinker keeps
/// its own history in an [`pl_core::OpLog`], and inventing a SnapGene history
/// node claiming a provenance this file does not have would be a fabrication.
/// The two histories are not the same object and one is not silently written as
/// the other.
///
/// # Coordinates
///
/// `<Segment range="a-b">` is **1-based inclusive**, which is the model's own
/// convention, so nothing shifts on the way out. Note that this is *not* the
/// convention `<BindingSite location>` uses in the same file — that one is
/// 0-based — and conflating them is the format's worst trap. `parse_primers`
/// adds one on the way in and [`primers_xml`] takes it back off on the way out,
/// in the same file, deliberately.
///
/// # Known gap
///
/// **Note *attributes* are not carried.** `<Created UTC="22:0:0">` reads as the
/// date alone, because `Molecule::notes` is `Vec<(String, String)>` and has
/// nowhere to put the time. Written here rather than left to be discovered,
/// because a `.dna` that lost its recorded creation time and said nothing is
/// the kind of quiet loss this section exists to prevent.
pub fn from_molecule(mol: &Molecule) -> Vec<u8> {
    from_molecule_reporting(mol).0
}

/// The same, plus anything the container could not carry.
///
/// The report is empty for every molecule that came from a real file. It is not
/// empty for a binding site starting before base 1, which has no 0-based
/// `location` form at all — and writing `location="-1-16"` to avoid saying so
/// would put a coordinate in the file that nothing ever recorded.
pub fn from_molecule_reporting(mol: &Molecule) -> (Vec<u8>, Vec<String>) {
    let mut unwritable: Vec<String> = Vec::new();
    let mut blocks = Vec::new();

    // Block 9. fileType 1 is DNA.
    //
    // The version pair was 14/14, under a comment claiming that is what the
    // corpus carries. It is not, and the project's own record says so:
    // `docs/DNA-FORMAT.md` §1 observed export versions {10, 11, 13, 15} and
    // import versions {5, 7, 10, 11, 12, 18, 19}, in the pairs 10/5, 10/7,
    // 11/10, 11/11, 13/12 and 15/18-19. 14 appears in neither set — it is the
    // header payload *length*, 0x0E, mistaken for a version — so every file we
    // wrote carried a version pair no real file was ever seen to use, in a
    // project whose whole provenance argument is that its format claims are
    // empirical. The test fixture in this very file already used the observed
    // pair 15/19, so the two sites disagreed.
    //
    // 10/5 is the observed pair with the lowest import version, and import
    // version is the *minimum reader required*: writing the lowest one observed
    // asks the least of whatever opens the file, which is the right trade for a
    // writer that deliberately emits only the long-stable blocks.
    const EXPORT_VERSION: u16 = 10;
    const IMPORT_VERSION: u16 = 5;
    let mut header = b"SnapGene".to_vec();
    header.extend_from_slice(&1u16.to_be_bytes());
    header.extend_from_slice(&EXPORT_VERSION.to_be_bytes());
    header.extend_from_slice(&IMPORT_VERSION.to_be_bytes());
    blocks.push(Block {
        kind: block::HEADER,
        payload: header,
    });

    // Block 0. Topology is bit 0 — published prose says bit 1 and is wrong;
    // this was established against the corpus.
    let mut flags = 0u8;
    if mol.topology.is_circular() {
        flags |= flag::CIRCULAR;
    }
    if mol.double_stranded.unwrap_or(true) {
        flags |= flag::DOUBLE_STRANDED;
    }
    if mol.methylation.dam {
        flags |= flag::DAM;
    }
    if mol.methylation.dcm {
        flags |= flag::DCM;
    }
    if mol.methylation.ecoki {
        flags |= flag::ECOKI;
    }
    let mut seq = vec![flags];
    seq.extend_from_slice(&mol.seq);
    blocks.push(Block {
        kind: block::SEQUENCE,
        payload: seq,
    });

    if !mol.features.is_empty() {
        blocks.push(Block {
            kind: block::FEATURES,
            payload: features_xml(&mol.features).into_bytes(),
        });
    }
    // Block 5. The reader has always populated `mol.primers` from it — 12 of
    // the 41 corpus files carry one — and this writer never emitted it, so
    // `pl convert --to dna` dropped every primer name, sequence, description,
    // bound strand and recorded melting temperature, printed nothing to stderr
    // and exited 0. The doc comment above said primers were written, and the
    // "deliberately not written" list named only blocks 2, 3 and 7, so nothing
    // in the file or the program disclosed it. `--to genbank` kept them, which
    // is how the two output formats came to disagree about whether a primer is
    // part of the molecule.
    if !mol.primers.is_empty() {
        blocks.push(Block {
            kind: block::PRIMERS,
            payload: primers_xml(&mol.primers, &mut unwritable).into_bytes(),
        });
    }
    if !mol.notes.is_empty() || !mol.description.is_empty() {
        blocks.push(Block {
            kind: block::NOTES,
            payload: notes_xml(mol).into_bytes(),
        });
    }
    (write_blocks(&blocks), unwritable)
}

fn features_xml(features: &[Feature]) -> String {
    let mut x = String::from("<Features>");
    for (i, f) in features.iter().enumerate() {
        x.push_str(&format!(
            "<Feature recentID=\"{i}\" name=\"{}\" type=\"{}\"",
            xml::escape(&f.name),
            xml::escape(&f.kind)
        ));
        if let Some(d) = f.strand.to_directionality() {
            x.push_str(&format!(" directionality=\"{d}\""));
        }
        x.push('>');
        for s in &f.segments {
            // 1-based inclusive, straight out of the model.
            x.push_str(&format!("<Segment range=\"{}-{}\"", s.start, s.end));
            if let Some(c) = &s.color {
                x.push_str(&format!(" color=\"{}\"", xml::escape(c)));
            }
            if s.translated {
                x.push_str(" translated=\"1\"");
            }
            if !s.kind.is_empty() {
                x.push_str(&format!(" type=\"{}\"", xml::escape(&s.kind)));
            }
            x.push_str("/>");
        }
        for (k, v) in &f.qualifiers {
            x.push_str(&format!("<Q name=\"{}\">", xml::escape(k)));
            // A valueless qualifier -- /pseudo, /ribosomal_slippage -- is a
            // different thing from an empty value, and collapsing the two has
            // already turned a pseudogene into an ordinary CDS once in this
            // project. `<Q>` with no `<V>` is how the absence is written.
            if let Some(v) = v {
                x.push_str(&format!("<V text=\"{}\"/>", xml::escape(v)));
            }
            x.push_str("</Q>");
        }
        x.push_str("</Feature>");
    }
    x.push_str("</Features>");
    x
}

/// The primer block, block 5.
///
/// `<HybridizationParams>` is **not** written. It records the search settings a
/// binding-site scan was run with, and this program did not run that scan; a
/// fabricated `minContinuousMatchLen` would be a claim about how these sites
/// were found. The sites themselves came from the file and are re-emitted.
///
/// No `simplified="1"` duplicate is written either. Real files carry one per
/// site and the reader drops it as a duplicate, so writing one back would make
/// every primer appear to bind twice on the next read.
fn primers_xml(primers: &[Primer], unwritable: &mut Vec<String>) -> String {
    let mut x = String::from("<Primers>");
    for (i, p) in primers.iter().enumerate() {
        x.push_str(&format!(
            "<Primer recentID=\"{i}\" name=\"{}\" sequence=\"{}\"",
            xml::escape(&p.name),
            xml::escape(&p.seq)
        ));
        if !p.description.is_empty() {
            x.push_str(&format!(" description=\"{}\"", xml::escape(&p.description)));
        }
        x.push('>');
        for s in &p.sites {
            // Back to 0-based, undoing the +1 `parse_primers` applied. This is
            // the format's worst trap and the two halves live twenty lines
            // apart on purpose: `location` is 0-based inclusive while the
            // identical-looking `range` on a Segment is 1-based, and a writer
            // that forgets to subtract shifts every primer by one base in a way
            // no byte-exact round-trip can see.
            let (Some(a), Some(b)) = (s.start.checked_sub(1), s.end.checked_sub(1)) else {
                unwritable.push(format!(
                    "primer {:?}: binding site {}..{} starts before base 1 and has no \
                     0-based `location` form; not written",
                    p.name, s.start, s.end
                ));
                continue;
            };
            x.push_str(&format!(
                "<BindingSite location=\"{a}-{b}\" boundStrand=\"{}\"",
                u8::from(s.strand.is_reverse())
            ));
            if let Some(tm) = s.tm {
                x.push_str(&format!(" meltingTemperature=\"{tm}\""));
            }
            x.push_str("/>");
        }
        x.push_str("</Primer>");
    }
    x.push_str("</Primers>");
    x
}

fn notes_xml(mol: &Molecule) -> String {
    let mut x = String::from("<Notes>");
    let mut seen_description = false;
    for (k, v) in &mol.notes {
        if k == "Description" {
            seen_description = true;
        }
        x.push_str(&format!(
            "<{k}>{}</{k}>",
            xml::escape(v),
            k = xml::escape(k)
        ));
    }
    if !seen_description && !mol.description.is_empty() {
        x.push_str(&format!(
            "<Description>{}</Description>",
            xml::escape(&mol.description)
        ));
    }
    x.push_str("</Notes>");
    x
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
    fn the_block_length_guard_is_a_subtraction_and_cannot_wrap_at_32_bit_width() {
        // The file-level test below cannot fail for the bug it was written
        // for. Its fixture is 28 bytes with the over-declaring block at offset
        // 19, so `body` is 24 and the pre-fix `body + len_us > n` gives
        // 2,147,483,672 through 4,294,967,319 on a 64-bit runner: no wrap, no
        // debug overflow, the guard fires anyway and returns the very same
        // `ShortBlock { claimed }`. Reverting the guard leaves it green. And
        // there is nowhere else the arithmetic runs at 32 bits — ci.yml's three
        // runners are all 64-bit, and the wasm32 job runs `cargo build`, never
        // `cargo test`.
        //
        // So the guard itself is exercised here at `u32`, which is the width
        // `usize` has on wasm32.
        let (n, body) = (28u32, 24u32);

        // These three genuinely wrap a 32-bit sum back under `n` — the second
        // is the 0xFFFFFFFB that killed the shipped module on a 19-byte file,
        // by turning the payload slice into `data[5..4]`.
        for claimed in [u32::MAX, 0xFFFF_FFFB, 0xFFFF_FFF0] {
            assert!(
                overruns(n, body, claimed),
                "{claimed:#x} bytes cannot fit in a 28-byte file"
            );
            assert!(
                body.wrapping_add(claimed) <= n,
                "{claimed:#x} was supposed to wrap under {n}; the fixture no longer \
                 demonstrates the bug"
            );
        }
        // ...and this one does not wrap even at 32 bits, which is precisely why
        // a fixture built from values like it proves nothing.
        assert!(overruns(n, body, 0x8000_0000));
        assert!(body.wrapping_add(0x8000_0000) > n);

        // Control: ordinary lengths still answer correctly, at both widths.
        assert!(!overruns(28u32, 24u32, 4), "exactly fits");
        assert!(overruns(28u32, 24u32, 5), "one byte too many");
        assert!(!overruns(28usize, 24usize, 4));
        assert!(overruns(28usize, 24usize, 5));
    }

    #[test]
    fn a_huge_declared_block_length_is_rejected_rather_than_read() {
        // Renamed: it never covered the 32-bit wrap its old name advertised
        // (see the test above, which does). What it does cover is that the
        // guard exists at all and names the length the file claimed, which is
        // worth keeping.
        for claimed in [u32::MAX, 0xFFFF_FFFB, 0xFFFF_FFF0, 0x8000_0000] {
            let mut f = header_block();
            let mut raw = vec![block::HEADER];
            raw.extend_from_slice(&(f.len() as u32).to_be_bytes());
            raw.append(&mut f);
            // A second block claiming more bytes than exist anywhere.
            raw.push(block::SEQUENCE);
            raw.extend_from_slice(&claimed.to_be_bytes());
            raw.extend_from_slice(b"ACGT");

            match read_blocks(&raw) {
                Err(Error::ShortBlock { claimed: c, .. }) => assert_eq!(c, claimed),
                other => panic!("expected ShortBlock for {claimed:#x}, got {other:?}"),
            }
        }
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
        assert_eq!(doc.molecule.double_stranded, Some(false));
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

    /// The one place `.dna` contradicts itself, so the one place worth a test
    /// that spells out *why*.
    ///
    /// `<Segment range="a-b">` is 1-based inclusive; `<BindingSite
    /// location="a-b">` is **0-based** inclusive. Both live in the same file,
    /// both look like `"a-b"`, and reading them the same way is wrong by one
    /// base for every primer.
    ///
    /// Established empirically rather than assumed: across the corpus, segment
    /// starts are never 0 and segment ends reach exactly `len`, while 32 of 32
    /// unambiguous binding sites reproduce their own recorded `annealedBases`
    /// only when read 0-based. See `docs/DNA-FORMAT.md`.
    ///
    /// This is invisible to a round-trip test — the writer re-emits the
    /// original block, so an off-by-one on read cancels on write.
    #[test]
    fn binding_sites_are_zero_based_unlike_segments() {
        // A 17 bp primer annealing to the very first bases of the molecule.
        // 0-based inclusive "0-16" is 17 bases starting at the first one.
        let p = parse_primers(
            r#"<Primers><Primer name="M13F" sequence="GTAAAACGACGGCCAGT">
                 <BindingSite location="0-16" boundStrand="0"/>
               </Primer></Primers>"#,
        );
        assert_eq!(
            (p[0].sites[0].start, p[0].sites[0].end),
            (1, 17),
            "location=\"0-16\" is the first 17 bases, i.e. 1..=17 for us"
        );
        assert_eq!(p[0].sites[0].end - p[0].sites[0].start + 1, 17);

        // The identical-looking attribute on a Segment is taken at face value.
        let f = parse_features(
            r#"<Features><Feature name="x" type="CDS">
                 <Segment range="1-17"/></Feature></Features>"#,
        );
        assert_eq!((f[0].segments[0].start, f[0].segments[0].end), (1, 17));
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

    fn round_trip(mol: &Molecule) -> Molecule {
        parse(&from_molecule(mol))
            .expect("what we wrote, we can read")
            .molecule
    }

    fn mol_with_feature() -> Molecule {
        let mut m = Molecule {
            seq: b"ATGGCCTAAGGATCCAAGCTTGAATTCACGT".to_vec(),
            topology: Topology::Circular,
            ..Default::default()
        };
        m.methylation = Methylation {
            dam: true,
            dcm: true,
            ecoki: false,
            cpg: false,
        };
        m.double_stranded = Some(true);
        let mut f = Feature::new("AmpR", "CDS");
        f.strand = Strand::Reverse;
        let mut seg = Segment::new(3, 12);
        seg.color = Some("#4f7fd0".into());
        seg.translated = true;
        f.segments.push(seg);
        f.qualifiers
            .push(("note".into(), Some("beta-lactamase".into())));
        f.qualifiers.push(("pseudo".into(), None));
        m.features.push(f);
        m.notes.push(("Description".into(), "a test".into()));
        m
    }

    #[test]
    fn a_molecule_that_never_came_from_a_dna_file_can_be_written_as_one() {
        // `write` re-emits the blocks a file was read from, which is useless for
        // a GenBank record being converted. This is the case that needed a real
        // writer.
        let m = mol_with_feature();
        let back = round_trip(&m);
        assert_eq!(back.seq, m.seq);
        assert_eq!(back.topology, Topology::Circular);
        assert_eq!(back.features.len(), 1);
        assert_eq!(back.features[0].name, "AmpR");
        assert_eq!(back.features[0].kind, "CDS");
        assert_eq!(back.features[0].strand, Strand::Reverse);
        assert_eq!(back.features[0].segments[0].start, 3);
        assert_eq!(back.features[0].segments[0].end, 12);
        assert_eq!(
            back.features[0].segments[0].color.as_deref(),
            Some("#4f7fd0")
        );
        assert!(back.features[0].segments[0].translated);
        assert_eq!(back.description, "a test");
    }

    #[test]
    fn topology_is_written_to_bit_zero() {
        // Published prose says bit 1; the corpus says bit 0. A writer that used
        // the documented bit would produce files SnapGene reads as linear, and
        // a plasmid drawn as a line is wrong in a way nobody misses -- but only
        // once it reaches SnapGene, not in our own round-trip.
        let mut m = mol_with_feature();
        m.topology = Topology::Circular;
        let bytes = from_molecule(&m);
        let seq_block = read_blocks(&bytes)
            .unwrap()
            .into_iter()
            .find(|b| b.kind == block::SEQUENCE)
            .expect("a sequence block");
        assert_eq!(seq_block.payload[0] & 0x01, 0x01, "circular is bit 0");

        m.topology = Topology::Linear;
        let bytes = from_molecule(&m);
        let seq_block = read_blocks(&bytes)
            .unwrap()
            .into_iter()
            .find(|b| b.kind == block::SEQUENCE)
            .unwrap();
        assert_eq!(seq_block.payload[0] & 0x01, 0x00);
        assert_eq!(round_trip(&m).topology, Topology::Linear);
    }

    #[test]
    fn methylation_flags_survive() {
        for (dam, dcm, ecoki) in [
            (true, false, false),
            (false, true, true),
            (true, true, true),
        ] {
            let mut m = mol_with_feature();
            m.methylation = Methylation {
                dam,
                dcm,
                ecoki,
                cpg: false,
            };
            let back = round_trip(&m);
            assert_eq!(back.methylation.dam, dam);
            assert_eq!(back.methylation.dcm, dcm);
            assert_eq!(back.methylation.ecoki, ecoki);
        }
    }

    #[test]
    fn a_valueless_qualifier_stays_valueless() {
        // /pseudo is written bare and is a different thing from /replace="".
        // Collapsing them has already turned a pseudogene into an ordinary
        // protein-coding gene once in this project.
        let back = round_trip(&mol_with_feature());
        let q = &back.features[0].qualifiers;
        assert!(q.iter().any(|(k, v)| k == "pseudo" && v.is_none()), "{q:?}");
        assert!(
            q.iter()
                .any(|(k, v)| k == "note" && v.as_deref() == Some("beta-lactamase")),
            "{q:?}"
        );
    }

    #[test]
    fn a_name_full_of_markup_does_not_escape_into_the_xml() {
        // Feature names are user data and routinely contain & and <.
        let mut m = mol_with_feature();
        m.features[0].name = "lacZ<alpha> & \"friends\"".into();
        assert_eq!(round_trip(&m).features[0].name, m.features[0].name);
    }

    #[test]
    fn a_synthesised_file_keeps_its_primers() {
        // `from_molecule` emitted blocks 9, 0, 10 and 6 and never block 5, so
        // `pl convert --to dna` dropped every primer name, sequence,
        // description, bound strand and recorded melting temperature — 12 of
        // the 41 corpus files carry a primer block — printed nothing to stderr
        // and exited 0. The doc comment said primers were written and the
        // "deliberately not written" list named only blocks 2, 3 and 7, so
        // nothing anywhere disclosed it. `--to genbank` kept them the whole
        // time, so the two output formats disagreed about whether a primer is
        // part of the molecule.
        let mut m = mol_with_feature();
        m.primers.push(Primer {
            name: "Fab2_D_SalI".into(),
            seq: "atatGTCGACTTAGAATATAACTCTTAGTCCTACTCCACC".into(),
            description: "reverse screening primer".into(),
            sites: vec![
                BindingSite {
                    start: 3,
                    end: 12,
                    strand: Strand::Reverse,
                    tm: Some(53.0),
                },
                BindingSite {
                    start: 1,
                    end: 8,
                    strand: Strand::Forward,
                    tm: None,
                },
            ],
        });

        let (bytes, report) = from_molecule_reporting(&m);
        assert!(
            report.is_empty(),
            "nothing was lost, so nothing to report: {report:?}"
        );
        let kinds: Vec<u8> = read_blocks(&bytes)
            .unwrap()
            .iter()
            .map(|b| b.kind)
            .collect();
        assert!(kinds.contains(&block::PRIMERS), "no block 5: {kinds:?}");

        let back = parse(&bytes).unwrap().molecule;
        assert_eq!(back.primers, m.primers, "the primer block did not survive");
        // Spelled out, because `PartialEq` passing on an empty Vec is exactly
        // how this went unnoticed for so long.
        assert_eq!(back.primers.len(), 1);
        assert_eq!(back.primers[0].name, "Fab2_D_SalI");
        assert_eq!(back.primers[0].description, "reverse screening primer");
        assert_eq!(
            back.primers[0].sites.len(),
            2,
            "the sites, not just the primer"
        );
        assert_eq!(back.primers[0].sites[0].strand, Strand::Reverse);
        assert_eq!(back.primers[0].sites[0].tm, Some(53.0));
        assert_eq!(back.primers[0].sites[1].tm, None);
    }

    #[test]
    fn a_written_binding_site_goes_back_to_zero_based_coordinates() {
        // The format's worst trap, in the direction only a writer can hit.
        // `location` is 0-based inclusive while the identical-looking `range`
        // on a Segment is 1-based, so the writer must take back the +1 the
        // reader applied. Getting it wrong shifts every primer by one base, and
        // a byte-exact round-trip cannot see it because the error cancels.
        let mut m = mol_with_feature();
        m.primers.push(Primer {
            name: "M13F".into(),
            seq: "GTAAAACGACGGCCAGT".into(),
            description: String::new(),
            // The first 17 bases of the molecule, 1-based inclusive.
            sites: vec![BindingSite {
                start: 1,
                end: 17,
                strand: Strand::Forward,
                tm: None,
            }],
        });
        let (bytes, _) = from_molecule_reporting(&m);
        let xml = read_blocks(&bytes)
            .unwrap()
            .into_iter()
            .find(|b| b.kind == block::PRIMERS)
            .map(|b| String::from_utf8_lossy(&b.payload).to_string())
            .expect("a primer block");
        assert!(
            xml.contains(r#"location="0-16""#),
            "1..17 must be written 0-based, as 0-16:\n{xml}"
        );
        // A Segment in the same file is written at face value: 3-12, not 2-11.
        let feats = read_blocks(&bytes)
            .unwrap()
            .into_iter()
            .find(|b| b.kind == block::FEATURES)
            .map(|b| String::from_utf8_lossy(&b.payload).to_string())
            .unwrap();
        assert!(feats.contains(r#"range="3-12""#), "{feats}");
    }

    #[test]
    fn a_binding_site_starting_before_base_one_is_reported_not_shifted() {
        // There is no 0-based form of base 0, and `location="-1-16"` would put
        // a coordinate in the file that nothing ever recorded. Saturating to 0
        // would be worse still: a silent one-base shift.
        let mut m = mol_with_feature();
        m.primers.push(Primer {
            name: "impossible".into(),
            seq: "ACGT".into(),
            description: String::new(),
            sites: vec![BindingSite {
                start: 0,
                end: 17,
                strand: Strand::Forward,
                tm: None,
            }],
        });
        let (bytes, report) = from_molecule_reporting(&m);
        let xml = read_blocks(&bytes)
            .unwrap()
            .into_iter()
            .find(|b| b.kind == block::PRIMERS)
            .map(|b| String::from_utf8_lossy(&b.payload).to_string())
            .expect("the primer itself is still written");
        assert!(
            !xml.contains("-1"),
            "a negative coordinate reached the file:\n{xml}"
        );
        assert_eq!(report.len(), 1, "{report:?}");
        assert!(report[0].contains("impossible"), "{report:?}");
    }

    #[test]
    fn a_primerless_molecule_gets_no_primer_block() {
        // Control: an empty `<Primers/>` block is not invented for a molecule
        // that has none, which would make the writer's output disagree with the
        // 29 corpus files that carry no block 5.
        let bytes = from_molecule(&mol_with_feature());
        let kinds: Vec<u8> = read_blocks(&bytes)
            .unwrap()
            .iter()
            .map(|b| b.kind)
            .collect();
        assert!(!kinds.contains(&block::PRIMERS), "{kinds:?}");
    }

    #[test]
    fn the_written_header_carries_a_version_pair_the_corpus_actually_uses() {
        // It carried 14/14 under a comment claiming that is what the corpus
        // holds. `docs/DNA-FORMAT.md` §1 says otherwise: export versions
        // {10, 11, 13, 15}, import versions {5, 7, 10, 11, 12, 18, 19}. 14 is
        // the header payload length, 0x0E, mistaken for a version — so every
        // file this program wrote declared a version pair no observed file
        // uses, in a project whose format claims are meant to be empirical.
        const OBSERVED_EXPORT: [u16; 4] = [10, 11, 13, 15];
        const OBSERVED_IMPORT: [u16; 7] = [5, 7, 10, 11, 12, 18, 19];

        let doc = parse(&from_molecule(&mol_with_feature())).unwrap();
        assert!(
            OBSERVED_EXPORT.contains(&doc.export_version),
            "export version {} is in no corpus file",
            doc.export_version
        );
        assert!(
            OBSERVED_IMPORT.contains(&doc.import_version),
            "import version {} is in no corpus file",
            doc.import_version
        );
        assert_eq!(doc.file_type, 1, "fileType 1 is DNA");
        // Import version is the minimum reader required, so the lowest observed
        // one is the friendliest thing to declare.
        assert_eq!(
            doc.import_version,
            *OBSERVED_IMPORT.iter().min().unwrap(),
            "asking for a newer reader than we need locks out files we modelled on"
        );
    }

    #[test]
    fn the_regenerable_caches_and_the_history_are_not_invented() {
        // Blocks 2 and 3 are 78% of a typical file and are pure caches; a stale
        // one is worse than none, because a reader would trust it. Block 7 is a
        // provenance graph, and writing one claiming a history this file does
        // not have would be a fabrication.
        let bytes = from_molecule(&mol_with_feature());
        let kinds: Vec<u8> = read_blocks(&bytes)
            .unwrap()
            .iter()
            .map(|b| b.kind)
            .collect();
        for absent in [
            block::CUTSITE_CACHE,
            block::ENZYME_TABLE,
            block::HISTORY_TREE,
        ] {
            assert!(
                !kinds.contains(&absent),
                "block {absent} must not be invented"
            );
        }
        assert_eq!(kinds[0], block::HEADER, "the header comes first");
        assert!(kinds.contains(&block::SEQUENCE));
    }

    #[test]
    fn an_empty_molecule_still_produces_a_readable_file() {
        let m = Molecule::default();
        let back = round_trip(&m);
        assert!(back.seq.is_empty());
        assert!(back.features.is_empty());
    }

    #[test]
    fn writing_the_same_molecule_twice_gives_the_same_bytes() {
        let m = mol_with_feature();
        assert_eq!(from_molecule(&m), from_molecule(&m));
    }
}
