//! Does the shipped code still only ever *hand off*, and only when asked?
//!
//! Two of `docs/RELEASING.md`'s four requirements are claims about what this
//! crate does **not** do, and a claim of that shape cannot be tested by calling
//! anything. No amount of exercising [`pl_update::fetch_and_verify`] can show
//! that it never launches the file it downloaded, because a version that
//! launched it would pass every functional test in `flow.rs` — the file would
//! still be in the right place with the right hash.
//!
//! * **Requirement 1**, it downloads nothing without being asked *each time*,
//!   is broken by a background thread, a timer, or a "check once a day" clock.
//!   None of those change the result of any call.
//! * **Requirement 4**, it never replaces a running binary silently, is broken
//!   by one convenient line — `Command::new(&handoff.path).spawn()`, or a
//!   `fs::copy` over `current_exe()` — added by somebody who thinks the extra
//!   click is a poor user experience. They are right about the click. That is
//!   why the promise needs something that objects.
//!
//! So this reads the sources. That is not a joke at the test's expense, and it
//! is the same shape as `crates/pl-design/tests/purity.rs`, which reads sources
//! to prove a crate does no I/O. The rule being enforced is about what the
//! crate *ships*, and a test binary is never linked into it.
//!
//! # What is scanned, and what is deliberately not
//!
//! Everything under `src/` **up to the `#[cfg(test)] mod tests` marker**. The
//! test modules are excluded because they legitimately do all of the banned
//! things: they write files, they read `current_exe` to check the guard that
//! refuses it, and none of it is compiled into a release build.
//! [`the_test_marker_is_the_last_item_in_every_file_that_has_one`] is what
//! makes that truncation sound — if a test module were not last, this scan
//! would silently stop early and pass on anything after it.
//!
//! # What it can still miss
//!
//! The same three as `purity.rs`, and they are worth naming rather than
//! implying: a root alias (`use std as sys;` then `sys::process::Command`), a
//! macro that assembles the call, and `pl-core` doing something on this crate's
//! behalf. The first two are deliberate evasion rather than the accident this
//! is written against; the third is answered by `pl-core` being the crate with
//! no I/O in it at all, which `crates/pl-design/tests/purity.rs`'s sibling
//! rules and the wasm32 build in `tools/ci.ps1` speak to.

use std::path::{Path, PathBuf};

/// Spellings that must appear nowhere in shipped code, and why.
///
/// Each is a call, not a concept: "no background work" is not checkable, and
/// `thread::spawn` is.
const BANNED: &[(&str, &str)] = &[
    (
        "thread::spawn",
        "requirement 1: an update check must happen because somebody asked for \
         one, and a thread is how it stops doing that",
    ),
    (
        "std::thread",
        "same: there is nothing here that should outlive the call that started it",
    ),
    (
        "sleep",
        "a wait loop is a timer wearing a different hat; curl owns the timeouts",
    ),
    (
        "Instant::now",
        "requirement 1: a clock is what 'check again if it has been a week' is \
         built out of",
    ),
    (
        "SystemTime",
        "same, and a wall clock additionally makes the answer depend on when it \
         was asked",
    ),
    (
        "Duration",
        "the only timeouts in this crate are curl's, expressed in seconds as \
         command-line arguments; a Duration here would be something waiting",
    ),
    (
        "cmd.exe",
        "the URL handling is careful precisely so that nothing is ever parsed by \
         a shell",
    ),
    ("powershell", "same"),
    ("/bin/sh", "same"),
    ("\"sh\"", "same"),
    (
        "ShellExecute",
        "requirement 4: the downloaded file is handed to a person, not launched",
    ),
    (
        "CommandExt",
        "raw_arg and creation flags are the two ways to get a string to a \
         process without the argv escaping this crate relies on",
    ),
    (
        "fs::write",
        "the only bytes this crate puts on disk are the ones curl writes to the \
         path given by --output, and the rename that follows a verified hash",
    ),
    (
        "fs::copy",
        "requirement 4: copying over an installed file is exactly what this \
         crate promises never to do",
    ),
    (
        "File::create",
        "same as fs::write: there is one writer, and it is curl",
    ),
    ("OpenOptions", "same"),
    (
        "set_permissions",
        "nothing here should ever be making a downloaded file executable",
    ),
    ("PermissionsExt", "same, in its platform-specific spelling"),
    (
        "unsafe",
        "there is no reason for any of this to need it, and a reviewer's \
         attention is finite",
    ),
];

/// `std`-rooted modules that may not be imported, even through a brace group.
///
/// `use std::{process::Command, thread};` never puts `std` and `thread`
/// adjacent, so the substring scan above cannot see it — the same gap
/// `purity.rs` documents and closes, arrived at the same way: an editor folding
/// an auto-import into an existing group.
const BANNED_STD_MODULES: &[(&str, &str)] = &[
    ("thread", "requirement 1: nothing here runs on its own"),
    ("time", "requirement 1: nothing here is on a clock"),
];

/// `Command::new` is allowed in exactly one file, and `current_exe` in one
/// other. Anywhere else, each is a different program being run or a different
/// decision being made about where this binary lives.
const ALLOWED: &[(&str, &str)] = &[("Command::new", "net.rs"), ("current_exe", "flow.rs")];

/// Every offending line in `text`, as `(line number, line, why)`.
///
/// Lines whose first non-space characters are `//` are exempt: the prose in
/// these files has to be able to name `thread::spawn` in order to say why it is
/// absent. This crate has no block comments and no `//` inside a string
/// literal, which is what makes that cheap rule sound.
fn offences_in(text: &str, file: &str) -> Vec<(usize, String, &'static str)> {
    let mut out = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        // A `std`-rooted `use` is judged whole, because rustfmt wraps a brace
        // group across lines the moment it outgrows one and no single line of
        // the wrapped form says anything incriminating.
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
        judge(at, &stmt, file, &mut out);
    }
    if let Some((at, stmt)) = open {
        judge(at, &stmt, file, &mut out);
    }
    out
}

fn judge(at: usize, stmt: &str, file: &str, out: &mut Vec<(usize, String, &'static str)>) {
    for (needle, why) in BANNED {
        if stmt.contains(needle) {
            out.push((at + 1, stmt.trim().to_string(), *why));
        }
    }
    for (needle, why) in BANNED_STD_MODULES {
        if grouped_std_import(stmt, needle) {
            out.push((at + 1, stmt.trim().to_string(), *why));
        }
    }
    for (needle, home) in ALLOWED {
        if stmt.contains(needle) && file != *home {
            out.push((
                at + 1,
                stmt.trim().to_string(),
                if *needle == "Command::new" {
                    "only net.rs runs a program, and the only program it runs is curl"
                } else {
                    "only flow.rs asks where this binary lives, and only to refuse \
                     to write there"
                },
            ));
        }
    }
}

fn opens_a_std_use(stmt: &str) -> bool {
    !stmt.contains(';') && use_tree(stmt).is_some_and(|tree| segments(tree).next() == Some("std"))
}

/// Does this `use` import a banned `std` module through a brace group?
///
/// Split into path segments rather than matched as a substring, for the reason
/// `purity.rs` gives: `", time"` as a needle fires on `fn f(a: u8, time: u64)`
/// and on `format!("{time}")`, and the self-test cannot notice because a probe
/// built from the needle trivially contains it.
fn grouped_std_import(line: &str, module: &str) -> bool {
    let Some(tree) = use_tree(line) else {
        return false;
    };
    let mut segments = segments(tree);
    if segments.next() != Some("std") {
        return false;
    }
    let mut after_as = false;
    for seg in segments {
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

fn use_tree(line: &str) -> Option<&str> {
    let stmt = line.trim_start();
    match stmt.strip_prefix("use ") {
        Some(t) => Some(t),
        None if stmt.starts_with("pub") => stmt.find("use ").map(|i| &stmt[i + "use ".len()..]),
        None => None,
    }
}

fn segments(tree: &str) -> impl Iterator<Item = &str> {
    tree.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty())
}

/// The marker that ends the shipped part of a file.
const TEST_MARKER: &str = "#[cfg(test)]\nmod tests {";

/// `text` up to its test module, which is the part that ships.
fn shipped(text: &str) -> &str {
    match text.find(TEST_MARKER) {
        Some(i) => &text[..i],
        None => text,
    }
}

fn sources() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the crate's own src/")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    out.sort();
    out
}

/// The shipped code starts nothing, waits for nothing, and writes nowhere it
/// should not.
#[test]
fn the_shipped_code_neither_runs_on_its_own_nor_installs_anything() {
    let files = sources();
    assert!(
        files.len() >= 6,
        "found only {} sources, so this scanned almost nothing: {files:?}",
        files.len()
    );

    let mut bad = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        // `testsign.rs` is the signer, and it is `#[cfg(test)]` in its entirety
        // rather than having a test module at the bottom, so it is excluded by
        // name. `the_signer_is_test_only` below is what holds that true.
        if name == "testsign.rs" {
            continue;
        }
        let text = std::fs::read_to_string(path).expect("readable");
        for (line, src, why) in offences_in(shipped(&text), &name) {
            bad.push(format!("{}:{line}: {src}\n    -- {why}", path.display()));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The one program this crate runs is `curl`, and it runs it without a shell.
///
/// The scan above bans `Command::new` outside `net.rs`; this says what the one
/// in `net.rs` is allowed to be. Together they are the whole claim: exactly one
/// program, named by a constant, with its arguments as separate argv elements.
#[test]
fn the_only_program_this_crate_runs_is_curl() {
    let net = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/net.rs"))
        .expect("src/net.rs");
    let shipped = shipped(&net);

    let calls: Vec<&str> = shipped
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .filter(|l| l.contains("Command::new"))
        .collect();
    assert!(!calls.is_empty(), "net.rs must be where curl is run");
    for call in &calls {
        assert!(
            call.contains("Command::new(PROGRAM)") || call.contains("Command::new(\"curl\")"),
            "a program other than curl is being run: {call}"
        );
    }
    // The constant, so that `PROGRAM` cannot quietly become something else.
    assert!(
        shipped.contains("const PROGRAM: &str = \"curl\""),
        "net.rs must name the program it runs in one place"
    );
}

/// `fetch_and_verify` returns a path and the crate stops there.
///
/// A structural check to go with the source scan: the type it hands back has a
/// `path` on it and no method that acts on it. If a future edit added
/// `Handoff::install(self)`, the scan above would catch the `Command::new` and
/// this catches the shape — a caller being handed something it can *run* rather
/// than something it can *show*.
#[test]
fn the_last_thing_this_crate_does_is_hand_over_a_path() {
    let flow =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/flow.rs")).unwrap();
    let shipped = shipped(&flow);
    assert!(shipped.contains("pub struct Handoff"));
    // No inherent methods on it at all: the struct is data, and `impl Handoff`
    // is where "and then run it" would be written.
    assert!(
        !shipped.contains("impl Handoff"),
        "Handoff is deliberately data with no behaviour; anything that acts on \
         the downloaded file belongs to the caller, and to a person"
    );
}

/// The verifier is handed the compiled-in key, unaltered.
///
/// Requirement 3 is not "a key is used", it is that *this* key is. No
/// behavioural test in this crate can tell the difference: a manifest signed by
/// the release key cannot be produced here, because the private half is
/// deliberately absent, so `RELEASE_PUBLIC_KEY` and any corruption of it both
/// refuse everything a test can construct, identically. That was found the way
/// the house rule intends — by flipping a bit of the key on its way into the
/// verifier and watching every test stay green.
///
/// What is left is to read the two lines. `RELEASE_PUBLIC_KEY` may appear in
/// the shipped part of `manifest.rs` exactly twice: the import, and the
/// argument. Anything else — a copy into a local, an XOR, a parameter with a
/// default, a second key consulted as a fallback — changes those lines.
///
/// The same argument covers `flow.rs`'s [`Verifier`] seam, which exists so the
/// steps after verification can be tested at all (its own doc explains why).
/// The seam is private, and the public entry point must pass
/// `VerifiedManifest::verify` — the one that uses the compiled-in key — which
/// is one line, and is read here.
#[test]
fn the_verifier_is_handed_the_compiled_in_key_unaltered() {
    let read = |name: &str| {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(name))
            .unwrap_or_else(|_| panic!("src/{name}"))
    };
    let lines = |text: &str, needle: &str| -> Vec<String> {
        text.lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .filter(|l| l.contains(needle))
            .map(|l| l.trim().to_string())
            .collect()
    };

    let manifest = read("manifest.rs");
    assert_eq!(
        lines(shipped(&manifest), "RELEASE_PUBLIC_KEY"),
        vec![
            "use crate::RELEASE_PUBLIC_KEY;".to_string(),
            "Self::verify_with(&RELEASE_PUBLIC_KEY, manifest, signature)".to_string(),
        ],
        "the compiled-in key must reach the verifier directly and untouched"
    );

    let flow = read("flow.rs");
    let shipped_flow = shipped(&flow);
    assert_eq!(
        lines(shipped_flow, "VerifiedManifest::verify"),
        vec!["fetch_and_verify_with(fetch, offered, into, VerifiedManifest::verify)".to_string()],
        "the public entry point must pass the release-key verifier, and it must \
         be the only verifier named in the shipped code"
    );
    // And it is the only caller of the seam, so there is no second path with a
    // different verifier. The declaration itself mentions the name and is not a
    // call, so it is excluded by the one thing that distinguishes them.
    let callers: Vec<String> = lines(shipped_flow, "fetch_and_verify_with(")
        .into_iter()
        .filter(|l| !l.starts_with("fn "))
        .collect();
    assert_eq!(
        callers,
        vec!["fetch_and_verify_with(fetch, offered, into, VerifiedManifest::verify)".to_string()],
        "the verifier seam must have exactly one caller in shipped code"
    );
    assert!(
        !shipped_flow.contains("pub fn fetch_and_verify_with"),
        "the verifier seam must stay private to this module"
    );
}

/// The truncation the whole scan rests on is sound.
///
/// If a `#[cfg(test)] mod tests` were not the last item in a file, everything
/// after it would be invisible to [`shipped`] and this suite would pass on
/// code it never read. That is the failure mode of a scanner that trims, and it
/// is silent, so it is asserted rather than assumed.
#[test]
fn the_test_marker_is_the_last_item_in_every_file_that_has_one() {
    for path in sources() {
        let text = std::fs::read_to_string(&path).expect("readable");
        let hits = text.matches(TEST_MARKER).count();
        assert!(
            hits <= 1,
            "{}: {hits} test modules; the scan assumes at most one",
            path.display()
        );
        if hits == 1 {
            let after = &text[text.find(TEST_MARKER).unwrap()..];
            assert!(
                after.trim_end().ends_with('}'),
                "{}: something follows the test module",
                path.display()
            );
            // Nothing but the test module itself may follow: the marker's
            // closing brace is the file's last non-empty line, at column 0.
            let last = after.lines().rev().find(|l| !l.trim().is_empty()).unwrap();
            assert_eq!(
                last,
                "}",
                "{}: the file does not end with the test module's \
                 closing brace",
                path.display()
            );
        }
    }
}

/// The signer never ships.
///
/// `src/testsign.rs` is excluded from the scan by name, so the thing that makes
/// that exclusion safe has to be checked somewhere. It is `#[cfg(test)]` at its
/// declaration in `lib.rs`, which means it is not compiled at all outside
/// `cargo test`.
#[test]
fn the_signer_is_test_only() {
    let lib =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs")).unwrap();
    assert!(
        lib.contains("#[cfg(test)]\nmod testsign;"),
        "src/testsign.rs must be declared behind #[cfg(test)], or an Ed25519 \
         signer ships in every Polylinker binary"
    );
    assert!(
        !lib.contains("pub mod testsign"),
        "the signer must not be part of the public API"
    );
}

/// The scanner finds what it is looking for.
///
/// Without this, every test above is a pattern that might match nothing. Each
/// probe is the edit it is written against, in the form somebody would actually
/// write it — including the braced `std` import, which is what an editor's
/// auto-import produces and which a substring scan reads straight past.
#[test]
fn the_scanner_finds_what_it_is_looking_for() {
    // Every banned spelling is detected, not merely the first.
    for (needle, _) in BANNED {
        let src = format!("let x = {needle};");
        assert_eq!(
            offences_in(&src, "flow.rs").len(),
            1,
            "{needle} is not actually checked"
        );
    }

    // The two edits requirement 4 exists to forbid.
    for probe in [
        "Command::new(&handoff.path).spawn().ok();",
        "std::fs::copy(&handoff.path, std::env::current_exe()?)?;",
    ] {
        assert!(
            !offences_in(probe, "flow.rs").is_empty(),
            "this must go red: {probe}"
        );
    }

    // The requirement 1 edits, in a file where they would live.
    for probe in [
        "std::thread::spawn(move || check(&Curl::default()));",
        "if last_check.elapsed() > Duration::from_secs(86400) { }",
        "let now = SystemTime::now();",
    ] {
        assert!(
            !offences_in(probe, "flow.rs").is_empty(),
            "this must go red: {probe}"
        );
    }

    // Braced std imports, one line and wrapped, where the two halves of the
    // path are never adjacent.
    for grouped in [
        "use std::{fmt, thread};",
        "use std::{process::Command, thread::sleep};",
        "    pub(crate) use std::{path::Path, time::Instant};",
    ] {
        assert!(
            !offences_in(grouped, "flow.rs").is_empty(),
            "a braced std import must go red: {grouped}"
        );
    }
    let wrapped = "use std::{\n    fmt,\n    thread,\n};\n";
    let found = offences_in(wrapped, "flow.rs");
    assert!(!found.is_empty(), "a wrapped std group must go red");
    assert_eq!(found[0].0, 1, "and it names the line that opened the use");

    // Allowed in its own file, banned elsewhere.
    assert!(offences_in("Command::new(PROGRAM)", "net.rs").is_empty());
    assert!(!offences_in("Command::new(PROGRAM)", "flow.rs").is_empty());
    assert!(offences_in("std::env::current_exe()", "flow.rs").is_empty());
    assert!(!offences_in("std::env::current_exe()", "manifest.rs").is_empty());

    // Comments are exempt, or the prose in these files could not explain
    // itself.
    assert!(offences_in("// no thread::spawn here, and here is why", "flow.rs").is_empty());
    assert!(offences_in("//! Duration and Instant are both absent", "flow.rs").is_empty());

    // And ordinary code that merely spells one of these words stays green,
    // which is why a `use` tree is matched by segment and not by substring.
    for innocent in [
        "fn f(a: u8, time: u64) -> u64 { time }",
        "let s = format!(\"{time}\");",
        "use crate::time::Stamp;",
        "use pl_core::sha256::sha256;",
        "use std::{fmt, path::Path};",
    ] {
        assert!(
            offences_in(innocent, "flow.rs").is_empty(),
            "ordinary code must not trip the gate: {innocent} -> {:?}",
            offences_in(innocent, "flow.rs")
        );
    }

    // `shipped` really truncates.
    let with_tests = format!("fn a() {{}}\n{TEST_MARKER}\n    fn b() {{}}\n}}\n");
    assert!(shipped(&with_tests).contains("fn a"));
    assert!(!shipped(&with_tests).contains("fn b"));
    assert_eq!(shipped("fn a() {}\n"), "fn a() {}\n");
}
