//! `SHA256SUMS.txt`, and the rule that it does not exist until its signature
//! has verified.
//!
//! # A type that cannot be built the wrong way
//!
//! `docs/RELEASING.md`'s requirement 2 says a checksum fetched from the same
//! server as the file proves nothing. Writing that down is easy; keeping it
//! true across every future edit is the hard part, because the tempting shape
//! is a `parse()` beside a `verify()` and a call site that has to remember to
//! use both.
//!
//! So there is no `parse()`. [`VerifiedManifest`] has exactly one constructor,
//! [`VerifiedManifest::verify`], which checks the Ed25519 signature against
//! [`crate::RELEASE_PUBLIC_KEY`] *before* it looks at a single line, and the
//! parsing function is private. A caller cannot obtain a manifest it has not
//! verified, and a future author cannot reach for the checksums without the
//! signature having passed — not because they were told not to, but because the
//! type they need is not constructible any other way.
//!
//! That is also why `digest_of` returns the digest rather than the whole file
//! being handed around as text: the only thing anybody ever wants out of this
//! is "what should this file hash to", and that answer is worth having only
//! when it came through here.
//!
//! # The format
//!
//! What `sha256sum` writes, because that is what
//! `.github/workflows/release.yml` runs:
//!
//! ```text
//! 8f43...  polylinker-0.1.1-linux-x64.tar.gz
//! ```
//!
//! 64 hex characters, two spaces, the file name. GNU coreutils writes ` *`
//! instead of the second space for a file read in binary mode, and both
//! spellings are accepted here since either could appear if the workflow ever
//! gains a `-b`.
//!
//! # Why parsing is strict when the bytes are already trusted
//!
//! It is not defence against the release key — anything reaching the parser has
//! been signed by it, and a hostile signer could simply sign a hostile file
//! name. The strictness is against *this project's own* mistakes: a manifest
//! listing `../../autoexec.bat` means the release process has gone wrong, and
//! the moment to find out is before a downloader is pointed at the name. Every
//! refusal below is a claim about what a Polylinker release looks like.

use crate::error::UpdateError;
use crate::RELEASE_PUBLIC_KEY;

/// The most manifest bytes that will be parsed. A release lists four files; a
/// megabyte is four orders of magnitude of headroom and still a bound.
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
/// The most lines that will be parsed.
const MAX_ENTRIES: usize = 1000;
/// The longest file name a release entry may have.
const MAX_NAME_LEN: usize = 255;

/// A checksum table whose Ed25519 signature verified against the compiled-in
/// release key.
///
/// The existence of a value of this type is the proof. There is no other way to
/// make one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedManifest {
    entries: Vec<(String, [u8; 32])>,
}

impl VerifiedManifest {
    /// Verify `signature` over `manifest` with the release key, then parse.
    ///
    /// In that order, and the order is the point. Nothing about the manifest's
    /// content is examined — not its length, not whether it looks like a
    /// checksum table, not whether it is UTF-8 — until the signature has
    /// verified, so a malformed-input bug in the parser is not reachable by
    /// anyone who cannot sign.
    pub fn verify(manifest: &[u8], signature: &[u8]) -> Result<VerifiedManifest, UpdateError> {
        Self::verify_with(&RELEASE_PUBLIC_KEY, manifest, signature)
    }

    /// [`verify`](Self::verify) against an arbitrary key.
    ///
    /// **Private, and it must stay private.** A public function taking a key
    /// would let a caller pass a key it fetched, which is the exact failure
    /// `crate::RELEASE_PUBLIC_KEY`'s documentation calls "a checksum with extra
    /// steps". It exists because the tests need a keypair they can sign with,
    /// and the release key's private half is deliberately not on any developer
    /// machine.
    fn verify_with(
        key: &[u8; 32],
        manifest: &[u8],
        signature: &[u8],
    ) -> Result<VerifiedManifest, UpdateError> {
        if signature.len() != 64 {
            return Err(UpdateError::SignatureWrongLength {
                got: signature.len(),
            });
        }
        if !pl_core::ed25519::verify_slices(key, manifest, signature) {
            return Err(UpdateError::SignatureInvalid);
        }
        Self::parse_verified(manifest)
    }

    /// [`verify_with`](Self::verify_with), reachable from this crate's other
    /// modules' tests and **only** from them.
    ///
    /// `#[cfg(test)]` rather than `pub(crate)`: a `pub(crate)` door is still a
    /// door in a shipped build, and somebody refactoring `flow.rs` in a hurry
    /// could reach for it. This one does not exist outside `cargo test`.
    #[cfg(test)]
    pub(crate) fn verify_for_test(
        key: &[u8; 32],
        manifest: &[u8],
        signature: &[u8],
    ) -> Result<VerifiedManifest, UpdateError> {
        Self::verify_with(key, manifest, signature)
    }

    /// Read the table. Only ever called on bytes whose signature has verified.
    fn parse_verified(manifest: &[u8]) -> Result<VerifiedManifest, UpdateError> {
        let malformed = |detail: String| UpdateError::ManifestMalformed { detail };

        if manifest.len() > MAX_MANIFEST_BYTES {
            return Err(malformed(format!(
                "it is {} bytes, and no Polylinker manifest is over {MAX_MANIFEST_BYTES}",
                manifest.len()
            )));
        }
        let text = core::str::from_utf8(manifest)
            .map_err(|e| malformed(format!("it is not UTF-8 ({e})")))?;

        let mut entries: Vec<(String, [u8; 32])> = Vec::new();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            if line.is_empty() {
                continue;
            }
            if entries.len() >= MAX_ENTRIES {
                return Err(malformed(format!("it has more than {MAX_ENTRIES} entries")));
            }
            let at = format!("line {}", n + 1);

            // 64 hex characters, then the two-character separator coreutils
            // writes: "  " for text mode, " *" for binary.
            if line.len() < 66 {
                return Err(malformed(format!("{at} is too short to be an entry")));
            }
            let (digest_hex, rest) = line.split_at(64);
            let digest = unhex32(digest_hex)
                .ok_or_else(|| malformed(format!("{at} does not begin with 64 hex digits")))?;
            let name = match rest.strip_prefix("  ").or_else(|| rest.strip_prefix(" *")) {
                Some(name) => name,
                None => {
                    return Err(malformed(format!(
                        "{at} does not separate the digest from the name with two spaces"
                    )))
                }
            };
            check_name(name).map_err(|why| malformed(format!("{at}: {why}")))?;
            if entries.iter().any(|(existing, _)| existing == name) {
                // Two digests for one name, and no way to tell which the
                // release meant. Refused rather than resolved by position.
                return Err(malformed(format!("{at}: {name} is listed twice")));
            }
            entries.push((name.to_string(), digest));
        }

        if entries.is_empty() {
            return Err(malformed("it lists no files at all".to_string()));
        }
        Ok(VerifiedManifest { entries })
    }

    /// What `file_name` must hash to, if this release lists it.
    pub fn digest_of(&self, file_name: &str) -> Option<[u8; 32]> {
        self.entries
            .iter()
            .find(|(name, _)| name == file_name)
            .map(|(_, digest)| *digest)
    }

    /// The file names this release lists, in the order the manifest gives.
    pub fn file_names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(name, _)| name.as_str())
    }

    /// How many files this release lists.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Never true — a manifest listing nothing is refused by the parser — but
    /// clippy asks for it next to `len`, and a caller reading it should be told
    /// that the answer is a constant rather than something to branch on.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Exactly 64 hex digits, either case, as 32 bytes.
fn unhex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    let bytes = text.as_bytes();
    for (i, byte) in out.iter_mut().enumerate() {
        let hi = (bytes[i * 2] as char).to_digit(16)?;
        let lo = (bytes[i * 2 + 1] as char).to_digit(16)?;
        *byte = ((hi << 4) | lo) as u8;
    }
    Some(out)
}

/// Is this a file name a Polylinker release would list?
///
/// Every rule is a refusal of something that would mean the release process
/// itself had gone wrong; see the module doc on why that is worth checking even
/// though these bytes are signed.
fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("the file name is empty".into());
    }
    if name.len() > MAX_NAME_LEN {
        return Err(format!("the file name is over {MAX_NAME_LEN} bytes"));
    }
    if !name.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(format!(
            "{name:?} has a space or a non-printable character in it"
        ));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(format!("{name} is a path, not a file name"));
    }
    if name.split('.').any(|part| part.is_empty()) && name.contains("..") {
        return Err(format!("{name} contains a parent-directory hop"));
    }
    if name.starts_with('-') {
        return Err(format!(
            "{name} begins with a dash, which a command line could read as a flag"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsign;

    /// A manifest shaped like the one `.github/workflows/release.yml` produces.
    fn sample() -> String {
        let mut text = String::new();
        for (digest, name) in [
            (
                "0000000000000000000000000000000000000000000000000000000000000001",
                "polylinker-1.2.3-linux-x64.tar.gz",
            ),
            (
                "0000000000000000000000000000000000000000000000000000000000000002",
                "polylinker-1.2.3-macos-universal.tar.gz",
            ),
            (
                "0000000000000000000000000000000000000000000000000000000000000003",
                "polylinker-1.2.3-windows-x64.msi",
            ),
            (
                "0000000000000000000000000000000000000000000000000000000000000004",
                "polylinker-1.2.3-windows-x64.zip",
            ),
        ] {
            text.push_str(digest);
            text.push_str("  ");
            text.push_str(name);
            text.push('\n');
        }
        text
    }

    const TEST_SEED: [u8; 32] = [0x11; 32];

    fn test_key() -> [u8; 32] {
        testsign::public_key(&TEST_SEED)
    }

    /// The positive case, without which every refusal below proves nothing.
    ///
    /// A `verify_with` that returned `SignatureInvalid` unconditionally would
    /// pass every other test in this module. This is the one that says the
    /// door opens.
    #[test]
    fn a_correctly_signed_manifest_verifies_and_parses() {
        let text = sample();
        let sig = testsign::sign(&TEST_SEED, text.as_bytes());
        let m = VerifiedManifest::verify_with(&test_key(), text.as_bytes(), &sig)
            .expect("a correctly signed manifest must verify");
        assert_eq!(m.len(), 4);
        assert_eq!(
            m.digest_of("polylinker-1.2.3-windows-x64.msi").unwrap()[31],
            3
        );
        assert_eq!(
            m.digest_of("polylinker-1.2.3-linux-x64.tar.gz").unwrap()[31],
            1
        );
        assert!(m.digest_of("polylinker-1.2.3-linux-arm64.tar.gz").is_none());
        assert!(m.digest_of("").is_none());
        assert_eq!(
            m.file_names().collect::<Vec<_>>(),
            vec![
                "polylinker-1.2.3-linux-x64.tar.gz",
                "polylinker-1.2.3-macos-universal.tar.gz",
                "polylinker-1.2.3-windows-x64.msi",
                "polylinker-1.2.3-windows-x64.zip",
            ]
        );

        // The binary-mode separator coreutils writes with `-b`.
        let binary = text.replace("  polylinker", " *polylinker");
        assert_ne!(binary, text);
        let sig = testsign::sign(&TEST_SEED, binary.as_bytes());
        let m = VerifiedManifest::verify_with(&test_key(), binary.as_bytes(), &sig).unwrap();
        assert_eq!(m.len(), 4);
    }

    /// A signature over *different bytes* does not verify these bytes.
    ///
    /// The attack this is written against is the obvious one: take the
    /// signature from a genuine release, serve it beside a manifest of your
    /// own. It is refused, and it is refused as
    /// [`UpdateError::SignatureInvalid`] rather than as anything a caller might
    /// treat as transient.
    #[test]
    fn a_signature_over_other_bytes_is_refused() {
        let text = sample();
        let elsewhere = text.replace("1.2.3", "9.9.9");
        assert_ne!(elsewhere, text);
        let sig = testsign::sign(&TEST_SEED, elsewhere.as_bytes());
        assert_eq!(
            VerifiedManifest::verify_with(&test_key(), text.as_bytes(), &sig),
            Err(UpdateError::SignatureInvalid)
        );

        // And the genuine signature does not verify the substituted manifest
        // either, which is the same statement from the other side.
        let genuine = testsign::sign(&TEST_SEED, text.as_bytes());
        assert_eq!(
            VerifiedManifest::verify_with(&test_key(), elsewhere.as_bytes(), &genuine),
            Err(UpdateError::SignatureInvalid)
        );
    }

    /// Every single-bit change to the manifest is refused.
    ///
    /// Not a sample: every bit of every byte, because "the signature covers the
    /// whole file" is exactly the claim that a partial check would satisfy for
    /// the bytes a sample happened to pick. A verifier that hashed only the
    /// first line, or stopped at the first newline, passes
    /// [`a_signature_over_other_bytes_is_refused`] and fails this.
    #[test]
    fn every_single_bit_flip_in_the_manifest_is_refused() {
        let text = sample();
        let key = test_key();
        let sig = testsign::sign(&TEST_SEED, text.as_bytes());
        assert!(VerifiedManifest::verify_with(&key, text.as_bytes(), &sig).is_ok());

        for i in 0..text.len() {
            for bit in 0..8 {
                let mut bad = text.clone().into_bytes();
                bad[i] ^= 1 << bit;
                assert_eq!(
                    VerifiedManifest::verify_with(&key, &bad, &sig),
                    Err(UpdateError::SignatureInvalid),
                    "byte {i} bit {bit} of the manifest changed and the signature still verified"
                );
            }
        }
    }

    /// Every single-bit change to the signature is refused, and so is every
    /// wrong length.
    ///
    /// The length half matters on its own: an Ed25519 signature is exactly 64
    /// bytes, and a truncated file is what a half-finished download leaves
    /// behind. It must be a refusal with its own name, not a panic in a
    /// `try_into().unwrap()`.
    #[test]
    fn a_damaged_or_wrong_length_signature_is_refused() {
        let text = sample();
        let key = test_key();
        let sig = testsign::sign(&TEST_SEED, text.as_bytes());

        for i in 0..64 {
            for bit in 0..8 {
                let mut bad = sig;
                bad[i] ^= 1 << bit;
                assert_eq!(
                    VerifiedManifest::verify_with(&key, text.as_bytes(), &bad),
                    Err(UpdateError::SignatureInvalid),
                    "byte {i} bit {bit} of the signature changed and it still verified"
                );
            }
        }

        for len in [0usize, 1, 32, 63, 65, 128] {
            let mut bad = sig.to_vec();
            bad.resize(len, 0);
            assert_eq!(
                VerifiedManifest::verify_with(&key, text.as_bytes(), &bad),
                Err(UpdateError::SignatureWrongLength { got: len }),
                "a {len}-byte signature must be refused for its length"
            );
        }

        // A missing signature file is the zero-length case, and it must not be
        // mistaken for "unsigned is fine".
        assert!(matches!(
            VerifiedManifest::verify_with(&key, text.as_bytes(), &[]),
            Err(UpdateError::SignatureWrongLength { got: 0 })
        ));
    }

    /// A signature by somebody else's key is refused.
    #[test]
    fn a_signature_by_another_key_is_refused() {
        let text = sample();
        let other_seed = [0x22; 32];
        assert_ne!(testsign::public_key(&other_seed), test_key());
        let sig = testsign::sign(&other_seed, text.as_bytes());
        assert_eq!(
            VerifiedManifest::verify_with(&test_key(), text.as_bytes(), &sig),
            Err(UpdateError::SignatureInvalid)
        );
    }

    /// The public entry point really uses the compiled-in release key.
    ///
    /// [`VerifiedManifest::verify`] is the only constructor anything outside
    /// this module can reach, and every other test in this file goes through
    /// `verify_with`. If `verify` passed some other key — or, in the shape this
    /// is actually written against, if a future edit gave it a key parameter
    /// with a default — nothing else here would notice.
    ///
    /// It can only be tested negatively: a signature that verifies under
    /// [`crate::RELEASE_PUBLIC_KEY`] cannot be produced without the private
    /// half, which is not on this machine by design. So what is asserted is
    /// that a manifest signed by a *test* key is refused by the public
    /// constructor while being accepted by the private one over the same bytes.
    #[test]
    fn the_public_constructor_uses_the_release_key_and_not_a_test_one() {
        let text = sample();
        let sig = testsign::sign(&TEST_SEED, text.as_bytes());
        assert!(VerifiedManifest::verify_with(&test_key(), text.as_bytes(), &sig).is_ok());
        assert_eq!(
            VerifiedManifest::verify(text.as_bytes(), &sig),
            Err(UpdateError::SignatureInvalid),
            "a manifest signed by a test key must not verify against the release key"
        );
        assert_ne!(test_key(), RELEASE_PUBLIC_KEY);
    }

    /// Malformed tables are refused *after* the signature passes.
    ///
    /// Each of these is signed correctly, so the only thing that can reject
    /// them is the parser — which is the point: this is where a release process
    /// that produced nonsense gets caught, not where an attacker does.
    #[test]
    fn a_correctly_signed_but_malformed_table_is_refused() {
        let key = test_key();
        for (what, text) in [
            ("nothing at all", String::new()),
            ("only whitespace", "\n\n\n".to_string()),
            ("prose", "no checksums here, just a note\n".to_string()),
            (
                "a short digest",
                "0000000000000000000000000000000000000000000000000000000000000  a.txt\n"
                    .to_string(),
            ),
            (
                "a non-hex digest",
                "zzzz000000000000000000000000000000000000000000000000000000000001  a.txt\n"
                    .to_string(),
            ),
            (
                "one space instead of two",
                "0000000000000000000000000000000000000000000000000000000000000001 a.txt\n"
                    .to_string(),
            ),
            (
                "a tab instead of two spaces",
                "0000000000000000000000000000000000000000000000000000000000000001\ta.txt\n"
                    .to_string(),
            ),
            (
                "no file name",
                "0000000000000000000000000000000000000000000000000000000000000001  \n".to_string(),
            ),
            (
                "a path instead of a name",
                "0000000000000000000000000000000000000000000000000000000000000001  ../../a.txt\n"
                    .to_string(),
            ),
            (
                "a Windows path",
                "0000000000000000000000000000000000000000000000000000000000000001  C:\\a.txt\n"
                    .to_string(),
            ),
            (
                "a name that reads as a flag",
                "0000000000000000000000000000000000000000000000000000000000000001  -o/tmp/x\n"
                    .to_string(),
            ),
            (
                "a name with a space in it",
                "0000000000000000000000000000000000000000000000000000000000000001  a b.txt\n"
                    .to_string(),
            ),
            (
                "the same name twice",
                "0000000000000000000000000000000000000000000000000000000000000001  a.txt\n\
                 0000000000000000000000000000000000000000000000000000000000000002  a.txt\n"
                    .to_string(),
            ),
            ("a trailing junk line", format!("{}garbage\n", sample())),
        ] {
            let sig = testsign::sign(&TEST_SEED, text.as_bytes());
            let got = VerifiedManifest::verify_with(&key, text.as_bytes(), &sig);
            assert!(
                matches!(got, Err(UpdateError::ManifestMalformed { .. })),
                "a signed manifest containing {what} must be refused as malformed, got {got:?}"
            );
        }
    }

    /// Not UTF-8 is refused rather than lossily converted.
    #[test]
    fn a_manifest_that_is_not_text_is_refused() {
        let key = test_key();
        let bytes = [0xffu8; 96];
        let sig = testsign::sign(&TEST_SEED, &bytes);
        assert!(matches!(
            VerifiedManifest::verify_with(&key, &bytes, &sig),
            Err(UpdateError::ManifestMalformed { .. })
        ));
    }

    /// The size and count bounds fire before the work does.
    #[test]
    fn an_absurd_manifest_is_refused_by_size() {
        let key = test_key();
        let huge = "x".repeat(MAX_MANIFEST_BYTES + 1);
        let sig = testsign::sign(&TEST_SEED, huge.as_bytes());
        assert!(matches!(
            VerifiedManifest::verify_with(&key, huge.as_bytes(), &sig),
            Err(UpdateError::ManifestMalformed { .. })
        ));

        let mut many = String::new();
        for i in 0..=MAX_ENTRIES {
            many.push_str(&format!(
                "00000000000000000000000000000000000000000000000000000000000000{:02x}  f{i}.txt\n",
                i % 256
            ));
        }
        let sig = testsign::sign(&TEST_SEED, many.as_bytes());
        assert!(matches!(
            VerifiedManifest::verify_with(&key, many.as_bytes(), &sig),
            Err(UpdateError::ManifestMalformed { .. })
        ));
    }

    /// Upper-case hex is read as the same digest as lower-case.
    #[test]
    fn hex_is_read_in_either_case() {
        assert_eq!(unhex32(&"ab".repeat(32)), unhex32(&"AB".repeat(32)));
        assert_eq!(unhex32(&"ab".repeat(32)).unwrap()[0], 0xab);
        assert!(unhex32(&"ab".repeat(31)).is_none());
        assert!(unhex32(&"gg".repeat(32)).is_none());
        assert!(unhex32("").is_none());
    }
}
