//! Building and refreshing a library, incrementally.
//!
//! # The invariant
//!
//! **`rescan(previous)` must produce byte-identical output to a full rebuild,
//! always.** Everything else here is an optimisation under that constraint, and
//! `rescan_always_equals_rebuild` in the integration tests is the assertion
//! that keeps it true across create, edit, truncate, delete, rename, move and
//! touch-without-change.
//!
//! # The invalidation key
//!
//! `(format, engine, path, size, mtime)`. All five.
//!
//! - **All five match** → reuse the row and its bases. No read, no parse, no
//!   hash.
//! - **New or changed** → read, hash, parse, re-derive.
//! - **Hash unchanged despite a new mtime** → update the mtime and skip the
//!   parse. Not tidiness: OneDrive and Dropbox rewrite mtimes on sync without
//!   changing content, and without this check every sync re-parses the whole
//!   library.
//! - **Gone from disk** → drop, and report the count — *unless the walk was
//!   incomplete*, in which case nothing is dropped at all. A share that blinks
//!   must never read as a mass deletion.
//!
//! `engine` is in the key because every derived column is a function of the
//! *parser*, not just the file. Ship a GenBank fix and, without it, every file
//! is "unchanged", every row is reused, and the fix never reaches the library.

use std::path::Path;

use pl_index::codec::Library;
use pl_index::{nibble, Row};

use crate::walk::walk;
use crate::{content_id, rows_for_file, ScanOptions, ScanReport};

/// Build or refresh the library for `root`.
///
/// `now_ns` is passed in rather than read from a clock so a build is
/// reproducible and testable.
pub fn scan(root: &Path, now_ns: u128, opts: &ScanOptions) -> (Library, ScanReport) {
    let (found, walk_report) = walk(root, &opts.walk);
    let mut report = ScanReport {
        files_seen: found.len(),
        incomplete: walk_report.incomplete.clone(),
        ..Default::default()
    };
    for (path, err) in &walk_report.errors {
        report.unreadable.push((path.clone(), err.clone()));
    }

    // The previous rows *are* the ledger: each carries the size, mtime and
    // content hash of the file it was derived from.
    let prev_rows: Vec<&Row> = opts
        .previous
        .as_ref()
        .map(|l| l.rows.iter().collect())
        .unwrap_or_default();
    let prev_packed = opts.previous.as_ref().map(|l| &l.packed);

    let mut rows: Vec<Row> = Vec::new();
    let mut bases: Vec<u8> = Vec::new();

    for f in &found {
        // Record 0 speaks for the file: every record of a file shares its
        // stamp, because they all came out of the same bytes.
        let prev = prev_rows
            .iter()
            .find(|r| r.path == f.rel && r.record == 0)
            .copied();
        let unchanged = prev.is_some_and(|r| r.size == f.size && r.mtime_ns == f.mtime_ns);

        if unchanged {
            let reused = copy_rows(&prev_rows, &f.rel, prev_packed, &mut bases);
            if !reused.is_empty() {
                report.reused += 1;
                report.records += reused.len();
                rows.extend(reused);
                continue;
            }
            // A stamped path with no rows is a stale ledger entry; re-derive
            // rather than invent an empty file.
        }

        if f.offline {
            rows.push(Row {
                path: f.rel.clone(),
                state: pl_index::State::NotDownloaded,
                problem: "a cloud placeholder that is not stored locally".into(),
                ..Default::default()
            });
            report.records += 1;
            continue;
        }

        let data = match std::fs::read(&f.path) {
            Ok(d) => d,
            Err(e) => {
                report.unreadable.push((f.rel.clone(), e.to_string()));
                rows.push(Row {
                    path: f.rel.clone(),
                    state: pl_index::State::Unreadable,
                    problem: e.to_string(),
                    ..Default::default()
                });
                report.records += 1;
                continue;
            }
        };
        let content = content_id(&data);

        // The mtime moved but the bytes did not -- a sync client, typically.
        // Restamp and reuse, rather than re-parsing the whole library every
        // time OneDrive touches it.
        if prev.is_some_and(|r| r.content == content) {
            let mut reused = copy_rows(&prev_rows, &f.rel, prev_packed, &mut bases);
            if !reused.is_empty() {
                for r in &mut reused {
                    r.size = f.size;
                    r.mtime_ns = f.mtime_ns;
                }
                report.touched_only += 1;
                report.records += reused.len();
                rows.extend(reused);
                continue;
            }
        }

        let (mut file_rows, file_bases) = rows_for_file(&f.rel, &data, f.size);
        // Offsets are assigned here, where the running total lives.
        let mut off = bases.len() as u64;
        for r in &mut file_rows {
            r.size = f.size;
            r.mtime_ns = f.mtime_ns;
            r.content = content.clone();
            if r.seq_bases > 0 {
                r.seq_off = off;
                off += r.seq_bases;
            }
        }
        bases.extend_from_slice(&file_bases);
        report.parsed += 1;
        report.records += file_rows.len();
        rows.extend(file_rows);
    }

    // Deletions, but only if we actually looked everywhere.
    if report.incomplete.is_none() {
        let seen: Vec<&str> = found.iter().map(|f| f.rel.as_str()).collect();
        report.removed = prev_rows
            .iter()
            .filter(|r| r.record == 0 && !seen.contains(&r.path.as_str()))
            .count();
    } else {
        // Carry forward every row we did not reach, so a partial walk does not
        // read as a deletion.
        let seen: Vec<&str> = found.iter().map(|f| f.rel.as_str()).collect();
        for r in &prev_rows {
            if !seen.contains(&r.path.as_str()) {
                let mut kept = (*r).clone();
                if kept.seq_bases > 0 {
                    if let Some(pp) = prev_packed {
                        let start = bases.len() as u64;
                        for i in 0..kept.seq_bases as usize {
                            bases.push(nibble::base_for(nibble::mask_at(
                                pp,
                                kept.seq_off as usize + i,
                            )));
                        }
                        kept.seq_off = start;
                    } else {
                        kept.seq_bases = 0;
                    }
                }
                rows.push(kept);
                report.records += 1;
            }
        }
    }

    // A total order, so this is byte-comparable with a rebuild.
    rows.sort_by(|a, b| a.path.cmp(&b.path).then(a.record.cmp(&b.record)));
    // Sorting moved rows around, but offsets refer to `bases`, which did not
    // move -- offsets are absolute, so no fix-up is needed.

    let packed_bases = bases.len() as u64;
    let lib = Library {
        root: root.to_string_lossy().replace('\\', "/"),
        built_ns: now_ns,
        complete: report.incomplete.is_none(),
        rows,
        packed: nibble::pack(&bases),
        packed_bases,
    };
    (lib, report)
}

/// Copy a path's rows out of the previous library, re-homing their bases.
fn copy_rows(
    prev_rows: &[&Row],
    rel: &str,
    prev_packed: Option<&Vec<u8>>,
    bases: &mut Vec<u8>,
) -> Vec<Row> {
    let mut out = Vec::new();
    for r in prev_rows.iter().filter(|r| r.path == rel) {
        let mut row = (*r).clone();
        if row.seq_bases > 0 {
            let Some(pp) = prev_packed else {
                return Vec::new();
            };
            let start = bases.len() as u64;
            for i in 0..row.seq_bases as usize {
                // Through the canonical letter, so a reused record is packed
                // from the same alphabet a fresh parse would produce. Anything
                // else and rescan stops equalling rebuild.
                bases.push(nibble::base_for(nibble::mask_at(
                    pp,
                    row.seq_off as usize + i,
                )));
            }
            row.seq_off = start;
        }
        out.push(row);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WalkOptions;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("pl-scan-scan-{name}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn gb(name: &str, seq: &str, circular: bool) -> String {
        format!(
            "LOCUS       {name}    {} bp    DNA     {} SYN 26-JUL-2026\nORIGIN\n        1 {seq}\n//\n",
            seq.len(),
            if circular { "circular" } else { "linear" }
        )
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let p = crate::abs(root, rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn opts() -> ScanOptions {
        ScanOptions {
            walk: WalkOptions::default(),
            previous: None,
        }
    }

    #[test]
    fn a_scan_finds_every_record_and_its_bases() {
        let root = tmp("basic");
        write(&root, "a.gb", &gb("a", "GAATTCAAAA", true));
        write(&root, "sub/b.gb", &gb("b", "CCCCCCCCCC", false));
        let (lib, report) = scan(&root, 1, &opts());
        assert_eq!(report.files_seen, 2);
        assert_eq!(report.parsed, 2);
        assert_eq!(lib.rows.len(), 2);
        assert!(lib.complete);
        assert_eq!(lib.packed_bases, 20);

        // The site is where it should be, through the real store.
        let motif = pl_index::scan::Motif::new("GAATTC").unwrap();
        let a = lib.rows.iter().find(|r| r.path == "a.gb").unwrap();
        let hits = pl_index::scan::find_in_row(&motif, &lib.packed, a);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].start, 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_second_record_reads_its_own_bases_not_the_first_records() {
        // Offsets are the deepest risk in the feature: get one wrong and the
        // index answers confidently about the wrong molecule.
        let root = tmp("offsets");
        write(&root, "a.gb", &gb("a", "AAAAAAAAAAA", true)); // odd length
        write(&root, "b.gb", &gb("b", "GGGAATTCGG", true));
        let (lib, _) = scan(&root, 1, &opts());
        let motif = pl_index::scan::Motif::new("GAATTC").unwrap();
        let a = lib.rows.iter().find(|r| r.path == "a.gb").unwrap();
        let b = lib.rows.iter().find(|r| r.path == "b.gb").unwrap();
        assert!(pl_index::scan::find_in_row(&motif, &lib.packed, a).is_empty());
        let hits = pl_index::scan::find_in_row(&motif, &lib.packed, b);
        assert_eq!(hits.len(), 1);
        assert_eq!(
            pl_index::scan::hit_bases(&lib.packed, b, &hits[0], 6),
            b"GAATTC".to_vec()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unreadable_root_never_looks_like_an_empty_folder() {
        let mut missing = std::env::temp_dir();
        missing.push("pl-scan-scan-nope-not-here");
        let _ = std::fs::remove_dir_all(&missing);
        let (lib, report) = scan(&missing, 1, &opts());
        assert!(report.incomplete.is_some());
        assert!(!lib.complete, "the flag must persist into the index");
        assert_eq!(
            report.removed, 0,
            "nothing may be removed on a partial walk"
        );
        let _ = std::fs::remove_dir_all(&missing);
    }
}
