//! The two steps: ask what is out there, and fetch it only if it proves itself.
//!
//! # The four requirements, and where each is met
//!
//! `docs/RELEASING.md` sets four requirements for any updater this project ever
//! grows. They are not met by this file being careful; they are met at named
//! places, and each place has a test.
//!
//! 1. **It downloads nothing without being asked, each time.** Nothing in this
//!    crate runs on a timer, on start-up, or in a background thread. There is
//!    no `spawn`, no `thread`, no `Instant`, and no state that would let it
//!    remember to try again later — [`check`] and [`fetch_and_verify`] are two
//!    functions that do exactly what they are called to do and then return.
//!    `tests/handoff.rs` scans this crate's sources for the machinery a
//!    background check would need and fails if any of it appears.
//!
//! 2. **A signature is verified before the bytes touch disk anywhere
//!    executable.** [`fetch_and_verify`] fetches the manifest and its signature
//!    into memory, verifies the signature against the compiled-in key, and only
//!    then asks for the artifact at all. If the signature fails, the artifact
//!    is never requested — not fetched and discarded, *never requested* — which
//!    `the_artifact_is_never_even_requested_when_the_signature_fails` asserts
//!    by looking at what the transport was asked for.
//!
//! 3. **The public key is compiled in.** [`crate::RELEASE_PUBLIC_KEY`], reached
//!    through the one constructor [`crate::VerifiedManifest::verify`], which is
//!    the only way a manifest can come into existence.
//!
//! 4. **It never replaces a running binary silently.** It never replaces a
//!    running binary at all. [`fetch_and_verify`] returns a [`Handoff`] — a
//!    path to a verified file on disk — and stops. Nothing here executes it,
//!    copies it over anything, or restarts the application; the file is for a
//!    person to run. It further refuses to write into the directory this binary
//!    is running from, so the verified download cannot even land beside the
//!    thing it would replace.
//!
//! # What `check` is, and is not
//!
//! [`check`] reads a version out of the *latest release's* manifest. That text
//! is **not verified** and cannot be: it is fetched before anyone knows which
//! release is being talked about, and it is what tells us which release that
//! is. So its answer is a claim by whoever is answering on that hostname, and
//! it is used for exactly two things — telling a human that something newer may
//! exist, and choosing the version number in the URLs that
//! [`fetch_and_verify`] then fetches *and verifies*.
//!
//! That is safe because of two properties, both of which are load-bearing and
//! tested. The claim can only ever become a [`Version`] — three integers — so
//! it cannot inject a path or a flag into the URLs built from it
//! (`version.rs`). And a claim that points at an *older* release is refused by
//! [`fetch_and_verify`], because an old release is genuinely signed and a
//! rollback would otherwise verify perfectly.
//!
//! The manifest is fetched again by [`fetch_and_verify`], from the URL of the
//! specific version rather than from the `latest` redirect. That is not
//! redundant: the second fetch is the one whose bytes are verified and whose
//! digests are used, and pinning it to a version number means the redirect
//! cannot move underneath the operation.

use crate::error::UpdateError;
use crate::manifest::VerifiedManifest;
use crate::net::Fetch;
use crate::version::{Version, CURRENT_VERSION};
use std::path::{Path, PathBuf};

/// Where releases live. Compiled in, like the key, and for the same reason.
pub const RELEASE_BASE_URL: &str = "https://github.com/liorlobel/polylinker/releases";

/// The manifest `.github/workflows/release.yml` publishes with every release.
pub const MANIFEST_FILE_NAME: &str = "SHA256SUMS.txt";

/// Its Ed25519 signature, 64 raw bytes, published beside it.
pub const SIGNATURE_FILE_NAME: &str = "SHA256SUMS.txt.sig";

/// The most manifest bytes [`check`] or [`fetch_and_verify`] will accept. A
/// release manifest is one line per published file — six once `windows-arm64`
/// publishes a zip and an MSI, four before it — and a few hundred bytes.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

/// The most signature bytes that will be accepted. Exactly 64 are expected; the
/// slack is so that a wrong-length file is refused by
/// [`UpdateError::SignatureWrongLength`], which says what is wrong, rather than
/// by a size limit, which does not.
pub const MAX_SIGNATURE_BYTES: usize = 1024;

/// The name every release artifact starts with, and the anchor [`check`] reads
/// the version from.
const ARTIFACT_PREFIX: &str = "polylinker-";

/// This platform's release artifact: the label the release workflow uses, and
/// the extension.
///
/// `None` for anything the workflow does not build, which is every platform but
/// these four — Linux on ARM, 32-bit Windows, the BSDs, `wasm32`. That is a
/// refusal rather than a guess: offering a user an x86-64 archive because it is
/// the closest thing available is how an update breaks an installation.
///
/// Windows gets the `.msi`, not the `.zip`. It is the file most Windows readers
/// take, it is what `tools/build-msi.ps1` produces and CI installs and
/// uninstalls as an oracle, and — the reason that matters here — the handoff at
/// the end of [`fetch_and_verify`] is a file for a person to run. On macOS and
/// Linux there is no installer to hand over, so it is the archive, and
/// [`Handoff`] says which of the two it is holding rather than calling both
/// "the installer".
///
/// # Why Windows on ARM64 is its own entry rather than a pointer at the x64 one
///
/// Sending `aarch64` to `windows-x64` would have "worked", in the sense that
/// nothing visibly breaks: Windows on ARM runs x86-64 binaries under emulation,
/// so the x64 MSI installs, and the emulated Polylinker starts and opens files.
/// That is precisely what makes it the wrong answer. The user gets an emulated
/// build, it works well enough that nothing ever prompts them to look, and —
/// because this table is consulted again at every release — they get the
/// emulated build again, and again, for as long as they keep updating. A native
/// build could ship for a year and they would never hear of it. Of all the
/// places to make that substitution, an updater is the worst one: it is the
/// place that makes it repeatedly, silently, and on the user's behalf.
///
/// # An entry here is a promise that a file exists; the old `None` was not
///
/// Before this arm existed, `aarch64-pc-windows-msvc` fell through to the
/// fallback below, [`artifact_file_name`] returned `None`, and `pl update`
/// declined with [`UpdateError::PlatformUnsupported`] before touching the
/// network. That was not a bug, and it is worth being exact about what
/// replacing it costs. `None` is a refusal that happens before anything is
/// fetched. An entry is a claim that a file with this exact name is attached to
/// every release from here on, and the failure mode of getting that claim wrong
/// is not a decline — it is a 404 in the middle of an update the user asked
/// for. So the arm and the published artifact land together or neither lands.
///
/// **This crate cannot check that half by itself, and must not be read as
/// though it does.** Nothing here reads `.github/workflows/release.yml`; the
/// tests below hold this table against `published_artifact_names`, which is a
/// second copy of the file list written beside them. Two copies agreeing is not
/// evidence about the release page. Reading the workflow and comparing it with
/// this table is `tools/ci.ps1`'s job — it already pins that workflow's build
/// matrix — and it is the only place the comparison can actually be made.
///
/// # What the compiler proves here, and what it does not
///
/// The arms and the fallback have to partition the space of targets, and rustc
/// enforces exactly that — for the target being compiled, and for no other. Two
/// arms matching is E0428, "the name `PLATFORM_ARTIFACT` is defined multiple
/// times"; none matching is E0425 at every use site below. Both are hard
/// errors, so any target that builds at all has exactly one entry, and that
/// part needs no test.
///
/// **Per-target is the trap.** A `#[cfg]`-disabled item is parsed but never
/// name-resolved or type-checked, so an arm for a target nobody builds is, to
/// an ordinary `cargo test` on an x86-64 machine, some lines of text that
/// happen to be syntactically valid. Add an arm and forget to exclude it from
/// the `not(any(...))` below and the workspace still builds everywhere except
/// the one platform the arm was written for — where it stops compiling, on CI,
/// after review, in a job whose failure looks like a toolchain problem.
/// `the_platform_cascade_and_its_fallback_stay_mutually_exclusive` reads this
/// file as text and compares the two lists, so that drift goes red on every
/// leg instead of only on the leg that builds the arm.
///
/// **Nothing on a developer machine establishes that the `windows-arm64` arm is
/// correct.** The ARM64 MSVC linker is not installed on the maintainer's
/// machine: `aarch64-pc-windows-msvc` compiles library crates there, and every
/// binary target fails with "linker `link.exe` not found", so no ARM64 build of
/// Polylinker can be produced or run locally at all. The thing that compiles
/// this arm, type-checks the tuple in it, and runs this crate's suite against
/// it is the `windows-11-arm` leg in `.github/workflows/ci.yml`. Until that leg
/// has run on a commit, the only claim anything has checked about the ARM64
/// entry is the textual one named above.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const PLATFORM_ARTIFACT: Option<(&str, &str)> = Some(("windows-x64", "msi"));
#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
const PLATFORM_ARTIFACT: Option<(&str, &str)> = Some(("windows-arm64", "msi"));
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const PLATFORM_ARTIFACT: Option<(&str, &str)> = Some(("linux-x64", "tar.gz"));
#[cfg(all(
    target_os = "macos",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
const PLATFORM_ARTIFACT: Option<(&str, &str)> = Some(("macos-universal", "tar.gz"));
#[cfg(not(any(
    all(target_os = "windows", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "aarch64"),
    all(target_os = "linux", target_arch = "x86_64"),
    all(
        target_os = "macos",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
)))]
const PLATFORM_ARTIFACT: Option<(&str, &str)> = None;

/// Is the artifact for this platform an installer, or an archive to unpack?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A Windows `.msi`. Running it installs.
    Installer,
    /// A `.tar.gz`. Unpacking it is the install, and `docs/RELEASING.md` has
    /// what macOS then needs told about `com.apple.quarantine`.
    Archive,
}

/// The file this platform would download for `version`, and what it is.
pub fn artifact_file_name(version: &Version) -> Option<(String, Kind)> {
    let (platform, extension) = PLATFORM_ARTIFACT?;
    let kind = if extension == "msi" {
        Kind::Installer
    } else {
        Kind::Archive
    };
    Some((
        format!("{ARTIFACT_PREFIX}{version}-{platform}.{extension}"),
        kind,
    ))
}

/// The latest release's manifest, through GitHub's `latest` redirect.
///
/// The one URL in this crate that does not name a version — it is the one
/// [`check`] asks in order to find out what the version is. `--location` is
/// what follows the redirect, and `--proto-redir =https` is what stops it going
/// anywhere but https on the way (`net.rs`).
pub fn latest_manifest_url() -> String {
    format!("{RELEASE_BASE_URL}/latest/download/{MANIFEST_FILE_NAME}")
}

/// The manifest for one specific release.
///
/// **Takes a [`Version`], never a `&str`.** That is the whole defence against
/// argument and path injection, and it is a property of the signature rather
/// than of the body: there is no string a caller could pass. See `version.rs`.
pub fn manifest_url(version: &Version) -> String {
    format!("{RELEASE_BASE_URL}/download/v{version}/{MANIFEST_FILE_NAME}")
}

/// The signature over [`manifest_url`]'s bytes.
pub fn signature_url(version: &Version) -> String {
    format!("{RELEASE_BASE_URL}/download/v{version}/{SIGNATURE_FILE_NAME}")
}

/// This platform's artifact for one release, or `None` if there is not one.
pub fn artifact_url(version: &Version) -> Option<String> {
    let (name, _) = artifact_file_name(version)?;
    Some(format!("{RELEASE_BASE_URL}/download/v{version}/{name}"))
}

/// What the release page claims, beside what this binary is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Check {
    /// The version compiled into this binary.
    pub current: Version,
    /// The version the release page claims is current. **Unverified.** See the
    /// module doc: this is a claim, and the only thing it is trusted to do is
    /// be three integers.
    pub offered: Version,
}

impl Check {
    /// Is the claimed version newer than this one? Numerically — `0.1.10` is
    /// newer than `0.1.2` and a string comparison would say otherwise.
    pub fn update_available(&self) -> bool {
        self.offered > self.current
    }
}

/// Ask what the current release is. Downloads nothing else.
///
/// One request, for a few hundred bytes of text. No artifact, no signature, no
/// side effect, nothing written anywhere. Called only when a person asks for
/// it — see requirement 1 in the module doc.
pub fn check(fetch: &dyn Fetch) -> Result<Check, UpdateError> {
    let current = Version::current().ok_or_else(|| UpdateError::UnreadableCompiledVersion {
        got: CURRENT_VERSION.to_string(),
    })?;
    let url = latest_manifest_url();
    let body = fetch.get(&url, MAX_MANIFEST_BYTES)?;
    let offered = version_named_by(&body)?;
    Ok(Check { current, offered })
}

/// Read the version out of the artifact names in a release manifest.
///
/// The manifest lists `polylinker-<version>-<platform>.<extension>`, so the
/// version is between the first `-` after the prefix and the next one. Every
/// occurrence must give the same answer: a body naming two versions is a body
/// this code does not understand, and the fail-closed reading of "I do not
/// understand this" is to make no update rather than to pick one.
///
/// Deliberately not a line-by-line parse of the checksum format. These bytes
/// are unverified, and the less structure that is assumed of them the smaller
/// the surface. All that is extracted is a run of characters that has to
/// survive [`Version::parse`], and everything else in the body is ignored.
fn version_named_by(body: &[u8]) -> Result<Version, UpdateError> {
    let unreadable = |detail: String| UpdateError::UnreadableVersion { detail };
    let text = core::str::from_utf8(body)
        .map_err(|_| unreadable("what it served is not text".to_string()))?;

    let mut found: Option<Version> = None;
    for start in text.match_indices(ARTIFACT_PREFIX).map(|(i, _)| i) {
        let rest = &text[start + ARTIFACT_PREFIX.len()..];
        let Some(end) = rest.find('-') else { continue };
        let Some(version) = Version::parse(&rest[..end]) else {
            return Err(unreadable(format!(
                "it names a file whose version field is not major.minor.patch: \
                 {ARTIFACT_PREFIX}{}-",
                brief(&rest[..end])
            )));
        };
        match found {
            None => found = Some(version),
            Some(first) if first != version => {
                return Err(unreadable(format!(
                    "it names two different versions, {first} and {version}"
                )))
            }
            Some(_) => {}
        }
    }
    found.ok_or_else(|| {
        unreadable(format!(
            "it names no {ARTIFACT_PREFIX}* file, so it is not a release manifest"
        ))
    })
}

/// The first 40 characters of an untrusted string, for an error message.
fn brief(text: &str) -> String {
    text.chars().take(40).collect()
}

/// A verified file on disk, and nothing else has happened.
///
/// # This is the handoff, and it is the end of what this crate does
///
/// Requirement 4 of `docs/RELEASING.md` is that a running binary is never
/// replaced silently. This crate's answer is stronger than "not silently": it
/// never replaces anything. There is no code here that copies over an
/// executable, renames one out of the way, schedules a replacement for the next
/// reboot, or runs the file it just downloaded — and `tests/handoff.rs` reads
/// the sources to make sure that stays true, because a future edit adding "and
/// then launch it" is exactly the convenience that would break the promise.
///
/// What the caller should do with this is show it to the person who asked, so
/// they can run it themselves and watch what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handoff {
    /// Where the verified file is. Not an executable location, and never the
    /// directory this binary is running from.
    pub path: PathBuf,
    /// Its name, as the signed manifest spells it.
    pub file_name: String,
    /// Whether running it installs, or whether it has to be unpacked first.
    pub kind: Kind,
    /// The release it belongs to.
    pub version: Version,
    /// Its SHA-256, as the verified manifest gave it — so a caller can show
    /// the user the same string the release page shows.
    pub sha256_hex: String,
}

/// Download one release, and keep it only if it proves itself.
///
/// The order of operations is the requirement, so it is spelled out:
///
/// 1. Refuse anything that is not newer than this binary (rollback).
/// 2. Refuse a platform with no release build.
/// 3. Refuse a destination inside this binary's own directory.
/// 4. Fetch `SHA256SUMS.txt` and `SHA256SUMS.txt.sig` **into memory**.
/// 5. Verify the signature against the compiled-in key. **On failure, stop.**
///    Nothing has been written and the artifact has not been requested.
/// 6. Look this platform's file up in the verified manifest. Not there: stop.
/// 7. Download the artifact to a `.part` file in the destination directory.
///    If the transfer fails part-way, delete the `.part` file and stop.
/// 8. Hash it and compare with the verified digest. On a mismatch, delete the
///    `.part` file and stop.
/// 9. Only now, rename it to its real name, and hand back the path.
///
/// `into` is a directory the caller chooses — a downloads folder, not an
/// install location. It is created if it does not exist.
pub fn fetch_and_verify(
    fetch: &dyn Fetch,
    offered: Version,
    into: &Path,
) -> Result<Handoff, UpdateError> {
    fetch_and_verify_with(fetch, offered, into, VerifiedManifest::verify)
}

/// How a manifest is turned into a verified one. One implementation ships.
///
/// A function pointer rather than a hard call, for a reason that is worth
/// stating because it is a seam in the middle of the security-critical path and
/// seams there are usually a mistake.
///
/// Everything after step 5 — the manifest lookup, the download, the digest
/// comparison, the deletion of a `.part` file that did not match, the rename —
/// is reachable only *through* a successful verification. The release key's
/// private half is deliberately absent from every developer machine, so no test
/// can produce a manifest that [`VerifiedManifest::verify`] accepts, so without
/// this seam none of those steps could be exercised end to end at all. They
/// would be tested in pieces, and the wiring between the pieces — which is
/// where "the `.part` file is deleted" lives — would be tested nowhere. That
/// was not a theory: removing the deletion left every test green.
///
/// What keeps the seam from being a hole: it is private to this module, the
/// only public entry point passes [`VerifiedManifest::verify`], and
/// `tests/handoff.rs`'s `the_verifier_is_handed_the_compiled_in_key_unaltered`
/// reads the source to hold both of those true.
type Verifier = fn(&[u8], &[u8]) -> Result<VerifiedManifest, UpdateError>;

fn fetch_and_verify_with(
    fetch: &dyn Fetch,
    offered: Version,
    into: &Path,
    verify: Verifier,
) -> Result<Handoff, UpdateError> {
    let current = Version::current().ok_or_else(|| UpdateError::UnreadableCompiledVersion {
        got: CURRENT_VERSION.to_string(),
    })?;
    if offered <= current {
        return Err(UpdateError::NotNewer { current, offered });
    }

    let (file_name, kind) = artifact_file_name(&offered).ok_or(UpdateError::PlatformUnsupported)?;

    std::fs::create_dir_all(into).map_err(|e| UpdateError::Io {
        what: format!("could not create {}", into.display()),
        detail: e.to_string(),
    })?;
    refuse_install_directory(into)?;

    // Into memory, both of them. Nothing is written to disk in this step, so
    // there is no state to undo if the verification below fails.
    let manifest_bytes = fetch.get(&manifest_url(&offered), MAX_MANIFEST_BYTES)?;
    let signature = fetch.get(&signature_url(&offered), MAX_SIGNATURE_BYTES)?;

    // Requirement 2. Everything after this line is reached only because these
    // bytes were signed by the holder of the release key.
    let manifest = verify(&manifest_bytes, &signature)?;

    let expected = manifest
        .digest_of(&file_name)
        .ok_or_else(|| UpdateError::NotInManifest {
            file: file_name.clone(),
        })?;

    let url = artifact_url(&offered).ok_or(UpdateError::PlatformUnsupported)?;
    // A `.part` name, in the destination directory rather than in the system
    // temporary directory, so that the rename at the end is within one
    // filesystem and cannot fail halfway with the bytes on the wrong volume.
    // The process id keeps two Polylinkers from writing the same partial file.
    let partial = into.join(format!("{file_name}.{}.part", std::process::id()));

    // ONE cleanup, and it covers the *transport* failure as well as the digest
    // failure. The two used to be separated by a `?`, and the transport half
    // was the one that leaks: `curl` keeps whatever bytes arrived when it exits
    // non-zero, so an ordinary interrupted download — a dropped link, a closed
    // lid, `ARTIFACT_MAX_TIME_SECS` firing on a hotel connection — left a
    // `.part` file in the directory the user chose, and nothing here or in
    // `bins/pl` ever removed it. One per attempt, because the name carries the
    // pid, so a flaky connection left a trail of them.
    //
    // Not `--remove-on-error`. `net.rs` runs whatever `curl` is on `PATH` and
    // has no way to know which one that is, and curl refuses an option it does
    // not recognise outright — `curl --disable --frobnicate --url ...` exits 2
    // with "option --frobnicate: is unknown", checked on the curl this was
    // written against — so a flag some system's curl lacks would trade a leaked
    // partial file for an update that cannot run at all there. Removing the
    // file here needs no flag, and it also covers a [`Fetch`] that is not curl.
    let outcome = fetch
        .download(&url, &partial)
        .and_then(|()| hash_and_place(&partial, into, &file_name, &expected));
    if outcome.is_err() {
        // A download that did not finish, or did not verify, is not left lying
        // about to be found later and run by hand.
        let _ = std::fs::remove_file(&partial);
    }
    let (path, actual_hex) = outcome?;

    Ok(Handoff {
        path,
        file_name,
        kind,
        version: offered,
        sha256_hex: actual_hex,
    })
}

/// Hash the downloaded file, compare with the verified digest, and only then
/// move it into place.
///
/// Split out so that the caller has one place to clean up from, and so the
/// early returns cannot accidentally skip the deletion.
fn hash_and_place(
    partial: &Path,
    into: &Path,
    file_name: &str,
    expected: &[u8; 32],
) -> Result<(PathBuf, String), UpdateError> {
    let size = std::fs::metadata(partial)
        .map_err(|e| UpdateError::Io {
            what: format!("could not stat {}", partial.display()),
            detail: e.to_string(),
        })?
        .len();
    if size > crate::net::ARTIFACT_MAX_BYTES {
        return Err(UpdateError::TooLarge {
            url: file_name.to_string(),
            limit: crate::net::ARTIFACT_MAX_BYTES as usize,
        });
    }

    // Read whole, because `pl_core::sha256` hashes a slice and has no
    // incremental form. That is a real cost — the artifact is tens of megabytes
    // and this holds all of it — and the alternative is either an incremental
    // API in `pl-core` (a second code path through a hash function that is
    // currently one auditable pass) or a second hash implementation here. The
    // size is bounded above, which is what keeps the cost from being unbounded.
    let bytes = std::fs::read(partial).map_err(|e| UpdateError::Io {
        what: format!("could not read {}", partial.display()),
        detail: e.to_string(),
    })?;
    let actual = pl_core::sha256::sha256(&bytes);
    if &actual != expected {
        return Err(UpdateError::DigestMismatch {
            file: file_name.to_string(),
            expected: pl_core::sha256::hex(expected),
            actual: pl_core::sha256::hex(&actual),
        });
    }

    let final_path = into.join(file_name);
    std::fs::rename(partial, &final_path).map_err(|e| UpdateError::Io {
        what: format!(
            "could not move the verified download to {}",
            final_path.display()
        ),
        detail: e.to_string(),
    })?;
    Ok((final_path, pl_core::sha256::hex(&actual)))
}

/// Refuse a destination that is this binary's own directory.
///
/// Requirement 4 says a running binary is never replaced silently. The cheapest
/// way to keep that promise is to never write where the running binary lives,
/// so a caller that passes the install directory — by mistake, or because a
/// "save to" dialog defaulted there — is refused rather than obeyed.
///
/// It fails closed when `current_exe` cannot answer. That is deliberate and it
/// is a real trade: on a platform where the executable's path is unavailable
/// this refuses to update at all. The alternative — skipping the check when it
/// cannot be made — is a guard that disappears exactly where it cannot be
/// observed.
///
/// Three of the four platforms this project releases for are known to answer
/// it, because `the_destination_may_not_be_the_directory_this_binary_runs_from`
/// calls it on each of them on every push. **The fourth is `windows-arm64`, and
/// the honest statement is that the same test is what will establish it**: this
/// crate is where an unanswerable `current_exe` would turn every ARM64 update
/// into [`UpdateError::UnknownInstallLocation`], and no x86-64 machine can ask
/// an ARM64 Windows anything. Until the `windows-11-arm` leg has run this
/// crate's suite, "it answers on ARM64" is an expectation about a Win32 API,
/// not a measurement.
fn refuse_install_directory(into: &Path) -> Result<(), UpdateError> {
    let unknown = |detail: String| UpdateError::UnknownInstallLocation { detail };
    let exe = std::env::current_exe().map_err(|e| unknown(e.to_string()))?;
    let dir = exe
        .parent()
        .ok_or_else(|| unknown(format!("{} has no parent directory", exe.display())))?;

    // Canonical form on both sides, so that `C:\App` and `C:\App\.` and a
    // symlinked path are one answer rather than three. Both exist by now --
    // `into` was just created -- so a failure here is a real filesystem error
    // and not a missing path.
    let a = std::fs::canonicalize(dir).map_err(|e| unknown(e.to_string()))?;
    let b = std::fs::canonicalize(into).map_err(|e| UpdateError::Io {
        what: format!("could not resolve {}", into.display()),
        detail: e.to_string(),
    })?;
    if a == b {
        return Err(UpdateError::DestinationIsInstallDirectory {
            path: into.to_path_buf(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsign;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    // ---------------------------------------------------------------- a server

    /// An in-memory release page.
    ///
    /// Records every URL it is asked for, which is what makes the negative
    /// requirements testable: "the artifact is never requested when the
    /// signature fails" is a statement about what was *not* asked for, and no
    /// amount of looking at the returned error can establish it.
    struct Server {
        files: BTreeMap<String, Vec<u8>>,
        asked: RefCell<Vec<String>>,
    }

    impl Server {
        fn new() -> Server {
            Server {
                files: BTreeMap::new(),
                asked: RefCell::new(Vec::new()),
            }
        }
        fn with(mut self, url: String, body: Vec<u8>) -> Server {
            self.files.insert(url, body);
            self
        }
        fn asked_for(&self, url: &str) -> bool {
            self.asked.borrow().iter().any(|u| u == url)
        }
    }

    impl Fetch for Server {
        fn get(&self, url: &str, limit: usize) -> Result<Vec<u8>, UpdateError> {
            self.asked.borrow_mut().push(url.to_string());
            let body = self.files.get(url).ok_or_else(|| UpdateError::Transport {
                url: url.to_string(),
                detail: "curl exited 22: HTTP 404".to_string(),
            })?;
            if body.len() > limit {
                return Err(UpdateError::TooLarge {
                    url: url.to_string(),
                    limit,
                });
            }
            Ok(body.clone())
        }

        fn download(&self, url: &str, to: &Path) -> Result<(), UpdateError> {
            self.asked.borrow_mut().push(url.to_string());
            let body = self.files.get(url).ok_or_else(|| UpdateError::Transport {
                url: url.to_string(),
                detail: "curl exited 22: HTTP 404".to_string(),
            })?;
            std::fs::write(to, body).map_err(|e| UpdateError::Io {
                what: format!("could not write {}", to.display()),
                detail: e.to_string(),
            })
        }
    }

    /// A transport that fails the way a real one fails: with bytes already on
    /// disk.
    ///
    /// [`Server`] cannot express this and that is why it had to be added.
    /// `Server::download` looks the URL up *before* it writes anything, so its
    /// only failure is a 404 that creates no file — which means every "leaves
    /// nothing behind" assertion in this module was, without anyone choosing
    /// it, an assertion about the digest path only. `curl` is the opposite: it
    /// keeps what arrived when it exits non-zero, so a timeout or a dropped
    /// link at byte 4096 of a 40 MB artifact leaves 4096 bytes named `.part`.
    /// That shape had no double at all, which is exactly why the missing
    /// cleanup was invisible.
    struct Interrupted {
        inner: Server,
        /// How much had arrived before it gave up.
        wrote: usize,
    }

    impl Fetch for Interrupted {
        fn get(&self, url: &str, limit: usize) -> Result<Vec<u8>, UpdateError> {
            self.inner.get(url, limit)
        }

        fn download(&self, url: &str, to: &Path) -> Result<(), UpdateError> {
            self.inner.asked.borrow_mut().push(url.to_string());
            std::fs::write(to, vec![b'x'; self.wrote]).expect("the partial write");
            Err(UpdateError::Transport {
                url: url.to_string(),
                detail: "curl exited 28: Operation timed out".to_string(),
            })
        }
    }

    // ------------------------------------------------------------ a directory

    /// A directory under the system temporary directory, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static N: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "pl-update-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).expect("a scratch directory");
            Scratch(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
        /// What is in it, sorted, as names.
        fn entries(&self) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(&self.0)
                .expect("readable")
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // ------------------------------------------------------------- a release

    const SEED: [u8; 32] = [0x5a; 32];

    /// The bytes of a made-up artifact for `version`.
    fn artifact_bytes(version: &Version) -> Vec<u8> {
        format!("this pretends to be Polylinker {version}\n").into_bytes()
    }

    /// Every file `.github/workflows/release.yml` attaches to a release, named
    /// the way the manifest names it, in the order that workflow's `sort -k2`
    /// leaves them in.
    ///
    /// This is the list [`PLATFORM_ARTIFACT`] points *into*, and the two
    /// drifting apart is a failure this file can see. A table entry with no file
    /// here is a `pl update` that stopped declining and started 404-ing; a file
    /// here that no table entry names is a platform that can download Polylinker
    /// and can never update itself.
    ///
    /// **It is a copy, so it is half of a check and not the whole of one.**
    /// Nothing in this crate reads the workflow. A name misspelled identically
    /// here and there agrees with itself perfectly and is still not on the
    /// release page. `tools/ci.ps1` is the only place that reads both, and
    /// holding this list against the workflow belongs to it.
    ///
    /// Six entries since `windows-arm64`, four before it: the two Windows
    /// platforms each publish a zip and an MSI, and macOS and Linux publish one
    /// archive each.
    fn published_artifact_names(version: &Version) -> Vec<String> {
        vec![
            format!("polylinker-{version}-linux-x64.tar.gz"),
            format!("polylinker-{version}-macos-universal.tar.gz"),
            format!("polylinker-{version}-windows-arm64.msi"),
            format!("polylinker-{version}-windows-arm64.zip"),
            format!("polylinker-{version}-windows-x64.msi"),
            format!("polylinker-{version}-windows-x64.zip"),
        ]
    }

    /// A manifest listing every release file, with a real digest for this
    /// platform's, signed by `SEED`'s key.
    fn release(version: &Version) -> (String, [u8; 64]) {
        let mut text = String::new();
        let mine = artifact_file_name(version).map(|(n, _)| n);
        let names = published_artifact_names(version);

        // The fixture and the table, tied together where the mismatch is
        // legible. Without this, a table entry naming a file this list does not
        // have surfaces three tests later as `NotInManifest`, which reads as a
        // problem with the manifest rather than with the entry that named a file
        // nobody publishes. On the ARM64 leg that is the difference between
        // "windows-arm64 is missing from the fixture" and half an hour spent in
        // the wrong module.
        if let Some(name) = mine.as_ref() {
            assert!(
                names.contains(name),
                "this target's PLATFORM_ARTIFACT entry names {name}, which is not \
                 one of the files the release workflow publishes: {names:?}"
            );
        }

        for name in names {
            let digest = if Some(&name) == mine.as_ref() {
                pl_core::sha256::sha256_hex(&artifact_bytes(version))
            } else {
                pl_core::sha256::sha256_hex(name.as_bytes())
            };
            text.push_str(&format!("{digest}  {name}\n"));
        }
        let sig = testsign::sign(&SEED, text.as_bytes());
        (text, sig)
    }

    /// A server that serves that release correctly, with `latest` pointing at
    /// it. The signature is by the **test** key, so anything reaching
    /// [`VerifiedManifest::verify`] will be refused; the tests that need it to
    /// pass call the private constructor. See
    /// `the_release_key_is_what_the_shipped_path_checks_against`.
    fn serving(version: &Version) -> Server {
        let (text, sig) = release(version);
        let mut s = Server::new()
            .with(latest_manifest_url(), text.clone().into_bytes())
            .with(manifest_url(version), text.into_bytes())
            .with(signature_url(version), sig.to_vec());
        if let Some(url) = artifact_url(version) {
            s = s.with(url, artifact_bytes(version));
        }
        s
    }

    fn newer() -> Version {
        let c = Version::current().unwrap();
        Version::new(c.major(), c.minor(), c.patch() + 1)
    }

    // -------------------------------------------------------- the platform table

    /// The file this target would ask for is a file the release workflow
    /// publishes, and the list it is checked against has not rotted into
    /// something nothing can fail.
    #[test]
    fn the_table_names_a_file_the_release_workflow_publishes() {
        let v = Version::parse("1.2.3").unwrap();
        let published = published_artifact_names(&v);

        let mut sorted = published.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted, published,
            "the published list must be sorted and free of duplicates, the way \
             release.yml's `sort -k2` leaves SHA256SUMS.txt"
        );
        for name in &published {
            assert_eq!(
                version_named_by(format!("aa  {name}\n").as_bytes()).unwrap(),
                v,
                "{name} does not read back as the version it was built for, so \
                 `check` could not read a version out of a manifest naming it"
            );
        }

        match artifact_file_name(&v) {
            Some((name, kind)) => {
                assert!(
                    published.contains(&name),
                    "this target asks for {name}, which the release workflow does \
                     not publish: {published:?}. A table entry with no file behind \
                     it turns `pl update` from a clean decline into a 404, which \
                     is strictly worse than declining."
                );
                assert_eq!(
                    kind == Kind::Installer,
                    name.ends_with(".msi"),
                    "{name} would be handed over as {kind:?}; the extension and the \
                     kind must agree or the user is told to run a file they have to \
                     unpack, or to unpack one they have to run"
                );
            }
            None => {
                // A target with no build at all. Refusing IS the correct
                // behaviour, and the two refusals have to be the same refusal.
                assert!(
                    artifact_url(&v).is_none(),
                    "artifact_file_name declines for this target and artifact_url \
                     does not, so a URL would be built for a file nothing named"
                );
            }
        }
    }

    /// One `#[cfg(...)] const PLATFORM_ARTIFACT ... = <value>;`, as source text,
    /// with every space and newline removed so that rustfmt's line breaking
    /// cannot make two identical predicates compare unequal.
    #[derive(Debug, PartialEq, Eq)]
    struct Arm {
        cfg: String,
        value: String,
    }

    /// The marker that ends the shipped part of this file. Spelled with an
    /// escape rather than a real newline so that this constant is not itself the
    /// thing it is looking for.
    const TESTS_MARKER: &str = "#[cfg(test)]\nmod tests {";

    /// Everything before the test module, which is where the cascade lives.
    fn before_the_tests(src: &str) -> &str {
        match src.find(TESTS_MARKER) {
            Some(i) => &src[..i],
            None => src,
        }
    }

    /// The length of the balanced run at the start of `text`, which begins just
    /// inside an opening `(`. `None` if that parenthesis never closes.
    fn balanced(text: &str) -> Option<usize> {
        let mut depth = 0usize;
        for (i, c) in text.char_indices() {
            match c {
                '(' => depth += 1,
                ')' if depth == 0 => return Some(i),
                ')' => depth -= 1,
                _ => {}
            }
        }
        None
    }

    /// `text` split at the commas that are not inside parentheses.
    fn top_level_commas(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut depth = 0usize;
        let mut start = 0usize;
        for (i, c) in text.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    out.push(text[start..i].to_string());
                    start = i + 1;
                }
                _ => {}
            }
        }
        if !text[start..].is_empty() {
            out.push(text[start..].to_string());
        }
        out
    }

    /// Every `PLATFORM_ARTIFACT` declaration in `src`, in source order.
    ///
    /// Comment lines are dropped whole before the whitespace is stripped:
    /// welding a `///` line's prose onto the code below it would let a doc
    /// comment invent an arm, or hide one. That this crate has no block comments
    /// is the same fact `tests/handoff.rs`'s scanner rests on.
    fn arms_in(src: &str) -> Vec<Arm> {
        const OPEN: &str = "#[cfg(";
        const DECL: &str = "constPLATFORM_ARTIFACT:Option<(&str,&str)>=";

        let dense: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .flat_map(str::chars)
            .filter(|c| !c.is_whitespace())
            .collect();

        let mut out = Vec::new();
        let mut at = 0usize;
        while let Some(i) = dense[at..].find(OPEN) {
            let start = at + i + OPEN.len();
            at = start;
            let Some(len) = balanced(&dense[start..]) else {
                continue;
            };
            let Some(rest) = dense[start + len..].strip_prefix(")]") else {
                continue;
            };
            let Some(rest) = rest.strip_prefix(DECL) else {
                continue;
            };
            let Some(end) = rest.find(';') else { continue };
            out.push(Arm {
                cfg: dense[start..start + len].to_string(),
                value: rest[..end].to_string(),
            });
        }
        out
    }

    /// Every arm of the cascade is excluded from the fallback, and every arm
    /// names a file the release workflow publishes.
    ///
    /// **THIS READS THE SOURCE, AND THAT IS THE ONLY WAY IT CAN SEE AN ARM FOR A
    /// TARGET THIS BUILD IS NOT FOR.** rustc guarantees that exactly one
    /// `PLATFORM_ARTIFACT` is defined — but only for the target being compiled.
    /// An arm added without being excluded from the `not(any(...))` fallback is
    /// two definitions, E0428, on that target *alone*; every other target builds,
    /// every test passes, and the failure surfaces on the one CI leg that
    /// compiles the arm, looking like a toolchain fault rather than a two-line
    /// edit. `windows-arm64` is the arm that costs the most to find that way: no
    /// developer machine here has the ARM64 MSVC linker, so nothing local can
    /// compile it at all.
    ///
    /// What this does NOT establish, and what no test in this crate can: that
    /// the release page actually carries the files named here.
    /// [`published_artifact_names`] is a copy of that list, not a reading of it.
    /// `tools/ci.ps1` is the only place `.github/workflows/release.yml` is read.
    #[test]
    fn the_platform_cascade_and_its_fallback_stay_mutually_exclusive() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/flow.rs");
        let src = std::fs::read_to_string(&path)
            .expect("this crate's own src/flow.rs")
            .replace("\r\n", "\n");
        let shipped = before_the_tests(&src);
        assert!(
            shipped.len() < src.len(),
            "the test-module marker was not found in {}, so this read the probe \
             strings below as though they were the real cascade",
            path.display()
        );

        let arms = arms_in(shipped);
        assert_eq!(
            arms.len(),
            5,
            "expected four platform entries and one fallback in {}, read {arms:#?}",
            path.display()
        );

        let (fallback, platforms) = arms.split_last().unwrap();
        assert_eq!(
            fallback.value, "None",
            "the last arm must be the refusal, so that a platform with no release \
             build declines rather than being handed the nearest thing"
        );

        let cfg = fallback.cfg.as_str();
        let inner = cfg
            .strip_prefix("not(any(")
            .and_then(|s| s.strip_suffix("))"))
            .unwrap_or_else(|| panic!("the fallback's cfg is not not(any(..)): {cfg}"));
        let mut listed = top_level_commas(inner);
        let mut declared: Vec<String> = platforms.iter().map(|a| a.cfg.clone()).collect();
        listed.sort();
        declared.sort();

        let mut distinct = declared.clone();
        distinct.dedup();
        assert_eq!(
            distinct, declared,
            "two platform entries carry the same cfg predicate, which is E0428 on \
             every target that matches it"
        );
        assert_eq!(
            listed, declared,
            "the fallback's not(any(..)) list and the entries above it have drifted \
             apart. A predicate in one and not the other is a target that defines \
             PLATFORM_ARTIFACT twice (E0428) or not at all (E0425), and that is a \
             compile error ONLY on that target -- red on the CI leg that builds it \
             and invisible everywhere else, including in this process."
        );

        let v = Version::parse("1.2.3").unwrap();
        let published = published_artifact_names(&v);
        let mut named = Vec::new();
        for arm in platforms {
            let value = arm.value.as_str();
            let pair = value
                .strip_prefix("Some((")
                .and_then(|s| s.strip_suffix("))"))
                .unwrap_or_else(|| panic!("an entry is not Some((label, extension)): {value}"));
            let (label, extension) = pair
                .split_once(',')
                .unwrap_or_else(|| panic!("{pair} is not a label and an extension"));
            let name = format!(
                "polylinker-{v}-{}.{}",
                label.trim_matches('"'),
                extension.trim_matches('"')
            );
            assert!(
                published.contains(&name),
                "the entry for {} names {name}, which is not one of the files the \
                 release workflow publishes: {published:?}",
                arm.cfg
            );
            named.push(name);
        }

        let mut distinct = named.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            named.len(),
            "two platforms are pointed at one artifact, which is exactly how a \
             user on one of them ends up updating forever into a build for the \
             other: {named:?}"
        );
        assert!(
            named.iter().any(|n| n.ends_with("-windows-arm64.msi")),
            "the windows-arm64 entry is the one no machine here can compile, so \
             its removal would go unnoticed until the ARM64 CI leg ran -- if it \
             is genuinely being withdrawn, withdraw the published artifact and \
             this assertion together, and put ARM64 back on the fallback"
        );
    }

    /// The name [`arms_in`] matches on, kept out of the fixtures below.
    const DECL_NAME: &str = "PLATFORM_ARTIFACT";

    /// A fixture with its `ARTIFACT` placeholder turned into the real name.
    ///
    /// **THE FIXTURES BELOW ARE NOT A SECOND COPY OF THE CASCADE AND MUST NOT
    /// READ AS ONE.** Two things scan this file as text looking for exactly that
    /// declaration, and they do not agree about where the file ends. [`arms_in`]
    /// stops at the test module, so it cannot see these. `tools/ci.ps1`'s
    /// `Get-UpdaterPlatformArtifacts` — the step that holds this table against
    /// `.github/workflows/release.yml`, and the only place those two are ever
    /// compared — reads every line of the file.
    ///
    /// So a fixture spelling the name out verbatim does not merely sit there: it
    /// arrives at the gate as extra platform arms and extra fallbacks, and the
    /// gate then reports a cascade this file does not contain, or throws on a
    /// declaration that has no `#[cfg]` above it because the line above it is a
    /// `r#"`. Measured, not guessed: with the fixtures written out longhand that
    /// step found 5 arms and 3 fallbacks in a file that has 4 and 1, and threw
    /// four times.
    ///
    /// Substituting the name in at run time keeps each fixture exact for the
    /// reader under test and invisible to the reader that is not. It is a
    /// narrower fix than it should be — the durable one is for a scanner of a
    /// Rust file to truncate at `#[cfg(test)]`, the way [`before_the_tests`] and
    /// `tests/handoff.rs` both do — and that belongs in `tools/ci.ps1`.
    fn spelled_out(fixture: &str) -> String {
        fixture.replace("ARTIFACT", DECL_NAME)
    }

    /// The reader above finds what it is looking for, and objects to the edit it
    /// exists to object to.
    ///
    /// Without this,
    /// [`the_platform_cascade_and_its_fallback_stay_mutually_exclusive`] is a
    /// pattern that might match nothing useful. An [`arms_in`] that returned no
    /// arms would fail its own length assertion, but one that mis-nested the
    /// parentheses would return five garbled strings and compare them against
    /// each other perfectly happily.
    #[test]
    fn the_cascade_reader_finds_what_it_is_looking_for() {
        const ONE_PLATFORM: &str = r#"#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const ARTIFACT: Option<(&str, &str)> = Some(("windows-x64", "msi"));
#[cfg(not(any(
    all(target_os = "windows", target_arch = "x86_64")
)))]
const ARTIFACT: Option<(&str, &str)> = None;
"#;
        // The wrapped shape rustfmt leaves the macOS arm in.
        const WRAPPED: &str = r#"#[cfg(all(
    target_os = "macos",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
const ARTIFACT: Option<(&str, &str)> = Some(("macos-universal", "tar.gz"));
"#;
        // THE EDIT THIS ALL EXISTS FOR: an arm added, and not excluded from the
        // fallback. It builds on x86-64 and defines the constant twice on ARM64.
        const DRIFTED: &str = r#"#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const ARTIFACT: Option<(&str, &str)> = Some(("windows-x64", "msi"));
#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
const ARTIFACT: Option<(&str, &str)> = Some(("windows-arm64", "msi"));
#[cfg(not(any(
    all(target_os = "windows", target_arch = "x86_64")
)))]
const ARTIFACT: Option<(&str, &str)> = None;
"#;

        let one = spelled_out(ONE_PLATFORM);
        let arms = arms_in(&one);
        assert_eq!(arms.len(), 2, "{arms:#?}");
        assert_eq!(
            arms[0].cfg,
            r#"all(target_os="windows",target_arch="x86_64")"#
        );
        assert_eq!(arms[0].value, r#"Some(("windows-x64","msi"))"#);
        assert_eq!(
            arms[1].cfg,
            r#"not(any(all(target_os="windows",target_arch="x86_64")))"#
        );
        assert_eq!(arms[1].value, "None");

        let wrapped = spelled_out(WRAPPED);
        assert_eq!(
            arms_in(&wrapped)[0].cfg,
            r#"all(target_os="macos",any(target_arch="x86_64",target_arch="aarch64"))"#,
            "rustfmt breaks this arm across four lines and the reader must not care"
        );

        // A doc comment that spells out an entry is prose, not an entry -- and
        // this file's own doc comments name the table over and over. The probe
        // is a well-formed arm, so it WOULD be read as a third one if the
        // comment filter stopped working.
        let commented = spelled_out(&format!(
            "/// #[cfg(all())] const ARTIFACT: Option<(&str, &str)> = None;\n{ONE_PLATFORM}"
        ));
        assert_eq!(
            arms_in(&commented),
            arms_in(&one),
            "a comment naming a table entry must not be read as one"
        );

        let drifted = spelled_out(DRIFTED);
        let arms = arms_in(&drifted);
        assert_eq!(arms.len(), 3, "{arms:#?}");
        let (fallback, platforms) = arms.split_last().unwrap();
        let inner = fallback.cfg.strip_prefix("not(any(").unwrap();
        let inner = inner.strip_suffix("))").unwrap();
        let listed = top_level_commas(inner);
        let declared: Vec<String> = platforms.iter().map(|a| a.cfg.clone()).collect();
        assert_ne!(
            listed, declared,
            "the reader must see that the ARM64 arm is missing from the fallback"
        );

        // And the two splitters, since everything above rests on them.
        assert_eq!(
            top_level_commas("all(a,b),all(c,any(d,e))"),
            vec!["all(a,b)".to_string(), "all(c,any(d,e))".to_string()],
            "a comma inside a nested predicate must not split it"
        );
        assert_eq!(
            top_level_commas("a,b,"),
            vec!["a".to_string(), "b".to_string()],
            "a trailing comma must not produce an empty predicate"
        );
        assert_eq!(balanced("a,b)tail"), Some(3));
        assert_eq!(balanced("all(x))"), Some(6));
        assert_eq!(balanced("never closes"), None);

        // And the truncation everything above rests on really truncates.
        let with_tests = format!("fn a() {{}}\n{TESTS_MARKER}\n    fn b() {{}}\n}}\n");
        assert!(before_the_tests(&with_tests).contains("fn a"));
        assert!(!before_the_tests(&with_tests).contains("fn b"));
    }

    // ------------------------------------------------------------------ URLs

    /// The URLs are exactly the ones `.github/workflows/release.yml` publishes
    /// to, written out here rather than assembled a second time.
    #[test]
    fn the_urls_are_the_release_pages_urls() {
        let v = Version::parse("0.2.0").unwrap();
        assert_eq!(
            manifest_url(&v),
            "https://github.com/liorlobel/polylinker/releases/download/v0.2.0/SHA256SUMS.txt"
        );
        assert_eq!(
            signature_url(&v),
            "https://github.com/liorlobel/polylinker/releases/download/v0.2.0/SHA256SUMS.txt.sig"
        );
        assert_eq!(
            latest_manifest_url(),
            "https://github.com/liorlobel/polylinker/releases/latest/download/SHA256SUMS.txt"
        );
        // The `v` prefix belongs to the tag and is added here, once. `Version`
        // itself refuses to carry it, so it cannot end up doubled.
        assert!(manifest_url(&v).contains("/v0.2.0/"));
        assert!(!manifest_url(&v).contains("/vv"));

        match artifact_url(&v) {
            Some(url) => {
                let (name, _) = artifact_file_name(&v).unwrap();
                assert_eq!(
                    url,
                    format!(
                        "https://github.com/liorlobel/polylinker/releases/download/v0.2.0/{name}"
                    )
                );
                assert!(name.starts_with("polylinker-0.2.0-"));
            }
            None => assert!(
                artifact_file_name(&v).is_none(),
                "the two must agree about whether this platform has a build"
            ),
        }
    }

    /// A hostile version string cannot reach a URL, because nothing that builds
    /// a URL will take one.
    ///
    /// The list is the one the task set: a path traversal, a curl flag, an
    /// absolute URL, a newline. They are refused at [`Version::parse`], which
    /// is the only door — `manifest_url`, `signature_url` and `artifact_url`
    /// all take a `&Version` and there is no `&str` overload to find.
    ///
    /// The second half is what makes this more than a restatement of
    /// `version.rs`'s tests: for every string that *does* parse, the URL built
    /// from it is checked to be a single https URL with no whitespace, no
    /// second argument hiding in it, and no parent-directory hop.
    #[test]
    fn hostile_version_strings_are_refused() {
        for hostile in [
            "../../x",
            "../../../etc/passwd",
            "-o/tmp/x",
            "-O",
            "--insecure",
            "https://evil.example/",
            "0.1.0\n",
            "0.1.0\r\n0.2.0",
            "0.1.0 --insecure",
            "0.1.0;curl evil.example",
            "0.1.0|sh",
            "0.1.0`id`",
            "0.1.0$(id)",
            "0.1.0/../../..",
            "%2e%2e%2f",
            "0.1.0#fragment",
            "0.1.0?x=y",
            "\0",
            "0.1.0\0",
        ] {
            assert!(
                Version::parse(hostile).is_none(),
                "{hostile:?} must never become a Version, because a Version is \
                 the only thing a URL is built from"
            );
        }

        for good in [
            "0.0.0",
            "1.2.3",
            "0.1.10",
            "4294967295.4294967295.4294967295",
        ] {
            let v = Version::parse(good).unwrap();
            for url in [
                manifest_url(&v),
                signature_url(&v),
                artifact_url(&v).unwrap_or_else(latest_manifest_url),
                latest_manifest_url(),
            ] {
                assert!(url.starts_with("https://github.com/liorlobel/polylinker/releases/"));
                assert!(!url.contains(".."), "{url}");
                assert!(!url.chars().any(char::is_whitespace), "{url}");
                assert!(!url.chars().any(|c| c.is_control()), "{url}");
                assert!(!url.contains('?') && !url.contains('#'), "{url}");
            }
        }
    }

    // ----------------------------------------------------------------- check

    #[test]
    fn check_reads_the_version_and_asks_for_nothing_else() {
        let v = Version::parse("9.9.9").unwrap();
        let server = serving(&v);
        let got = check(&server).expect("a well-formed manifest");
        assert_eq!(got.offered, v);
        assert_eq!(got.current, Version::current().unwrap());
        assert!(got.update_available());

        // One request, and it is the small text one. Requirement 1: no
        // artifact is fetched by a check.
        assert_eq!(server.asked.borrow().len(), 1);
        assert!(server.asked_for(&latest_manifest_url()));
        if let Some(url) = artifact_url(&v) {
            assert!(
                !server.asked_for(&url),
                "check() must not download an artifact"
            );
        }
        assert!(!server.asked_for(&signature_url(&v)));
    }

    /// The largest version strictly below `v`: decrement the lowest component
    /// that is not zero, and saturate the ones below it.
    ///
    /// The largest rather than any, because the tightest boundary is the one
    /// worth testing.
    ///
    /// **AT AN `x.y.0` RELEASE IT ALSO CATCHES A TEXTUAL COMPARISON, AND AT
    /// EVERY OTHER RELEASE IT DOES NOT.** At 0.10.0 the minor arm ran and
    /// returned 0.9.4294967295, whose STRING sorts above `"0.10.0"`, so a
    /// comparison that had become lexical failed here too. At 0.10.1 the patch
    /// arm runs and returns 0.10.0, and `"0.10.0" < "0.10.1"` textually as well
    /// as numerically, so that second value lapses — silently, and without this
    /// helper changing at all.
    ///
    /// That is recorded rather than fixed because the property is not this
    /// helper's to keep: `version::numeric_ordering_is_not_lexical` pins
    /// `0.10.0 > 0.9.9` and `0.1.10 > 0.1.2` with hard-coded literals that no
    /// version bump can degenerate, and THAT is the check the lexical trap
    /// rests on. Do not read a green run here as evidence about ordering; the
    /// job of this helper is to produce a strictly older version, and the
    /// `assert!(older < current)` at the call site is what holds it to that.
    ///
    /// Panics at 0.0.0, where nothing older exists. A released crate cannot be
    /// at 0.0.0, and a panic naming that is better than a fixture that quietly
    /// equals `current` — which is exactly the failure this helper replaced.
    fn older_than(v: Version) -> Version {
        if v.patch() > 0 {
            Version::new(v.major(), v.minor(), v.patch() - 1)
        } else if v.minor() > 0 {
            Version::new(v.major(), v.minor() - 1, u32::MAX)
        } else if v.major() > 0 {
            Version::new(v.major() - 1, u32::MAX, u32::MAX)
        } else {
            panic!("no version is older than 0.0.0")
        }
    }

    /// An older release is reported as older rather than as an update, and so
    /// is the identical one.
    ///
    /// **THE OLDER VERSION IS DERIVED AND THEN ASSERTED TO BE OLDER**, and the
    /// assertion is the point. This built `older` as
    /// `Version::new(current.major(), current.minor(), 0)` behind an
    /// `if older == current` guard whose two arms were the same expression, so
    /// at every `x.y.0` release `older` WAS `current` and the test named for
    /// the older case exercised only the equal one. v0.10.0 is such a release:
    /// the test went dead by a version bump, silently, with the whole suite
    /// green — a check that cannot fail for the thing it names, arrived at
    /// without anyone touching it.
    ///
    /// `assert!(older < current)` is what stops that recurring. Any future
    /// derivation that fails to produce an older version now fails loudly here
    /// instead of quietly weakening the case below it.
    ///
    /// Both versions are exercised because `check`'s boundary is `>`, not `>=`:
    /// the equal case was all this test had left, and dropping it to fix the
    /// older case would trade one hole for another.
    #[test]
    fn check_does_not_call_an_older_release_an_update() {
        let current = Version::current().unwrap();
        let older = older_than(current);
        assert!(
            older < current,
            "the fixture must be strictly older than {current}, and {older} is not"
        );

        for offered in [older, current] {
            let server = serving(&offered);
            let got = check(&server).unwrap();
            assert_eq!(got.offered, offered);
            assert_eq!(got.current, current);
            assert!(
                !got.update_available(),
                "{offered} is not newer than {current} and must not be offered \
                 as an update"
            );
        }
    }

    /// Anything that is not a release manifest is refused, and refused in a way
    /// that leads nowhere.
    #[test]
    fn check_refuses_a_body_that_does_not_name_one_version() {
        for (what, body) in [
            (
                "an HTML error page",
                "<html><body>404</body></html>".to_string(),
            ),
            ("an empty body", String::new()),
            ("a manifest of other files", "abc  README.md\n".to_string()),
            (
                "two versions at once",
                "aa  polylinker-1.2.3-linux-x64.tar.gz\nbb  polylinker-1.2.4-windows-x64.msi\n"
                    .to_string(),
            ),
            (
                "a version field that is not a version",
                "aa  polylinker-latest-linux-x64.tar.gz\n".to_string(),
            ),
            (
                "a version field with a flag in it",
                "aa  polylinker- --insecure-linux-x64.tar.gz\n".to_string(),
            ),
            (
                "a version field that is a path",
                "aa  polylinker-../../etc-linux-x64.tar.gz\n".to_string(),
            ),
            ("no dash after the prefix", "aa  polylinker-\n".to_string()),
        ] {
            let server = Server::new().with(latest_manifest_url(), body.into_bytes());
            let got = check(&server);
            assert!(
                matches!(got, Err(UpdateError::UnreadableVersion { .. })),
                "{what} must not yield a version, got {got:?}"
            );
        }

        // Not text at all.
        let server = Server::new().with(latest_manifest_url(), vec![0xff; 32]);
        assert!(matches!(
            check(&server),
            Err(UpdateError::UnreadableVersion { .. })
        ));
    }

    /// A transport failure is a transport failure, and says so.
    #[test]
    fn check_reports_a_missing_release_page_as_transport() {
        let server = Server::new();
        assert!(matches!(check(&server), Err(UpdateError::Transport { .. })));
    }

    // ------------------------------------------------------- fetch_and_verify

    /// The verifier the tests substitute: the same code, the test key.
    ///
    /// See [`Verifier`] for why this seam exists. It is a free function rather
    /// than a closure because [`Verifier`] is a plain `fn` pointer, which is
    /// what keeps the shipped call site a single unambiguous line for
    /// `tests/handoff.rs` to read.
    fn verify_with_the_test_key(
        manifest: &[u8],
        signature: &[u8],
    ) -> Result<VerifiedManifest, UpdateError> {
        VerifiedManifest::verify_for_test(&testsign::public_key(&SEED), manifest, signature)
    }

    /// The whole flow, end to end, when everything is as it should be.
    ///
    /// The positive case, and it is the one that stops all the refusals below
    /// from being satisfied by an updater that simply never works. It runs the
    /// shipped [`fetch_and_verify_with`] — the same function the public entry
    /// point calls, with the same steps in the same order — differing only in
    /// which key the signature is checked against, because the release key's
    /// private half is deliberately not on this machine.
    #[test]
    fn a_verified_download_is_placed_and_handed_over() {
        let v = newer();
        let scratch = Scratch::new();
        let server = serving(&v);

        let Some((name, kind)) = artifact_file_name(&v) else {
            // A platform with no release build. Refusing is the whole of the
            // correct behaviour here.
            assert_eq!(
                fetch_and_verify_with(&server, v, scratch.path(), verify_with_the_test_key),
                Err(UpdateError::PlatformUnsupported)
            );
            return;
        };

        let handoff = fetch_and_verify_with(&server, v, scratch.path(), verify_with_the_test_key)
            .expect("a correctly signed release with a matching digest");

        assert_eq!(handoff.path, scratch.path().join(&name));
        assert_eq!(handoff.file_name, name);
        assert_eq!(handoff.version, v);
        assert_eq!(handoff.kind, kind);
        assert_eq!(
            handoff.sha256_hex,
            pl_core::sha256::sha256_hex(&artifact_bytes(&v))
        );
        assert_eq!(std::fs::read(&handoff.path).unwrap(), artifact_bytes(&v));

        // Exactly one file in the destination: the artifact, under its real
        // name. No `.part` left behind, and nothing else invented.
        assert_eq!(scratch.entries(), vec![name.clone()]);

        // Three requests, in order: manifest, signature, artifact. The artifact
        // last, which is requirement 2 expressed as a sequence.
        let asked = server.asked.borrow().clone();
        assert_eq!(
            asked,
            vec![
                manifest_url(&v),
                signature_url(&v),
                artifact_url(&v).unwrap()
            ]
        );

        // And the running binary is untouched: this crate has done nothing but
        // put a file somewhere and say where it is.
        assert!(std::env::current_exe().unwrap().exists());
    }

    /// The same flow, when the signature is the release key's business.
    ///
    /// The public entry point refuses, because `serving` signs with the test
    /// key. That is [`the_release_key_is_what_the_shipped_path_checks_against`]
    /// from the other end, and it is repeated here to assert the *consequences*
    /// of the refusal: nothing written, and the artifact never requested.
    #[test]
    fn the_public_entry_point_refuses_what_the_test_key_signed() {
        let v = newer();
        let scratch = Scratch::new();
        let server = serving(&v);
        assert_eq!(
            fetch_and_verify(&server, v, scratch.path()),
            Err(UpdateError::SignatureInvalid)
        );
        assert!(
            scratch.entries().is_empty(),
            "a failed signature must leave the destination empty, found {:?}",
            scratch.entries()
        );
        if let Some(artifact) = artifact_url(&v) {
            assert!(
                !server.asked_for(&artifact),
                "requirement 2: the artifact must not be requested until the \
                 manifest's signature has verified"
            );
        }
    }

    /// A digest that does not match, through the whole flow.
    ///
    /// Distinct from [`a_download_whose_hash_is_wrong_is_deleted_and_never_placed`],
    /// which exercises [`hash_and_place`] directly: this one goes through
    /// [`fetch_and_verify_with`], so it also covers the wiring — the `.part`
    /// file being removed by the caller of `hash_and_place` rather than by
    /// `hash_and_place` itself. Removing that one line left every other test in
    /// this crate green.
    #[test]
    fn a_download_that_does_not_match_the_signed_manifest_leaves_nothing_behind() {
        let v = newer();
        let Some((name, _)) = artifact_file_name(&v) else {
            return;
        };
        let (text, sig) = release(&v);
        let server = Server::new()
            .with(manifest_url(&v), text.into_bytes())
            .with(signature_url(&v), sig.to_vec())
            // The right URL, the wrong bytes: a server that substituted the
            // artifact but could not re-sign the manifest.
            .with(artifact_url(&v).unwrap(), b"substituted".to_vec());

        let scratch = Scratch::new();
        let got = fetch_and_verify_with(&server, v, scratch.path(), verify_with_the_test_key);
        match got {
            Err(UpdateError::DigestMismatch { file, .. }) => assert_eq!(file, name),
            other => panic!("expected a digest mismatch, got {other:?}"),
        }
        assert!(
            scratch.entries().is_empty(),
            "neither the artifact nor a .part file may survive a digest \
             mismatch, found {:?}",
            scratch.entries()
        );
    }

    /// A transfer that dies part-way leaves nothing behind, and does not claim
    /// more than that.
    ///
    /// Two defects in one place, and they are the same defect seen from each
    /// side. The cleanup sat *below* the `?` on `fetch.download`, so it ran for
    /// a digest mismatch and never for a transport failure: every ordinary
    /// interrupted download — closed lid, dropped link, the 900-second
    /// `ARTIFACT_MAX_TIME_SECS` on a slow line — left its `.part` file in the
    /// user's chosen directory, a fresh one per attempt because the name
    /// carries the pid. And the message said "Nothing was written" while those
    /// bytes were sitting there.
    ///
    /// The message is asserted as well as the directory because the directory
    /// alone would let the sentence stay: a future edit that moves the cleanup
    /// back below the `?` breaks the first assertion, and one that restores the
    /// old wording breaks the second, which is the point of having both.
    #[test]
    fn an_interrupted_download_leaves_no_partial_file_and_says_nothing_it_cannot_keep() {
        let v = newer();
        if artifact_file_name(&v).is_none() {
            return;
        }
        let scratch = Scratch::new();
        let fetch = Interrupted {
            inner: serving(&v),
            wrote: 4096,
        };

        let got = fetch_and_verify_with(&fetch, v, scratch.path(), verify_with_the_test_key);
        let err = match got {
            Err(e @ UpdateError::Transport { .. }) => e,
            other => panic!("expected a transport failure, got {other:?}"),
        };

        assert!(
            scratch.entries().is_empty(),
            "an interrupted transfer must not leave its .part file behind, found {:?}",
            scratch.entries()
        );

        let text = err.to_string();
        assert!(
            !text.to_lowercase().contains("nothing was written"),
            "4096 bytes were written before this failed, so the message must not \
             say otherwise: {text}"
        );
        assert!(
            text.contains("No update was made"),
            "the message must still say the operation did not happen: {text}"
        );
    }

    /// A signed release that does not list this platform's file.
    #[test]
    fn a_release_without_this_platforms_artifact_is_refused_before_downloading() {
        let v = newer();
        let Some((name, _)) = artifact_file_name(&v) else {
            return;
        };
        let (full, _) = release(&v);
        let trimmed: String = full
            .lines()
            .filter(|l| !l.ends_with(&name))
            .map(|l| format!("{l}\n"))
            .collect();
        assert!(!trimmed.contains(&name) && !trimmed.is_empty());
        let sig = testsign::sign(&SEED, trimmed.as_bytes());

        let server = Server::new()
            .with(manifest_url(&v), trimmed.into_bytes())
            .with(signature_url(&v), sig.to_vec())
            .with(artifact_url(&v).unwrap(), artifact_bytes(&v));

        let scratch = Scratch::new();
        let got = fetch_and_verify_with(&server, v, scratch.path(), verify_with_the_test_key);
        assert_eq!(got, Err(UpdateError::NotInManifest { file: name }));
        assert!(scratch.entries().is_empty());
        assert!(
            !server.asked_for(&artifact_url(&v).unwrap()),
            "a file the signed manifest does not list must not be downloaded"
        );
    }

    /// A wrong digest deletes the download and reports what it was.
    #[test]
    fn a_download_whose_hash_is_wrong_is_deleted_and_never_placed() {
        let v = newer();
        let Some((name, _)) = artifact_file_name(&v) else {
            return;
        };
        let scratch = Scratch::new();
        let partial = scratch.path().join(format!("{name}.part"));
        std::fs::write(&partial, b"not what the manifest says").unwrap();

        let expected = pl_core::sha256::sha256(&artifact_bytes(&v));
        let got = hash_and_place(&partial, scratch.path(), &name, &expected);
        match got {
            Err(UpdateError::DigestMismatch {
                file,
                expected: e,
                actual,
            }) => {
                assert_eq!(file, name);
                assert_eq!(e, pl_core::sha256::hex(&expected));
                assert_eq!(
                    actual,
                    pl_core::sha256::sha256_hex(b"not what the manifest says")
                );
                assert_ne!(e, actual);
            }
            other => panic!("expected a digest mismatch, got {other:?}"),
        }
        assert!(
            !scratch.path().join(&name).exists(),
            "a file that did not match must never be placed under its real name"
        );
    }

    /// Every one of the 32 bytes of the expected digest is compared.
    ///
    /// This exists because flipping bits in the *artifact* cannot prove it. A
    /// SHA-256 avalanches, so changing one bit of the input changes essentially
    /// every byte of the digest, and a comparison that looked at only the first
    /// four bytes would catch every such test and still be a 32-bit check that
    /// an attacker could brute-force in seconds. The difference is only visible
    /// from the other side: hold the file still and change one byte of the
    /// digest it is being compared against, which a caller can do because the
    /// expectation is a plain `[u8; 32]` and needs no preimage.
    #[test]
    fn every_byte_of_the_expected_digest_is_compared() {
        let v = newer();
        let Some((name, _)) = artifact_file_name(&v) else {
            return;
        };
        let good = artifact_bytes(&v);
        let digest = pl_core::sha256::sha256(&good);
        for i in 0..32 {
            let scratch = Scratch::new();
            let partial = scratch.path().join(format!("{name}.part"));
            std::fs::write(&partial, &good).unwrap();
            let mut expected = digest;
            expected[i] ^= 1;
            assert!(
                matches!(
                    hash_and_place(&partial, scratch.path(), &name, &expected),
                    Err(UpdateError::DigestMismatch { .. })
                ),
                "byte {i} of the expected digest is not being compared, so the \
                 comparison is narrower than SHA-256"
            );
            assert!(!scratch.path().join(&name).exists());
        }
    }

    /// One flipped bit in the artifact is caught, which is what says the digest
    /// comparison looks at all of it.
    #[test]
    fn a_single_flipped_bit_in_the_download_is_caught() {
        let v = newer();
        let Some((name, _)) = artifact_file_name(&v) else {
            return;
        };
        let good = artifact_bytes(&v);
        let expected = pl_core::sha256::sha256(&good);
        for i in 0..good.len() {
            let scratch = Scratch::new();
            let partial = scratch.path().join(format!("{name}.part"));
            let mut bad = good.clone();
            bad[i] ^= 1;
            std::fs::write(&partial, &bad).unwrap();
            assert!(
                matches!(
                    hash_and_place(&partial, scratch.path(), &name, &expected),
                    Err(UpdateError::DigestMismatch { .. })
                ),
                "flipping bit 0 of byte {i} must be caught"
            );
            assert!(!scratch.path().join(&name).exists());
        }
    }

    /// A signature over other bytes stops the flow before anything is fetched
    /// or written, and says it was the signature.
    #[test]
    fn the_artifact_is_never_even_requested_when_the_signature_fails() {
        let v = newer();
        let (text, _) = release(&v);
        // A signature over something else entirely, of the right length.
        let wrong = testsign::sign(&SEED, b"some other bytes");
        let mut server = Server::new()
            .with(manifest_url(&v), text.into_bytes())
            .with(signature_url(&v), wrong.to_vec());
        if let Some(url) = artifact_url(&v) {
            server = server.with(url, artifact_bytes(&v));
        }

        let scratch = Scratch::new();
        let got = fetch_and_verify(&server, v, scratch.path());
        assert_eq!(got, Err(UpdateError::SignatureInvalid));
        assert!(scratch.entries().is_empty());
        if let Some(url) = artifact_url(&v) {
            assert!(!server.asked_for(&url));
        }

        // And the message does not read like a network hiccup. The variant is
        // checked above; this is the text a user actually sees.
        let text = got.unwrap_err().to_string().to_lowercase();
        assert!(text.contains("signature"));
        assert!(!text.contains("try again"));
    }

    /// A missing signature file is not "unsigned, therefore fine".
    #[test]
    fn a_missing_or_short_signature_stops_the_flow() {
        let v = newer();
        let (text, _) = release(&v);
        let scratch = Scratch::new();

        // Not published at all: a transport error, and no artifact request.
        let server = Server::new().with(manifest_url(&v), text.clone().into_bytes());
        let got = fetch_and_verify(&server, v, scratch.path());
        assert!(matches!(got, Err(UpdateError::Transport { .. })), "{got:?}");
        assert!(scratch.entries().is_empty());
        if let Some(url) = artifact_url(&v) {
            assert!(!server.asked_for(&url));
        }

        // Published but truncated.
        let server = Server::new()
            .with(manifest_url(&v), text.into_bytes())
            .with(signature_url(&v), vec![0; 63]);
        let got = fetch_and_verify(&server, v, scratch.path());
        assert_eq!(got, Err(UpdateError::SignatureWrongLength { got: 63 }));
        assert!(scratch.entries().is_empty());
    }

    /// The shipped path checks against the compiled-in release key.
    ///
    /// The only way to show this without the release private key is from the
    /// other side: a manifest that is *correctly signed by a different key* is
    /// refused. If `fetch_and_verify` had been wired to a key it fetched, or to
    /// a test key, this would pass verification and fail later — or not fail at
    /// all.
    #[test]
    fn the_release_key_is_what_the_shipped_path_checks_against() {
        let v = newer();
        let server = serving(&v);
        let scratch = Scratch::new();
        assert_eq!(
            fetch_and_verify(&server, v, scratch.path()),
            Err(UpdateError::SignatureInvalid)
        );
        assert_ne!(testsign::public_key(&SEED), crate::RELEASE_PUBLIC_KEY);
    }

    /// An equal or older version is refused before anything is fetched.
    #[test]
    fn a_release_that_is_not_newer_is_refused_before_any_request() {
        let current = Version::current().unwrap();
        let scratch = Scratch::new();
        for offered in [
            current,
            Version::new(current.major(), current.minor(), 0),
            Version::new(0, 0, 0),
        ] {
            if offered > current {
                continue;
            }
            let server = serving(&offered);
            let got = fetch_and_verify(&server, offered, scratch.path());
            assert_eq!(got, Err(UpdateError::NotNewer { current, offered }));
            assert!(
                server.asked.borrow().is_empty(),
                "a rollback must be refused without asking the network for anything"
            );
            assert!(scratch.entries().is_empty());
        }
    }

    /// The destination is never this binary's own directory.
    #[test]
    fn the_destination_may_not_be_the_directory_this_binary_runs_from() {
        let exe = std::env::current_exe().unwrap();
        let dir = exe.parent().unwrap();
        let v = newer();
        let got = fetch_and_verify(&serving(&v), v, dir);
        assert!(
            matches!(got, Err(UpdateError::DestinationIsInstallDirectory { .. })),
            "writing next to the running binary must be refused, got {got:?}"
        );

        // And the guard is not vacuous: a different directory passes it.
        let scratch = Scratch::new();
        refuse_install_directory(scratch.path()).expect("a scratch directory is not the exe's");

        // Including a spelling of the same directory that is not the same
        // string, which is what the canonicalisation is for.
        let indirect = dir.join(".");
        assert!(matches!(
            refuse_install_directory(&indirect),
            Err(UpdateError::DestinationIsInstallDirectory { .. })
        ));
    }

    /// A manifest larger than the ceiling is refused rather than read in part.
    #[test]
    fn an_oversized_manifest_is_refused() {
        let v = newer();
        let server = Server::new().with(latest_manifest_url(), vec![b'x'; MAX_MANIFEST_BYTES + 1]);
        assert!(matches!(check(&server), Err(UpdateError::TooLarge { .. })));

        let server = Server::new()
            .with(manifest_url(&v), vec![b'x'; MAX_MANIFEST_BYTES + 1])
            .with(signature_url(&v), vec![0; 64]);
        let scratch = Scratch::new();
        assert!(matches!(
            fetch_and_verify(&server, v, scratch.path()),
            Err(UpdateError::TooLarge { .. })
        ));
        assert!(scratch.entries().is_empty());
    }

    /// The version reader accepts the real thing and nothing loose.
    #[test]
    fn the_version_reader_agrees_with_the_manifest_it_reads() {
        let v = Version::parse("12.34.56").unwrap();
        let (text, _) = release(&v);
        assert_eq!(version_named_by(text.as_bytes()).unwrap(), v);

        // One line is enough, and trailing junk after the version field is not
        // read as part of it.
        assert_eq!(
            version_named_by(b"aa  polylinker-0.1.10-linux-x64.tar.gz\n").unwrap(),
            Version::parse("0.1.10").unwrap()
        );
        // The same version named several times is one answer, not a conflict --
        // including the two Windows platforms, whose file names differ only in
        // the label and would be a conflict if the version field were read from
        // the wrong dash.
        assert_eq!(
            version_named_by(
                b"a  polylinker-1.0.0-linux-x64.tar.gz\n\
                  b  polylinker-1.0.0-windows-x64.msi\n\
                  c  polylinker-1.0.0-windows-arm64.msi\n"
            )
            .unwrap(),
            Version::parse("1.0.0").unwrap()
        );
    }
}
