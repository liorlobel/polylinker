//! The index file: header, a text table, packed nibbles, and a checksum.
//!
//! # Layout
//!
//! One file, not two. Two would need two renames and could be observed
//! mismatched between them; one cannot. All integers big-endian, matching the
//! `to_be_bytes` idiom in `pl_core::oplog`.
//!
//! ```text
//! offset  0   magic         8 B   "PLINDEX\n"   (trailing \n so `head -c8` is legible)
//! offset  8   format_ver    u32   file layout version
//! offset 12   engine_ver    u32   derivation version -- see `crate::ENGINE`
//! offset 16   flags         u32   reserved, MUST be 0; nonzero means rebuild
//! offset 20   _pad          u32   zero
//! offset 24   table_len     u64   bytes of the TSV section
//! offset 32   seq_len       u64   bytes of the nibble section
//! offset 40   record_count  u64
//! offset 48   reserved     16 B   zero-filled
//! offset 64   table               table_len bytes of UTF-8 TSV
//!             seqs                seq_len bytes of packed nibbles
//!             trailer      20 B   SHA-1 over every preceding byte
//! ```
//!
//! The table is literal text, so `strings library.plx | head` shows the schema.
//! The part a human can audit is auditable, and the megabytes that cannot be
//! text do not pretend to be.
//!
//! # Why every failure ends in "rebuild"
//!
//! Crash mid-write → an orphaned temporary and an intact index, because the
//! live file is never opened for writing. Bit rot → the trailer catches it.
//! A newer format → refuse and say so, never overwrite. An older format, or a
//! stale engine → discard and rebuild. There is no repair path by design: a
//! repaired index is an index that might be wrong, and this file is derived, so
//! the cost of discarding it is seconds.

use crate::{Row, State, Topology, ENGINE, FORMAT};
use pl_core::sha1::sha1;

const MAGIC: &[u8; 8] = b"PLINDEX\n";
const HEADER: usize = 64;
const TRAILER: usize = 20;

/// The columns, in order. Adding one is a `FORMAT` bump.
const COLUMNS: &[&str] = &[
    "path",
    "record",
    "state",
    "name",
    "topology",
    "length",
    "declared_len",
    "n_features",
    "ambiguous",
    "seq_off",
    "seq_bases",
    "text",
    "problem",
];

/// Everything known about one indexed folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Library {
    /// The folder this describes, as it was when built.
    pub root: String,
    /// Unix nanoseconds. Passed in rather than read from a clock, because this
    /// crate has no I/O and because a build must be reproducible.
    pub built_ns: u128,
    /// Did the directory walk finish?
    ///
    /// A partial walk must never be read as a mass deletion: when this is
    /// false, no rows were dropped and the caller says so.
    pub complete: bool,
    pub rows: Vec<Row>,
    /// Every record's bases, packed end to end. `Row::seq_off` indexes it.
    pub packed: Vec<u8>,
    /// Bases in `packed`, which is not `packed.len() * 2` when the last run is
    /// odd.
    pub packed_bases: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    NotAnIndex,
    Truncated {
        need: usize,
        got: usize,
    },
    /// Written by a newer build. **Do not overwrite it**: with a shared index
    /// that would destroy a colleague's newer work.
    FromTheFuture {
        found: u32,
        ours: u32,
    },
    /// An older layout, or a different derivation. Both mean rebuild.
    Stale {
        reason: String,
    },
    Corrupt,
    BadTable(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::NotAnIndex => write!(f, "not a Polylinker index"),
            OpenError::Truncated { need, got } => {
                write!(f, "truncated: needs {need} bytes, has {got}")
            }
            OpenError::FromTheFuture { found, ours } => write!(
                f,
                "written by a newer Polylinker (format {found}; this build reads {ours}). \
                 delete it or upgrade -- it will not be overwritten"
            ),
            OpenError::Stale { reason } => write!(f, "{reason}; rebuilding"),
            OpenError::Corrupt => write!(
                f,
                "checksum mismatch -- the index is damaged and will be rebuilt"
            ),
            OpenError::BadTable(e) => write!(f, "{e}"),
        }
    }
}

impl OpenError {
    /// Can the caller fix this by rebuilding, or must it stop?
    ///
    /// Everything except `FromTheFuture` is recoverable, because the index is
    /// derived. A newer file is the one case where acting would destroy
    /// something we cannot reproduce.
    pub fn rebuildable(&self) -> bool {
        !matches!(self, OpenError::FromTheFuture { .. })
    }
}

/// Escape a cell for the table.
///
/// `\r` is escaped, unlike the otherwise-identical codec in `pl-features`.
/// `str::lines()` strips a trailing `\r`, so a cell ending in one silently
/// loses it on round-trip there — and GenBank text written on Windows is full
/// of `\r`.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// Reverse [`escape`], in one pass.
///
/// Single-pass on purpose. The chained-`replace` version in `pl-features` —
/// `s.replace("\\t", "\t").replace("\\\\", "\\")` — does **not** round-trip a
/// cell containing a literal backslash followed by `t`: `C:\temp` escapes to
/// `C:\\temp` and unescapes back to `C:<TAB>emp`, because the first pass sees
/// the `\t` formed by the *second* escape backslash and the `t`. A note quoting
/// a Windows path is enough to hit it. Scanning left to right and consuming the
/// escape character cannot make that mistake.
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            // An unknown escape is kept verbatim rather than dropped: losing a
            // byte is worse than keeping one we did not write.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Serialise. Deterministic: the same library always produces the same bytes.
///
/// Rows are emitted in `(path, record)` order, which is what makes an
/// incremental rescan byte-comparable against a full rebuild. `pl-features`
/// already shipped a `HashMap`-iteration-order bug that needed exactly this
/// discipline.
pub fn to_bytes(lib: &Library) -> Vec<u8> {
    let mut rows: Vec<&Row> = lib.rows.iter().collect();
    rows.sort_by(|a, b| a.path.cmp(&b.path).then(a.record.cmp(&b.record)));

    let mut table = String::new();
    table.push_str(&format!("#!version {FORMAT}\n"));
    table.push_str(&format!("#!engine {ENGINE}\n"));
    table.push_str(&format!("#!root {}\n", escape(&lib.root)));
    table.push_str(&format!("#!built {}\n", lib.built_ns));
    table.push_str(&format!(
        "#!complete {}\n",
        if lib.complete { 1 } else { 0 }
    ));
    table.push_str(&format!("#!bases {}\n", lib.packed_bases));
    table.push_str(&COLUMNS.join("\t"));
    table.push('\n');
    for r in &rows {
        let cells = [
            escape(&r.path),
            r.record.to_string(),
            r.state.as_str().to_string(),
            escape(&r.name),
            r.topology.as_str().to_string(),
            r.length.to_string(),
            r.declared_len.to_string(),
            r.n_features.to_string(),
            r.ambiguous.to_string(),
            r.seq_off.to_string(),
            r.seq_bases.to_string(),
            escape(&r.text),
            escape(&r.problem),
        ];
        table.push_str(&cells.join("\t"));
        table.push('\n');
    }

    let table = table.into_bytes();
    let mut out = Vec::with_capacity(HEADER + table.len() + lib.packed.len() + TRAILER);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT.to_be_bytes());
    out.extend_from_slice(&ENGINE.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // flags
    out.extend_from_slice(&0u32.to_be_bytes()); // pad
    out.extend_from_slice(&(table.len() as u64).to_be_bytes());
    out.extend_from_slice(&(lib.packed.len() as u64).to_be_bytes());
    out.extend_from_slice(&(rows.len() as u64).to_be_bytes());
    out.extend_from_slice(&[0u8; 16]);
    debug_assert_eq!(out.len(), HEADER);
    out.extend_from_slice(&table);
    out.extend_from_slice(&lib.packed);
    let digest = sha1(&out);
    out.extend_from_slice(&digest);
    out
}

fn be_u32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn be_u64(b: &[u8], at: usize) -> u64 {
    let mut v = [0u8; 8];
    v.copy_from_slice(&b[at..at + 8]);
    u64::from_be_bytes(v)
}

/// Parse, verifying the checksum. Never returns a partial `Library`.
///
/// The checksum is verified **on every open, not lazily**: it costs about 26 ms
/// per 13 MB against a 9.7 ms read, and an index that is wrong in a way nobody
/// checked is worse than no index. Note this is strictly stronger than the
/// alternative that was considered — SQLite's amalgamation ships no checksum
/// VFS, so a default SQLite index would detect less.
pub fn parse(data: &[u8]) -> Result<Library, OpenError> {
    if data.len() < HEADER + TRAILER || &data[..8] != MAGIC {
        return Err(OpenError::NotAnIndex);
    }
    let format_ver = be_u32(data, 8);
    let engine_ver = be_u32(data, 12);
    let flags = be_u32(data, 16);
    if format_ver > FORMAT {
        return Err(OpenError::FromTheFuture {
            found: format_ver,
            ours: FORMAT,
        });
    }
    if format_ver < FORMAT {
        return Err(OpenError::Stale {
            reason: format!("index layout {format_ver}, this build writes {FORMAT}"),
        });
    }
    if flags != 0 {
        return Err(OpenError::Stale {
            reason: format!("unknown flags {flags:#x}"),
        });
    }
    let table_len = be_u64(data, 24) as usize;
    let seq_len = be_u64(data, 32) as usize;
    let record_count = be_u64(data, 40) as usize;

    let need = HEADER
        .checked_add(table_len)
        .and_then(|v| v.checked_add(seq_len))
        .and_then(|v| v.checked_add(TRAILER))
        .ok_or(OpenError::NotAnIndex)?;
    if data.len() != need {
        return Err(OpenError::Truncated {
            need,
            got: data.len(),
        });
    }

    // Checksum before interpreting anything, so a damaged length field cannot
    // steer the parse.
    let body = &data[..need - TRAILER];
    if sha1(body) != data[need - TRAILER..] {
        return Err(OpenError::Corrupt);
    }
    if engine_ver != ENGINE {
        return Err(OpenError::Stale {
            reason: format!(
                "built by derivation engine {engine_ver}, this build is {ENGINE} \
                 (a parser change means every row must be re-derived)"
            ),
        });
    }

    let table = std::str::from_utf8(&data[HEADER..HEADER + table_len])
        .map_err(|e| OpenError::BadTable(format!("table is not UTF-8: {e}")))?;

    let mut lib = Library {
        packed: data[HEADER + table_len..HEADER + table_len + seq_len].to_vec(),
        ..Default::default()
    };
    let mut header_seen = false;
    for (n, line) in table.lines().enumerate() {
        if let Some(rest) = line.strip_prefix("#!") {
            let (key, value) = rest.split_once(' ').unwrap_or((rest, ""));
            match key {
                "root" => lib.root = unescape(value),
                "built" => lib.built_ns = value.parse().unwrap_or(0),
                "complete" => lib.complete = value == "1",
                "bases" => lib.packed_bases = value.parse().unwrap_or(0),
                _ => {}
            }
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if !header_seen {
            let got: Vec<&str> = line.split('\t').collect();
            if got != COLUMNS {
                return Err(OpenError::BadTable(format!(
                    "line {}: columns are {got:?}, expected {COLUMNS:?}",
                    n + 1
                )));
            }
            header_seen = true;
            continue;
        }
        let c: Vec<&str> = line.split('\t').collect();
        if c.len() != COLUMNS.len() {
            return Err(OpenError::BadTable(format!(
                "line {}: {} cells, expected {}",
                n + 1,
                c.len(),
                COLUMNS.len()
            )));
        }
        let num = |i: usize| -> Result<u64, OpenError> {
            c[i].parse::<u64>().map_err(|e| {
                OpenError::BadTable(format!("line {}: column {}: {e}", n + 1, COLUMNS[i]))
            })
        };
        lib.rows.push(Row {
            path: unescape(c[0]),
            record: num(1)? as u32,
            state: State::from_name(c[2]).ok_or_else(|| {
                OpenError::BadTable(format!("line {}: unknown state {:?}", n + 1, c[2]))
            })?,
            name: unescape(c[3]),
            topology: Topology::from_name(c[4]).ok_or_else(|| {
                OpenError::BadTable(format!("line {}: unknown topology {:?}", n + 1, c[4]))
            })?,
            length: num(5)?,
            declared_len: num(6)?,
            n_features: num(7)? as u32,
            ambiguous: num(8)?,
            seq_off: num(9)?,
            seq_bases: num(10)?,
            text: unescape(c[11]),
            problem: unescape(c[12]),
        });
    }
    if !header_seen {
        return Err(OpenError::BadTable("no column header".into()));
    }
    if lib.rows.len() != record_count {
        return Err(OpenError::BadTable(format!(
            "header says {record_count} records, table holds {}",
            lib.rows.len()
        )));
    }
    // A row pointing past the store would make every query about it answer
    // from someone else's bases, or panic. Checked here, once, rather than
    // trusted at every read.
    for r in &lib.rows {
        let end = r.seq_off.saturating_add(r.seq_bases);
        if end > lib.packed_bases || (end as usize).div_ceil(2) > lib.packed.len() {
            return Err(OpenError::BadTable(format!(
                "{}#{}: bases {}..{} lie outside the {} stored",
                r.path, r.record, r.seq_off, end, lib.packed_bases
            )));
        }
    }
    Ok(lib)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nibble;

    fn rng(state: &mut u64) -> u64 {
        *state ^= *state >> 12;
        *state ^= *state << 25;
        *state ^= *state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn sample() -> Library {
        let seqs: [&[u8]; 3] = [b"GAATTCAAAA", b"ACGTACGTACGTA", b"TTTT"];
        let mut packed_seq = Vec::new();
        let mut rows = Vec::new();
        let mut off = 0u64;
        for (i, s) in seqs.iter().enumerate() {
            packed_seq.extend_from_slice(s);
            rows.push(Row {
                path: format!("sub dir/p{i}.gb"),
                record: 0,
                state: State::Ok,
                name: format!("p{i}"),
                topology: if i == 0 {
                    Topology::Circular
                } else {
                    Topology::Undeclared
                },
                length: s.len() as u64,
                declared_len: 0,
                n_features: i as u32,
                ambiguous: 0,
                seq_off: off,
                seq_bases: s.len() as u64,
                text: "AmpR\tori\nnote with\ttabs".to_string(),
                problem: String::new(),
            });
            off += s.len() as u64;
        }
        // One record with no bases at all, which must not perturb the offsets.
        rows.push(Row {
            path: "track.gb".into(),
            record: 0,
            state: State::AnnotationTrack,
            name: "track".into(),
            declared_len: 3000,
            n_features: 1,
            ..Default::default()
        });
        Library {
            root: "C:/lab/plasmids".into(),
            built_ns: 1_753_600_000_000_000_000,
            complete: true,
            packed: nibble::pack(&packed_seq),
            packed_bases: off,
            rows,
        }
    }

    #[test]
    fn a_library_round_trips_through_its_own_bytes() {
        let lib = sample();
        let bytes = to_bytes(&lib);
        let back = parse(&bytes).expect("parse");
        // Rows come back in canonical order, so compare that way.
        let mut want = lib.clone();
        want.rows
            .sort_by(|a, b| a.path.cmp(&b.path).then(a.record.cmp(&b.record)));
        assert_eq!(back, want);
    }

    #[test]
    fn serialisation_is_deterministic_whatever_order_rows_arrive_in() {
        // Byte-equality of rescan against rebuild is the invariant the whole
        // incremental path rests on, and it needs a total emission order.
        let lib = sample();
        let canonical = to_bytes(&lib);
        let mut st = 0x1234u64;
        for _ in 0..20 {
            let mut shuffled = lib.clone();
            let n = shuffled.rows.len();
            for i in 0..n {
                let j = (rng(&mut st) % n as u64) as usize;
                shuffled.rows.swap(i, j);
            }
            assert_eq!(to_bytes(&shuffled), canonical);
        }
    }

    #[test]
    fn escaping_round_trips_including_a_lone_cr() {
        // `str::lines()` strips a trailing \r, so a cell ending in one silently
        // loses it if \r is not escaped -- and Windows GenBank is full of them.
        let mut st = 0xfeed_1234_5678_9abcu64;
        const CHARS: &[char] = &[
            'a', 'Z', '\t', '\n', '\r', '\\', ' ', '#', '!', 'é', '🧬', '"', '\'', 't', 'n', 'r',
        ];
        for _ in 0..4000 {
            let n = (rng(&mut st) % 12) as usize;
            let s: String = (0..n)
                .map(|_| CHARS[(rng(&mut st) % CHARS.len() as u64) as usize])
                .collect();
            assert_eq!(unescape(&escape(&s)), s, "{s:?}");
        }
        for s in [
            "",
            "\r",
            "\r\n",
            "a\r",
            "\\",
            "\\\\",
            // The case the chained-replace codec in pl-features gets wrong: a
            // literal backslash followed by `t`, as in a quoted Windows path.
            "C:\\temp\\thing",
            "\\t",
            "\\n",
            "\\r",
            "ends with backslash\\",
        ] {
            assert_eq!(unescape(&escape(s)), s, "{s:?}");
        }
    }

    #[test]
    fn the_text_column_is_not_trimmed() {
        // A feature name with meaningful leading whitespace, or a note value
        // that is entirely whitespace, must stay findable.
        let mut lib = sample();
        lib.rows[0].text = "   leading and trailing   ".into();
        lib.rows[1].text = "   ".into();
        let back = parse(&to_bytes(&lib)).unwrap();
        let by_path = |p: &str| back.rows.iter().find(|r| r.path == p).unwrap().text.clone();
        assert_eq!(by_path("sub dir/p0.gb"), "   leading and trailing   ");
        assert_eq!(by_path("sub dir/p1.gb"), "   ");
    }

    /// Test 13, first half: a truncated file must never yield a partial
    /// `Library`.
    #[test]
    fn every_truncation_is_rejected() {
        let bytes = to_bytes(&sample());
        for cut in 0..bytes.len() {
            let r = parse(&bytes[..cut]);
            assert!(
                r.is_err(),
                "a {cut}-byte prefix of a {}-byte index parsed",
                bytes.len()
            );
        }
        assert!(parse(&bytes).is_ok(), "the whole file still parses");
    }

    /// Test 13, second half: bit rot.
    #[test]
    fn every_single_bit_flip_is_caught() {
        let bytes = to_bytes(&sample());
        let mut st = 0xaaaa_bbbb_cccc_ddddu64;
        for _ in 0..400 {
            let at = (rng(&mut st) % bytes.len() as u64) as usize;
            let bit = (rng(&mut st) % 8) as u32;
            let mut damaged = bytes.clone();
            damaged[at] ^= 1 << bit;
            assert!(
                parse(&damaged).is_err(),
                "flipping bit {bit} of byte {at} went unnoticed"
            );
        }
    }

    #[test]
    fn a_newer_index_is_refused_and_never_treated_as_rebuildable() {
        let mut bytes = to_bytes(&sample());
        bytes[8..12].copy_from_slice(&(FORMAT + 1).to_be_bytes());
        // The checksum must be repaired, or this tests corruption instead.
        let n = bytes.len();
        let digest = sha1(&bytes[..n - TRAILER]);
        bytes[n - TRAILER..].copy_from_slice(&digest);

        let err = parse(&bytes).unwrap_err();
        assert!(matches!(err, OpenError::FromTheFuture { .. }), "{err:?}");
        assert!(
            !err.rebuildable(),
            "a newer index must not be overwritten -- that would destroy work \
             this build cannot reproduce"
        );
        assert!(err.to_string().contains("will not be overwritten"), "{err}");
    }

    #[test]
    fn a_stale_engine_forces_a_rebuild_even_though_the_bytes_are_fine() {
        // The staleness nobody catches: every derived column is a function of
        // the parser, so a parser fix must invalidate rows whose files have not
        // changed at all.
        let mut bytes = to_bytes(&sample());
        bytes[12..16].copy_from_slice(&(ENGINE + 1).to_be_bytes());
        let n = bytes.len();
        let digest = sha1(&bytes[..n - TRAILER]);
        bytes[n - TRAILER..].copy_from_slice(&digest);

        let err = parse(&bytes).unwrap_err();
        assert!(matches!(err, OpenError::Stale { .. }), "{err:?}");
        assert!(err.rebuildable());
        assert!(err.to_string().contains("re-derived"), "{err}");
    }

    #[test]
    fn a_row_pointing_outside_the_store_is_rejected_not_answered() {
        // The deepest risk in the feature: a bad offset makes every query about
        // that row answer from another molecule's bases. Caught once, at parse.
        let mut lib = sample();
        lib.rows[0].seq_bases = 9_999;
        let err = parse(&to_bytes(&lib)).unwrap_err();
        assert!(matches!(err, OpenError::BadTable(_)), "{err:?}");
        assert!(err.to_string().contains("outside"), "{err}");
    }

    #[test]
    fn something_that_is_not_an_index_is_not_mistaken_for_a_damaged_one() {
        // The distinction matters: "not an index" is a wrong path, "corrupt" is
        // a rebuild. Telling a user their index is damaged when they pointed at
        // a GenBank file would send them looking for the wrong problem.
        assert_eq!(parse(b"").unwrap_err(), OpenError::NotAnIndex);
        assert_eq!(parse(b"LOCUS  x  4 bp").unwrap_err(), OpenError::NotAnIndex);
        assert_eq!(
            parse(&[0u8; HEADER + TRAILER]).unwrap_err(),
            OpenError::NotAnIndex
        );
    }

    #[test]
    fn an_empty_library_is_a_valid_index_not_an_error() {
        // "I scanned that folder and it holds no sequence files" is a real
        // answer, and must survive a round trip so the next run does not
        // rescan.
        let lib = Library {
            root: "C:/empty".into(),
            built_ns: 42,
            complete: true,
            ..Default::default()
        };
        let back = parse(&to_bytes(&lib)).unwrap();
        assert_eq!(back, lib);
        assert!(back.rows.is_empty());
        assert!(back.complete);
    }

    #[test]
    fn an_incomplete_walk_survives_the_round_trip() {
        // If this flag were lost, the next scan would treat every file it did
        // not reach as deleted.
        let mut lib = sample();
        lib.complete = false;
        assert!(!parse(&to_bytes(&lib)).unwrap().complete);
    }
}
