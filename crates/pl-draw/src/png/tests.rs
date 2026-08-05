//! Tests for the PNG container.
//!
//! Structure is checked here; **pixels are checked by PIL**, in `tools/ci.ps1`
//! via `crates/pl-draw/tests/pngfile.rs`. The split is the same one
//! `deflate/tests.rs` makes and for the same reason: this file can prove the
//! bytes are the bytes we meant to write, and only an outside decoder can prove
//! they are a picture anybody else sees the same way.

use super::*;

/// Walk the chunk list: `(type, data length)` in file order.
///
/// Parsed rather than searched for, so a test cannot pass on a byte sequence
/// that merely appears somewhere in the compressed data.
fn chunks(png: &[u8]) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut i = 8;
    while i + 12 <= png.len() {
        let len = u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]) as usize;
        let kind = String::from_utf8_lossy(&png[i + 4..i + 8]).to_string();
        out.push((kind, len));
        i += 12 + len;
    }
    out
}

/// The signature, the chunk order, and IHDR's contents.
#[test]
fn the_file_is_shaped_like_a_png() {
    let png = encode(&Image::filled(7, 3, [10, 20, 30]), None);
    assert_eq!(
        &png[..8],
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
        "the eight-byte signature is what tells a reader this is a PNG at all, \
         and the 0x89 and the CRLF/LF pair are there to catch a transfer that \
         mangled line endings"
    );
    let ks: Vec<String> = chunks(&png).iter().map(|(k, _)| k.clone()).collect();
    assert_eq!(
        ks.first().map(String::as_str),
        Some("IHDR"),
        "IHDR must come first: {ks:?}"
    );
    assert_eq!(
        ks.last().map(String::as_str),
        Some("IEND"),
        "IEND must come last: {ks:?}"
    );
    let idat = ks.iter().position(|k| k == "IDAT").expect("an IDAT");
    for ancillary in ["sRGB", "gAMA", "cHRM"] {
        let at = ks
            .iter()
            .position(|k| k == ancillary)
            .unwrap_or_else(|| panic!("no {ancillary} chunk: {ks:?}"));
        assert!(
            at < idat,
            "{ancillary} sits after IDAT, where the spec says a decoder may \
             ignore it: {ks:?}"
        );
    }

    let (_, n) = chunks(&png)[0].clone();
    assert_eq!(n, 13, "IHDR is thirteen bytes");
    let ihdr = &png[16..16 + 13];
    assert_eq!(u32::from_be_bytes([ihdr[0], ihdr[1], ihdr[2], ihdr[3]]), 7);
    assert_eq!(u32::from_be_bytes([ihdr[4], ihdr[5], ihdr[6], ihdr[7]]), 3);
    assert_eq!(
        &ihdr[8..],
        &[8, 2, 0, 0, 0],
        "depth 8, colour type 2, deflate, filter method 0, no interlace"
    );
}

/// Every chunk's CRC must be the CRC of its type and data.
///
/// Not of its length — that is the classic way to write a file which greps
/// fine, opens in nothing, and gives an error message about the first chunk
/// rather than about the CRC.
#[test]
fn every_chunk_carries_the_right_crc() {
    let png = encode(&Image::filled(9, 5, [1, 2, 3]), Some(300.0));
    let mut i = 8;
    let mut seen = 0;
    while i + 12 <= png.len() {
        let len = u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]) as usize;
        let want = crate::deflate::crc32(&png[i + 4..i + 8 + len]);
        let got = u32::from_be_bytes([
            png[i + 8 + len],
            png[i + 9 + len],
            png[i + 10 + len],
            png[i + 11 + len],
        ]);
        let kind = String::from_utf8_lossy(&png[i + 4..i + 8]).to_string();
        assert_eq!(want, got, "{kind}'s CRC");
        // ...and it must not be the CRC of length+type+data, which is the
        // mistake this test exists to make impossible.
        assert_ne!(
            got,
            crate::deflate::crc32(&png[i..i + 8 + len]),
            "{kind}'s CRC also matches length+type+data, so this test cannot \
             tell the two apart and proves nothing"
        );
        seen += 1;
        i += 12 + len;
    }
    assert_eq!(i, png.len(), "the chunks do not tile the file exactly");
    assert!(seen >= 6, "only {seen} chunks");
}

/// pHYs carries the requested resolution, and is absent when none was asked.
///
/// The roadmap row is "at specified physical width and dpi". A PNG with no
/// pHYs arrives in a manuscript at whatever size the layout program guesses, so
/// the presence of the chunk *is* the feature. And `None` must omit it rather
/// than write a default: silently claiming 72 dpi is worse than claiming
/// nothing, because it is a number somebody will trust.
#[test]
fn the_physical_resolution_survives_into_the_file() {
    for dpi in [300.0f64, 600.0, 1200.0] {
        let png = encode(&Image::filled(4, 4, [0, 0, 0]), Some(dpi));
        let at = png
            .windows(4)
            .position(|w| w == b"pHYs")
            .unwrap_or_else(|| panic!("no pHYs at {dpi} dpi"));
        let d = &png[at + 4..at + 13];
        let x = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
        let y = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);
        assert_eq!(x, y, "non-square pixels at {dpi} dpi");
        assert_eq!(d[8], 1, "unit specifier must be 1, the metre");
        // Back to dpi, and it must land on what was asked for. Whole pixels per
        // metre is the only loss there is, so the bound is half of one of
        // those: 0.5 * 0.0254 = 0.0127 dpi, whatever the figure. Asserting
        // anything tighter is asserting against the file format rather than
        // against this code — 600 dpi is 23622.047 px/m and cannot come back
        // as exactly 600 from any encoder.
        let back = f64::from(x) * 0.0254;
        assert!(
            (back - dpi).abs() <= 0.0127,
            "{dpi} dpi stored as {x} px/m reads back as {back}, which is \
             further out than rounding to whole pixels per metre can explain"
        );
    }
    let plain = encode(&Image::filled(4, 4, [0, 0, 0]), None);
    assert!(
        !plain.windows(4).any(|w| w == b"pHYs"),
        "a figure with no stated size claimed one anyway"
    );
    // A nonsense dpi must not reach the header either.
    for bad in [0.0, -300.0, f64::NAN, f64::INFINITY] {
        let png = encode(&Image::filled(4, 4, [0, 0, 0]), Some(bad));
        assert!(
            !png.windows(4).any(|w| w == b"pHYs"),
            "a dpi of {bad} was written into the file as a physical size"
        );
    }
}

/// A zero dimension must become one, not reach IHDR.
///
/// `Fit::pixels` on a small figure at a low dpi can honestly round to zero, and
/// a zero in IHDR is not a small PNG — it is a file every reader rejects.
#[test]
fn a_zero_sized_image_is_floored_rather_than_written() {
    let png = encode(&Image::filled(0, 0, [255, 255, 255]), Some(300.0));
    let ihdr = &png[16..29];
    assert_eq!(u32::from_be_bytes([ihdr[0], ihdr[1], ihdr[2], ihdr[3]]), 1);
    assert_eq!(u32::from_be_bytes([ihdr[4], ihdr[5], ihdr[6], ihdr[7]]), 1);
}

/// Every scanline must be prefixed with filter type 0.
///
/// The module comment measures `None` as the best filter for this crate's
/// content by 25–45%. That is a claim about the bytes actually written, so it
/// is checked on the bytes actually written: decompress IDAT and look.
#[test]
fn every_scanline_says_filter_none() {
    let mut img = Image::filled(11, 6, [200, 100, 50]);
    // Vary the content so a filter byte cannot be mistaken for image data.
    for (i, b) in img.pixels_mut().iter_mut().enumerate() {
        *b = (i * 7 % 251) as u8;
    }
    let png = encode(&img, None);
    let raw = idat_of(&png);
    let stride = 11 * 3 + 1;
    assert_eq!(raw.len(), 6 * stride, "the scanlines are the wrong length");
    for y in 0..6 {
        assert_eq!(
            raw[y * stride],
            0,
            "row {y} is filtered with type {}, not None",
            raw[y * stride]
        );
    }
    // ...and the pixels between the filter bytes are the pixels we set.
    for y in 0..6 {
        let row = &raw[y * stride + 1..(y + 1) * stride];
        assert_eq!(row, &img.pixels()[y * 33..(y + 1) * 33], "row {y}");
    }
}

/// Decompress the IDAT payload, using the decoder from `deflate`'s tests.
fn idat_of(png: &[u8]) -> Vec<u8> {
    let mut i = 8;
    while i + 12 <= png.len() {
        let len = u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]) as usize;
        if &png[i + 4..i + 8] == b"IDAT" {
            let z = &png[i + 8..i + 8 + len];
            return crate::deflate::tests::inflate(&z[2..z.len() - 4]).expect("IDAT decodes");
        }
        i += 12 + len;
    }
    panic!("no IDAT")
}
