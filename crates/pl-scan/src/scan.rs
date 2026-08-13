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
//!   must never read as a mass deletion. "Incomplete" here is a wider notion
//!   than `walk`'s own: a link this walk declined to follow is a sub-tree
//!   nobody looked at, so it suppresses the deletion pass and the completeness
//!   stamp even though `walk` rightly does not call it an error. See
//!   `unfollowed_links` below for why the two notions have to differ.
//!
//! `engine` is in the key because every derived column is a function of the
//! *parser*, not just the file. Ship a GenBank fix and, without it, every file
//! is "unchanged", every row is reused, and the fix never reaches the library.

use std::path::Path;

use pl_index::codec::Library;
use pl_index::{nibble, Row};

use crate::walk::{walk, WalkOptions, WalkReport};
use crate::{content_id, rows_for_file, ScanOptions, ScanReport, MAX_BYTES};

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

    // A link we declined to follow is a sub-tree nobody looked at, and from here
    // down that has to read exactly like a share that blinked.
    //
    // `walk` counts these in `links_skipped` and deliberately leaves its own
    // `incomplete` unset (walk.rs:219-226), which is right *for the walk*:
    // refusing to follow a link is the documented default, not a failure, and it
    // is the only thing keeping a link back to an ancestor from making the walk
    // unbounded. It was wrong for what happens next here. The deletion pass
    // below was gated on `incomplete.is_none()` alone, so `pl index
    // --follow-links C:/lab` followed by any flagless run over the same folder
    // -- `pl find`, `pl library`, or merely opening the GUI's Library tab, which
    // cannot pass the flag at all because it scans with `..Default::default()`
    // (bins/pl-gui/src/library.rs:114-124) -- counted every row behind the
    // junction as removed, dropped it, and wrote `complete: true` over the good
    // index. Measured on a filesystem with nothing wrong with it: `2 removed`
    // and `#!complete 1`, after which `pl library` had no partial scan to warn
    // about and `pl find` answered "not in my library" for plasmids still on
    // disk.
    //
    // `docs/AUDIT-2026-07-29.md` deferred this as D2 because choosing between
    // "persist the walk options in the index and reuse them" and "refuse
    // deletions when a link was skipped" is a product decision about what an
    // index *is*. That reason is honest and still stands -- the codec stores no
    // walk options and this change adds none, so `--follow-links` is still not
    // persisted and the flag still has to be repeated per invocation. What could
    // not wait for that decision is the *claim*: an index that stamps
    // `complete: true` over rows it deleted without looking is the single
    // outcome the whole `incomplete` mechanism exists to prevent, and a stale
    // row -- the worst this costs -- is recoverable by one rescan, while a
    // deleted one is not recoverable at all.
    //
    // Scoped to the deletion pass and the completeness stamp, which is all
    // `ScanReport::incomplete` reaches: nothing is pushed into `unreadable`, and
    // `walk`'s own report is untouched, so a skipped link is still not an error.
    // The cost is that a library holding an unfollowed link stops recording
    // deletions until someone indexes it with the flag, exactly as an unreadable
    // sub-directory already does. On the corpus this design was measured against
    // it does not fire: OneDrive's 68,811 reparse points do not carry the
    // name-surrogate bit `is_symlink` tests, which is why the walker tests that
    // bit and not the reparse point (walk.rs:203-218).
    if report.incomplete.is_none() {
        report.incomplete = unfollowed_links(root, &opts.walk, &walk_report);
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

        // The byte cap, applied where the size is known and before a byte is
        // read. `rows_for_file` also checks it, but it only ever sees bytes,
        // and getting them means `std::fs::read` on the whole file: on the
        // corpus this design is written against that is a 1.39 GB allocation
        // plus a SHA-1 pass over all 1.39 GB, on the first index and again
        // after every mtime change, to produce a row that carries no bases
        // either way. The row is deliberately left without a content hash --
        // hashing is the expensive half of what we are refusing to do -- and
        // `pl verify` already skips rows whose `content` is empty.
        if f.size > MAX_BYTES {
            rows.push(Row {
                path: f.rel.clone(),
                state: pl_index::State::TooLarge,
                size: f.size,
                mtime_ns: f.mtime_ns,
                problem: format!("{} bytes; the cap is {MAX_BYTES}", f.size),
                ..Default::default()
            });
            // Counted as parsed because the row was derived here rather than
            // reused; the file itself is reported through the row's TooLarge
            // state, which is what `pl library --problems` lists.
            report.parsed += 1;
            report.records += 1;
            continue;
        }

        // Cap the read itself, not just the walk-time `f.size`: a file that grew
        // between the walk's stat and here (a sequencer still writing) would
        // otherwise be allocated and SHA-1'd in full despite the gate above,
        // paying the very cost the cap exists to avoid.
        let data = {
            use std::io::Read;
            let read = std::fs::File::open(&f.path).and_then(|fh| {
                let mut buf = Vec::new();
                fh.take(MAX_BYTES + 1).read_to_end(&mut buf)?;
                Ok(buf)
            });
            match read {
                Ok(d) if d.len() as u64 > MAX_BYTES => {
                    // Grew past the cap since the walk; treat it as TooLarge, the
                    // same outcome the `f.size` gate above would have given.
                    rows.push(Row {
                        path: f.rel.clone(),
                        state: pl_index::State::TooLarge,
                        size: f.size,
                        mtime_ns: f.mtime_ns,
                        problem: format!("grew past the {MAX_BYTES}-byte cap while being read"),
                        ..Default::default()
                    });
                    report.parsed += 1;
                    report.records += 1;
                    continue;
                }
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

    // Deletions, but only if we actually looked everywhere -- and "everywhere"
    // includes the sub-trees behind links this walk declined to follow, which
    // `unfollowed_links` promoted into `incomplete` above.
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
        // Windows-only fold, for the reason spelled out on `rel`: off Windows a
        // backslash is part of a directory's name, not a separator.
        root: crate::slash_separated(&root.to_string_lossy()),
        built_ns: now_ns,
        complete: report.incomplete.is_none(),
        rows,
        packed: nibble::pack(&bases),
        packed_bases,
    };
    (lib, report)
}

/// Why a walk that skipped a link did not look everywhere, if it did not.
///
/// `Some(why)` means the deletion pass must not run and the index must not
/// claim to be complete. `None` means nothing was skipped that could be hiding
/// a file, so a row that is missing from this walk really is a row whose file
/// is gone.
///
/// Two conditions, and the second one is the one that is easy to get wrong.
/// `WalkReport::links_skipped` counts two different things depending on the
/// flag that was passed:
///
/// - With `follow_links` **off** it counts links the walk refused to enter
///   (walk.rs:219-226). Behind one of those there may be a folder of
///   constructs, and nothing in this run looked at it. That is the case this
///   function exists for.
/// - With `follow_links` **on** it counts *cycle* skips (walk.rs:266-278): a
///   link whose target is already on the descent path, whose files are reached
///   under their real path in the very same walk. Those are complete -- walk.rs
///   says so where it skips them -- and calling them partial would stop a
///   `--follow-links` library from ever recording a deletion because of a
///   `loop -> .` that costs it nothing.
///
/// The count is all there is to go on. `WalkReport` records *how many* links
/// were skipped and not *which paths* they were, so the suppression is over the
/// whole index rather than over the sub-trees behind them; and `ScanReport` has
/// no `links_skipped` field, so the number cannot be handed on to a caller that
/// might want to name the junction in its warning. Both are worth having, and
/// neither is needed to stop an index asserting completeness over rows it
/// deleted without looking, which is the only thing being fixed here.
fn unfollowed_links(root: &Path, opts: &WalkOptions, walk_report: &WalkReport) -> Option<String> {
    if opts.follow_links || walk_report.links_skipped == 0 {
        return None;
    }
    Some(format!(
        "{}: {} link(s) or junction(s) were not followed, because --follow-links \
         is off; whatever is behind them was not looked at",
        root.display(),
        walk_report.links_skipped
    ))
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
    fn a_depth_truncated_rescan_drops_nothing_and_never_claims_to_be_complete() {
        // The whole failure, end to end. `--max-depth` is one documented flag
        // away (bins/pl exposes it), and an ACL change or a network sub-tree
        // that drops gets here by itself. With the truncation recorded only in
        // `errors`, this rescan reported `removed = 1`, dropped `sub/b.gb` from
        // the rebuilt library and wrote `complete: 1` — after which `pl find`
        // answers "not in my library" for a plasmid still on disk and
        // `pl library` has nothing to warn about.
        let root = tmp("depth-truncated");
        write(&root, "a.gb", &gb("a", "GAATTCAAAA", true));
        write(&root, "sub/b.gb", &gb("b", "GGGGAATTCC", true));
        let (lib, first) = scan(&root, 1, &opts());
        assert_eq!(lib.rows.len(), 2);
        assert!(lib.complete);
        assert_eq!(first.removed, 0);

        let shallow = ScanOptions {
            walk: WalkOptions {
                max_depth: 0,
                ..Default::default()
            },
            previous: Some(lib.clone()),
        };
        let (after, report) = scan(&root, 2, &shallow);
        assert!(report.incomplete.is_some(), "the walk did not finish");
        assert_eq!(
            report.removed, 0,
            "nothing may be dropped on a partial walk"
        );
        assert!(!after.complete, "and the index must say so on disk");
        let mut paths: Vec<&str> = after.rows.iter().map(|r| r.path.as_str()).collect();
        paths.sort_unstable();
        assert_eq!(
            paths,
            vec!["a.gb", "sub/b.gb"],
            "the unreached row survives"
        );

        // And it survived with its bases, so it still answers.
        let motif = pl_index::scan::Motif::new("GAATTC").unwrap();
        let b = after.rows.iter().find(|r| r.path == "sub/b.gb").unwrap();
        assert_eq!(
            pl_index::scan::find_in_row(&motif, &after.packed, b).len(),
            1
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_over_cap_file_is_neither_read_nor_hashed() {
        // The observable proof that the file was not opened is the *absence*
        // of a content hash: `content_id` cannot run without the bytes, so an
        // empty `content` on a TooLarge row means no 1.39 GB allocation and no
        // SHA-1 pass over it. Before the gate moved ahead of the read, this row
        // came back carrying a 40-character hash of 64 MB of zeroes.
        let root = tmp("toolarge");
        let p = crate::abs(&root, "huge.fa");
        let f = std::fs::File::create(&p).unwrap();
        // `set_len` rather than writing 64 MB: the size in the directory entry
        // is what the gate reads, and the test should not itself cost the
        // allocation it is asserting against.
        f.set_len(crate::MAX_BYTES + 1).unwrap();
        drop(f);

        let (lib, report) = scan(&root, 1, &opts());
        assert_eq!(lib.rows.len(), 1);
        let row = &lib.rows[0];
        assert_eq!(row.state, pl_index::State::TooLarge);
        assert!(
            row.content.is_empty(),
            "a hash means the whole file was read: {:?}",
            row.content
        );
        assert!(row.problem.contains("the cap is"), "{:?}", row.problem);
        assert_eq!(
            row.size,
            crate::MAX_BYTES + 1,
            "the stamp is still recorded"
        );
        assert!(row.mtime_ns > 0, "and so is the mtime, for the next rescan");
        assert_eq!(lib.packed_bases, 0, "an over-cap file contributes no bases");
        assert_eq!(report.records, 1, "and it is reported, never silent");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_under_the_cap_is_still_read_and_hashed() {
        // The control for the gate above: moving it ahead of the read must not
        // cost every ordinary file its content hash, which is what lets a
        // sync-client mtime bump skip the parse.
        let root = tmp("undercap");
        write(&root, "a.gb", &gb("a", "GAATTCAAAA", true));
        let (lib, _) = scan(&root, 1, &opts());
        assert_eq!(lib.rows[0].state, pl_index::State::Ok);
        assert_eq!(lib.rows[0].content.len(), 40, "SHA-1, in hex");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PROVEN TO FAIL at f0e4a6f: `scan` took its `incomplete` from `walk`'s
    /// report alone, and `walk` counts a link it declined to follow without
    /// setting `incomplete` — so `links_skipped > 0` with `incomplete == None`
    /// reached the deletion pass as a walk that had looked everywhere.
    ///
    /// Mutation that re-breaks it: in `unfollowed_links`, replace the body with
    /// `None`. (Equivalently, delete the `report.incomplete =
    /// unfollowed_links(...)` assignment in `scan`.)
    ///
    /// Mutation that re-breaks the second half: delete `opts.follow_links ||`
    /// from the guard in `unfollowed_links`, which makes a `--follow-links`
    /// walk that skipped one cycle report itself partial and stop recording
    /// deletions for a `loop -> .` that cost it nothing.
    ///
    /// No filesystem and no link: `walk`'s two counters are the whole input to
    /// the decision, so the pair the audit found untested in both directions —
    /// `links_skipped > 0` with `incomplete == None` — can be stated directly.
    /// That also means this check runs on every machine, including one where
    /// the end-to-end test below has to skip because it cannot make a junction.
    #[test]
    fn a_skipped_link_is_a_partial_walk_but_a_cycle_skip_is_not() {
        let root = Path::new("C:/lab");
        let plain = WalkOptions::default();
        assert!(
            !plain.follow_links,
            "the default is what the GUI scans with"
        );
        let following = WalkOptions {
            follow_links: true,
            ..Default::default()
        };

        let nothing_skipped = WalkReport::default();
        assert!(
            unfollowed_links(root, &plain, &nothing_skipped).is_none(),
            "a walk that skipped nothing must still be allowed to record \
             deletions, or no library ever converges"
        );

        let one_skipped = WalkReport {
            links_skipped: 1,
            ..Default::default()
        };
        let why = unfollowed_links(root, &plain, &one_skipped)
            .expect("a link that was not followed is a sub-tree nobody looked at");
        assert!(
            why.contains("lab") && why.contains("follow-links"),
            "the operator needs the folder and the flag that would fix it: {why:?}"
        );

        assert!(
            unfollowed_links(root, &following, &one_skipped).is_none(),
            "under --follow-links this counter is cycle skips, whose files were \
             reached under their real path in the same walk"
        );
    }

    /// Make `link` a link to the directory `target`; `false` if the platform
    /// refused.
    ///
    /// A junction on Windows rather than a symbolic link, for the reason
    /// `walk.rs`'s copy of this helper gives: `mklink /D` needs elevation or
    /// Developer Mode, `mklink /J` needs neither, and Rust reports both as
    /// `is_symlink()` because both carry the name-surrogate bit.
    ///
    /// This copy returns a bool where that one asserts, so a machine that may
    /// not create a link does not turn into a failure of the thing being
    /// tested. The skip hides nothing: `walk.rs`'s
    /// `a_link_is_not_followed_by_default_and_the_skip_is_counted` still
    /// asserts on exactly this operation in this crate, so a machine where this
    /// returns `false` is already red there — and the decision itself is pinned
    /// by `a_skipped_link_is_a_partial_walk_but_a_cycle_skip_is_not` above,
    /// which needs no filesystem at all.
    #[cfg(windows)]
    fn link_dir(target: &Path, link: &Path) -> bool {
        let out = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                &link.display().to_string(),
                &target.display().to_string(),
            ])
            .output();
        match out {
            Ok(o) => o.status.success(),
            Err(_) => false,
        }
    }

    #[cfg(not(windows))]
    fn link_dir(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    /// PROVEN TO FAIL at f0e4a6f: with `links_skipped` invisible to `scan`, the
    /// flagless rescan below counted the linked row as removed, dropped it from
    /// the rebuilt library and wrote `complete: true` — the whole of finding #8,
    /// end to end, and the GUI reaches it by opening the Library tab.
    ///
    /// Mutation that re-breaks it: in `unfollowed_links`, replace the body with
    /// `None`.
    ///
    /// The two scans are the two commands: `pl index --follow-links <root>`,
    /// then anything at all over the same folder without the flag — the flag is
    /// per-invocation, nothing in the index records it, and `bins/pl-gui` has no
    /// way to pass it.
    #[test]
    fn a_rescan_without_follow_links_carries_the_linked_rows_forward_instead_of_deleting_them() {
        let root = tmp("followlinks-rescan");
        let target = tmp("followlinks-rescan-target");
        write(&root, "plain.gb", &gb("plain", "GAATTCAAAA", true));
        write(&target, "linked.gb", &gb("linked", "GGGGAATTCC", true));
        if !link_dir(&target, &crate::abs(&root, "archive")) {
            eprintln!(
                "skipped: this machine would not create a directory link. the decision \
                 itself is covered by a_skipped_link_is_a_partial_walk_but_a_cycle_skip_is_not"
            );
            let _ = std::fs::remove_dir_all(&root);
            let _ = std::fs::remove_dir_all(&target);
            return;
        }

        // `pl index --follow-links`: the linked sub-tree is indexed under the
        // path the user gave.
        let following = ScanOptions {
            walk: WalkOptions {
                follow_links: true,
                ..Default::default()
            },
            previous: None,
        };
        let (lib, first) = scan(&root, 1, &following);
        let mut indexed: Vec<&str> = lib.rows.iter().map(|r| r.path.as_str()).collect();
        indexed.sort_unstable();
        assert_eq!(
            indexed,
            vec!["archive/linked.gb", "plain.gb"],
            "the link was created but not walked, so this test would prove nothing"
        );
        assert!(lib.complete, "nothing was skipped with the flag on");
        assert_eq!(first.removed, 0);

        // Anything at all over the same folder without the flag.
        let flagless = ScanOptions {
            walk: WalkOptions::default(),
            previous: Some(lib.clone()),
        };
        let (after, report) = scan(&root, 2, &flagless);
        assert!(
            report.incomplete.is_some(),
            "a walk that refused to enter the junction did not look everywhere"
        );
        assert_eq!(
            report.removed, 0,
            "a link we chose not to follow is not 400 plasmids being deleted"
        );
        assert!(
            !after.complete,
            "and the index must not claim on disk to cover what it did not walk"
        );
        let mut kept: Vec<&str> = after.rows.iter().map(|r| r.path.as_str()).collect();
        kept.sort_unstable();
        assert_eq!(
            kept,
            vec!["archive/linked.gb", "plain.gb"],
            "the row behind the link survives"
        );

        // And it survived with its bases, so the library still answers about it.
        let motif = pl_index::scan::Motif::new("GAATTC").unwrap();
        let linked = after
            .rows
            .iter()
            .find(|r| r.path == "archive/linked.gb")
            .unwrap();
        assert_eq!(
            pl_index::scan::find_in_row(&motif, &after.packed, linked).len(),
            1,
            "a carried-forward row kept its sequence"
        );

        // The junction goes first, so nothing can reach through it.
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&target);
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
