//! Stamp the Windows VERSIONINFO block into `pl-mcp.exe`.
//!
//! The whole of it: no commit stamp, because `pl-mcp` has no `--version` and no
//! About page to print one on, and no icon, because it is a stdio server an
//! agent launches rather than something anybody double-clicks.
//!
//! It exists so that all three binaries this workspace ships answer
//! `(Get-Item ...).VersionInfo` the same way. `tools/ci.ps1` asserts the version
//! block on `polylinker.exe` and `pl.exe`; a third binary that quietly reported
//! nothing would be the one an installer or an inventory tool could not
//! identify.
//!
//! See `bins/winres.rs` for why the `.res` is hand-written rather than taken as
//! a `[build-dependencies]` entry.

#[path = "../winres.rs"]
mod winres;

fn main() {
    winres::emit("pl-mcp", None);
}
