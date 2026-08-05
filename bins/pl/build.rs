//! Stamp the commit into the binary.
//!
//! `docs/RELEASING.md` says the update path is that a user checks when they want
//! to, and that `pl --version` tells them which build they have. A version
//! number alone does not: every build between two releases says `0.1.0`, and
//! "which build is this?" is the first question after any bug report.
//!
//! It also stamps the Windows VERSIONINFO block -- see `bins/winres.rs`. No
//! icon: `pl` is a console tool with no window to put one on, and a group icon
//! here would only make Explorer show a picture for something nobody
//! double-clicks. `tools/ci.ps1` asserts the absence, because an icon appearing
//! on `pl.exe` would mean the two build scripts had been crossed.
//!
//! Failure is not fatal. A source tarball has no `.git`, and a build that
//! refuses to proceed without one would make the project unbuildable by exactly
//! the people most likely to package it. The stamp becomes `unknown` and the
//! binary still builds.

#[path = "../winres.rs"]
mod winres;

use std::process::Command;

fn main() {
    // The version block, with no icon. A no-op off Windows-MSVC.
    winres::emit("pl", None);

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
