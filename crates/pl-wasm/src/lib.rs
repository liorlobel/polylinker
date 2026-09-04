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
use pl_fileio::{genbank, snapgene, Format, LoadReport};

use json::Json;

#[derive(Default)]
struct State {
    molecule: Option<Molecule>,
    format: Option<Format>,
    /// What the file held beyond the one record we keep.
    ///
    /// Kept because every other front door in this project has it and this one
    /// did not: `pl info` prints "records N in this file; showing the first",
    /// the desktop GUI appends "showing record 1 of N", `pl-mcp` prefixes its
    /// answers with the same note, and `pl convert` refuses outright. Here the
    /// report was dropped inside `pl_fileio::load`, so a 3-record FASTA opened
    /// as record 1 with nothing anywhere in the ABI saying so.
    report: LoadReport,
    /// Only present for `.dna`, and only so the container can be described.
    container: Option<snapgene::Document>,
    out: Vec<u8>,
    /// What the last successful call cost, when it cost something short of a
    /// refusal. See [`pl_warn_ptr`].
    warn: Vec<u8>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Publish an output buffer and CLEAR the warning buffer.
///
/// Clearing is the whole reason this is not two assignments at the call sites.
/// `warn` outlives one call the way `out` does — the page reads both after the
/// function returns — so a warning left over from an earlier export would
/// attach itself to the next one, and the next one might be a file with
/// nothing wrong with it. Every path that produces output goes through this,
/// [`set_out_bytes`] or [`set_out_warned`], and only the last of the three
/// leaves anything in the buffer.
fn set_out(s: String) {
    set_out_bytes(s.into_bytes());
}

/// The same, for output that is not text: raw bases, a rewritten `.dna`.
///
/// It exists so that the sentence above — every path that produces output
/// clears the warning — is true rather than nearly true. `pl_sequence` and
/// `pl_to_dna` hand over `Vec<u8>` and so could not call `set_out`; both
/// assigned `st.out` directly, which left a previous export's notice standing
/// beside output that had nothing to do with it.
fn set_out_bytes(bytes: Vec<u8>) {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.out = bytes;
        st.warn.clear();
    });
}

/// Publish an output buffer together with what producing it cost.
///
/// The pair is set in one call so there is no ordering to get wrong. Separate
/// setters would work only in one order — the output one clears the warning —
/// and a caller that wrote them the other way round would throw the warning
/// away silently, which is exactly the failure this channel exists to close.
fn set_out_warned(s: String, warning: String) {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.out = s.into_bytes();
        st.warn = warning.into_bytes();
    });
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
    // Reject a length the slice/allocator precondition forbids (`> isize::MAX`),
    // and return null on allocation failure instead of trapping: `with_capacity`
    // routes OOM through `handle_alloc_error` → a wasm `unreachable`, but the ABI
    // otherwise expects a null the caller can check. `try_reserve_exact` keeps
    // capacity == len, so `pl_free`'s `from_raw_parts(ptr, 0, len)` still matches.
    if len > isize::MAX as usize {
        return std::ptr::null_mut();
    }
    let mut v: Vec<u8> = Vec::new();
    if v.try_reserve_exact(len).is_err() {
        return std::ptr::null_mut();
    }
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
    // The same `> isize::MAX` refusal `pl_alloc`, `pl_open` and `read_str` make.
    // e0109f8 added it to those three and left this one, so the commit's claim
    // that "the FFI entry points reject len > isize::MAX before from_raw_parts"
    // was true of every such call but this. A capacity above `isize::MAX` cannot
    // have come from `pl_alloc` — it refuses those — so this can only be reached
    // by a caller that broke the documented precondition, and reconstructing the
    // Vec would hand the allocator a capacity it never issued.
    if !ptr.is_null() && len > 0 && len <= isize::MAX as usize {
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

/// Pointer to the warning buffer: what the last successful call COST.
///
/// Empty except after a call that produced real output and had something to
/// say about it. Read it after a zero return code; a non-zero code puts its
/// reason in the output buffer instead, and clears this.
///
/// # Why a second buffer exists at all
///
/// [`unwritable_refusal`] below is built on the fact that a download has one
/// buffer and one return code, so a write cannot both hand over the bytes and
/// hand over the report — and concludes that a file which would be missing
/// something must not be written here at all. That is right for a missing
/// annotation and much too strong for a reduced one: for a few hours on
/// 2026-09-03 a primer's dropped free-text description was reported through
/// the same channel, and this build stopped exporting any molecule whose
/// primer merely carried a note. Reverting it kept the export working and
/// left the loss silent.
///
/// The premise, not the conclusion, was what had to give. There is one buffer
/// because nobody had written a second one. With this, a reduced export both
/// downloads and says what it cost — which is what every other surface in the
/// project already does, and the reason `pl convert` and the desktop editor
/// never faced this choice.
#[no_mangle]
pub extern "C" fn pl_warn_ptr() -> *const u8 {
    STATE.with(|st| st.borrow().warn.as_ptr())
}

/// Length of the warning buffer; 0 when the last call cost nothing.
#[no_mangle]
pub extern "C" fn pl_warn_len() -> usize {
    STATE.with(|st| st.borrow().warn.len())
}

/// ABI version, so a stale inlined module is detected rather than mis-read.
///
/// 2 since 2026-09-04: [`pl_warn_ptr`] and [`pl_warn_len`] were added, and a
/// page built against ABI 1 would download a reduced export without showing
/// the notice that now exists for it. That is precisely the silence this
/// version added a channel to end, so it is a version bump and not an
/// additive-and-therefore-free change.
#[no_mangle]
pub extern "C" fn pl_abi_version() -> u32 {
    2
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
    // `from_raw_parts` is instant UB for `len > isize::MAX`; on wasm32 a
    // 32-bit `len` can name that range, so a hand-forged len is refused here.
    if len > isize::MAX as usize {
        set_out(error_json("length exceeds isize::MAX"));
        return 1;
    }
    let data = std::slice::from_raw_parts(ptr, len);

    // Keep the container around for `.dna` so pl_blocks_json can describe it.
    let container = if pl_fileio::detect(data) == Some(Format::SnapGene) {
        snapgene::parse(data).ok()
    } else {
        None
    };

    // `load_with_report`, not `load`: `load` drops the `LoadReport` in its own
    // body, so nothing downstream could learn that the file held more than the
    // record we keep, whether the topology was declared or merely defaulted, or
    // that the parse produced something that does not look like a molecule. A
    // 3-record FASTA opened as record 1, `pl_to_genbank` then wrote that one
    // record out as though it were the file, and both calls returned 0.
    match pl_fileio::load_with_report(data) {
        Ok((mol, fmt, report)) => {
            let summary = summary_json(&mol, fmt, &report);
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.molecule = Some(mol);
                st.format = Some(fmt);
                st.report = report;
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
                st.report = LoadReport::default();
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

/// A JSON array of strings, always emitted even when empty.
///
/// Same contract as `sites` and `attrs` below: a key that appears only
/// sometimes is the harder one to consume.
fn str_array(j: &mut Json, key: &str, items: &[String]) {
    j.key(key).arr();
    for s in items {
        j.str(s);
    }
    j.end_arr();
}

fn summary_json(mol: &Molecule, fmt: Format, report: &LoadReport) -> String {
    let mut j = Json::new();
    j.obj()
        .kv_str("format", fmt.name())
        // Records in the *file*, not records returned: only the first is open.
        // Staying silent is how 1,879 features went missing from a 124-record
        // `.gbk` without anyone noticing, and the browser was the last front
        // door with no way to say it. Spelled like the CLI's `records_in_file`
        // rather than plain `records` because `features`, `primers` and `notes`
        // in this same object are arrays, and a scalar named `records` reads
        // like a fourth one.
        .kv_num("recordsInFile", report.records as u64)
        .kv_str("name", &mol.name)
        .kv_str("description", &mol.description)
        .kv_num("bp", mol.len())
        .kv_num("span", mol.span())
        .kv_num("annotationSpan", mol.annotation_span())
        .kv_bool("circular", mol.topology.is_circular())
        // Did the file *say* so? `circular: false` conflates "this file says
        // linear" with "this file has no topology field", and FASTA never has
        // one. A Plasmidsaurus plasmid assembly arrives as FASTA at an
        // arbitrary rotation, so reporting it as linear with no hedge loses
        // exactly the origin-straddling sites it was sequenced to check.
        .kv_bool("topologyDeclared", report.topology_declared)
        .kv_bool("sequenceAbsent", mol.sequence_absent())
        .kv_bool("annotationTrack", mol.is_annotation_track())
        // `genbank::parse` and `fasta::parse` cannot fail, so a file of noise
        // that happens to contain the word LOCUS opens "successfully" as an
        // empty molecule and the page draws a blank map with no error. An
        // observation, not a diagnosis -- a corrupt file and an exotic one look
        // the same from here.
        .kv_bool("suspect", report.suspect)
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
    // `Feature::start`/`end` are a min and a max over the segments, which is the
    // extent only for an ordinary spliced join. An origin-crossing feature
    // reaches the browser in the join form `genbank::write` emits —
    // `join(2677..2686,1..7)` — whose minimum start is always exactly 1 and
    // whose maximum end is always exactly the molecule length, so a 17 bp
    // promoter arrived as `"start": 1, "end": 2686` and the page had nothing to
    // tell it otherwise. `extent` reports the pair the way `Molecule::subseq`
    // reads one: `end < start` means the span crosses the origin.
    let feat_span = mol.span();
    let feat_circular = mol.topology.is_circular();
    for f in &mol.features {
        let (fs, fe) = f
            .extent(feat_span, feat_circular)
            .unwrap_or((f.start(), f.end()));
        j.obj()
            .kv_str("name", &f.name)
            .kv_str("kind", &f.kind)
            .kv_str("strand", strand_str(f.strand))
            .kv_num("start", fs)
            .kv_num("end", fe)
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
    for n in &mol.notes {
        j.obj().kv_str("name", &n.key).kv_str("value", &n.value);
        // `attrs` is emitted even when empty, exactly as `sites` is above. A key
        // that appears only sometimes is the harder contract to consume, and the
        // half of `<Created UTC="22:0:0">2022.12.13</Created>` that lives in the
        // attribute has nowhere else to go — the alternative was concatenating
        // it into `value`, which every consumer would then have to un-parse.
        j.key("attrs").arr();
        for (k, v) in &n.attrs {
            j.obj().kv_str("name", k).kv_str("value", v).end_obj();
        }
        j.end_arr();
        j.end_obj();
    }
    j.end_arr();

    // The last two channels of the load report. `unrepresentableLocations` is
    // what `pl info` prints as "location(s) this reader cannot represent" -- a
    // `bond(...)` operator or a remote reference such as `J00194.1:200..300`,
    // which would otherwise leave a feature quietly claiming a span it does not
    // have. `unrepresentableNotes` is its `.dna` sibling, e.g. a
    // `<References><Reference/></References>` subtree the note model has no
    // shape for. Deliberately two keys and not one: folding a notes path into
    // the locations list would have a caller say something false about
    // coordinates, which is the mistake this whole channel exists to avoid.
    str_array(
        &mut j,
        "unrepresentableLocations",
        &report.unrepresentable_locations,
    );
    str_array(
        &mut j,
        "unrepresentableNotes",
        &report.unrepresentable_notes,
    );

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
                set_out_bytes(s);
                0
            }
            None => {
                set_out(error_json("no file open"));
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

/// Why an export refuses a file that held more records than we kept.
///
/// `pl convert` errors out on exactly this condition, and a viewer's "Save as
/// GenBank" is the same operation with a different destination. Before this,
/// a 3-record FASTA exported to GenBank produced a complete, plausible,
/// one-record file with records 2 and 3 gone, return code 0, and nothing
/// anywhere saying so.
///
/// The refusal is written into the **output buffer** and not only signalled by
/// the return code. It had to be, because the shipped page discarded the return
/// value of these two exports and downloaded the buffer regardless, so a
/// code-only signal would have been a silent no-op there — the same defect in a
/// new place. It still has to be, for the opposite reason: those handlers were
/// fixed and now read `if (rc !== 0) { fail(coreText()); return; }`
/// (`prototype/dna-reader.template.html`, the `expGb`/`expFa` handlers), and
/// `coreText()` is this buffer. The reason the user sees is the string put here.
/// A download whose contents are the refusal is ugly; a download that looks like
/// the user's plasmid and is not is worse; and a refusal with no reason in it is
/// a dialog that says only "no".
fn truncation_refusal(report: &LoadReport) -> Option<String> {
    report.truncated().then(|| {
        format!(
            "this file holds {} records and writing it here would keep only the first. Split the file first.",
            report.records
        )
    })
}

/// Why a GenBank export refuses a molecule the format cannot hold whole.
///
/// `genbank::write` is `write_reporting(..).0` and its own doc says to prefer
/// the reporting form "anywhere the caller can tell the user what the format
/// could not carry — an annotation GenBank has no form for leaves no trace in
/// the file it is missing from". This module called `write`, so a feature whose
/// every segment has no GenBank location, a primer binding site past the end, a
/// control character flattened out of DEFINITION and an ORIGIN character
/// rewritten as `n` were all computed, formatted into strings, and dropped —
/// and what came back to the page was a complete, plausible `.gb` of the user's
/// plasmid with something missing from it. That is the shape
/// [`truncation_refusal`] above exists for, one class down: a download that
/// looks like the user's plasmid and is not.
///
/// **This buffer was the only channel there was.** The desktop GUI writes the
/// file AND says what it cost, because it has a status line to say it in; `pl
/// convert` does the same through stderr. Here there was one output buffer and
/// one return code, so a write could not both hand over the bytes and hand
/// over the report. Between shipping a lossy file silently and refusing with
/// the reason in the buffer, this crate has already chosen once — see
/// `truncation_refusal` — and the page reads it the way that choice needs:
/// `expGb` checks the return code and calls `fail(coreText())` rather than
/// downloading, so the refusal is shown and nothing is saved.
///
/// # 2026-09-04: this takes `absent` only
///
/// `write_reporting` used to return one vector and it now returns two — see
/// `pl_fileio::genbank::WriteReport`. This function takes the ABSENT half: an
/// annotation that is not in the file. The REDUCED half — an annotation that
/// IS in the file with something taken off it, a `/note` whose line break
/// became a space, a primer's description GenBank has nowhere to put — used to
/// arrive here too, and refusing a whole plasmid over a primer's note is not a
/// trade worth making. Those go to [`pl_warn_ptr`], which is the second buffer
/// the paragraph above says did not exist: the file downloads AND the page
/// says what it cost. Nothing became silent in the move.
///
/// **The consequence, stated plainly:** `pl convert file.gb --to genbank`
/// writes such a file and prints the report; this returns 1 and writes none. A
/// byte-for-byte comparison of the two over a corpus (`tests/drive_wasm.mjs`,
/// which CI runs without a corpus directory) would now differ on any file with
/// a non-empty `absent` list — a reduced-only file writes the same bytes on
/// both sides, since that comparison reads `out` and never `warn`. The
/// divergence is deliberate. It was once justified by the browser having no
/// second channel for the hedge; since [`pl_warn_ptr`] that premise is gone
/// and the conclusion is not, because an annotation the file does not contain
/// is not something a notice beside a download can make true — and a hedge
/// that cannot be printed must not be swallowed.
fn unwritable_refusal(unwritable: &[String]) -> Option<String> {
    (!unwritable.is_empty()).then(|| {
        format!(
            "GenBank has no form for {} thing(s) in this file, so writing it here would hand back a plausible plasmid with those parts missing and nothing saying so: {}. Nothing was written.",
            unwritable.len(),
            unwritable.join("; ")
        )
    })
}

/// What a written file cost, for the buffer the page shows beside the download.
///
/// The sibling of [`unwritable_refusal`] and deliberately the other shape: that
/// one blocks the write and this one annotates it. The wording has to carry
/// that difference on its own, because the user meets one of these two
/// sentences with no idea which function produced it — "Nothing was written"
/// there, "this file has them" here.
fn reduction_notice(reduced: &[String]) -> Option<String> {
    (!reduced.is_empty()).then(|| {
        format!(
            "{} thing(s) in this document have no GenBank form and were written in a reduced form; the file is saved and this is what it cost: {}",
            reduced.len(),
            reduced.join("; ")
        )
    })
}

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
        if let Some(msg) = truncation_refusal(&st_ref.report) {
            drop(st_ref);
            set_out(error_json(&msg));
            return 1;
        }
        // `write_reporting`, never `write`: see [`unwritable_refusal`] for what
        // the plain wrapper threw away and why this refuses instead of handing
        // back a plasmid with parts of it quietly absent.
        let date = (day, month as usize, year);
        let (text, report) = genbank::write_reporting(mol, &title, date);
        if let Some(msg) = unwritable_refusal(&report.absent) {
            drop(st_ref);
            set_out(error_json(&msg));
            return 1;
        }
        drop(st_ref);
        // The reduced half downloads and is SAID, rather than being either
        // swallowed or promoted into a refusal. See [`pl_warn_ptr`].
        match reduction_notice(&report.reduced) {
            Some(msg) => set_out_warned(text, msg),
            None => set_out(text),
        }
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
        // Refused for the same reason as GenBank above: a multi-FASTA in gives
        // a single-record FASTA out, which is the one case where the output
        // format makes the loss look like the whole file.
        if let Some(msg) = truncation_refusal(&st_ref.report) {
            drop(st_ref);
            set_out(error_json(&msg));
            return 1;
        }
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
        set_out_bytes(bytes);
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
    // `from_raw_parts` is instant UB for `len > isize::MAX`; treat a forged
    // over-range len as empty rather than construct an invalid slice.
    if ptr.is_null() || len == 0 || len > isize::MAX as usize {
        return String::new();
    }
    String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned()
}

/// Rotate the open circular molecule so `origin` becomes position 1.
#[no_mangle]
pub extern "C" fn pl_rotate(origin: u64) -> i32 {
    STATE.with(|st| {
        let mut st_mut = st.borrow_mut();
        // Read the format and the load report before taking a mutable borrow of
        // the molecule. The report is cloned rather than re-derived: rotating
        // does not change what the file held, and a summary that dropped the
        // record count on rotation would put the warning back to sleep for any
        // caller that re-reads it after a rotate.
        let fmt = st_mut.format.unwrap_or(Format::GenBank);
        let report = st_mut.report.clone();
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
        let out = summary_json(mol, fmt, &report);
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

    /// The same fixture with a block 6 appended.
    fn dna_with_notes(notes: &str) -> Vec<u8> {
        let mut out = dna_fixture();
        out.push(snapgene::block::NOTES);
        out.extend_from_slice(&(notes.len() as u32).to_be_bytes());
        out.extend_from_slice(notes.as_bytes());
        out
    }

    #[test]
    fn a_notes_attribute_reaches_the_browser_as_its_own_field() {
        // Nothing looked at notes on this path: `dna_fixture` has no block 6,
        // and `tests/drive_wasm.mjs` — the driver that compares this module's
        // output against `pl.exe --json` over a corpus and reports
        // "identical: 33/33" — does not mention notes at all. So `attrs` could
        // be emitted empty on every note with every check in the repository
        // green, while the browser reader silently lost the half of
        // `<Created UTC="22:0:0">2022.12.13</Created>` that lives in the
        // attribute. Two hand-written serialisers now emit this shape (the other
        // is `pl info --json`); this pins one of them.
        let (rc, json) = open(&dna_with_notes(
            r#"<Notes><Created UTC="22:0:0">2022.12.13</Created><Empty/></Notes>"#,
        ));
        assert_eq!(rc, 0, "{json}");
        assert!(
            json.contains(
                r#"{"name":"Created","value":"2022.12.13","attrs":[{"name":"UTC","value":"22:0:0"}]}"#
            ),
            "{json}"
        );
        // `attrs` is present and empty rather than absent, which is the contract
        // the comment at the emission site states and the only one a consumer
        // can read without a fallback.
        assert!(
            json.contains(r#"{"name":"Empty","value":"","attrs":[]}"#),
            "an attribute-free note still carries the key: {json}"
        );
    }

    #[test]
    fn a_bad_file_yields_error_json_not_a_panic() {
        let (rc, json) = open(b"not a sequence file at all");
        assert_eq!(rc, 1);
        assert!(json.starts_with(r#"{"error":"#), "{json}");
    }

    /// The allocator ABI, which e0109f8 changed and left untested.
    ///
    /// That commit made `pl_alloc` return null instead of routing failure
    /// through `handle_alloc_error` to a wasm trap. Null is the friendlier
    /// contract only if callers check it — address 0 is inside linear memory, so
    /// `set(bytes, 0)` overwrites the module's own data without throwing — and
    /// the two JS callers did not. They do now; this pins the Rust half.
    #[test]
    fn an_over_range_allocation_returns_null_rather_than_trapping() {
        // Refused for exceeding the slice precondition, not attempted.
        assert!(pl_alloc(usize::MAX).is_null());
        assert!(pl_alloc(isize::MAX as usize + 1).is_null());
        // The boundary itself is a request, not a refusal: it will fail in the
        // allocator and come back null too, but by the other route.
        let ok = pl_alloc(64);
        assert!(!ok.is_null(), "an ordinary allocation must still succeed");
        unsafe { pl_free(ok, 64) };
        // A length no `pl_alloc` can have issued is not handed to the allocator
        // as a capacity. Nothing to observe but the absence of a crash.
        unsafe { pl_free(std::ptr::null_mut(), 0) };
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

    /// PROVEN TO FAIL at f0e4a6f: `pl_to_genbank` called `genbank::write`,
    /// which is `write_reporting(..).0`, so a feature the format has no
    /// location for was skipped by the writer and the page downloaded a
    /// complete, plausible `.gb` of the user's plasmid with that feature
    /// silently absent — rc 0, empty report, nothing anywhere saying so.
    ///
    /// MUTATION THAT RE-BREAKS IT: in `pl_to_genbank`, replace
    /// `let (text, report) = genbank::write_reporting(mol, &title, date);`
    /// with `let (text, report) = (genbank::write(mol, &title, date),
    /// genbank::WriteReport::default());`. `unwritable_refusal` then always
    /// returns `None`, rc is 0 and the buffer holds the lossy file.
    #[test]
    fn a_genbank_export_refuses_rather_than_dropping_what_it_cannot_write() {
        // Twelve bases, and a feature at 30..40. `parse_location` accepts that
        // — it checks only `end >= start && start > 0` and has no length to
        // compare against, which is why pl-fileio ships a corpus survey for the
        // class — and `location_parts(30, 40, 12)` then returns `None`, so the
        // writer skips the feature. The skip is not the finding; the silence
        // was. Built by concatenation and not with `\`-continued source lines:
        // that escape eats the leading whitespace of the next line and a
        // GenBank feature table is column-significant.
        let gb = concat!(
            "LOCUS       x                        12 bp    DNA     linear SYN 26-JUL-2026\n",
            "FEATURES             Location/Qualifiers\n",
            "     misc_feature    30..40\n",
            "                     /label=\"tet leader\"\n",
            "ORIGIN\n        1 acgtacgtacgt\n//\n"
        );
        let (rc, json) = open(gb.as_bytes());
        assert_eq!(rc, 0, "{json}");
        assert!(json.contains("tet leader"), "the premise: {json}");

        let title = "past-end.gb";
        let rc = unsafe { pl_to_genbank(title.as_ptr(), title.len(), 26, 7, 2026) };
        let out = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());
        assert_eq!(rc, 1, "the export must refuse: {out}");
        assert!(out.starts_with(r#"{"error":"#), "{out}");
        assert!(
            out.contains("tet leader"),
            "and name what would have gone missing: {out}"
        );
        assert!(
            !out.contains("LOCUS"),
            "nothing that looks like the plasmid may be handed back: {out}"
        );

        // THE CONTROL, so the fix cannot be "refuse everything": a molecule
        // GenBank can hold whole still exports, at rc 0, with the file in the
        // buffer.
        let (rc, json) = open(&dna_fixture());
        assert_eq!(rc, 0, "{json}");
        let rc = unsafe { pl_to_genbank(title.as_ptr(), title.len(), 26, 7, 2026) };
        let out = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());
        assert_eq!(rc, 0, "{out}");
        assert!(out.contains("LOCUS"), "{out}");
        assert_eq!(
            pl_warn_len(),
            0,
            "a clean export must not hand the page a notice to show"
        );
    }

    /// A REDUCED export downloads, and says what it cost.
    ///
    /// PROVEN TO FAIL on 2026-09-04 with `pl_warn_len()` at 0 and no channel
    /// to put a value in: before this, `write_reporting` returned one vector
    /// and both severities went down it, so this molecule met one of two
    /// outcomes and both were wrong.
    ///
    /// - Report the description through `unwritable`, as this crate did for a
    ///   few hours on 2026-09-03, and `unwritable_refusal` fires: rc 1, no
    ///   download, and the page tells a user their perfectly ordinary plasmid
    ///   cannot be exported because one primer carries a note. That is what
    ///   the revert removed.
    /// - Leave it out, which is what the revert did, and the file downloads
    ///   with the description gone and nothing anywhere saying so — the exact
    ///   shape `unwritable_refusal`'s own doc calls worse than a refusal: "a
    ///   download that looks like the user's plasmid and is not".
    ///
    /// The third outcome needed a second buffer, and the argument that there
    /// could not be one — one buffer, one return code — was a description of
    /// the ABI rather than a constraint on it. See [`pl_warn_ptr`].
    #[test]
    fn a_reduced_export_downloads_and_says_what_it_cost() {
        // A `.dna` built the way SnapGene builds one, so the description
        // reaches the molecule through the real reader rather than being
        // planted in it: `<Primer description="..."><BindingSite/></Primer>`
        // is block 5, and `pl convert x.dna --to dna` round-trips it exactly.
        let mut mol = Molecule {
            seq: b"GAATTCaaaaaaaaaaGGATCCtttttttttt".to_vec(),
            topology: pl_core::Topology::Circular,
            ..Default::default()
        };
        mol.primers.push(pl_core::Primer {
            name: "M13F".into(),
            seq: "GTAAAACGACGGCCAGT".into(),
            description: "anneals in the linker, use at 58 C".into(),
            sites: vec![pl_core::BindingSite {
                start: 2,
                end: 8,
                strand: Strand::Forward,
                tm: Some(55.3),
            }],
        });
        let dna = snapgene::from_molecule(&mol);
        let (rc, json) = open(&dna);
        assert_eq!(rc, 0, "{json}");
        assert!(
            json.contains("anneals in the linker"),
            "the premise: the page has the description before the export: {json}"
        );

        let title = "described.dna";
        let rc = unsafe { pl_to_genbank(title.as_ptr(), title.len(), 4, 8, 2026) };
        let out = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());

        // IT DOWNLOADS. This is the half the 2026-09-03 revert was protecting.
        assert_eq!(rc, 0, "a reduction is not a reason to refuse a file: {out}");
        assert!(out.contains("LOCUS"), "{out}");
        assert!(
            out.contains("primer_bind") && out.contains("M13F"),
            "the primer itself is in the file:\n{out}"
        );
        assert!(
            !out.contains("anneals in the linker"),
            "the premise: GenBank really has nowhere to put it:\n{out}"
        );

        // AND IT SAYS SO. This is the half the revert cost.
        let warn = STATE.with(|st| String::from_utf8(st.borrow().warn.clone()).unwrap());
        assert!(!warn.is_empty(), "the loss is silent again");
        assert_eq!(warn.len(), pl_warn_len(), "the ABI must expose all of it");
        assert!(
            warn.contains("anneals in the linker"),
            "and it has to name the text, which now exists nowhere else: {warn}"
        );
        assert!(
            warn.contains("saved"),
            "the wording has to distinguish this from a refusal, because the \
             user meets one of the two sentences and cannot see which \
             function wrote it: {warn}"
        );

        // AND THE NOTICE DOES NOT OUTLIVE ITS EXPORT. `warn` is read after the
        // call returns, exactly as `out` is, so a stale one would attach
        // itself to the next file — and the next file may be clean.
        let (rc, json) = open(&dna_fixture());
        assert_eq!(rc, 0, "{json}");
        assert_eq!(pl_warn_len(), 0, "opening a file cleared nothing");
        let rc = unsafe { pl_to_genbank(title.as_ptr(), title.len(), 4, 8, 2026) };
        assert_eq!(rc, 0);
        assert_eq!(
            pl_warn_len(),
            0,
            "the previous export's notice is still on offer"
        );
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
        assert_eq!(pl_abi_version(), 2);
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

    // ---- what the file held beyond record 1 -------------------------------
    //
    // Every one of these went through `pl_fileio::load`, which drops the
    // `LoadReport` inside its own body. Nothing in this crate could see any of
    // it, and nothing in this test module asked.

    const THREE_FASTA: &[u8] = b">plasmidA first\nGAATTCAAAAAAAAAAAAAAAA\n\
>plasmidB second\nGGATCCTTTTTTTTTTTTTTTT\n\
>plasmidC third\nAAAAAAAAAAAAAAAAAAAAAA\n";

    #[test]
    fn a_multi_record_file_says_how_many_records_it_held() {
        // A 3-record FASTA opened as `plasmidA` and the summary described it as
        // though it were the file: no record count anywhere in the ABI, while
        // `pl info` prints "records 3 in this file; showing the first" and the
        // desktop GUI appends "showing record 1 of 3".
        let (rc, json) = open(THREE_FASTA);
        assert_eq!(rc, 0, "{json}");
        assert!(json.contains(r#""name":"plasmidA""#), "{json}");
        assert!(
            json.contains(r#""recordsInFile":3"#),
            "the browser must be told what it is not showing: {json}"
        );
    }

    #[test]
    fn exports_refuse_to_hand_over_record_one_as_the_whole_file() {
        // Both of these returned 0 and produced a complete, plausible
        // single-record file. `pl convert multi.fa --to genbank` refuses the
        // identical operation; the browser did it silently, and the page throws
        // the return code away and downloads the buffer either way — so the
        // refusal has to be *in the buffer*.
        let (rc, _) = open(THREE_FASTA);
        assert_eq!(rc, 0);

        let title = "multi.fa";
        let rc = unsafe { pl_to_genbank(title.as_ptr(), title.len(), 26, 7, 2026) };
        let out = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());
        assert_eq!(rc, 1, "GenBank export must refuse: {out}");
        assert!(out.starts_with(r#"{"error":"#), "{out}");
        assert!(out.contains("3 records"), "{out}");
        assert!(!out.contains("LOCUS"), "no record may be written: {out}");

        let rc = unsafe { pl_to_fasta(title.as_ptr(), title.len(), 70) };
        let out = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());
        assert_eq!(rc, 1, "FASTA export must refuse: {out}");
        assert!(out.contains("3 records"), "{out}");
        assert!(!out.contains("GAATTC"), "no bases may be written: {out}");
    }

    #[test]
    fn a_single_record_file_still_exports() {
        // The refusal above must fire on truncation and nothing else: one
        // record in, one record out, unchanged. Without this the fix could be
        // "refuse everything" and the test above would still pass.
        let (rc, json) = open(b">only one\nGAATTCACGTACGT\n");
        assert_eq!(rc, 0, "{json}");
        assert!(json.contains(r#""recordsInFile":1"#), "{json}");
        let title = "only.fa";
        let rc = unsafe { pl_to_fasta(title.as_ptr(), title.len(), 70) };
        let out = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());
        assert_eq!(rc, 0, "{out}");
        assert!(out.contains("GAATTCACGTACGT"), "{out}");
    }

    #[test]
    fn fasta_never_declares_topology_and_the_summary_says_which() {
        // `"circular":false` had two meanings and the browser could not tell
        // them apart. A Plasmidsaurus plasmid assembly arrives as FASTA at an
        // arbitrary rotation; reading that as a linear molecule loses the
        // origin-straddling sites it was sequenced to check.
        let (_, fasta) = open(b">p ACGT\nACGTACGTACGT\n");
        assert!(fasta.contains(r#""circular":false"#), "{fasta}");
        assert!(
            fasta.contains(r#""topologyDeclared":false"#),
            "FASTA has no topology field at all: {fasta}"
        );

        let (_, dna) = open(&dna_fixture());
        assert!(dna.contains(r#""circular":true"#), "{dna}");
        assert!(
            dna.contains(r#""topologyDeclared":true"#),
            "a .dna always carries the flag: {dna}"
        );
    }

    #[test]
    fn a_file_that_parsed_to_nothing_is_flagged_suspect() {
        // `genbank::parse` cannot fail, so noise containing the word LOCUS
        // opened with rc 0 and the page drew an empty map with no error at all.
        let (rc, json) = open(b"LOCUS\nnot a record, just noise\n");
        assert_eq!(rc, 0, "this really does parse 'successfully': {json}");
        assert!(
            json.contains(r#""suspect":true"#),
            "an empty parse must be visible: {json}"
        );
        // ...and an ordinary file must not be smeared with the same flag.
        let (_, ok) = open(&dna_fixture());
        assert!(ok.contains(r#""suspect":false"#), "{ok}");
    }

    #[test]
    fn unrepresentable_locations_and_notes_reach_the_browser() {
        // Both channels existed in `LoadReport` and neither had a way out of
        // this crate. `bond(5,10)` leaves a CDS with no segments at all, and the
        // browser drew that feature-free molecule with nothing to explain it;
        // `pl info` prints "location(s) this reader cannot represent".
        // Built by concatenation, not with `\`-continued source lines: that
        // escape eats the *leading whitespace* of the next line, and a GenBank
        // feature table is column-significant, so the continued form silently
        // produced a file with no features at all.
        let gb = concat!(
            "LOCUS       x                        12 bp    DNA     linear SYN 26-JUL-2026\n",
            "FEATURES             Location/Qualifiers\n",
            "     CDS             bond(5,10)\n",
            "                     /label=\"odd\"\n",
            "ORIGIN\n        1 acgtacgtacgt\n//\n"
        );
        let (rc, json) = open(gb.as_bytes());
        assert_eq!(rc, 0, "{json}");
        assert!(
            json.contains(r#""unrepresentableLocations":["#) && json.contains("bond"),
            "{json}"
        );
        assert!(json.contains(r#""unrepresentableNotes":[]"#), "{json}");

        // The `.dna` sibling: a citation subtree the flat note model has no
        // shape for. Kept as its own key -- reporting it as a *location* would
        // have the caller say something false about coordinates.
        let (rc, json) = open(&dna_with_notes(
            r#"<Notes><References><Reference pubMedID="1"/></References></Notes>"#,
        ));
        assert_eq!(rc, 0, "{json}");
        assert!(
            json.contains(r#""unrepresentableNotes":["Notes/References/Reference"]"#),
            "{json}"
        );
    }

    #[test]
    fn rotating_does_not_forget_what_the_file_held() {
        // `pl_rotate` re-emits the summary. Rebuilding it without the report
        // would put the warning back to sleep for any caller that re-reads the
        // summary after a rotate -- which the page does on every drag of the
        // origin.
        let gb = "LOCUS       one           10 bp    DNA     circular SYN 26-JUL-2026\n\
ORIGIN\n        1 ACGTACGTAC\n//\n\
LOCUS       two            8 bp    DNA     linear SYN 26-JUL-2026\n\
ORIGIN\n        1 TTTTGGGG\n//\n";
        let (rc, json) = open(gb.as_bytes());
        assert_eq!(rc, 0, "{json}");
        assert!(json.contains(r#""recordsInFile":2"#), "{json}");

        assert_eq!(pl_rotate(3), 0);
        let after = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());
        assert!(
            after.contains(r#""recordsInFile":2"#),
            "the count survives a rotation: {after}"
        );
    }

    #[test]
    fn enzyme_set_is_published() {
        assert_eq!(pl_enzymes_json(), 0);
        let j = STATE.with(|st| String::from_utf8(st.borrow().out.clone()).unwrap());
        assert!(j.contains(r#"{"name":"AatII","site":"GACGTC""#), "{j}");
        assert_eq!(j.matches(r#""name":"#).count(), pl_enzymes::ENZYMES.len());
    }
}
