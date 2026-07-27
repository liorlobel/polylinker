//! Does an incremental rescan agree with a full rebuild?
//!
//! This is the test the brief called the one that will actually bite, and no
//! example test finds it. The invariant is byte-equality — `codec::to_bytes` of
//! a rescan must equal `to_bytes` of a rebuild — because comparing anything
//! weaker lets a wrong `seq_off` through, and a wrong offset makes the index
//! answer confidently about the wrong molecule.
//!
//! Every mutation a real folder undergoes is exercised: create, edit, truncate,
//! delete, rename, move across directories, touch-without-change, and
//! change-without-touch.

use std::path::{Path, PathBuf};

use pl_index::codec;
use pl_scan::{scan, ScanOptions, WalkOptions};

fn tmp(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("pl-scan-conv-{name}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn gb(name: &str, seq: &str, circular: bool) -> String {
    format!(
        "LOCUS       {name}    {} bp    DNA     {} SYN 26-JUL-2026\n\
         FEATURES             Location/Qualifiers\n     \
         misc_feature    1..3\n                     /label=\"f-{name}\"\n\
         ORIGIN\n        1 {seq}\n//\n",
        seq.len(),
        if circular { "circular" } else { "linear" }
    )
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = pl_scan::abs(root, rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn rm(root: &Path, rel: &str) {
    let _ = std::fs::remove_file(pl_scan::abs(root, rel));
}

fn mv(root: &Path, from: &str, to: &str) {
    let dst = pl_scan::abs(root, to);
    std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
    let _ = std::fs::rename(pl_scan::abs(root, from), dst);
}

fn rng(state: &mut u64) -> u64 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    state.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

/// One filesystem mutation, named for the failure message.
type Step = (&'static str, Box<dyn Fn(&Path, &mut u64)>);

fn opts(previous: Option<pl_index::codec::Library>) -> ScanOptions {
    ScanOptions {
        walk: WalkOptions::default(),
        previous,
    }
}

/// Compare on bytes, with `built_ns` held equal so the timestamp is not the
/// thing that differs.
fn assert_converged(root: &Path, previous: &pl_index::codec::Library, label: &str) {
    let (rescanned, _) = scan(root, 1, &opts(Some(previous.clone())));
    let (rebuilt, _) = scan(root, 1, &opts(None));
    assert_eq!(
        codec::to_bytes(&rescanned).len(),
        codec::to_bytes(&rebuilt).len(),
        "{label}: rescan and rebuild differ in size"
    );
    assert_eq!(
        codec::to_bytes(&rescanned),
        codec::to_bytes(&rebuilt),
        "{label}: rescan is not byte-identical to a rebuild\n\
         rescan rows: {:?}\nrebuild rows: {:?}",
        rescanned
            .rows
            .iter()
            .map(|r| (&r.path, r.record, r.seq_off, r.seq_bases))
            .collect::<Vec<_>>(),
        rebuilt
            .rows
            .iter()
            .map(|r| (&r.path, r.record, r.seq_off, r.seq_bases))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn rescan_always_equals_rebuild_across_every_kind_of_mutation() {
    let root = tmp("mutations");
    write(&root, "a.gb", &gb("a", "GAATTCAAAA", true));
    write(&root, "b.gb", &gb("b", "CCCCCCCCCC", false));
    write(&root, "sub/c.gb", &gb("c", "TTTTGGGGAA", true));
    write(&root, "d.fa", ">d\nACGTACGTAC\n");

    let (mut lib, _) = scan(&root, 1, &opts(None));
    assert_converged(&root, &lib, "no change");

    let mut st = 0x1234_5678_9abc_def0u64;
    let steps: Vec<Step> = vec![
        (
            "create",
            Box::new(|r: &Path, _: &mut u64| write(r, "new.gb", &gb("new", "AAAAGAATTC", true))),
        ),
        (
            "edit",
            Box::new(|r: &Path, _: &mut u64| write(r, "a.gb", &gb("a", "GGGGGGGGGG", true))),
        ),
        (
            "truncate",
            Box::new(|r: &Path, _: &mut u64| write(r, "b.gb", "")),
        ),
        (
            "delete",
            Box::new(|r: &Path, _: &mut u64| rm(r, "sub/c.gb")),
        ),
        (
            "rename",
            Box::new(|r: &Path, _: &mut u64| mv(r, "d.fa", "renamed.fa")),
        ),
        (
            "move across dirs",
            Box::new(|r: &Path, _: &mut u64| mv(r, "renamed.fa", "sub/moved.fa")),
        ),
        (
            "multi-record",
            Box::new(|r: &Path, _: &mut u64| {
                let two = format!(
                    "{}{}",
                    gb("m1", "ACGTACGTAC", true),
                    gb("m2", "TTTTTTTTTT", false)
                );
                write(r, "multi.gb", &two);
            }),
        ),
        (
            "annotation track",
            Box::new(|r: &Path, _: &mut u64| {
                write(
                    r,
                    "track.gb",
                    "LOCUS       t    3000 bp    DNA     circular SYN 26-JUL-2026\nORIGIN\n//\n",
                );
            }),
        ),
        (
            "not a sequence file",
            Box::new(|r: &Path, _: &mut u64| {
                let mut abif = b"ABIF".to_vec();
                abif.extend_from_slice(&[0u8; 64]);
                std::fs::write(pl_scan::abs(r, "trace.ab1"), abif).unwrap();
            }),
        ),
        (
            "odd length",
            Box::new(|r: &Path, _: &mut u64| write(r, "odd.gb", &gb("odd", "ACGTACG", true))),
        ),
        (
            "recreate deleted",
            Box::new(|r: &Path, _: &mut u64| write(r, "sub/c.gb", &gb("c", "GAATTCGGGG", true))),
        ),
        (
            "random edit",
            Box::new(|r: &Path, s: &mut u64| {
                let bases = b"ACGT";
                let seq: String = (0..10)
                    .map(|_| bases[(rng(s) % 4) as usize] as char)
                    .collect();
                write(r, "a.gb", &gb("a", &seq, rng(s) % 2 == 0));
            }),
        ),
        (
            "delete everything in sub",
            Box::new(|r: &Path, _: &mut u64| {
                let _ = std::fs::remove_dir_all(pl_scan::abs(r, "sub"));
            }),
        ),
    ];

    for (label, step) in &steps {
        step(&root, &mut st);
        assert_converged(&root, &lib, label);
        lib = scan(&root, 1, &opts(Some(lib.clone()))).0;
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_unchanged_file_is_reused_rather_than_reparsed() {
    // Otherwise the incremental path is only decoration: correct, and no
    // faster than a rebuild.
    let root = tmp("reuse");
    for i in 0..5 {
        write(&root, &format!("f{i}.gb"), &gb("x", "GAATTCAAAA", true));
    }
    let (lib, first) = scan(&root, 1, &opts(None));
    assert_eq!(first.parsed, 5);
    assert_eq!(first.reused, 0);

    let (_, second) = scan(&root, 2, &opts(Some(lib.clone())));
    assert_eq!(
        second.parsed, 0,
        "nothing changed, so nothing should re-parse"
    );
    assert_eq!(second.reused, 5);

    // Change one, and only that one is re-parsed.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write(&root, "f2.gb", &gb("x", "TTTTTTTTTT", true));
    let (_, third) = scan(&root, 3, &opts(Some(lib)));
    assert_eq!(third.parsed, 1, "only the edited file");
    assert_eq!(third.reused, 4);

    let _ = std::fs::remove_dir_all(&root);
}

/// The mtime hole, which cannot be reproduced by writing twice.
///
/// `write; scan; write; scan` always crosses an mtime tick on NTFS, so the
/// ledger looks exact and the hole ships untested. This forges the timestamp
/// back so `(size, mtime)` are unchanged over changed content.
#[test]
fn changed_content_under_an_unchanged_timestamp_is_missed_by_a_plain_scan() {
    let root = tmp("mtimehole");
    let file = pl_scan::abs(&root, "a.gb");

    // Both writes are pinned to the same fixed timestamp. Restoring a captured
    // mtime does not work: the platform hands it back at 100 ns resolution and
    // the tools that set it take seconds, so the "restored" value differs and
    // the file looks changed for the wrong reason.
    write(&root, "a.gb", &gb("a", "GAATTCAAAA", true));
    set_mtime(&file);

    let (lib, _) = scan(&root, 1, &opts(None));
    let before = lib.rows[0].content.clone();

    // Same length, different bases, same timestamp.
    write(&root, "a.gb", &gb("a", "TTTTTTTTTT", true));
    set_mtime(&file);
    assert_eq!(
        std::fs::metadata(&file).unwrap().len(),
        lib.rows[0].size,
        "the test needs the size to be unchanged too"
    );

    let (after, report) = scan(&root, 2, &opts(Some(lib.clone())));
    assert_eq!(
        report.parsed, 0,
        "with size and mtime unchanged, the cheap path cannot know"
    );
    assert_eq!(
        after.rows[0].content, before,
        "the stale row is reused, which is the documented limitation"
    );

    // And `verify` -- re-reading the bytes -- is what makes that limitation
    // checkable rather than theoretical.
    let disk = std::fs::read(pl_scan::abs(&root, "a.gb")).unwrap();
    assert_ne!(
        pl_scan::content_id(&disk),
        after.rows[0].content,
        "re-reading the file does catch it"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_file_whose_mtime_moved_but_whose_bytes_did_not_is_not_reparsed() {
    // What a sync client does to every file it touches. Without this check,
    // every OneDrive sync re-parses the entire library.
    let root = tmp("touched");
    write(&root, "a.gb", &gb("a", "GAATTCAAAA", true));
    let (lib, _) = scan(&root, 1, &opts(None));

    // Rewrite identical bytes, which moves the mtime and not the content.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write(&root, "a.gb", &gb("a", "GAATTCAAAA", true));

    let (_, report) = scan(&root, 2, &opts(Some(lib)));
    assert_eq!(report.parsed, 0, "the bytes are the same; do not re-parse");
    assert_eq!(report.touched_only, 1, "and say that is what happened");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn two_byte_identical_files_do_not_swap_places_when_one_is_renamed() {
    // The false-positive twin of rename detection: with two identical files, a
    // naive content-hash lookup adopts the wrong row and the paths end up
    // swapped, so every later query points at the wrong file on disk.
    let root = tmp("twins");
    let body = gb("same", "GAATTCAAAA", true);
    write(&root, "one.gb", &body);
    write(&root, "two.gb", &body);
    let (lib, _) = scan(&root, 1, &opts(None));
    assert_eq!(lib.rows.len(), 2);

    mv(&root, "one.gb", "renamed.gb");
    let (after, _) = scan(&root, 2, &opts(Some(lib)));
    let mut paths: Vec<&str> = after.rows.iter().map(|r| r.path.as_str()).collect();
    paths.sort_unstable();
    assert_eq!(paths, vec!["renamed.gb", "two.gb"]);
    // And each row's name still belongs to a file that exists.
    for r in &after.rows {
        assert!(
            pl_scan::abs(&root, &r.path).exists(),
            "{} points at nothing",
            r.path
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_partial_walk_removes_nothing() {
    // A share that blinks must never read as a mass deletion.
    let root = tmp("partial");
    write(&root, "a.gb", &gb("a", "GAATTCAAAA", true));
    write(&root, "b.gb", &gb("b", "CCCCCCCCCC", true));
    let (lib, _) = scan(&root, 1, &opts(None));
    assert_eq!(lib.rows.len(), 2);

    // The root vanishes entirely.
    std::fs::remove_dir_all(&root).unwrap();
    let (after, report) = scan(&root, 2, &opts(Some(lib.clone())));
    assert!(report.incomplete.is_some());
    assert_eq!(
        report.removed, 0,
        "nothing may be dropped on a partial walk"
    );
    assert_eq!(
        after.rows.len(),
        2,
        "every row we could not reach is carried forward"
    );
    assert!(!after.complete, "and the index says it is partial");

    // The carried-forward rows still answer correctly, which means their bases
    // came along too.
    let motif = pl_index::scan::Motif::new("GAATTC").unwrap();
    let a = after.rows.iter().find(|r| r.path == "a.gb").unwrap();
    assert_eq!(
        pl_index::scan::find_in_row(&motif, &after.packed, a).len(),
        1,
        "a carried-forward row kept its sequence"
    );
}

#[test]
fn a_deleted_file_is_dropped_and_counted_when_the_walk_did_finish() {
    let root = tmp("deleted");
    write(&root, "a.gb", &gb("a", "GAATTCAAAA", true));
    write(&root, "b.gb", &gb("b", "CCCCCCCCCC", true));
    let (lib, _) = scan(&root, 1, &opts(None));

    rm(&root, "b.gb");
    let (after, report) = scan(&root, 2, &opts(Some(lib)));
    assert_eq!(report.removed, 1);
    assert_eq!(after.rows.len(), 1);
    assert_eq!(after.rows[0].path, "a.gb");
    assert!(after.complete);
    let _ = std::fs::remove_dir_all(&root);
}

/// Force a file's mtime to one fixed instant.
///
/// `std` cannot set a timestamp and a dependency for one test is not worth it,
/// so this shells out. A **fixed** value rather than a captured-and-restored
/// one, because the platform reports mtimes at 100 ns resolution and both of
/// these tools take whole seconds — restoring a captured value lands somewhere
/// nearby and the file then looks changed for a reason the test is not about.
const FIXED_MTIME: &str = "2020-01-02T03:04:05";

#[cfg(windows)]
fn set_mtime(path: &Path) {
    let script = format!(
        "(Get-Item -LiteralPath '{}').LastWriteTimeUtc = \
         [DateTime]::ParseExact('{FIXED_MTIME}','yyyy-MM-ddTHH:mm:ss',$null)",
        path.display()
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .expect("powershell");
    assert!(
        out.status.success(),
        "failed to forge mtime: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(not(windows))]
fn set_mtime(path: &Path) {
    let out = std::process::Command::new("touch")
        .args([
            "-d",
            &format!("{FIXED_MTIME}Z"),
            &path.display().to_string(),
        ])
        .output()
        .expect("touch");
    assert!(out.status.success());
}
