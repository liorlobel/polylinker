//! WebAssembly surface for the Polylinker core.
//!
//! # Why a hand-written C ABI
//!
//! `wasm-bindgen` would pull in a large dependency tree and emit a separate JS
//! glue file. The browser tool's whole point is that it is **one HTML file**
//! that works from a USB stick on a machine with no install rights and no
//! network, so a second file is not an option — and neither is a dependency
//! tree in the layer that decides whether a plasmid map is correct.
//!
//! The ABI is therefore deliberately tiny.
//!
//! # Protocol
//!
//! Results do not cross the boundary as return values. Each call leaves its
//! output in an internal buffer, and the caller reads it:
//!
//! ```js
//! const p = wasm.pl_alloc(bytes.length);
//! new Uint8Array(wasm.memory.buffer, p, bytes.length).set(bytes);
//! const ok = wasm.pl_open(p, bytes.length);      // 0 = parsed, 1 = failed
//! wasm.pl_free(p, bytes.length);
//! const json = readOut();                        // pl_out_ptr / pl_out_len
//! ```
//!
//! `pl_out_ptr` is only valid until the next call that writes output, and any
//! call may grow wasm memory and invalidate previously-taken views, so the
//! caller must re-create its `Uint8Array` after every call. That rule is the
//! one real sharp edge here, so it is stated twice on purpose.
//!
//! Coordinates in the JSON are 1-based inclusive, as everywhere else.

pub mod json;

use std::cell::RefCell;

use pl_core::{Molecule, Strand};
use pl_fileio::{genbank, snapgene, Format};

use json::Json;

#[derive(Default)]
struct State {
    molecule: Option<Molecule>,
    format: Option<Format>,
    /// Only present for `.dna`, and only so the container can be described.
    container: Option<snapgene::Document>,
    out: Vec<u8>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn set_out(s: String) {
    STATE.with(|st| st.borrow_mut().out = s.into_bytes());
}

fn error_json(msg: &str) -> String {
    let mut j = Json::new();
    j.obj().kv_str("error", msg).end_obj();
    j.finish()
}

// ---------------------------------------------------------------------------
// memory
// ---------------------------------------------------------------------------

/// Allocate `len` bytes for the caller to write into. Free it with [`pl_free`].
#[no_mangle]
pub extern "C" fn pl_alloc(len: usize) -> *mut u8 {
    let mut v: Vec<u8> = Vec::with_capacity(len);
    let ptr = v.as_mut_ptr();
    std::mem::forget(v);
    ptr
}

/// Release a buffer obtained from [`pl_alloc`]. `len` must match the request.
///
/// # Safety
/// `ptr` must come from `pl_alloc` with the same `len`, and must not be used
/// afterwards.
#[no_mangle]
pub unsafe extern "C" fn pl_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        drop(Vec::from_raw_parts(ptr, 0, len));
    }
}

/// Pointer to the current output buffer. Invalid after the next call.
#[no_mangle]
pub extern "C" fn pl_out_ptr() -> *const u8 {
    STATE.with(|st| st.borrow().out.as_ptr())
}

/// Length of the current output buffer.
#[no_mangle]
pub extern "C" fn pl_out_len() -> usize {
    STATE.with(|st| st.borrow().out.len())
}

/// ABI version, so a stale inlined module is detected rather than mis-read.
#[no_mangle]
pub extern "C" fn pl_abi_version() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// reading
// ---------------------------------------------------------------------------

/// Parse a file. Returns 0 on success, 1 on failure; either way the output
/// buffer holds JSON (a summary, or `{"error": "..."}`).
///
/// # Safety
/// `ptr`/`len` must describe an initialised buffer that stays valid for the call.
#[no_mangle]
pub unsafe extern "C" fn pl_open(ptr: *const u8, len: usize) -> i32 {
    if ptr.is_null() {
        set_out(error_json("null pointer"));
        return 1;
    }
    let data = std::slice::from_raw_parts(ptr, len);

    // Keep the container around for `.dna` so pl_blocks_json can describe it.
    let container = if pl_fileio::detect(data) == Some(Format::SnapGene) {
        snapgene::parse(data).ok()
    } else {
        None
    };

    match pl_fileio::load(data) {
        Ok((mol, fmt)) => {
            let summary = summary_json(&mol, fmt);
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.molecule = Some(mol);
                st.format = Some(fmt);
                st.container = container;
            });
            set_out(summary);
            0
        }
        Err(e) => {
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.molecule = None;
                st.format = None;
                st.container = None;
            });
            set_out(error_json(&e.to_string()));
            1
        }
    }
}

fn strand_str(s: Strand) -> &'static str {
    match s {
        Strand::Forward => "+",
        Strand::Reverse => "-",
        Strand::Both => "both",
        Strand::Unoriented => "none",
    }
}

fn summary_json(mol: &Molecule, fmt: Format) -> String {
    let mut j = Json::new();
    j.obj()
        .kv_str("format", fmt.name())
        .kv_str("name", &mol.name)
        .kv_str("description", &mol.description)
        .kv_num("bp", mol.len())
        .kv_num("span", mol.span())
        .kv_num("annotationSpan", mol.annotation_span())
        .kv_bool("circular", mol.topology.is_circular())
        .kv_bool("sequenceAbsent", mol.sequence_absent())
        .kv_bool("annotationTrack", mol.is_annotation_track())
        .kv_opt_float("gc", mol.gc_percent())
        .kv_num(
            "lowercase",
            mol.seq.iter().filter(|b| b.is_ascii_lowercase()).count() as u64,
        )
        .kv_num("ambiguous", mol.composition().other);

    // null when the source does not record strandedness, so the caller can stay
    // silent rather than assert a guess.
    match mol.double_stranded {
        Some(v) => j.kv_bool("doubleStranded", v),
        None => j.key("doubleStranded").null(),
    };

    j.key("methylation")
        .obj()
        .kv_bool("dam", mol.methylation.dam)
        .kv_bool("dcm", mol.methylation.dcm)
        .kv_bool("ecoki", mol.methylation.ecoki)
        .end_obj();

    j.key("features").arr();
    for f in &mol.features {
        j.obj()
            .kv_str("name", &f.name)
            .kv_str("kind", &f.kind)
            .kv_str("strand", strand_str(f.strand))
            .kv_num("start", f.start())
            .kv_num("end", f.end())
            .kv_opt_str("color", f.color());
        j.key("segments").arr();
        for s in &f.segments {
            j.obj()
                .kv_num("start", s.start)
                .kv_num("end", s.end)
                .kv_opt_str("color", s.color.as_deref())
                .kv_bool("translated", s.translated)
                .end_obj();
        }
        j.end_arr();
        j.key("qualifiers").arr();
        for (k, v) in &f.qualifiers {
            // `valueless` distinguishes a bare `/pseudo` from `/replace=""`.
            // Emitting both as `""` would tell a caller that a pseudogene
            // carries an empty note rather than a flag.
            j.obj()
                .kv_str("name", k)
                .kv_str("value", v.as_deref().unwrap_or(""))
                .kv_bool("valueless", v.is_none())
                .end_obj();
        }
        j.end_arr();
        j.end_obj();
    }
    j.end_arr();

    j.key("primers").arr();
    for p in &mol.primers {
        j.obj()
            .kv_str("name", &p.name)
            .kv_str("seq", &p.seq)
            .kv_str("description", &p.description);
        j.key("sites").arr();
        for s in &p.sites {
            j.obj()
                .kv_num("start", s.start)
                .kv_num("end", s.end)
                .kv_bool("reverse", s.strand.is_reverse())
                .kv_opt_float("tm", s.tm)
                .end_obj();
        }
        j.end_arr();
        j.end_obj();
    }
    j.end_arr();

    j.key("notes").arr();
    for (k, v) in &mol.notes {
        j.obj().kv_str("name", k).kv_str("value", v).end_obj();
    }
    j.end_arr();

    j.end_obj();
    j.finish()
}

/// The bases of the open molecule, raw, not JSON.
///
/// Kept out of the summary because a genome is megabytes and JSON-escaping it
/// would double the cost for no benefit. Returns 0, or 1 if nothing is open.
#[no_mangle]
pub extern "C" fn pl_sequence() -> i32 {
    STATE.with(|st| {
        let seq = st.borrow().molecule.as_ref().map(|m| m.seq.clone());
        match seq {
            Some(s) => {
                st.borrow_mut().out = s;
                0
            }
            None => {
                st.borrow_mut().out = error_json("no file open").into_bytes();
                1
            }
        }
    })
}

/// Container anatomy for a `.dna` file: one entry per block.
#[no_mangle]
pub extern "C" fn pl_blocks_json() -> i32 {
    STATE.with(|st| {
        let st_ref = st.borrow();
        let Some(doc) = st_ref.container.as_ref() else {
            drop(st_ref);
            set_out(error_json("not a SnapGene .dna file"));
            return 1;
        };
        let mut j = Json::new();
        j.obj()
            .kv_num("totalBytes", doc.total_bytes() as u64)
            .kv_num("derivedBytes", doc.derived_bytes() as u64)
            .kv_num("fileType", doc.file_type as u64)
            .kv_num("exportVersion", doc.export_version as u64)
            .kv_num("importVersion", doc.import_version as u64)
            .kv_bool("historyPresent", doc.history_present)
            .kv_bool("historyCompressed", doc.history_compressed);
        j.key("blocks").arr();
        for b in &doc.blocks {
            j.obj()
                .kv_num("kind", b.kind as u64)
                .kv_num("bytes", b.size_on_disk() as u64)
                .kv_bool("derived", b.is_derived())
                .kv_str("meaning", block_name(b.kind))
                .end_obj();
        }
        j.end_arr();
        j.end_obj();
        let out = j.finish();
        drop(st_ref);
        set_out(out);
        0
    })
}

fn block_name(kind: u8) -> &'static str {
    match kind {
        snapgene::block::SEQUENCE => "sequence",
        snapgene::block::CUTSITE_CACHE => "cut-site cache",
        snapgene::block::ENZYME_TABLE => "enzyme table",
        snapgene::block::PRIMERS => "primers",
        snapgene::block::NOTES => "notes",
        snapgene::block::HISTORY_TREE => "history tree",
        snapgene::block::EXTRA_PROPS => "extra properties",
        snapgene::block::HEADER => "header",
        snapgene::block::FEATURES => "features",
        snapgene::block::HISTORY_NODE => "history node",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// analysis
// ---------------------------------------------------------------------------

/// Digest the open molecule with the built-in enzyme set.
#[no_mangle]
pub extern "C" fn pl_digest_json() -> i32 {
    STATE.with(|st| {
        let st_ref = st.borrow();
        let Some(mol) = st_ref.molecule.as_ref() else {
            drop(st_ref);
            set_out(error_json("no file open"));
            return 1;
        };
        if mol.seq.is_empty() {
            drop(st_ref);
            set_out(error_json("this file carries no bases to digest"));
            return 1;
        }
        let results = pl_enzymes::digest_all(mol);
        let mut j = Json::new();
        j.arr();
        for d in &results {
            j.obj()
                .kv_str("enzyme", d.enzyme.name)
                .kv_str("site", d.enzyme.site)
                .kv_bool("blunt", d.enzyme.is_blunt());
            j.key("positions").arr();
            for p in &d.positions {
                j.num(*p);
            }
            j.end_arr();
            j.key("fragments").arr();
            for f in d.fragments(mol.len(), mol.topology) {
                j.num(f);
            }
            j.end_arr();
            j.end_obj();
        }
        j.end_arr();
        let out = j.finish();
        drop(st_ref);
        set_out(out);
        0
    })
}

// ---------------------------------------------------------------------------
// writing
// ---------------------------------------------------------------------------

/// # Safety
/// `title_ptr`/`title_len` must describe valid UTF-8 for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn pl_to_genbank(
    title_ptr: *const u8,
    title_len: usize,
    day: u32,
    month: u32,
    year: i32,
) -> i32 {
    let title = read_str(title_ptr, title_len);
    STATE.with(|st| {
        let st_ref = st.borrow();
        let Some(mol) = st_ref.molecule.as_ref() else {
            drop(st_ref);
            set_out(error_json("no file open"));
            return 1;
        };
        let text = genbank::write(mol, &title, (day, month as usize, year));
        drop(st_ref);
        set_out(text);
        0
    })
}

/// # Safety
/// See [`pl_to_genbank`].
#[no_mangle]
pub unsafe extern "C" fn pl_to_fasta(title_ptr: *const u8, title_len: usize, width: usize) -> i32 {
    let title = read_str(title_ptr, title_len);
    STATE.with(|st| {
        let st_ref = st.borrow();
        let Some(mol) = st_ref.molecule.as_ref() else {
            drop(st_ref);
            set_out(error_json("no file open"));
            return 1;
        };
        let text = pl_fileio::fasta::write(mol, &title, width);
        drop(st_ref);
        set_out(text);
        0
    })
}

/// Rewrite the open `.dna` file.
///
/// With `drop_derived` non-zero the two regenerable cache blocks are omitted,
/// which is the open experiment in `docs/DNA-FORMAT.md` §4: if SnapGene opens
/// the result, those blocks are optional on read and write support is close to
/// solved. With it zero the output is byte-identical to the input.
///
/// Only works when a `.dna` was opened, because this rewrites a container
/// rather than synthesising one — the writer preserves blocks it does not
/// understand instead of inventing them.
#[no_mangle]
pub extern "C" fn pl_to_dna(drop_derived: i32) -> i32 {
    STATE.with(|st| {
        let st_ref = st.borrow();
        let Some(doc) = st_ref.container.as_ref() else {
            drop(st_ref);
            set_out(error_json(
                "no .dna file open -- this rewrites a container, it does not synthesise one",
            ));
            return 1;
        };
        let bytes = snapgene::write(doc, drop_derived != 0);
        drop(st_ref);
        STATE.with(|s| s.borrow_mut().out = bytes);
        0
    })
}

/// The built-in enzyme set, so callers need no table of their own.
#[no_mangle]
pub extern "C" fn pl_enzymes_json() -> i32 {
    let mut j = Json::new();
    j.arr();
    for e in pl_enzymes::ENZYMES {
        j.obj()
            .kv_str("name", e.name)
            .kv_str("site", e.site)
            .kv_num("cutOffset", e.fst5 as u64)
            .kv_num("overhang", e.overhang_len() as u64)
            .kv_bool("blunt", e.is_blunt())
            .end_obj();
    }
    j.end_arr();
    set_out(j.finish());
    0
}

/// A GenBank-safe LOCUS name for a filename, useful for naming downloads.
///
/// # Safety
/// See [`pl_to_genbank`].
#[no_mangle]
pub unsafe extern "C" fn pl_locus_name(ptr: *const u8, len: usize) -> i32 {
    let title = read_str(ptr, len);
    set_out(genbank::locus_name(&title));
    0
}

unsafe fn read_str(ptr: *const u8, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned()
}

/// Rotate the open circular molecule so `origin` becomes position 1.
#[no_mangle]
pub extern "C" fn pl_rotate(origin: u64) -> i32 {
    STATE.with(|st| {
        let mut st_mut = st.borrow_mut();
        // Read the format before taking a mutable borrow of the molecule.
        let fmt = st_mut.format.unwrap_or(Format::GenBank);
        let Some(mol) = st_mut.molecule.as_mut() else {
            drop(st_mut);
            set_out(error_json("no file open"));
            return 1;
        };
        if !mol.rotate(origin) {
            drop(st_mut);
            set_out(error_json("cannot rotate: linear, or origin out of range"));
            return 1;
        }
        let out = summary_json(mol, fmt);
        drop(st_mut);
        set_out(out);
        0
    })
}

// These run on the host during `cargo test`; the wasm ABI is exercised for real
// by the browser test harness, which drives the actual module.
#[cfg(test)]
mod tests {
    use super::*;

    fn dna_fixture() -> Vec<u8> {
        let mut payload = snapgene::MAGIC.to_vec();
        payload.extend_from_slice(&1u16.to_be_bytes());
        payload.extend_from_slice(&15u16.to_be_bytes());
        payload.extend_from_slice(&19u16.to_be_bytes());

        let mut seq = vec![snapgene::flag::CIRCULAR | snapgene::flag::DOUBLE_STRANDED];
        seq.extend_from_slice(b"GAATTCaaaaaaaaaaGGATCCtttttttttt");

        let blocks = [
            (snapgene::block::HEADER, payload),
            (snapgene::block::SEQUENCE, seq),
            (
                snapgene::block::FEATURES,
                // ## delimiters: the colour value contains "# , which would
                // close a plain br#"..."# literal.
                br##"<Features><Feature name="test" type="CDS" directionality="1"><Segment range="1-10" color="#ff0000"/></Feature></Features>"##.to_vec(),
            ),
        ];
        let mut out = Vec::new();
        for (kind, payload) in blocks {
            out.push(kind);
            out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            out.extend_from_slice(&payload);
        }
        out
    }

    fn open(data: &[u8]) -> (i32, String) {
        let rc = unsafe { pl_open(data.as_ptr(), data.len()) };
        let out = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());
        (rc, out)
    }

    #[test]
    fn opens_a_dna_file_and_summarises_it() {
        let (rc, json) = open(&dna_fixture());
        assert_eq!(rc, 0, "{json}");
        assert!(json.contains(r#""format":"SnapGene .dna""#), "{json}");
        assert!(json.contains(r#""bp":32"#), "{json}");
        assert!(json.contains(r#""circular":true"#));
        assert!(json.contains(r#""name":"test""#));
        // Written with ## delimiters: a colour value contains "# , which closes
        // a plain r#"..."# string.
        assert!(json.contains(r##""color":"#ff0000""##), "{json}");
        assert!(
            json.contains(r#""lowercase":20"#),
            "case must be reported: {json}"
        );
    }

    #[test]
    fn a_bad_file_yields_error_json_not_a_panic() {
        let (rc, json) = open(b"not a sequence file at all");
        assert_eq!(rc, 1);
        assert!(json.starts_with(r#"{"error":"#), "{json}");
    }

    #[test]
    fn chromatograms_are_named_in_the_error() {
        let (rc, json) = open(b"ABIF\x00\x01\x02\x03");
        assert_eq!(rc, 1);
        assert!(json.contains("ABIF"), "{json}");
    }

    #[test]
    fn sequence_comes_back_raw_and_case_preserved() {
        open(&dna_fixture());
        assert_eq!(pl_sequence(), 0);
        let s = STATE.with(|st| st.borrow().out.clone());
        assert_eq!(s, b"GAATTCaaaaaaaaaaGGATCCtttttttttt".to_vec());
    }

    #[test]
    fn digest_finds_the_expected_cutters() {
        open(&dna_fixture());
        assert_eq!(pl_digest_json(), 0);
        let j = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());
        // EcoRI G^AATTC at 1 -> position 2; BamHI G^GATCC at 17 -> 18.
        assert!(
            j.contains(r#"{"enzyme":"EcoRI","site":"GAATTC","blunt":false,"positions":[2]"#),
            "{j}"
        );
        assert!(j.contains(r#""enzyme":"BamHI"#), "{j}");
    }

    #[test]
    fn blocks_describe_the_container() {
        open(&dna_fixture());
        assert_eq!(pl_blocks_json(), 0);
        let j = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());
        assert!(j.contains(r#""meaning":"header""#), "{j}");
        assert!(j.contains(r#""meaning":"sequence""#));
        assert!(j.contains(r#""derivedBytes":0"#));
    }

    #[test]
    fn blocks_refuses_politely_for_a_non_dna_file() {
        open(b">x\nACGT\n");
        assert_eq!(pl_blocks_json(), 1);
        let j = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());
        assert!(j.contains("not a SnapGene"), "{j}");
    }

    #[test]
    fn genbank_export_round_trips_through_the_reader() {
        open(&dna_fixture());
        let title = "fixture.dna";
        let rc = unsafe { pl_to_genbank(title.as_ptr(), title.len(), 26, 6, 2026) };
        assert_eq!(rc, 0);
        let gb = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());
        let back = genbank::parse(&gb);
        assert_eq!(back.seq, b"GAATTCaaaaaaaaaaGGATCCtttttttttt".to_vec());
        assert!(back.topology.is_circular());
        assert_eq!(back.features.len(), 1);
    }

    #[test]
    fn export_without_an_open_file_is_an_error_not_a_crash() {
        STATE.with(|st| *st.borrow_mut() = State::default());
        assert_eq!(pl_sequence(), 1);
        assert_eq!(pl_digest_json(), 1);
        let rc = unsafe { pl_to_fasta("x".as_ptr(), 1, 70) };
        assert_eq!(rc, 1);
    }

    #[test]
    fn rotation_moves_annotations_with_the_sequence() {
        open(&dna_fixture());
        assert_eq!(pl_rotate(17), 0);
        assert_eq!(pl_sequence(), 0);
        let s = STATE.with(|st| st.borrow().out.clone());
        assert!(s.starts_with(b"GGATCC"), "{}", String::from_utf8_lossy(&s));
    }

    #[test]
    fn rotating_a_linear_molecule_is_refused() {
        open(b">x\nACGTACGT\n");
        assert_eq!(pl_rotate(3), 1);
    }

    #[test]
    fn abi_version_is_exposed() {
        assert_eq!(pl_abi_version(), 1);
    }

    #[test]
    fn rewriting_a_dna_file_is_byte_exact_and_can_shed_caches() {
        let raw = dna_fixture();
        open(&raw);
        assert_eq!(pl_to_dna(0), 0);
        let same = STATE.with(|st| st.borrow().out.clone());
        assert_eq!(same, raw, "rewrite with caches kept must be byte-exact");

        assert_eq!(pl_to_dna(1), 0);
        let slim = STATE.with(|st| st.borrow().out.clone());
        // The fixture carries no cache blocks, so dropping them changes nothing.
        assert_eq!(slim, raw);
    }

    #[test]
    fn rewriting_dna_is_refused_for_a_non_dna_file() {
        open(b">x\nACGT\n");
        assert_eq!(pl_to_dna(0), 1);
        let j = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());
        assert!(j.contains("does not synthesise"), "{j}");
    }

    #[test]
    fn enzyme_set_is_published() {
        assert_eq!(pl_enzymes_json(), 0);
        let j = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());
        assert!(j.contains(r#"{"name":"AatII","site":"GACGTC""#), "{j}");
        assert_eq!(j.matches(r#""name":"#).count(), pl_enzymes::ENZYMES.len());
    }
}
