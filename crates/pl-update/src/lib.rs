//! The release trust anchor — the Ed25519 public key that release manifests are
//! checked against, **compiled into the binary** — and the updater that uses
//! it.
//!
//! # What is here
//!
//! | Module | What it is |
//! |---|---|
//! | this file | the compiled-in public key, three ways |
//! | [`version`] | three integers, compared as integers; the only thing the network may put in a URL |
//! | [`net`] | the transport: the system `curl`, with the flags argued for one at a time |
//! | [`manifest`] | `SHA256SUMS.txt`, in a type that cannot exist unless its signature verified |
//! | [`flow`] | [`check`] and [`fetch_and_verify`], and where each of the four requirements is met |
//!
//! # Who calls this
//!
//! Two places, and the list is short on purpose because it is the whole network
//! surface of the product:
//!
//! * `bins/pl`'s `cmd_update` — the `pl update` verb. Both [`check`] and
//!   [`fetch_and_verify`], and only when somebody types the verb.
//! * `bins/pl-gui`'s `update` module — [`check`] only, at most once per launch,
//!   and only when a setting that ships OFF has been switched on. The desktop
//!   app has no code that downloads a release.
//!
//! `bins/pl-mcp` does not depend on this crate. Each of the two that do carries
//! a test that reads its own sources and fails if this crate is named outside
//! the one function or module listed above, so the list stays a fact rather
//! than becoming a comment. `docs/RELEASING.md`, "The decision to expose it",
//! is the review that allowed either of them.
//!
//! # The four requirements
//!
//! `docs/RELEASING.md` sets four requirements for any updater this project ever
//! grows. [`flow`]'s module doc names the line where each is met and the test
//! that holds it there. In brief: nothing runs unless it is called
//! ([`check`], [`fetch_and_verify`], no timer and no thread); the manifest's
//! signature is verified before the artifact is so much as *requested*; the key
//! it is verified against is [`RELEASE_PUBLIC_KEY`], below; and the last thing
//! [`fetch_and_verify`] does is return the path of a verified file for a person
//! to run, having replaced nothing.
//!
//! # Why the key is compiled in — requirement 3
//!
//! `docs/RELEASING.md` sets four requirements for any updater this project
//! ever grows. The third is that **the public key is compiled into the binary
//! being replaced, so the trust anchor is not fetched from the network**. This
//! crate is that requirement, and it is worth spelling out what it buys,
//! because it is the one of the four that sounds like an implementation detail
//! and is not.
//!
//! A signature is worth exactly the independence of the key that checks it. If
//! the updater fetched `polylinker.pub` from the release page next to
//! `SHA256SUMS.txt.sig`, then whoever can serve a modified archive can serve a
//! modified manifest, a signature over it, *and* the key that accepts that
//! signature — three files from one server, all agreeing, every check passing.
//! That is not a weaker version of signature verification; it is a checksum
//! with extra steps, which is precisely what requirement 2 rejects. The key has
//! to come from somewhere the attacker does not control, and the only such
//! place available offline is the copy of Polylinker the user is already
//! running and has therefore already decided to trust.
//!
//! So the trust flows backwards through the install history rather than
//! sideways off the network: version *n* vouches for version *n+1*, and the
//! very first copy is vouched for by however the user got it — a checksum they
//! compared by hand, a package manager, a colleague's USB stick. Nothing here
//! improves that first hop, and nothing can.
//!
//! A keyring file sitting beside the executable was considered and rejected for
//! the same reason in a smaller form: anything that can write next to the
//! executable can rewrite the keyring, and on Windows the per-user install this
//! project ships by default puts both in a directory the user's own processes
//! can write. Compiled in, replacing the key means replacing the binary, which
//! is the thing the signature was protecting in the first place.
//!
//! # What this does not do
//!
//! **It does not survive key compromise.** Every copy already installed trusts
//! this key forever; there is no revocation channel, because a revocation
//! channel is a network call and the whole point is that there is not one. If
//! the private half leaks, the remedy is a new release that users install by
//! hand, announced wherever they will actually read it. That is a real cost of
//! this design and it is the price of the offline guarantee.
//!
//! **It does not say a release is good**, only that it was made by whoever
//! holds the private half. A signature is a claim about origin and nothing
//! else.
//!
//! **Nothing that ships from here signs anything, and nothing ever should.**
//! The release private half is not on any developer machine and is not in this
//! repository; it is a GitHub Actions secret used by
//! `.github/workflows/release.yml` and nowhere else. `pl-core::ed25519` refuses
//! to sign for reasons its own module doc gives at length — none of it is
//! constant-time, and every shortcut that makes it auditable becomes a leak the
//! moment a secret scalar flows through it.
//!
//! There *is* a signer in `src/testsign.rs`, behind `#[cfg(test)]`, and it is
//! worth saying why that is not a contradiction. It is compiled only by
//! `cargo test`, never into a shipped binary; the only keys it touches are
//! generated a few lines above the call; and without it every verification test
//! in this crate would be a refusal with nothing to contrast against, which a
//! verifier returning `false` unconditionally would pass. Its own module doc
//! makes the argument in full.
//!
//! # What the signature covers
//!
//! `SHA256SUMS.txt.sig` is a signature over `SHA256SUMS.txt`, not over any
//! archive. The archives are covered transitively: the manifest names each file
//! and its SHA-256, so verifying the signature and then the digest of the file
//! on disk is a chain from this constant to those bytes. Requirement 2 is met
//! by that chain, and only if **both** links are checked. A future updater that
//! verifies the signature and forgets to re-hash the download has verified
//! nothing about the download.
//!
//! # Editing this file
//!
//! The three constants below are three spellings of one 32-byte value.
//! [`RELEASE_PUBLIC_KEY`] is the load-bearing one — it is what a verifier will
//! read — and the other two exist so that a careless edit to thirty-two
//! hexadecimal bytes cannot pass unnoticed: `tests/key.rs` re-encodes the array
//! and requires all three to agree, so a changed key has to be changed
//! deliberately in three places or the build goes red.
//!
//! **`.github/workflows/release.yml` reads [`RELEASE_PUBLIC_KEY_BASE64`] out of
//! this file by pattern**, to check the signature it has just produced against
//! the key that actually ships rather than against a second copy pasted into
//! YAML. It requires exactly one 43-character base64 string ending in `=` to
//! appear in this file and fails the release loudly if it finds none or several.
//! Do not add another one — not in a doc comment, not in an example — without
//! changing that step to match.

pub mod error;
pub mod flow;
pub mod manifest;
pub mod net;
pub mod version;

/// The signer, for the tests only. See this file's module doc for why it is
/// allowed to exist here when `pl-core` refuses to have one at all.
#[cfg(test)]
mod testsign;

pub use error::UpdateError;
pub use flow::{
    artifact_file_name, artifact_url, check, fetch_and_verify, latest_manifest_url, manifest_url,
    signature_url, Check, Handoff, Kind, RELEASE_BASE_URL,
};
pub use manifest::VerifiedManifest;
pub use net::{curl_available, Curl, Fetch};
pub use version::{Version, CURRENT_VERSION};

/// The Ed25519 public key that signs Polylinker release manifests.
///
/// `5a53cfdab24df9b4d8e918aed8e03338bdcac10b073a6f59d21d3ee9836be3b7`, as the
/// 32 raw bytes `pl_core::ed25519::verify` takes. See the module doc for why it
/// is here rather than fetched.
pub const RELEASE_PUBLIC_KEY: [u8; 32] = [
    0x5a, 0x53, 0xcf, 0xda, 0xb2, 0x4d, 0xf9, 0xb4, 0xd8, 0xe9, 0x18, 0xae, 0xd8, 0xe0, 0x33, 0x38,
    0xbd, 0xca, 0xc1, 0x0b, 0x07, 0x3a, 0x6f, 0x59, 0xd2, 0x1d, 0x3e, 0xe9, 0x83, 0x6b, 0xe3, 0xb7,
];

/// [`RELEASE_PUBLIC_KEY`] as lower-case hexadecimal, 64 characters.
///
/// The form the release notes and `docs/RELEASING.md` quote, so that a reader
/// can compare what they were told against what the binary contains. Pinned to
/// the array by `tests/key.rs`.
pub const RELEASE_PUBLIC_KEY_HEX: &str =
    "5a53cfdab24df9b4d8e918aed8e03338bdcac10b073a6f59d21d3ee9836be3b7";

/// [`RELEASE_PUBLIC_KEY`] as standard base64 with padding, 44 characters.
///
/// The form OpenSSL and every other tool speak: a SubjectPublicKeyInfo PEM for
/// an Ed25519 key is the fixed 12-byte prefix `302a300506032b6570032100`
/// followed by these 32 bytes, and 12 is divisible by 3, so the PEM body is
/// literally `MCowBQYDK2VwAyEA` followed by this string with no re-encoding.
/// That is what lets `.github/workflows/release.yml` build a verification key
/// out of this constant with string concatenation and no tools at all.
///
/// Pinned to the array by `tests/key.rs`.
pub const RELEASE_PUBLIC_KEY_BASE64: &str = "WlPP2rJN+bTY6Riu2OAzOL3KwQsHOm9Z0h0+6YNr47c=";
