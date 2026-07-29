//! Is the search engine really free of storage concerns?
//!
//! `tools/ci.ps1` builds this crate for `wasm32-unknown-unknown` under the step
//! name "pl-index stays pure (wasm32)". That build is not the check its name
//! suggests, and this file is the other half.
//! `wasm32-unknown-unknown` ships the full standard filesystem, environment,
//! subprocess and clock surfaces through its `unsupported` platform layer: the
//! call compiles, links, and returns `ErrorKind::Unsupported` at run time — on
//! a target this project never runs. That build catches a wasm-incompatible
//! *dependency* and an OS-specific API, which is worth having and is not the
//! same claim. It cannot catch a line of code. This can.
//!
//! ci.ps1 used to assert the stronger claim in a comment — "wasm32 has no
//! filesystem, so this step goes red the day a storage concern leaks in" —
//! which is the sentence a developer reads first when the gate goes red. It now
//! says what it enforces and points here. If that comment is ever rewritten,
//! this paragraph has to move with it: two files disagreeing about one gate is
//! how the wrong one gets debugged.
//!
//! Reading source files to prove the source reads no files is not a joke at
//! this test's expense. The rule is about what `pl-index` *ships*; a test is
//! built for the host, never for wasm, and never linked into the library.
//!
//! **What it can miss**: a root alias (`use std as sys;`, then
//! `sys::fs::read`), a macro that assembles the path, a dependency that does
//! the I/O on this crate's behalf. The last of those is what the wasm32 build
//! is for. The first two are deliberate evasion rather than the accident this
//! is written against — the accident is someone reaching for `std::fs::read`
//! in `codec.rs` because the function is called `parse` and a path was to hand.
//!
//! **What it does not miss**, though this note once said it did: an aliased
//! *leaf*, `use std::fs as disk;`. Aliasing the leaf still writes `std::fs` in
//! the import, so the file goes red on that line whether or not the later
//! `disk::read(..)` is recognised. Naming it as a gap sent a maintainer looking
//! at a non-gap, and away from the one that was real — a brace group,
//! `use std::{cmp, fs, mem};`, whose two path halves are never adjacent for
//! `str::contains` to find. That one is not evasion at all; it is what an
//! editor's auto-import writes the moment this crate grows any braced `std`
//! import. `grouped_std_import` closes it, and because rustfmt wraps that
//! group across lines as soon as it outgrows the line — leaving `fs` alone on
//! a line that names nothing else — `leaks_in` joins a `std`-rooted `use` to
//! its `;` before judging it. Closing the one-line form and not the wrapped
//! one would have left the gap open in the shape an editor actually produces.

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

/// Every offending statement in `text`, as `(line number, statement, why)`.
///
/// One line is one statement, except for a `std`-rooted `use` wrapped across
/// several — that is joined to its `;` and reported against the line that
/// opened it, because no single line of a wrapped brace group says anything
/// incriminating on its own.
///
/// Comments are exempt. The prose in `lib.rs` has to be able to name
/// `std::fs::read` in order to explain why it is not there, and a line whose
/// first non-space characters are `//` cannot execute. This crate has no block
/// comments and no `//` inside a string literal, which is what makes the cheap
/// rule sound; a `/*` appearing here later would need this revisited.
fn leaks_in(text: &str) -> Vec<(usize, String, &'static str)> {
    let mut out = Vec::new();
    // A `std`-rooted `use` is judged as one statement, not line by line.
    // rustfmt wraps a brace group the moment it outgrows the line, and the
    // wrapped form is where every half of the path is on a line that gives
    // nothing away: in
    //
    //     use std::{
    //         cmp,
    //         fs,
    //     };
    //
    // the line naming `std` names nothing banned and the line naming `fs` is
    // just an identifier. Joining to the `;` is what makes closing the one-line
    // group mean something, since the one-line group is what an editor writes
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
        // `contains` first, so a line that spells the path out is reported
        // once rather than twice.
        if stmt.contains(needle) || grouped_std_import(stmt, needle) {
            out.push((at + 1, stmt.trim().to_string(), *why));
        }
    }
}

/// Is this the start of a `std`-rooted `use` that has not reached its `;` yet?
///
/// Deliberately narrow. Only a tree already known to be std's is allowed to
/// swallow the lines beneath it, so a wrapped `use crate::{..}` — or any line
/// that merely begins with the word `use` — is still judged on its own and
/// cannot drag unrelated code into one blob.
fn opens_a_std_use(stmt: &str) -> bool {
    !stmt.contains(';') && use_tree(stmt).is_some_and(|tree| segments(tree).next() == Some("std"))
}

/// Does this `use` statement import a banned module through a brace group?
///
/// `line.contains("std::fs")` cannot see `use std::{cmp, fs, mem};` — inside a
/// group the two halves of the path are never adjacent — and that is the one
/// spelling nobody has to *intend*: an editor's auto-import folds `fs` into an
/// existing braced `std` import, and `fs::read` then reaches `codec.rs` past
/// both this gate and the wasm32 build, which cannot see hand-written
/// filesystem code either. A `use` tree holds nothing but path segments and
/// aliases, so splitting it on everything that is not an identifier character
/// is exact. Matching substrings instead — adding `", fs"` and `"{fs"` to
/// [`BANNED`] — was the obvious repair and is wrong: it fires on
/// `fn f(a: u8, fs: &Store)`, on `let (n, fs) = ..`, and on `format!("{fs}")`,
/// and the `BANNED` self-test below cannot notice, because a probe built as
/// `format!("let x = {needle};")` trivially contains its own needle.
fn grouped_std_import(line: &str, needle: &str) -> bool {
    // Only the path-rooted needles have halves a brace can separate;
    // `SystemTime` and `Instant::now` are single tokens `contains` already sees.
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

    // The brace group, which the joined-literal rule alone let through. This
    // is the accident, not the evasion: nobody types it on purpose.
    for grouped in [
        "use std::{cmp, fs, mem};",
        "use std::{fs::read, mem};",
        "    pub(crate) use std::{env, fmt};",
        "use std::{net::TcpStream, str::from_utf8};",
        "use std::{fmt, process::Command};",
    ] {
        assert_eq!(
            leaks_in(grouped).len(),
            1,
            "a braced std import must go red: {grouped}"
        );
    }

    // A leaf alias was documented as a miss and is not one; keep it honest.
    assert_eq!(leaks_in("use std::fs as disk;").len(), 1);

    // And the same group after rustfmt has wrapped it, which is the form the
    // one-line group turns into the moment it outgrows the line. Every line
    // here is innocent on its own, which is exactly why the statement is
    // judged whole.
    let wrapped = "use std::{\n    cmp,\n    fs,\n    mem,\n};\nlet _ = fs::read(\"x\");\n";
    let found = leaks_in(wrapped);
    assert_eq!(found.len(), 1, "a wrapped std group must go red: {found:?}");
    assert_eq!(found[0].0, 1, "and it names the line that opened the use");
    // A comment inside the group must not end the statement early.
    assert_eq!(
        leaks_in("use std::{\n    // grouped by an editor\n    fs,\n};").len(),
        1
    );
    // A `use` that never met its `;` is malformed, not exempt.
    assert_eq!(leaks_in("use std::{\n    process::Command,").len(), 1);

    // Wrapped trees that are not std's, and std trees holding nothing banned,
    // must stay green -- joining lines must not smear unrelated code together.
    for innocent_block in [
        "use crate::{\n    fs::Table,\n    scan::Motif,\n};",
        "use pl_core::{\n    env::Params,\n    sha1::sha1,\n};",
        "use std::{\n    fmt,\n    str::from_utf8,\n};",
        "use std::{\n    fmt::Write as fs,\n};",
        // The word `use` inside ordinary code opens nothing.
        "let banner = \"use std::{\";\nlet n = 1;",
    ] {
        assert!(
            leaks_in(innocent_block).is_empty(),
            "must stay green: {innocent_block:?} -> {:?}",
            leaks_in(innocent_block)
        );
    }

    // And ordinary Rust that merely spells one of these words must stay green,
    // which is why this matches path segments rather than substrings.
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
