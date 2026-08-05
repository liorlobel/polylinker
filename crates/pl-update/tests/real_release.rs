//! The signature CI actually produced, checked by the verifier that ships.
//!
//! Everything else about the signing path is tested against keys and
//! signatures the tests make up themselves. That proves the code is
//! self-consistent, and self-consistency is exactly what a broken pipeline also
//! has: a release that signed with the wrong key, or with a key derived
//! slightly wrongly from the secret, would satisfy every one of those tests and
//! still ship a signature no installed copy could check.
//!
//! So these two files are the real ones, downloaded from the v0.1.2 release
//! page on 2026-08-06:
//!
//!   * `SHA256SUMS.txt` as published, listing the four artifacts of that
//!     release.
//!   * `SHA256SUMS.txt.sig`, the 64 raw bytes `openssl pkeyutl -sign -rawin`
//!     produced on the runner from `POLYLINKER_RELEASE_KEY`.
//!
//! Nothing here reaches the network. These are committed bytes, and the
//! signature over them stays valid forever, so this is a pinned end-to-end
//! record that the private half held by CI and the public half compiled into
//! `pl` and `polylinker` are two halves of one key.
//!
//! If a future release changes how signing works and gets it wrong, this test
//! will not notice by itself -- it describes v0.1.2, not the future. What it
//! does catch is the thing most likely to go wrong silently: someone editing
//! the 32 bytes of `RELEASE_PUBLIC_KEY`, or replacing the key without
//! re-signing anything, and every made-up-key test carrying on green.

const MANIFEST: &[u8] = include_bytes!("fixtures/v0.1.2-SHA256SUMS.txt");
const SIGNATURE: &[u8] = include_bytes!("fixtures/v0.1.2-SHA256SUMS.txt.sig");

#[test]
fn the_key_compiled_in_verifies_what_ci_signed() {
    assert_eq!(
        SIGNATURE.len(),
        64,
        "an Ed25519 signature is 64 bytes; this fixture is {} and cannot be one",
        SIGNATURE.len()
    );
    assert!(
        pl_core::ed25519::verify_slices(
            &pl_update::RELEASE_PUBLIC_KEY,
            MANIFEST,
            SIGNATURE
        ),
        "the signature published with v0.1.2 does not verify under the public key \
         compiled into this build. Either RELEASE_PUBLIC_KEY was edited, or the \
         POLYLINKER_RELEASE_KEY secret is not the private half of it -- and in \
         that case every release since is carrying a signature nobody can check."
    );
}

#[test]
fn a_single_flipped_bit_anywhere_breaks_it() {
    // Without this the test above would pass against a verifier that returns
    // true unconditionally, which is the shape of failure this project keeps
    // finding in its own tests.
    for bit in [0usize, 1, 7, 200, 403 * 8 + 3] {
        let mut m = MANIFEST.to_vec();
        if bit / 8 >= m.len() {
            continue;
        }
        m[bit / 8] ^= 1 << (bit % 8);
        assert!(
            !pl_core::ed25519::verify_slices(
                &pl_update::RELEASE_PUBLIC_KEY,
                &m,
                SIGNATURE
            ),
            "flipping bit {bit} of the manifest still verified"
        );
    }
    for bit in [0usize, 255, 383, 511] {
        let mut s = SIGNATURE.to_vec();
        s[bit / 8] ^= 1 << (bit % 8);
        assert!(
            !pl_core::ed25519::verify_slices(
                &pl_update::RELEASE_PUBLIC_KEY,
                MANIFEST,
                &s
            ),
            "flipping bit {bit} of the signature still verified"
        );
    }
}

#[test]
fn a_different_key_does_not_verify_it() {
    // The signature must be tied to THIS key, not merely be a well-formed
    // signature. One byte of the public key is changed; the point must either
    // fail to decompress or fail the equation, and either way verify() is false.
    let mut other = pl_update::RELEASE_PUBLIC_KEY;
    other[0] ^= 0x01;
    assert!(
        !pl_core::ed25519::verify_slices(&other, MANIFEST, SIGNATURE),
        "a public key one bit away from the release key also verified the release signature"
    );
}

#[test]
fn the_fixture_is_the_manifest_it_claims_to_be() {
    // Guards against the fixture being replaced by something that happens to
    // verify -- for example an empty file signed with the same key. If this
    // ever needs updating, the manifest and the signature must be replaced
    // together, from the same release.
    let text = std::str::from_utf8(MANIFEST).expect("the manifest is text");
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 4, "v0.1.2 published four artifacts");
    for l in &lines {
        let (hash, name) = l.split_once("  ").expect("sha256sum format");
        assert_eq!(hash.len(), 64, "a sha256 is 64 hex characters");
        assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(name.contains("0.1.2"), "{name} is not a v0.1.2 artifact");
    }
}
