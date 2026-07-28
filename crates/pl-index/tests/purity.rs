//! Is the search engine really free of storage concerns?
//!
//! `tools/ci.ps1` builds this crate for `wasm32-unknown-unknown` and says the
//! step "goes red the day a storage concern leaks in". It does not.
//! `wasm32-unknown-unknown` ships the full standard filesystem, environment,
//! subprocess and clock surfaces through its `unsupported` platform layer: the
//! call compiles, links, and returns `ErrorKind::Unsupported` at run time — on
//! a target this project never runs. That build catches a wasm-incompatible
//! *dependency* and an OS-specific API, which is worth having and is not the
//! same claim. It cannot catch a line of code. This can.
//!
//! Reading source files to prove the source reads no files is not a joke at
//! this test's expense. The rule is about what `pl-index` *ships*; a test is
//! built for the host, never for wasm, and never linked into the library.
//!
//! **What it can miss**: an aliased import (`use std::fs as disk;`), a macro
//! that assembles the path, a dependency that does the I/O on this crate's
//! behalf. The last of those is what the wasm32 build is for. The first two
//! are deliberate evasion rather than the accident this is written against —
//! the accident is someone reaching for `std::fs::read` in `codec.rs` because
//! the function is called `parse` and a path was to hand.

use std::path::{Path, PathBuf};

/// Spellings that would mean a storage, environment, subprocess or clock
/// concern had grown inside the pure layer, and why each one matters here.
const BANNED: &[(&str, &str)] = &[
    ("std::fs", "the filesystem is pl-scan's, and only pl-scan's"),
    (
        "std::env",
        "an answer that depends on the environment is not provable",
    ),
    ("std::process", "nothing here may shell out"),
    ("std::net", "an index query must never reach the network"),
    (
        "SystemTime",
        "a clock makes a pure function's answer depend on when it was asked; \
         `built_ns` is passed in for exactly this reason",
    ),
    ("Instant::now", "same: no ambient clock"),
];

/// Every offending line of `text`, as `(line number, line, why)`.
///
/// Comments are exempt. The prose in `lib.rs` has to be able to name
/// `std::fs::read` in order to explain why it is not there, and a line whose
/// first non-space characters are `//` cannot execute. This crate has no block
/// comments and no `//` inside a string literal, which is what makes the cheap
/// rule sound; a `/*` appearing here later would need this revisited.
fn leaks_in(text: &str) -> Vec<(usize, String, &'static str)> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("//") {
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

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read the crate's own src/") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out.sort();
}

#[test]
fn no_source_file_in_this_crate_reaches_for_the_filesystem_or_the_clock() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rs_files(&src, &mut files);
    // A check that found nothing to check is the failure mode this whole
    // finding was about, so say the number out loud.
    assert!(
        files.len() >= 4,
        "only {} source file(s) under {} — the check is looking in the wrong place",
        files.len(),
        src.display()
    );

    let mut leaks: Vec<String> = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).expect("read a source file");
        for (line_no, line, why) in leaks_in(&text) {
            leaks.push(format!(
                "{}:{line_no}: {why}\n    {line}",
                f.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    assert!(
        leaks.is_empty(),
        "a storage concern has leaked into the pure search engine. It belongs \
         in pl-scan, and the wasm32 build will not catch it:\n{}",
        leaks.join("\n")
    );
}

#[test]
fn the_purity_check_would_actually_go_red() {
    // A gate nobody has watched fail is not a gate — which is the whole reason
    // this file exists. The predicate is run against the exact line from the
    // finding, and against the prose that must stay legal.
    let leaked = "pub fn parse(bytes: &[u8]) {\n    let _ = std::fs::read(\"library.plx\");\n}\n";
    let found = leaks_in(leaked);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, 2, "and it names the line");

    for line in [
        "//! Nothing here calls std::fs::read; pl-scan does.",
        "    // std::env::var would make this untestable",
        "    /// `SystemTime` is deliberately absent: `built_ns` is passed in.",
    ] {
        assert!(
            leaks_in(line).is_empty(),
            "prose must be able to name what it is forbidding: {line}"
        );
    }

    // And every banned spelling is detected, not just the first.
    for (needle, _) in BANNED {
        let src = format!("let x = {needle};");
        assert_eq!(leaks_in(&src).len(), 1, "{needle} is not actually checked");
    }
}
