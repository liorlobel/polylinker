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

use pl_core::{
    BindingSite, Feature, Methylation, Molecule, Note, Primer, Segment, Strand, Topology,
};

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
    /// Paths inside block 6 that [`Molecule::notes`] has no shape for.
    ///
    /// A note is a key, its text and its attributes; this model is flat. Three
    /// path forms appear, distinguishable by spelling so that one report can
    /// carry all three (see `parse_notes` for what each costs):
    ///
    /// - `Notes/References/Reference` — a subtree under a note. Real files
    ///   contain one:
    ///   `<References><Reference title=".." pubMedID=".." journal=".." authors=".."/></References>`
    ///   is how SnapGene stores a plasmid's citation, so this is not a
    ///   theoretical channel. Named rather than flattened into its parent's
    ///   value, because a citation's title appearing as the text of
    ///   `<References>` is a claim the file never made.
    /// - `Notes/Comments/text()` — text following a nested child, which a single
    ///   `value` cannot hold without fusing two runs across a hole.
    /// - `Notes@version` — an attribute on the `<Notes>` root.
    ///
    /// Empty for every file whose block 6 is flat, which is most of them, and
    /// empty for every format that is not `.dna`.
    pub unrepresentable_notes: Vec<String>,
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
                let (notes, nested) = parse_notes(&String::from_utf8_lossy(&payload));
                doc.molecule.notes = notes;
                doc.unrepresentable_notes = nested;
            }
            block::HISTORY_TREE => {
                doc.history_present = true;
                doc.history_compressed = payload.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]);
            }
            _ => {}
        }
    }

    // First-wins; see `Molecule::note`, which is where that rule is written down.
    if let Some(d) = doc.molecule.note("Description").map(str::to_string) {
        doc.molecule.description = d;
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

/// Read block 6 into ordered [`Note`]s, plus the paths of anything nested under
/// one that a flat note has no shape for.
///
/// # The note is built on its opening tag, not on its text
///
/// This reader used to push a `(key, value)` pair when a `Event::Text` arrived
/// at depth 2, which meant the *existence* of a note depended on it having text.
/// Three things fell out of that and all three are fixed here by keying off
/// `Event::Open` instead:
///
/// - `<Empty/>` and `<Created UTC="21:55:7"/>` produced nothing at all. A
///   self-closing tag never emits a text event, so a note the file plainly
///   contained had no entry — and the unit test below asserted the disappearance
///   rather than the note.
/// - `<Type></Type>` was indistinguishable from an absent `<Type>`, for the same
///   reason: `xml::scan` suppresses whitespace-only text.
/// - A value split by a comment or CDATA — `<Created>2022<!--x-->.12.13</Created>`
///   — fired twice and produced two `Created` notes. The runs are concatenated
///   now, which is what the file says.
///
/// # Attributes are the point
///
/// `<Created UTC="22:0:0">2022.12.13</Created>` is one timestamp written across
/// an attribute and a text node. The `..` that used to sit in the `Event::Open`
/// pattern below threw the attribute away, so the date survived, the time did
/// not, and `from_molecule` wrote `<Created>2022.12.13</Created>` back out — a
/// `.dna` converted to a `.dna` losing its recorded creation time, exit 0,
/// nothing on stderr. [`pl_core::Note`] carries them now.
///
/// # What is still not carried, and is named instead of dropped
///
/// Three shapes, all reported by path in the second half of the tuple, which
/// surfaces through `LoadReport::unrepresentable_notes` and `pl info`:
///
/// - **A subtree below a note**, reported at every depth as `Notes/A/B`,
///   `Notes/A/B/C`. `<References><Reference title=".." pubMedID=".." journal=".."
///   authors=".."/></References>` is real — it is how SnapGene stores a plasmid's
///   citation, and files carrying it were checked while writing this, so the
///   claim this comment used to make that "no corpus file has one" was simply
///   wrong. `Note` is flat, so it is emphatically *not* folded into the parent's
///   text: a citation title appearing as the value of `<References>` would be a
///   claim the file never made, and the next writer would emit it as one.
/// - **Text after a nested child**, reported as `Notes/A/text()`. This used to
///   append every text run at note depth to `value`, which meant
///   `<Comments>Grown at 37 <sup>o</sup>C overnight</Comments>` became
///   `"Grown at 37 C overnight"` — a sentence that is not in the file, that
///   `from_molecule` then wrote back out as if it were, and that `pl-scan`
///   indexed. Fusing two runs across a hole is the same invention as folding the
///   subtree in; the value is the text *before* the first child, which is also
///   what `ElementTree.text` gives the reference implementation, and the
///   discarded tail is named.
/// - **Attributes on `<Notes>` itself**, reported as `Notes@version`. No
///   observed file has one; the branch exists so that a future one is named
///   rather than dropped, which is the whole complaint that produced this
///   function's rewrite, one level up.
///
/// `snapgene::write` is unaffected either way: it re-emits the original block
/// verbatim, so only the `from_molecule` path ever depended on this.
fn parse_notes(x: &str) -> (Vec<Note>, Vec<String>) {
    let mut out: Vec<Note> = Vec::new();
    let mut nested: Vec<String> = Vec::new();
    // Element names from `<Notes>` down, so a nested element can be reported by
    // path rather than by a bare tag name — `Reference` alone would not tell
    // anyone which note lost it.
    let mut stack: Vec<String> = Vec::new();
    let mut cur: Option<Note> = None;
    // Has the note currently open had a child element? Notes do not nest — a
    // note is a direct child of `<Notes>` — so one flag serves, reset per note.
    let mut cur_has_child = false;
    // ...and did text follow one? That text is at note depth and belongs to the
    // note, but there is no shape for "value, hole, value" and no separator that
    // would not be invented, so it is reported instead of glued on.
    let mut cur_lost_tail = false;

    for ev in xml::scan(x) {
        match ev {
            Event::Open {
                name,
                attrs,
                self_closing,
            } => {
                match stack.len() {
                    // `<Notes>` itself. It carries no attributes in any observed
                    // file, and `Molecule` has nowhere to put one, so any that
                    // turns up is named rather than written away: `Notes@version`
                    // reads as an attribute of the root and cannot be mistaken
                    // for an element path.
                    0 => {
                        for (k, _) in &attrs {
                            let path = format!("{name}@{k}");
                            if !nested.contains(&path) {
                                nested.push(path);
                            }
                        }
                    }
                    // A direct child: this is a note.
                    1 => {
                        let note = Note {
                            key: name.clone(),
                            value: String::new(),
                            attrs,
                        };
                        cur_has_child = false;
                        cur_lost_tail = false;
                        if self_closing {
                            out.push(note);
                        } else {
                            cur = Some(note);
                        }
                    }
                    // Deeper than a note. Named, never flattened.
                    _ => {
                        if stack.len() == 2 {
                            cur_has_child = true;
                        }
                        let mut path = stack.join("/");
                        path.push('/');
                        path.push_str(&name);
                        // One line per distinct path: a `<References>` holding
                        // eight `<Reference/>` elements is one thing this model
                        // cannot hold, not eight.
                        if !nested.contains(&path) {
                            nested.push(path);
                        }
                    }
                }
                if !self_closing {
                    stack.push(name);
                }
            }
            Event::Close { .. } => {
                if stack.len() == 2 {
                    if cur_lost_tail {
                        let mut path = stack.join("/");
                        path.push_str("/text()");
                        if !nested.contains(&path) {
                            nested.push(path);
                        }
                    }
                    if let Some(n) = cur.take() {
                        out.push(n);
                    }
                }
                stack.pop();
            }
            Event::Text(t) => {
                // The note's own text, up to its first child element. Text
                // *inside* a nested element belongs to that element; text
                // *after* one is this note's and is unreachable from a flat
                // value — see the doc comment. `xml::scan` suppresses
                // whitespace-only runs, so the newlines around a real
                // `<Reference/>` never reach here and never report a loss.
                if stack.len() == 2 {
                    if let Some(n) = cur.as_mut() {
                        if cur_has_child {
                            cur_lost_tail = true;
                        } else {
                            n.value.push_str(&t);
                        }
                    }
                }
            }
        }
    }
    // An unterminated final note still counts. `xml::scan` stops cleanly on a
    // truncated tag rather than erroring, so a block 6 cut short by a bad write
    // yields the notes that were complete plus this one — and any tail it lost,
    // on the same terms as a note that closed properly. `stack` is still
    // `[root, key]` here, because the close that would have popped it never came.
    if let Some(n) = cur.take() {
        if cur_lost_tail && stack.len() == 2 {
            let mut path = stack.join("/");
            path.push_str("/text()");
            if !nested.contains(&path) {
                nested.push(path);
            }
        }
        out.push(n);
    }
    for n in &mut out {
        let trimmed = n.value.trim().to_string();
        n.value = trimmed;
    }
    (out, nested)
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
/// flags, the feature XML, the primer XML, and the notes with their element
/// attributes.
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
/// **A subtree nested under a note is not written, because it was never read.**
/// `<References><Reference title=".." pubMedID=".."/></References>` — a
/// plasmid's citation — arrives as a `<References>` note with no text and no
/// attributes, and leaves as one. [`parse_notes`] names each such path on the
/// way in and `pl info` prints it, so the loss is visible at the point it
/// happens rather than inferred from a diff; carrying it needs `Note` to become
/// a tree, which is a `pl-core` change plus a rendering decision in each of
/// `pl-scan`, `pl-wasm` and the GUI's notes grid.
///
/// Note *attributes* used to be listed here and no longer are: `<Created
/// UTC="22:0:0">` keeps its time of day now, in the model and in the file.
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
    // `is_empty()` on the Vec, not on the text: a note may now carry nothing but
    // attributes — `<Created UTC="21:55:7"/>` is a real shape — and gating on
    // "does any note have a value" would drop exactly the notes this change
    // exists to keep.
    if !mol.notes.is_empty() || !mol.description.is_empty() {
        blocks.push(Block {
            kind: block::NOTES,
            payload: notes_xml(mol, &mut unwritable).into_bytes(),
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

/// Is this a legal XML element or attribute name?
///
/// [`xml::escape`] escapes *text and attribute values*; there is no escaping
/// that turns an arbitrary string into a legal XML Name, and [`notes_xml`]
/// interpolates note keys and attribute names straight into markup. It used to
/// run the key through `xml::escape` and emit `<{key}>`, which is worse than
/// doing nothing: a key of `A B` produced `<A B>`, which `xml::scan` reads back
/// as an element `A` carrying a valueless attribute `B` — the note silently
/// changes its name and grows an attribute — and a key of `a&b` produced
/// `<a&amp;b>`, which is not a tag at all. Notes come out of someone else's
/// file, so both are untrusted input, and adding an attribute channel widens the
/// surface rather than narrowing it.
///
/// This is XML 1.0 5th edition's `NameStartChar`/`NameChar` productions,
/// codepoint range for codepoint range, and not an approximation of them. It was
/// `first.is_alphabetic()` followed by `c.is_alphanumeric() || _ : - .`, which
/// reads like a stricter rule and is not one in the direction that matters:
/// `char::is_alphanumeric` is true for Unicode category `No`, so `A\u{b2}`
/// (SUPERSCRIPT TWO) was accepted and written as `<A²>`, which is a false accept
/// — U+00B2 is in none of `NameChar`'s ranges and `ET.fromstring` refuses the
/// element. It was simultaneously a false reject for U+00B7 MIDDLE DOT, which
/// XML allows. Twelve lines of ranges is the price of the doc comment above
/// being true, and a false accept here is the expensive direction: a rejected
/// name is *reported* through `unwritable` and costs a line on stderr, while an
/// accepted illegal one writes a file no parser can read.
fn is_xml_name(s: &str) -> bool {
    fn start(c: char) -> bool {
        matches!(c,
            ':' | '_'
            | 'A'..='Z' | 'a'..='z'
            | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{2ff}'
            | '\u{370}'..='\u{37d}' | '\u{37f}'..='\u{1fff}'
            | '\u{200c}'..='\u{200d}' | '\u{2070}'..='\u{218f}'
            | '\u{2c00}'..='\u{2fef}' | '\u{3001}'..='\u{d7ff}'
            | '\u{f900}'..='\u{fdcf}' | '\u{fdf0}'..='\u{fffd}'
            | '\u{10000}'..='\u{effff}')
    }
    fn rest(c: char) -> bool {
        start(c)
            || matches!(c,
                '-' | '.' | '0'..='9' | '\u{b7}'
                | '\u{300}'..='\u{36f}' | '\u{203f}'..='\u{2040}')
    }
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    start(first) && chars.all(rest)
}

/// The notes block, block 6.
///
/// Attributes are written, which is the whole point of [`pl_core::Note`]: a
/// `<Created UTC="22:0:0">` that arrived with a time of day leaves with one.
///
/// A note with no text is written as `<Key></Key>` rather than `<Key/>`. The two
/// are the same element to any reader — including `parse_notes` above, which is
/// what has to agree with this — and one code path is easier to keep honest than
/// two. The `.dna` this synthesises is not byte-compared against anything; it is
/// [`write`] that reproduces a file exactly.
///
/// Two things about an attribute list are checked, not one. [`is_xml_name`]
/// covers the name's *spelling*; uniqueness is the other half of well-formedness
/// and was missed, so a `.dna` whose block 6 said
/// `<Created UTC="1" UTC="2">` round-tripped straight back out —
/// `xml::parse_attrs` is deliberately lenient and keeps both — and the file `pl`
/// had just written was refused outright by this project's own reference
/// implementation (`ParseError: duplicate attribute`). Writing markup that a
/// conformant parser cannot read is exactly the outcome `is_xml_name` exists to
/// prevent, so the repeat is reported and dropped on the same terms.
fn notes_xml(mol: &Molecule, unwritable: &mut Vec<String>) -> String {
    let mut x = String::from("<Notes>");
    let mut seen_description = false;
    // Reused per note; the constraint is per element, not per document.
    let mut written_attrs: Vec<&str> = Vec::new();
    for n in &mol.notes {
        if !is_xml_name(&n.key) {
            unwritable.push(format!(
                "note {:?}: not a legal XML element name, so the note and its value {:?} are \
                 not written",
                n.key, n.value
            ));
            continue;
        }
        if n.key == "Description" {
            seen_description = true;
        }
        x.push_str(&format!("<{}", n.key));
        written_attrs.clear();
        for (k, v) in &n.attrs {
            if !is_xml_name(k) {
                unwritable.push(format!(
                    "note {:?}: attribute name {k:?} is not a legal XML name, so the value {v:?} \
                     is not written",
                    n.key
                ));
                continue;
            }
            // XML 1.0 §3.1, well-formedness constraint "Unique Att Spec". There
            // is no spelling for a repeated name, and choosing one value or
            // merging them would state something the source did not.
            if written_attrs.contains(&k.as_str()) {
                unwritable.push(format!(
                    "note {:?}: attribute {k:?} appears more than once and XML allows it once, \
                     so the later value {v:?} is not written",
                    n.key
                ));
                continue;
            }
            written_attrs.push(k);
            x.push_str(&format!(" {k}=\"{}\"", xml::escape(v)));
        }
        x.push_str(&format!(">{}</{}>", xml::escape(&n.value), n.key));
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
    fn notes_read_as_ordered_notes_with_their_attributes() {
        // This test used to assert the bug. Its `<Created>` was attribute-free,
        // so the case the whole change is about was never exercised, and its
        // exhaustive `assert_eq!` against a two-element vec asserted that the
        // `<Empty/>` in its own input produced *nothing* — a note the file
        // plainly contained, whose disappearance was pinned as correct.
        let (n, nested) = parse_notes(
            r#"<Notes><UUID>abc-123</UUID><Created UTC="22:0:0">2026.07.26</Created><Empty/></Notes>"#,
        );
        assert_eq!(
            n,
            vec![
                Note::new("UUID", "abc-123"),
                Note {
                    key: "Created".into(),
                    value: "2026.07.26".into(),
                    attrs: vec![("UTC".into(), "22:0:0".into())],
                },
                // A self-closing child is a note with no text, not an absence.
                Note::new("Empty", ""),
            ]
        );
        assert!(nested.is_empty(), "nothing here is nested");
    }

    #[test]
    fn an_attribute_only_note_survives_and_an_empty_value_is_not_an_absent_note() {
        // Real `.dna` files carry `<Created UTC="21:55:7">` with an unpadded
        // time; the attribute-only and empty-bodied forms are the same shape
        // with the text removed, and the old reader — which created a note only
        // when a text event arrived — produced nothing for any of them.
        let (n, _) = parse_notes(r#"<Notes><Created UTC="21:55:7"/><Type></Type></Notes>"#);
        assert_eq!(n.len(), 2, "got {n:?}");
        assert_eq!(n[0].key, "Created");
        assert_eq!(n[0].value, "");
        assert_eq!(n[0].attr("UTC"), Some("21:55:7"));
        assert_eq!(n[1], Note::new("Type", ""));
    }

    #[test]
    fn a_nested_subtree_is_reported_by_path_and_never_folded_into_its_parent() {
        // `<References><Reference .../></References>` is how SnapGene stores a
        // plasmid's citation, and it is not hypothetical — real files carry it,
        // contrary to the claim this reader's comment used to make. `Note` is
        // flat, so the subtree cannot be represented; the requirement is that it
        // is *named*, and that the citation's title does not turn up as the text
        // of `<References>`.
        let (n, nested) = parse_notes(
            r#"<Notes><Type>Synthetic</Type><References>
                 <Reference title="Precise deletions in E. coli" pubMedID="9335267"/>
               </References></Notes>"#,
        );
        assert_eq!(n[0], Note::new("Type", "Synthetic"));
        assert_eq!(n[1].key, "References");
        assert_eq!(
            n[1].value, "",
            "the citation must not be flattened into its parent's text"
        );
        assert!(n[1].attrs.is_empty());
        assert_eq!(nested, vec!["Notes/References/Reference".to_string()]);

        // One line per distinct path, not per element: a note that cites eight
        // papers is one thing this model cannot hold, not eight.
        let (_, many) = parse_notes(
            r#"<Notes><References><Reference pubMedID="1"/><Reference pubMedID="2"/></References></Notes>"#,
        );
        assert_eq!(many, vec!["Notes/References/Reference".to_string()]);

        // Every depth, with the full path. `reference/python/snapdna.py` walked
        // one level and hard-coded a three-segment path, so on this input the
        // two implementations answered `["Notes/A/B", "Notes/A/B/C"]` and
        // `["Notes/A/B"]` — a hard disagreement in `xcheck_rust.py`, in the one
        // field that exists to prove this fix against an independent reader. No
        // real file nests three deep, which is why the corpus could not see it
        // and why the contract is pinned here instead.
        let (_, deep) = parse_notes(r#"<Notes><A><B><C x="1"/></B></A></Notes>"#);
        assert_eq!(
            deep,
            vec!["Notes/A/B".to_string(), "Notes/A/B/C".to_string()]
        );
    }

    #[test]
    fn text_after_a_nested_child_is_reported_rather_than_fused_onto_the_value() {
        // Every text run at note depth was appended, which includes the run
        // *after* a child element pops the stack back. So a `<Comments>` field —
        // and SnapGene's do carry markup; one real file in this corpus has an
        // `&lt;br>` in one — came back as a sentence the file does not contain,
        // with the two halves glued directly together. `from_molecule` then
        // wrote that invented string back out and `pl-scan` indexed it. Fusing
        // two runs across a hole is the same invention as folding the subtree
        // into its parent, so the value stops at the first child and the tail is
        // named.
        let (n, nested) =
            parse_notes("<Notes><Comments>Grown at 37 <sup>o</sup>C overnight</Comments></Notes>");
        assert_eq!(n.len(), 1);
        assert_eq!(
            n[0].value, "Grown at 37",
            "the value is the text before the first child, trimmed — not \
             \"Grown at 37 C overnight\", which is in no file"
        );
        assert_eq!(
            nested,
            vec![
                "Notes/Comments/sup".to_string(),
                "Notes/Comments/text()".to_string()
            ]
        );

        // Whitespace between tags is not a loss and must not be reported: the
        // three real files that carry a citation write it as
        // `<References>\n<Reference/>\n</References>`, and a report on every one
        // of them would make the notice noise that gets ignored.
        let (_, quiet) =
            parse_notes("<Notes><References>\n<Reference pubMedID=\"1\"/>\n</References></Notes>");
        assert_eq!(quiet, vec!["Notes/References/Reference".to_string()]);
    }

    #[test]
    fn an_attribute_on_the_notes_root_is_named_rather_than_dropped() {
        // The same shape of loss the finding is about, one level up: the `0 =>`
        // arm bound `attrs` and said nothing, so `pl convert --to dna` wrote a
        // root attribute away with exit 0 and an empty stderr. `Notes@version`
        // rather than `Notes/version`, so it cannot be misread as an element.
        let (n, nested) = parse_notes(r#"<Notes version="3"><Type>Synthetic</Type></Notes>"#);
        assert_eq!(n, vec![Note::new("Type", "Synthetic")]);
        assert_eq!(nested, vec!["Notes@version".to_string()]);
    }

    #[test]
    fn a_repeated_attribute_name_is_reported_rather_than_written_twice() {
        // XML 1.0 §3.1's "Unique Att Spec" well-formedness constraint. The name
        // check next door covers spelling and this covers uniqueness, and it was
        // missing: `xml::parse_attrs` is deliberately lenient and keeps both, so
        // a `.dna` carrying the repeat round-tripped it straight back out and the
        // file `pl` had just written was refused outright by this project's own
        // reference implementation — `ParseError: duplicate attribute` — with
        // exit 0 and nothing on stderr. Unreachable before this change: there was
        // no attribute channel at all.
        let raw = build(&[
            (block::HEADER, header_block()),
            (
                block::SEQUENCE,
                vec![flag::CIRCULAR, b'A', b'C', b'G', b'T'],
            ),
            (
                block::NOTES,
                br#"<Notes><Created UTC="22:0:0" UTC="09:00:00">2022.12.13</Created></Notes>"#
                    .to_vec(),
            ),
        ]);
        let doc = parse(&raw).unwrap();
        assert_eq!(
            doc.molecule.notes[0].attrs.len(),
            2,
            "the reader reports what the file said; it is the writer that must refuse"
        );

        let (bytes, unwritable) = from_molecule_reporting(&doc.molecule);
        let payload = notes_payload(&bytes);
        assert_eq!(
            payload.matches("UTC=").count(),
            1,
            "one element, one attribute of that name: {payload}"
        );
        assert!(payload.contains(r#"UTC="22:0:0""#), "{payload}");
        assert_eq!(unwritable.len(), 1, "got {unwritable:?}");
        assert!(unwritable[0].contains("\"09:00:00\""), "{unwritable:?}");

        // The other route to the same output was `Note::set_attr`, which
        // appended despite its name, so two calls produced the state XML has no
        // spelling for and `Note::attr` then answered with the stale first one.
        let mut n = Note::new("Created", "2022.12.13");
        n.set_attr("UTC", "22:0:0");
        n.set_attr("UTC", "09:00:00");
        assert_eq!(n.attrs.len(), 1, "set_attr replaces: {:?}", n.attrs);
        assert_eq!(n.attr("UTC"), Some("09:00:00"));
    }

    #[test]
    fn a_name_xml_forbids_cannot_reach_the_file_even_when_rust_calls_it_alphanumeric() {
        // `is_xml_name` was `is_alphabetic()` then `is_alphanumeric() || _ : - .`
        // and its doc comment called that "a shade stricter than XML 1.0". It was
        // looser in the direction that costs: `char::is_alphanumeric` is true for
        // Unicode category No, so U+00B2 SUPERSCRIPT TWO passed and `<A²>` was
        // written — a tag `ET.fromstring` refuses. It was also a false *reject*
        // for U+00B7 MIDDLE DOT, which XML allows as a NameChar. Both directions
        // are asserted, because a rule that is wrong twice can be made green by
        // fixing either half.
        assert!(!is_xml_name("A\u{b2}"), "U+00B2 is in no NameChar range");
        assert!(is_xml_name("A\u{b7}"), "U+00B7 is a NameChar");
        assert!(!is_xml_name("\u{b7}A"), "...but not a NameStartChar");
        assert!(
            !is_xml_name("\u{aa}x"),
            "U+00AA is below NameStartChar's range"
        );
        assert!(is_xml_name("\u{c0}x"), "U+00C0 opens it");
        assert!(is_xml_name("Created") && is_xml_name("_x") && is_xml_name("a:b-c.d0"));
        assert!(!is_xml_name("") && !is_xml_name("0x") && !is_xml_name("a b"));

        let mut m = Molecule {
            seq: b"ACGT".to_vec(),
            ..Default::default()
        };
        m.notes.push(Note::new("A\u{b2}", "kept out"));
        let (bytes, unwritable) = from_molecule_reporting(&m);
        assert_eq!(unwritable.len(), 1, "got {unwritable:?}");
        assert!(
            !notes_payload(&bytes).contains('\u{b2}'),
            "{}",
            notes_payload(&bytes)
        );
    }

    #[test]
    fn a_value_split_by_a_comment_is_one_note_not_two() {
        // Pushing on every text event made `xml::scan`'s per-run events into
        // per-note entries, so a comment or a CDATA section inside a value
        // silently doubled the key.
        let (n, _) = parse_notes("<Notes><Created>2022<!--x-->.12.13</Created></Notes>");
        assert_eq!(n, vec![Note::new("Created", "2022.12.13")]);
    }

    #[test]
    fn the_utc_attribute_survives_read_write_read() {
        // The finding, end to end: `.dna` in, model, `.dna` out, model. The
        // whole point of `from_molecule` is that it throws the original blocks
        // away, so this is the path on which the time of day was lost.
        let raw = build(&[
            (block::HEADER, header_block()),
            (block::SEQUENCE, vec![flag::CIRCULAR, b'A', b'C', b'G', b'T']),
            (
                block::NOTES,
                br#"<Notes><Created UTC="22:0:0">2022.12.13</Created><LastModified UTC="9:16:3">2024.7.5</LastModified></Notes>"#.to_vec(),
            ),
        ]);
        let doc = parse(&raw).unwrap();
        assert_eq!(doc.molecule.notes[0].attr("UTC"), Some("22:0:0"));

        let rebuilt = parse(&from_molecule(&doc.molecule)).unwrap();
        assert_eq!(rebuilt.molecule.notes, doc.molecule.notes);
        assert_eq!(rebuilt.molecule.notes[0].attr("UTC"), Some("22:0:0"));
        assert_eq!(rebuilt.molecule.notes[1].attr("UTC"), Some("9:16:3"));

        // ...and on the bytes, because a round trip through a matching reader
        // and writer cancels: an attribute written onto the wrong element, or
        // with the wrong quoting, would be read back into the same place and
        // compare equal. The time is not reformatted either — real files write
        // `9:16:3` unpadded, and parsing it to a time type would rewrite it.
        let payload = notes_payload(&from_molecule(&doc.molecule));
        assert!(
            payload.contains(r#"<Created UTC="22:0:0">2022.12.13</Created>"#),
            "block 6 was {payload}"
        );
    }

    /// Block 6 of a synthesised file, as text.
    fn notes_payload(bytes: &[u8]) -> String {
        let doc = parse(bytes).expect("what we wrote, we can read");
        let b = doc
            .blocks
            .iter()
            .find(|b| b.kind == block::NOTES)
            .expect("no notes block");
        String::from_utf8_lossy(&b.payload).into_owned()
    }

    #[test]
    fn a_note_key_that_is_not_an_xml_name_is_reported_rather_than_written() {
        // `notes_xml` used to build the tag as `format!("<{k}>", k =
        // xml::escape(k))`. Escaping is for text and attribute values; there is
        // no escaping that makes an arbitrary string a legal XML Name. A key of
        // `A B` emitted `<A B>`, which reads back as an element `A` with a
        // valueless attribute `B` — the note renamed itself — and attribute
        // names, which now come from the same untrusted file, have the identical
        // problem.
        let mut m = Molecule {
            seq: b"ACGT".to_vec(),
            ..Default::default()
        };
        m.notes.push(Note::new("A B", "kept out"));
        m.notes.push(Note::new("Type", "Synthetic"));
        let mut bad_attr = Note::new("Created", "2022.12.13");
        bad_attr.set_attr("UTC time", "22:0:0");
        m.notes.push(bad_attr);

        let (bytes, unwritable) = from_molecule_reporting(&m);
        let payload = notes_payload(&bytes);
        assert!(
            !payload.contains("A B"),
            "an illegal name must not reach the file: {payload}"
        );
        assert!(payload.contains("<Type>Synthetic</Type>"));
        assert!(
            payload.contains("<Created>2022.12.13</Created>"),
            "the note survives; only the illegal attribute is refused: {payload}"
        );
        assert_eq!(unwritable.len(), 2, "got {unwritable:?}");
        assert!(unwritable[0].contains("\"A B\""), "{unwritable:?}");
        assert!(unwritable[1].contains("\"UTC time\""), "{unwritable:?}");

        // And what came out is still readable: the illegal key did not truncate
        // the block or turn the rest of it into attributes of a broken tag.
        let back = parse(&bytes).unwrap();
        assert_eq!(back.molecule.notes.len(), 2);
        assert_eq!(back.molecule.notes[0], Note::new("Type", "Synthetic"));
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
        m.notes.push(Note::new("Description", "a test"));
        // An attributed note in the shared fixture, so the ~10 tests that
        // round-trip this molecule — including
        // `writing_the_same_molecule_twice_gives_the_same_bytes` — cover the
        // attribute path for free rather than only where it is asserted.
        let mut created = Note::new("Created", "2022.12.13");
        created.set_attr("UTC", "22:0:0");
        m.notes.push(created);
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
