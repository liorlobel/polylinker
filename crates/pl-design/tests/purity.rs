//! Is the designer really free of storage, clock and environment?
//!
//! Modelled on `crates/pl-index/tests/purity.rs`, and for the reason that file
//! gives: a `wasm32-unknown-unknown` build catches a wasm-incompatible
//! *dependency* and an OS-specific API, and cannot catch a line of code —
//! wasm32 ships the full filesystem, environment, subprocess and clock surfaces
//! through its `unsupported` platform layer, so the call compiles, links, and
//! returns `ErrorKind::Unsupported` on a target this project never runs. Both
//! checks exist; they are different claims.
//!
//! Reading source to prove the source reads no files is not a joke at this
//! test's expense. The rule is about what `pl-design` *ships*; a test is built
//! for the host and never linked into the library.
//!
//! `HashMap` and `HashSet` are banned here as well as I/O, and that is the
//! design-specific half: `RandomState` is seeded per process, so a report whose
//! order came from a hash iteration would differ between runs of the same
//! binary. `determinism.rs` would catch it; this says where to look.

use std::path::{Path, PathBuf};

const BANNED: &[(&str, &str)] = &[
    ("std::fs", "the filesystem is pl-scan's, and only pl-scan's"),
    (
        "std::env",
        "an answer that depends on the environment is not provable",
    ),
    ("std::process", "nothing here may shell out"),
    ("std::net", "a primer design must never reach the network"),
    (
        "SystemTime",
        "a clock makes a pure function's answer depend on when it was asked",
    ),
    ("Instant::now", "same: no ambient clock"),
    (
        "HashMap",
        "RandomState is seeded per process, so anything iterating a HashMap \
         makes the report order differ between runs; use BTreeMap",
    ),
    (
        "HashSet",
        "same: use BTreeSet, or a Vec that is sorted where the order matters",
    ),
    (
        "partial_cmp",
        "`partial_cmp().unwrap()` panics on NaN, and NaN is reachable from the \
         command line in this repo's own history -- `pl tm --na nan` parsed, \
         failed `NaN > 0.0`, and printed a wrong number. Compare quantised \
         integers, or f64::total_cmp",
    ),
];

/// Every offending line, as `(line number, line, why)`.
///
/// Comments are exempt: the prose above has to be able to name `std::fs` in
/// order to explain why it is not there, and a line whose first non-space
/// characters are `//` cannot execute. This crate has no block comments and no
/// `//` inside a string literal.
fn leaks_in(text: &str) -> Vec<(usize, String, &'static str)> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        for (needle, why) in BANNED {
            if line.contains(needle) {
                out.push((i + 1, line.trim().to_string(), *why));
            }
        }
    }
    out
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir)
        .expect("the crate's own src/")
        .flatten()
    {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn nothing_under_src_touches_storage_a_clock_or_a_hash_order() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &mut files);
    files.sort();
    assert!(files.len() >= 7, "found only {} sources", files.len());

    let mut bad = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).expect("readable");
        for (line, src, why) in leaks_in(&text) {
            bad.push(format!("{}:{line}: {src}\n    -- {why}", f.display()));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The scanner itself can fail, which is what stops the test above being a
/// tautology over a pattern that never matches.
#[test]
fn the_scanner_finds_what_it_is_looking_for() {
    let sample = "let x = std::fs::read(p);\n// std::fs::read in a comment is fine\n\
                  let m: HashMap<u8, u8> = Default::default();\n\
                  a.partial_cmp(&b).unwrap();\n";
    let hits = leaks_in(sample);
    assert_eq!(hits.len(), 3, "{hits:?}");
    assert_eq!(hits[0].0, 1);
    assert_eq!(hits[1].0, 3);
    assert_eq!(hits[2].0, 4);
}

/// The crate declares no external dependencies.
///
/// The workspace rule, checked where it can be read rather than trusted to a
/// review. A `crates.io` dependency would also be what the wasm32 build in
/// `tools/ci.ps1` is there to catch, but this names the rule.
#[test]
fn the_manifest_lists_only_workspace_crates() {
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let deps = manifest
        .split("[dependencies]")
        .nth(1)
        .expect("a dependencies section");
    for line in deps.lines().filter(|l| l.contains('=')) {
        assert!(
            line.contains(".workspace = true"),
            "external dependency: {line}"
        );
        assert!(
            line.trim().starts_with("pl-"),
            "not a workspace crate: {line}"
        );
    }
}
