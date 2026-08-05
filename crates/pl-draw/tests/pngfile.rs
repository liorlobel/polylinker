//! Write PNGs for an outside decoder to open.
//!
//! The unit tests in `src/png/tests.rs` prove the bytes are the bytes this
//! crate meant to write. They cannot prove the file is a picture anybody else
//! sees the same way, because they parse it with the same understanding that
//! wrote it. `reference/python/tests/xcheck_png.py` opens each of these with
//! PIL and compares every pixel against the `.rgb` beside it.
//!
//! Running this alone proves nothing; the gate step is the half with the oracle
//! in it.

use std::fs;
use std::path::PathBuf;

use pl_draw::png::{encode, Image};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("png")
}

/// Images chosen so that a decoder disagreeing about anything shows up.
fn cases() -> Vec<(&'static str, Image, Option<f64>)> {
    let mut v = Vec::new();

    // One pixel: the smallest legal PNG, and the case where a stride or an
    // off-by-one in the scanline loop has nowhere to hide.
    v.push(("one-pixel", Image::filled(1, 1, [0xDE, 0xAD, 0xBE]), None));

    // Flat colour at a real figure size, with a real dpi.
    v.push((
        "flat-300dpi",
        Image::filled(64, 40, [0x4A, 0x7E, 0xBB]),
        Some(300.0),
    ));

    // A width whose stride is not a multiple of anything convenient, filled
    // with a pattern that differs in all three channels and along both axes.
    // A row/column transposition, an RGB/BGR swap and a stride slip each move
    // pixels, and each would survive a flat-colour image untouched.
    let (w, h) = (37u32, 23u32);
    let mut gradient = Image::filled(w, h, [0, 0, 0]);
    {
        let px = gradient.pixels_mut();
        for y in 0..h as usize {
            for x in 0..w as usize {
                let i = (y * w as usize + x) * 3;
                px[i] = (x * 7 % 256) as u8;
                px[i + 1] = (y * 11 % 256) as u8;
                px[i + 2] = ((x + y) * 3 % 256) as u8;
            }
        }
    }
    v.push(("asymmetric-pattern", gradient, Some(600.0)));

    // Big enough that the compressor uses more than one block and the match
    // finder reaches back across scanlines.
    let (bw, bh) = (400u32, 300u32);
    let mut big = Image::filled(bw, bh, [255, 255, 255]);
    {
        let px = big.pixels_mut();
        for y in 0..bh as usize {
            for x in 0..bw as usize {
                let i = (y * bw as usize + x) * 3;
                let dx = x as f64 - 200.0;
                let dy = y as f64 - 150.0;
                let r = (dx * dx + dy * dy).sqrt();
                let c: [u8; 3] = if (r - 100.0).abs() < 2.0 {
                    [0x33, 0x38, 0x3D]
                } else if r < 100.0 {
                    [0x4A, 0x7E, 0xBB]
                } else {
                    [255, 255, 255]
                };
                px[i..i + 3].copy_from_slice(&c);
            }
        }
    }
    v.push(("map-like", big, Some(1200.0)));

    v
}

#[test]
fn write_pngs_for_the_python_cross_check() {
    let d = dir();
    fs::create_dir_all(&d).expect("a place to write");
    let mut manifest = String::new();
    for (name, img, dpi) in cases() {
        fs::write(d.join(format!("{name}.png")), encode(&img, dpi)).expect("the png");
        // The truth, beside it: raw RGB, so the checker never has to trust a
        // second encoder of ours to say what the pixels were.
        fs::write(d.join(format!("{name}.rgb")), img.pixels()).expect("the pixels");
        manifest.push_str(&format!(
            "{name}\t{}\t{}\t{}\n",
            img.width(),
            img.height(),
            dpi.map(|d| d.to_string()).unwrap_or_else(|| "-".into())
        ));
    }
    fs::write(d.join("MANIFEST"), manifest).expect("the manifest");
}
