//! What can go wrong, kept distinguishable on purpose.
//!
//! # Why a signature failure is its own variant
//!
//! `docs/RELEASING.md`'s requirement 2 is that the update is gated on a
//! signature rather than on a checksum from the same server. An implementation
//! can satisfy that on paper and lose it in the error handling: if a failed
//! verification is reported as "could not update, please try again", the next
//! thing the user does is try again, and the third thing they do is download
//! the file by hand from the page the attacker is serving. A wrong signature is
//! not a transient condition and must never be worded like one, so
//! [`UpdateError::SignatureInvalid`] exists, says what happened in as many
//! words, and tells the reader to stop rather than retry.
//!
//! `a_bad_signature_is_never_reported_as_a_network_problem` in `flow.rs`
//! asserts that, by matching on the variant *and* reading the rendered text, so
//! that a future edit to the wording cannot quietly turn it back into a
//! shrug.
//!
//! # Every variant here means NO UPDATE
//!
//! There is no partial success and no "verified except for". Each of these is
//! returned instead of a path to an installer, and the caller has nothing it
//! could proceed with.

use crate::version::Version;
use core::fmt;
use std::path::PathBuf;

/// Why the update did not happen. All of them mean it did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// `curl` is not on this system.
    ///
    /// Not an internal error and not worth a workaround: this crate has no HTTP
    /// client of its own by design (see `net.rs`), so the honest thing is to
    /// say so and stop.
    CurlMissing,
    /// `curl` ran and did not come back with the bytes: no route, DNS failure,
    /// timeout, TLS refusal, 404, 500. Retrying may help. Nothing was written.
    Transport { url: String, detail: String },
    /// The server sent more than the caller was willing to hold in memory.
    /// Refused rather than truncated — a truncated manifest is a manifest.
    TooLarge { url: String, limit: usize },
    /// The text that should have named the current version did not.
    UnreadableVersion { detail: String },
    /// `CARGO_PKG_VERSION` is not `major.minor.patch`, so this binary cannot
    /// say what version it is and therefore cannot say what is newer.
    UnreadableCompiledVersion { got: String },
    /// The manifest's bytes are not a checksum table this crate can read.
    ManifestMalformed { detail: String },
    /// The signature file is not 64 bytes. Ed25519 signatures are exactly 64;
    /// anything else is not a short signature, it is not a signature.
    SignatureWrongLength { got: usize },
    /// **The signature over the manifest did not verify against the
    /// compiled-in release key.** Stop. Do not retry, do not fall back to the
    /// checksum, do not download the file by hand from the same page.
    SignatureInvalid,
    /// This platform has no release artifact — an architecture or an operating
    /// system the release workflow does not build for.
    PlatformUnsupported,
    /// The verified manifest does not list the file this platform needs. The
    /// release is genuine and simply does not contain it.
    NotInManifest { file: String },
    /// The download's SHA-256 is not the one the **verified** manifest gives.
    /// The temporary file has been deleted.
    DigestMismatch {
        file: String,
        expected: String,
        actual: String,
    },
    /// The offered version is not newer than the running one. Refused, because
    /// a genuinely signed *older* release is exactly what a rollback looks
    /// like; see `version.rs`.
    NotNewer { current: Version, offered: Version },
    /// The destination is the directory this binary is running from. Refused:
    /// requirement 4 is that a running binary is never replaced, and the
    /// simplest way to keep that promise is never to write there.
    DestinationIsInstallDirectory { path: PathBuf },
    /// Where this binary lives could not be determined, so the check above
    /// cannot be made. Fails closed rather than skipping the check.
    UnknownInstallLocation { detail: String },
    /// This program was about to run `curl` with an argument vector that breaks
    /// one of its own rules, and refused to. A bug here, not out there.
    UnsafeRequest { why: String },
    /// A filesystem operation failed.
    Io { what: String, detail: String },
}

/// A single-line, control-character-free, length-bounded version of `text`.
///
/// Error text ends up in a terminal and in a GUI label, and some of it comes
/// from outside: `curl`'s stderr, a file name out of a manifest. A bare `\r`
/// can rewrite a terminal line and hide the rest of the message, and an escape
/// sequence can do considerably more, so nothing that is not printable ASCII
/// survives into a message.
fn tidy(text: &str) -> String {
    let mut out: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let trimmed = out.trim();
    if trimmed.len() != out.len() {
        out = trimmed.to_string();
    }
    if out.len() > 400 {
        out.truncate(397);
        out.push_str("...");
    }
    out
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::CurlMissing => write!(
                f,
                "curl was not found. Polylinker has no HTTP client of its own and \
                 uses the system curl, which ships with Windows 10 and later, with \
                 macOS, and with every mainstream Linux distribution. Install curl, \
                 or download the release by hand from the releases page"
            ),
            UpdateError::Transport { url, detail } => write!(
                f,
                "could not fetch {}: {}. Nothing was written",
                tidy(url),
                tidy(detail)
            ),
            UpdateError::TooLarge { url, limit } => write!(
                f,
                "{} is larger than the {limit} bytes expected of it, so it was \
                 refused rather than read in part",
                tidy(url)
            ),
            UpdateError::UnreadableVersion { detail } => write!(
                f,
                "the release page did not name a version this build can read: {}. \
                 No update",
                tidy(detail)
            ),
            UpdateError::UnreadableCompiledVersion { got } => write!(
                f,
                "this build's own version is {:?}, which is not major.minor.patch, \
                 so it cannot tell whether a release is newer than itself",
                tidy(got)
            ),
            UpdateError::ManifestMalformed { detail } => write!(
                f,
                "the release manifest is not a checksum table: {}. No update",
                tidy(detail)
            ),
            UpdateError::SignatureWrongLength { got } => write!(
                f,
                "the release manifest's signature is {got} bytes; an Ed25519 \
                 signature is exactly 64. The manifest is unsigned as far as this \
                 build is concerned, so no update was made"
            ),
            UpdateError::SignatureInvalid => write!(
                f,
                "THE RELEASE MANIFEST'S SIGNATURE DID NOT VERIFY against the key \
                 built into this copy of Polylinker. This is not a network \
                 problem and retrying will not help: the bytes served were not \
                 signed by the Polylinker release key. Nothing was downloaded and \
                 nothing was written. Do not work around this by downloading the \
                 file by hand from the same page"
            ),
            UpdateError::PlatformUnsupported => write!(
                f,
                "there is no Polylinker release build for this operating system \
                 and architecture, so there is nothing to update to"
            ),
            UpdateError::NotInManifest { file } => write!(
                f,
                "the signed manifest for this release does not list {}, the file \
                 this platform would need. No update",
                tidy(file)
            ),
            UpdateError::DigestMismatch {
                file,
                expected,
                actual,
            } => write!(
                f,
                "{} does not match the signed manifest: expected SHA-256 {}, got \
                 {}. The download has been deleted",
                tidy(file),
                tidy(expected),
                tidy(actual)
            ),
            UpdateError::NotNewer { current, offered } => write!(
                f,
                "the release page offers {offered} and this is {current}, which is \
                 not older, so there is nothing to install"
            ),
            UpdateError::DestinationIsInstallDirectory { path } => write!(
                f,
                "refusing to download into {}, which is where this copy of \
                 Polylinker is running from. The update is handed over as a file \
                 for you to run, and it is never written next to the program it \
                 would replace",
                tidy(&path.display().to_string())
            ),
            UpdateError::UnknownInstallLocation { detail } => write!(
                f,
                "could not determine where this copy of Polylinker is installed \
                 ({}), so it cannot be shown that the download would land \
                 somewhere else. No update",
                tidy(detail)
            ),
            UpdateError::UnsafeRequest { why } => write!(
                f,
                "Polylinker refused to make its own request because it would \
                 have broken one of its own rules ({}). Nothing was fetched. \
                 This is a bug in Polylinker; please report it",
                tidy(why)
            ),
            UpdateError::Io { what, detail } => {
                write!(f, "{}: {}", tidy(what), tidy(detail))
            }
        }
    }
}

impl std::error::Error for UpdateError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing this type prints can rewrite the line it is printed on.
    ///
    /// `curl`'s stderr and a manifest's file names both reach these strings.
    /// The manifest is verified before its contents are read, but the *failure*
    /// messages are produced from unverified bytes by construction — that is
    /// what they are reporting on.
    #[test]
    fn message_text_is_stripped_of_control_characters() {
        let nasty = "line one\r\x1b[2Kline two\nand\ta tab\u{7}";
        let errors = [
            UpdateError::Transport {
                url: nasty.into(),
                detail: nasty.into(),
            },
            UpdateError::ManifestMalformed {
                detail: nasty.into(),
            },
            UpdateError::NotInManifest { file: nasty.into() },
            UpdateError::UnreadableVersion {
                detail: nasty.into(),
            },
            UpdateError::DigestMismatch {
                file: nasty.into(),
                expected: nasty.into(),
                actual: nasty.into(),
            },
            UpdateError::Io {
                what: nasty.into(),
                detail: nasty.into(),
            },
        ];
        for e in errors {
            let text = e.to_string();
            assert!(
                !text.contains('\n')
                    && !text.contains('\r')
                    && !text.contains('\t')
                    && !text.contains('\u{1b}')
                    && !text.contains('\u{7}'),
                "control characters survived into {text:?}"
            );
        }
    }

    /// Long output from `curl` cannot flood the message.
    #[test]
    fn message_text_is_bounded() {
        let e = UpdateError::Transport {
            url: "x".repeat(10_000),
            detail: "y".repeat(10_000),
        };
        assert!(e.to_string().len() < 1_000, "{}", e.to_string().len());
    }

    /// The wording of a signature failure is load-bearing, so it is asserted
    /// rather than left to whoever edits it next.
    ///
    /// Requirement 2 of `docs/RELEASING.md` is lost the moment this message
    /// reads like a hiccup, because "try again" and "just download it from the
    /// page" are what a user does with a hiccup — and the page is the thing
    /// under suspicion.
    #[test]
    fn a_signature_failure_says_so_and_does_not_suggest_retrying() {
        let text = UpdateError::SignatureInvalid.to_string().to_lowercase();
        assert!(text.contains("signature"), "{text}");
        assert!(text.contains("did not verify"), "{text}");
        assert!(
            text.contains("not a network problem"),
            "a signature failure must not be mistakable for a transport failure: {text}"
        );
        assert!(
            text.contains("retrying will not help"),
            "the user must be told not to retry: {text}"
        );
        assert!(
            !text.contains("try again"),
            "the one thing this message must never say: {text}"
        );
    }
}
