//! Write this crate's zlib streams to disk, for an outside decoder to judge.
//!
//! `crates/pl-draw/src/deflate/tests.rs` round-trips the encoder against a
//! decoder written from RFC 1951 in the same repository. That catches nearly
//! everything, but it cannot catch the two of them **misreading the
//! specification the same way** — and a stream both agree on and `zlib` does
//! not is a PNG nobody can open.
//!
//! So this writes each corpus case out as a pair, and
//! `reference/python/tests/xcheck_deflate.py` asserts that Python's `zlib` —
//! a different implementation by different people — returns the input. It is a
//! test rather than a script because the corpus belongs next to the encoder,
//! and it writes under `target/` because that is the one directory a test may
//! litter.
//!
//! Running this alone proves nothing. It is half of a check; the gate step is
//! the other half, and it is the half with the oracle in it.

use std::fs;
use std::path::PathBuf;

fn dir() -> PathBuf {
    // `CARGO_TARGET_TMPDIR` is a directory Cargo sets aside for exactly this
    // and cleans up with `cargo clean`, so nothing here escapes the build.
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("zstream")
}

/// The cases, chosen for the encoder path each one reaches.
fn cases() -> Vec<(&'static str, Vec<u8>)> {
    let mut v: Vec<(&'static str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("one-byte", vec![0x42]),
        ("three-bytes", vec![7, 7, 7]),
        ("one-symbol", vec![0xAB; 5000]),
        ("all-256-bytes", (0..=255u8).collect()),
        ("run-past-258", vec![0x5A; 258 * 3 + 7]),
    ];
    let mut edge = vec![0u8; 32768];
    for (i, b) in edge.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }
    let head = edge[..64].to_vec();
    edge.extend_from_slice(&head);
    v.push(("window-edge", edge));

    let mut noise = Vec::with_capacity(40000);
    let mut x: u32 = 0x1234_5678;
    for _ in 0..40000 {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        noise.push((x >> 24) as u8);
    }
    v.push(("noise", noise));

    let mut many = Vec::with_capacity(400_000);
    let mut y: u32 = 99;
    for _ in 0..400_000 {
        y = y.wrapping_mul(48271) % 0x7FFF_FFFF;
        many.push((y % 7) as u8);
    }
    v.push(("many-blocks", many));

    // The shape a PNG actually hands the compressor: a filter byte, then flat
    // colour with antialiased edges between the flats.
    let mut scan = Vec::with_capacity(3 * 900 * 300 + 300);
    for y in 0..300u32 {
        scan.push(0u8);
        for x in 0..900u32 {
            let edge = (x as i32 - 300 - y as i32 / 4).unsigned_abs();
            let v = if edge < 2 {
                128u8
            } else if x < 300 {
                255
            } else {
                74
            };
            scan.extend_from_slice(&[v, v, v]);
        }
    }
    v.push(("map-scanlines", scan));
    v
}

#[test]
fn write_streams_for_the_python_cross_check() {
    let d = dir();
    fs::create_dir_all(&d).expect("a place to write");
    for (name, data) in cases() {
        fs::write(d.join(format!("{name}.raw")), &data).expect("the input");
        fs::write(d.join(format!("{name}.z")), pl_draw::deflate::zlib(&data)).expect("the stream");
    }
    // A manifest, so the checker fails loudly on a case that was never written
    // rather than quietly checking the ones that were.
    let names: Vec<&str> = cases().iter().map(|(n, _)| *n).collect();
    fs::write(d.join("MANIFEST"), names.join("\n")).expect("the manifest");
    assert!(d.join("MANIFEST").exists());
}
