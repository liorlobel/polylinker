//! Stamp the commit into the GUI binary.
//!
//! The same script `bins/pl` has carried since the beginning, for a case that
//! matters more than the CLI's. `docs/RELEASING.md` says the update path is that
//! "the user checks when the user wants to", and that "`pl --version` prints the
//! version and the commit" — and for somebody who was handed `polylinker.exe`
//! and never opens a terminal, that sentence was false. The About page is what
//! makes the release document true.
//!
//! It also stamps the Windows resource: the application icon, and the
//! VERSIONINFO block that puts a version in Add/Remove Programs. See
//! `bins/winres.rs`, which writes the `.res` by hand.
//!
//! A `build.rs` is not a dependency. It adds nothing to `[dependencies]`, pulls
//! in no crate, and -- because the `.res` is hand-written rather than delegated
//! to `winres` or `embed-resource` -- needs no `rc.exe` on PATH either. So this
//! stays inside the rule that the GUI's four externals are the whole list.
//!
//! Failure is not fatal, for the reason the CLI's copy gives: a source tarball
//! has no `.git`, and a build that refused to proceed without one would make the
//! project unbuildable by exactly the people most likely to package it. The
//! icon is the one exception -- it is checked into this repository, so a
//! failure to read it is a broken checkout and says what to run.

#[path = "../winres.rs"]
mod winres;

use std::path::Path;
use std::process::Command;

fn main() {
    // The icon, and the version block. A no-op off Windows-MSVC.
    winres::emit("polylinker", Some(Path::new("icon/polylinker.ico")));

    // Rebuild when the checked-out commit changes, or the stamp goes stale the
    // moment anyone switches branch.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");

    let commit = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into());

    // Uncommitted changes mean the commit does not describe this binary, and
    // saying so is the difference between a traceable build and a number.
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    println!(
        "cargo:rustc-env=PL_COMMIT={commit}{}",
        if dirty { "-dirty" } else { "" }
    );
}
