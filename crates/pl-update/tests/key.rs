//! Does the compiled-in trust anchor still hold the key it is supposed to?
//!
//! The failure this is written against is not sabotage. It is a rebase, a
//! merge resolution, or a hand-edit of thirty-two hexadecimal byte literals in
//! which one nibble comes out wrong. Nothing else in this repository would
//! notice: the crate still compiles, every other test still passes, and the
//! first symptom is a released binary that refuses every genuine update — or,
//! if the bad edit happened to land on a key somebody else holds, accepts a
//! forged one. A constant with no oracle is a constant nobody is checking.
//!
//! The oracle is redundancy of spelling. `src/lib.rs` carries the same 32 bytes
//! three ways — array, hex, base64 — and this file re-derives the second and
//! third from the first. One careless edit therefore has to be made
//! consistently in three notations, in two different alphabets, to survive.
//!
//! **These tests deliberately do not use `pl-core`'s `base64` module**, though
//! it exists, is tested, and is now reachable — `pl-core` became a dependency
//! when this crate grew the updater that uses the key. Two reasons, and only
//! the second of them survived that change.
//!
//! The first was that the crate had no dependencies at all, so there was
//! nothing to reach for. That is no longer true, and pretending otherwise would
//! be exactly the kind of stale claim this repository keeps finding in its own
//! comments. What is still true, and is the reason that matters: an oracle
//! should not come through the same door as the thing it is checking. If a
//! `pl-core` build ever went wrong in a way that affected both, an encoder
//! borrowed from it would agree with the corruption.
//!
//! The second reason is unchanged and is by itself sufficient:
//! `pl_core::base64` only emits the unpadded form, so it could not check the
//! padded string a PEM needs. The encoders here are eleven lines and are
//! themselves checked against RFC 4648's published vectors, which is what stops
//! this file agreeing with itself.

use std::path::Path;

/// Lower-case hexadecimal, the spelling `RELEASE_PUBLIC_KEY_HEX` uses.
fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        s.push(char::from_digit((b & 15) as u32, 16).unwrap());
    }
    s
}

/// Standard base64 **with** padding, per RFC 4648 §4.
///
/// Padded, unlike `pl_core::base64`, because a PEM body is padded and the point
/// of `RELEASE_PUBLIC_KEY_BASE64` is that it can be concatenated straight into
/// one. 32 bytes is not a multiple of 3, so the padding is not decorative here:
/// it is the last character of the string the release workflow reads.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        s.push(ALPHABET[(n >> 18 & 63) as usize] as char);
        s.push(ALPHABET[(n >> 12 & 63) as usize] as char);
        s.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        s.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    s
}

/// The two encoders, against published answers rather than against each other.
///
/// Without this the file is circular: `hex` and `base64` would be checked only
/// by constants they themselves produced the expected value for. The base64
/// vectors are RFC 4648 §10 verbatim, padding included — that section is the
/// reason the padded form can be tested at all, since `pl-core`'s own vectors
/// have the `=` stripped.
#[test]
fn the_encoders_agree_with_rfc_4648_and_with_known_hex() {
    for (input, expected) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64(input.as_bytes()), expected, "base64({input:?})");
    }

    // Both nibbles, both halves of the alphabet, and the two boundary bytes.
    assert_eq!(hex(&[0x00, 0x0f, 0xf0, 0xff]), "000ff0ff");
    assert_eq!(hex(b"foobar"), "666f6f626172");
    assert_eq!(hex(&[]), "");
}

/// The three spellings in `src/lib.rs` are one value.
#[test]
fn the_key_is_the_same_thirty_two_bytes_in_all_three_notations() {
    assert_eq!(
        hex(&pl_update::RELEASE_PUBLIC_KEY),
        pl_update::RELEASE_PUBLIC_KEY_HEX,
        "the compiled-in bytes and the hex constant have drifted apart. \
         Whichever is wrong, do not 'fix' the one that is easier to edit -- \
         the array is what a verifier reads and the hex is what the release \
         notes quote, and getting the wrong one right is worse than neither."
    );
    assert_eq!(
        base64(&pl_update::RELEASE_PUBLIC_KEY),
        pl_update::RELEASE_PUBLIC_KEY_BASE64,
        "the compiled-in bytes and the base64 constant have drifted apart, \
         and .github/workflows/release.yml verifies the release signature \
         against the base64 one"
    );

    // The literal, one more time, written out here so that this file does not
    // merely say the three constants agree with each other -- three agreeing
    // constants in one file can all be wrong together, because whoever edited
    // one could have run the test, seen it fail, and edited the other two.
    // docs/RELEASING.md and the release notes carry the same string.
    assert_eq!(
        pl_update::RELEASE_PUBLIC_KEY_HEX,
        "5a53cfdab24df9b4d8e918aed8e03338bdcac10b073a6f59d21d3ee9836be3b7"
    );
    assert_eq!(pl_update::RELEASE_PUBLIC_KEY.len(), 32);
}

/// The comparison above is load-bearing: no bit of the key is unchecked.
///
/// "A check that cannot fail proves nothing." A test that compares an encoding
/// against a constant can still be blind — if `hex` dropped its last byte, or
/// `base64` ignored the low bits of the third byte in each group, the assertion
/// would hold for this key and fail to hold for a key differing only there. So
/// every single bit is flipped in turn and both encodings are required to
/// change, which is exactly the property "one wrong nibble is caught" needs.
#[test]
fn flipping_any_single_bit_of_the_key_changes_both_encodings() {
    let good = pl_update::RELEASE_PUBLIC_KEY;
    let good_hex = hex(&good);
    let good_b64 = base64(&good);
    for i in 0..good.len() {
        for bit in 0..8 {
            let mut bad = good;
            bad[i] ^= 1 << bit;
            assert_ne!(
                hex(&bad),
                good_hex,
                "byte {i} bit {bit} is invisible to hex"
            );
            assert_ne!(
                base64(&bad),
                good_b64,
                "byte {i} bit {bit} is invisible to base64"
            );
        }
    }
}

/// `.github/workflows/release.yml` reads the base64 key out of `src/lib.rs` by
/// pattern. This is that pattern, checked here rather than on release day.
///
/// The workflow does it that way so the key it verifies the release signature
/// with is the key that ships in the binary, and not a second copy pasted into
/// YAML where it could drift. The cost of that is a coupling between a shell
/// step and the text of a Rust file, and an untested coupling to a file people
/// edit is a coupling that breaks. The failure mode it guards against is
/// specific: a second 44-character base64 token appearing in `src/lib.rs` — in
/// a doc comment, an example, an added constant — would make the workflow's
/// `sort -u` yield two candidates, and it would then have to either guess or
/// fail. It fails; this says so before the tag is pushed.
#[test]
fn exactly_one_base64_key_appears_in_the_source_the_release_workflow_reads() {
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
        .expect("the crate's own src/lib.rs");

    // The same shape as the workflow's `grep -oE '[A-Za-z0-9+/]{43}='`: a run
    // of 43 base64 symbols followed by the single '=' that 32 bytes always
    // produce. Written as a scan rather than a regex because this crate has no
    // regex engine and pulling one in for a test would contradict the crate.
    let symbol = |c: char| c.is_ascii_alphanumeric() || c == '+' || c == '/';
    let chars: Vec<char> = src.chars().collect();
    let mut found: Vec<String> = Vec::new();
    let mut run = 0usize;
    for (i, &c) in chars.iter().enumerate() {
        if symbol(c) {
            run += 1;
        } else {
            if c == '=' && run >= 43 {
                // The last 43 symbols before the '=', which is also where
                // `grep -oE '[A-Za-z0-9+/]{43}='` lands: a fixed-width match
                // can only succeed at the offset that puts the '=' at its end.
                found.push(chars[i - 43..=i].iter().collect());
            }
            run = 0;
        }
    }
    found.sort();
    found.dedup();
    assert_eq!(
        found,
        vec![pl_update::RELEASE_PUBLIC_KEY_BASE64.to_string()],
        "src/lib.rs must contain the release key's base64 form and no other \
         44-character base64 token; .github/workflows/release.yml extracts it \
         by that pattern and refuses to guess between candidates"
    );
}

/// The crate declares `pl-core` and nothing else — and neither does `pl-core`.
///
/// This test used to require *no* dependencies at all, and the reason is still
/// the right one: this crate holds the one value an attacker most wants to
/// change, and a build script or a proc macro belonging to any dependency runs
/// with the ability to rewrite it during compilation, invisibly to anyone
/// reading `src/lib.rs`. What changed is that the crate now has to *use* the
/// key, and the Ed25519, SHA-512 and SHA-256 it needs are `pl-core`'s;
/// `Cargo.toml` sets out why copying a verifier in here or taking one from the
/// caller are both worse.
///
/// So the rule is checked as the property it was standing in for, one step
/// further down, and the check is stricter than the old one in every direction
/// but that single edge:
///
/// * `pl-update` declares exactly `pl-core`, through the workspace.
/// * `pl-core` declares nothing at all, so nothing arrives transitively.
/// * Neither crate has a `build.rs`, which is the mechanism the original
///   argument was actually about — a dependency with no build script and no
///   proc macro cannot execute anything during this crate's compilation.
/// * `[dev-dependencies]` is still checked in both. A dev-dependency does not
///   ship, but it is compiled and run on the machine that decides whether this
///   crate is correct, which is enough.
#[test]
fn the_dependency_surface_is_pl_core_and_nothing_it_pulls_in() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));

    let mine = std::fs::read_to_string(here.join("Cargo.toml")).unwrap();
    let declared = dependency_lines(&mine);
    assert_eq!(
        declared,
        vec!["pl-core.workspace = true".to_string()],
        "pl-update may depend on pl-core and on nothing else"
    );

    let core = here.join("..").join("pl-core");
    let theirs = std::fs::read_to_string(core.join("Cargo.toml"))
        .expect("crates/pl-core/Cargo.toml, which this crate now depends on");
    assert!(
        dependency_lines(&theirs).is_empty(),
        "pl-core has grown a dependency, so pl-update has inherited one: {:?}",
        dependency_lines(&theirs)
    );

    for (name, dir) in [("pl-update", here.to_path_buf()), ("pl-core", core)] {
        assert!(
            !dir.join("build.rs").exists(),
            "{name} has a build.rs, which runs arbitrary code during the \
             compilation that bakes RELEASE_PUBLIC_KEY into the binary"
        );
    }

    // The sections have to exist, or `dependency_lines` proves nothing:
    // deleting `[dependencies]` would make this pass by having nothing to look
    // at. That is the exact shape of a check that cannot fail.
    assert!(mine.contains("[dependencies]"));
    assert!(theirs.contains("[dependencies]"));
}

/// Every dependency declaration in a manifest, from every `*dependencies*`
/// section, with comments and blank lines dropped.
fn dependency_lines(manifest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut section = "";
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            section = t;
            continue;
        }
        if section.contains("dependencies") && !t.starts_with('#') && t.contains('=') {
            out.push(t.to_string());
        }
    }
    out
}
