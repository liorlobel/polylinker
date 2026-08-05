//! PNG, from a finished pixel buffer.
//!
//! The container only: chunks, filters, and the physical size. What produces
//! the pixels is not here, and neither is compression — that is
//! [`crate::deflate`].
//!
//! # Only filter type 0, and that is the measured answer
//!
//! PNG lets each scanline choose one of five predictors, and the usual advice
//! is an adaptive heuristic — try them all per row, keep the smallest. On this
//! crate's output that advice is simply wrong. Every row filtered `None`,
//! through `zlib -9`, against the alternatives:
//!
//! | figure | None | Sub | Up | Paeth |
//! |---|---|---|---|---|
//! | sparse map, 2880 px | **132.7 kB** | 173.4 kB | 168.3 kB | 175.9 kB |
//! | fixture map, 2880 px | **143.6 kB** | 188.1 kB | 190.4 kB | 196.8 kB |
//! | dense map, 2880 px | **361.2 kB** | 477.6 kB | 524.6 kB | 524.5 kB |
//! | dense map, 5760 px | **891.9 kB** | 1166.5 kB | 1247.5 kB | 1240.5 kB |
//!
//! `None` wins on every figure at every size, by 25–45%, and beats resvg's own
//! adaptive output on the same image (143.6 kB against 200.5 kB). A plasmid map
//! is flat colour: long runs of one repeating pixel that LZ77 matches directly,
//! including across scanlines, since a row here is a few kB and the window is
//! 32 kB. A predictor turns those runs into runs of zeros — no better — while
//! scattering the antialiased edges into residuals that break the byte-level
//! repetition the matcher was living on.
//!
//! So there is no filter heuristic, no Paeth predictor, and no per-row trial
//! encoding in this file. **The caveat is content**: this holds for
//! vector-derived line art on a flat ground. A photograph or a smooth gradient
//! would reverse it, and if this crate ever rasterizes one, that is when the
//! adaptive path earns its lines. The gel is not such a case — `pl gel` emits
//! paths and text and no gradients.

use crate::deflate;

/// An 8-bit RGB image, row-major, no padding.
pub struct Image {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

impl Image {
    /// A `w` × `h` image filled with one colour.
    ///
    /// Both dimensions are floored at 1. A zero in `IHDR` is not a small PNG,
    /// it is an invalid one that every reader rejects, and the caller most
    /// likely to produce a zero is an honest `Fit::pixels` on a figure sized in
    /// millimetres at a low dpi.
    pub fn filled(w: u32, h: u32, rgb: [u8; 3]) -> Image {
        let (w, h) = (w.max(1), h.max(1));
        let mut px = Vec::with_capacity(w as usize * h as usize * 3);
        for _ in 0..(w as usize * h as usize) {
            px.extend_from_slice(&rgb);
        }
        Image { w, h, px }
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    /// The pixels, row-major, three bytes each.
    pub fn pixels(&self) -> &[u8] {
        &self.px
    }

    /// The pixels, to be written into.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.px
    }
}

/// One chunk: length, type, data, CRC.
///
/// The CRC covers the **type and the data, not the length** — getting that
/// wrong produces a file that looks structurally fine and fails at the first
/// chunk in every reader.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = deflate::crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

/// Pixels per metre for a dots-per-inch figure, as `pHYs` wants it.
///
/// `pHYs` has no notion of inches; it is pixels per unit with the unit fixed at
/// the metre. The rounding is therefore unavoidable and worth stating exactly:
/// 300 dpi is 11811.02 px/m, stored as 11811, and reads back as 299.9994 —
/// two parts per million. The worst any dpi can be out is half a pixel per
/// metre, which is 0.0127 dpi whatever the figure, and cannot move a pixel on
/// anything that will ever be printed.
fn per_metre(dpi: f64) -> u32 {
    // 1/0.0254 metres per inch. Guarded because a dpi of 0 or NaN reaching a
    // file header is a figure whose physical size is a lie.
    if !dpi.is_finite() || dpi <= 0.0 {
        return 0;
    }
    (dpi / 0.0254).round().max(1.0).min(u32::MAX as f64) as u32
}

/// The image as a PNG, optionally carrying a physical resolution.
///
/// `dpi` is what makes this a *publication* export rather than a pile of
/// pixels: `docs/PLAN.md`'s roadmap row is "PNG/TIFF at specified physical
/// width and dpi", and a PNG that does not record its resolution arrives in a
/// manuscript at whatever size the layout program guesses. `None` omits `pHYs`
/// entirely, which is the honest encoding of "no stated size" — it is not the
/// same as claiming 72.
pub fn encode(img: &Image, dpi: Option<f64>) -> Vec<u8> {
    let mut out = Vec::with_capacity(img.px.len() / 4 + 1024);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend(img.w.to_be_bytes());
    ihdr.extend(img.h.to_be_bytes());
    ihdr.extend_from_slice(&[
        8, // bit depth
        2, // colour type 2: truecolour, no alpha
        0, // compression: deflate, the only value
        0, // filter method 0, the only value
        0, // no interlacing
    ]);
    chunk(&mut out, b"IHDR", &ihdr);

    // Colour space, before IDAT as the spec requires. `sRGB` says what the
    // numbers mean; `gAMA` and `cHRM` repeat it in the older vocabulary,
    // because PNG asks that a writer emitting `sRGB` also emit those two so a
    // decoder that does not know the chunk still lands in the right place. The
    // values are the ones the spec prints for sRGB, not values of our own.
    chunk(&mut out, b"sRGB", &[0]); // rendering intent 0: perceptual
    chunk(&mut out, b"gAMA", &45455u32.to_be_bytes());
    let mut chrm = Vec::with_capacity(32);
    for v in [31270u32, 32900, 64000, 33000, 30000, 60000, 15000, 6000] {
        chrm.extend(v.to_be_bytes());
    }
    chunk(&mut out, b"cHRM", &chrm);

    if let Some(d) = dpi {
        let ppm = per_metre(d);
        if ppm > 0 {
            let mut phys = Vec::with_capacity(9);
            phys.extend(ppm.to_be_bytes());
            phys.extend(ppm.to_be_bytes());
            phys.push(1); // unit specifier 1: the metre
            chunk(&mut out, b"pHYs", &phys);
        }
    }

    // Scanlines, each prefixed with its filter type. See the module comment for
    // why that byte is always zero.
    let stride = img.w as usize * 3;
    let mut raw = Vec::with_capacity(img.h as usize * (stride + 1));
    for row in img.px.chunks(stride) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    chunk(&mut out, b"IDAT", &deflate::zlib(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests;
