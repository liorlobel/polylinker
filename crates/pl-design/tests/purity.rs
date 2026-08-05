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
//!
//! **The brace group.** This file said it was modelled on `pl-index`'s and for
//! a while was not: it kept `pl-index`'s *original* `line.contains(needle)`,
//! which `pl-index` had already replaced. `use std::{cmp, fs};` never puts
//! `std` and `fs` adjacent, so a `str::contains` scanner reads straight past
//! it, and the meta-test below probed only the adjacent spelling — so the gap
//! was invisible from inside the suite. That is not evasion, which is what the
//! remaining known misses are; it is what an editor's auto-import writes the
//! moment this crate grows any braced `std` import. `grouped_std_import` closes
//! it by parsing the `use` tree into path segments rather than matching
//! substrings, and because rustfmt wraps such a group across lines as soon as
//! it outgrows the line — leaving `fs` alone on a line that names nothing else
//! — `leaks_in` joins a `std`-rooted `use` to its `;` before judging it.
//! Closing the one-line form and not the wrapped one would have left the gap
//! open in the shape rustfmt actually produces.
//!
//! **What it can still miss**, the same three as `pl-index`: a root alias
//! (`use std as sys;`, then `sys::fs::read`), a macro that assembles the path,
//! and a dependency doing the I/O on this crate's behalf. The last is what
//! `the_manifest_lists_only_workspace_crates` is for; the first two are
//! deliberate evasion rather than the accident this is written against. An
//! aliased *leaf*, `use std::fs as disk;`, is **not** a miss — aliasing the
//! leaf still writes `std::fs` in the import — and the meta-test pins that,
//! because naming a non-gap sends a maintainer looking in the wrong place.

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

/// Every offending statement in `text`, as `(line number, statement, why)`.
///
/// One line is one statement, except for a `std`-rooted `use` wrapped across
/// several — that is joined to its `;` and reported against the line that
/// opened it, because no single line of a wrapped brace group says anything
/// incriminating on its own.
///
/// Comments are exempt: the prose above has to be able to name `std::fs` in
/// order to explain why it is not there, and a line whose first non-space
/// characters are `//` cannot execute. This crate has no block comments and no
/// `//` inside a string literal, which is what makes the cheap rule sound; a
/// `/*` appearing here later would need this revisited.
fn leaks_in(text: &str) -> Vec<(usize, String, &'static str)> {
    let mut out = Vec::new();
    // A `std`-rooted `use` is judged as one statement, not line by line.
    // rustfmt wraps a brace group the moment it outgrows the line, and the
    // wrapped form is where every half of the path sits on a line that gives
    // nothing away: in
    //
    //     use std::{
    //         cmp,
    //         fs,
    //     };
    //
    // the line naming `std` names nothing banned and the line naming `fs` is
    // just an identifier. Joining to the `;` is what makes closing the one-line
    // group mean anything, since the one-line group is what an editor writes
    // and the wrapped one is what rustfmt turns it into.
    let mut open: Option<(usize, String)> = None;
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let (at, stmt) = match open.take() {
            Some((at, mut acc)) => {
                acc.push(' ');
                acc.push_str(line.trim());
                (at, acc)
            }
            None => (i, line.to_string()),
        };
        if opens_a_std_use(&stmt) {
            open = Some((at, stmt));
            continue;
        }
        judge(at, &stmt, &mut out);
    }
    // A `use` with no `;` is malformed source rather than clean source, but it
    // still has to be judged: dropping it would let a leak hide behind a typo.
    if let Some((at, stmt)) = open {
        judge(at, &stmt, &mut out);
    }
    out
}

/// Record every banned spelling in one whole statement.
fn judge(at: usize, stmt: &str, out: &mut Vec<(usize, String, &'static str)>) {
    for (needle, why) in BANNED {
        // `contains` first, so a statement that spells the path out is
        // reported once rather than twice.
        if stmt.contains(needle) || grouped_std_import(stmt, needle) {
            out.push((at + 1, stmt.trim().to_string(), *why));
        }
    }
}

/// Is this the start of a `std`-rooted `use` that has not reached its `;` yet?
///
/// Deliberately narrow. Only a tree already known to be std's may swallow the
/// lines beneath it, so a wrapped `use crate::{..}` — or any line that merely
/// begins with the word `use` — is still judged on its own and cannot drag
/// unrelated code into one blob.
fn opens_a_std_use(stmt: &str) -> bool {
    !stmt.contains(';') && use_tree(stmt).is_some_and(|tree| segments(tree).next() == Some("std"))
}

/// Does this `use` statement import a banned module through a brace group?
///
/// `line.contains("std::fs")` cannot see `use std::{cmp, fs, mem};` — inside a
/// group the two halves of the path are never adjacent — and that is the one
/// spelling nobody has to *intend*: an editor's auto-import folds `fs` into an
/// existing braced `std` import, and `fs::read` then reaches `report.rs` past
/// this gate. A `use` tree holds nothing but path segments and aliases, so
/// splitting it on everything that is not an identifier character is exact.
/// Matching substrings instead — adding `", fs"` and `"{fs"` to [`BANNED`] —
/// was the obvious repair and is wrong: it fires on `fn f(a: u8, fs: &Store)`,
/// on `let (n, fs) = ..`, and on `format!("{fs}")`, and the `BANNED` self-test
/// cannot notice, because a probe built as `format!("let x = {needle};")`
/// trivially contains its own needle.
fn grouped_std_import(line: &str, needle: &str) -> bool {
    // Only the path-rooted needles have halves a brace can separate.
    // `SystemTime`, `Instant::now`, `HashMap`, `HashSet` and `partial_cmp` are
    // single tokens that survive any grouping, so `contains` already sees them.
    let Some(module) = needle.strip_prefix("std::") else {
        return false;
    };
    let Some(tree) = use_tree(line) else {
        return false;
    };
    let mut segments = segments(tree);
    // Rooted at `std`, or it is not std's. `use crate::fs::Table;` and
    // `use pl_core::env::Params;` are this workspace's own modules and must not
    // trip a filesystem gate.
    if segments.next() != Some("std") {
        return false;
    }
    let mut after_as = false;
    for seg in segments {
        // `use std::io::Read as fs;` binds a local name. It does not reach for
        // `std::fs`, and flagging it would be a false positive.
        if after_as {
            after_as = false;
        } else if seg == "as" {
            after_as = true;
        } else if seg == module {
            return true;
        }
    }
    false
}

/// The path tree of a `use` statement, or `None` if this is not one.
fn use_tree(line: &str) -> Option<&str> {
    let stmt = line.trim_start();
    match stmt.strip_prefix("use ") {
        Some(t) => Some(t),
        // `pub use`, `pub(crate) use`.
        None if stmt.starts_with("pub") => stmt.find("use ").map(|i| &stmt[i + "use ".len()..]),
        None => None,
    }
}

/// The identifiers in a `use` tree, in order.
///
/// A tree holds nothing but path segments, separators, braces and `as`, so
/// splitting on everything that is not an identifier character is exact — and
/// exactness is the point: matching substrings instead fires on ordinary code
/// that merely spells one of these words.
fn segments(tree: &str) -> impl Iterator<Item = &str> {
    tree.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty())
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
///
/// The brace-group half of this test is the part that had to be added: a bare
/// `line.contains("std::fs")` cannot see `use std::{cmp, fs};`, because inside
/// a group the two halves of the path are never adjacent. That spelling is not
/// evasion — it is what an editor's auto-import writes — so every form it takes
/// is probed here: the one-line group, the group after rustfmt has wrapped it
/// (where no single line says anything incriminating), a comment inside the
/// wrapped group, a `use` that never met its `;`, and a leaf alias.
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

    // Every banned spelling is detected, not merely the first.
    for (needle, _) in BANNED {
        let src = format!("let x = {needle};");
        assert_eq!(leaks_in(&src).len(), 1, "{needle} is not actually checked");
    }

    // The one-line brace group. Nobody types this on purpose; an editor folds
    // `fs` into an existing braced `std` import and the leak arrives with it.
    for grouped in [
        "use std::{cmp, fs, mem};",
        "use std::{fs::read, mem};",
        "    pub(crate) use std::{env, fmt};",
        "use std::{net::TcpStream, str::from_utf8};",
        "use std::{fmt, process::Command};",
        "use std::collections::{BTreeMap, HashMap};",
    ] {
        assert_eq!(
            leaks_in(grouped).len(),
            1,
            "a braced std import must go red: {grouped}"
        );
    }

    // A leaf alias still writes `std::fs` in the import, so it is not a gap.
    assert_eq!(leaks_in("use std::fs as disk;").len(), 1);

    // The same group after rustfmt has wrapped it, which is what the one-line
    // form turns into the moment it outgrows the line. Every line here is
    // innocent on its own, which is exactly why the statement is judged whole.
    let wrapped = "use std::{\n    cmp,\n    fs,\n    mem,\n};\nlet _ = fs::read(\"x\");\n";
    let found = leaks_in(wrapped);
    assert_eq!(found.len(), 1, "a wrapped std group must go red: {found:?}");
    assert_eq!(found[0].0, 1, "and it names the line that opened the use");

    // A comment inside the group must not end the statement early.
    assert_eq!(
        leaks_in("use std::{\n    // grouped by an editor\n    fs,\n};").len(),
        1
    );

    // A `use` that never met its `;` is malformed source, not exempt source:
    // dropping it would let a leak hide behind a typo.
    assert_eq!(leaks_in("use std::{\n    process::Command,").len(), 1);

    // Joining lines must not smear unrelated code together. A wrapped tree that
    // is not std's, and a std tree holding nothing banned, must stay green.
    for innocent_block in [
        "use crate::{\n    fs::Table,\n    scan::Motif,\n};",
        "use pl_core::{\n    env::Params,\n    sha1::sha1,\n};",
        "use std::{\n    fmt,\n    str::from_utf8,\n};",
        "use std::{\n    fmt::Write as fs,\n};",
        "let banner = \"use std::{\";\nlet n = 1;",
    ] {
        assert!(
            leaks_in(innocent_block).is_empty(),
            "must stay green: {innocent_block:?} -> {:?}",
            leaks_in(innocent_block)
        );
    }

    // And ordinary Rust that merely spells one of these words must stay green,
    // which is why a `use` tree is matched by path segment and not by
    // substring: `", fs"` and `"{fs"` as needles would fire on all of these.
    for innocent in [
        "fn write(a: u8, fs: &Store) -> u8 { a }",
        "let (n, fs) = split(x);",
        "let s = format!(\"{fs}\");",
        "let r = Row { fs: 1, env: 2 };",
        "use crate::fs::Table;",
        "use pl_core::env::Params;",
        "use std::io::Read as fs;",
        "use std::{fmt, str::from_utf8};",
    ] {
        assert!(
            leaks_in(innocent).is_empty(),
            "ordinary code must not trip the gate: {innocent} -> {:?}",
            leaks_in(innocent)
        );
    }
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
